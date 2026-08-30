use crate::diagnostics::CsDbWrapper;
use codespan_reporting::term::{
    self,
    termcolor::{BufferWriter, ColorChoice},
};
use common::file::File;
use common::{
    InputDb, define_input_db,
    diagnostics::{
        CompleteDiagnostic, Severity, cmp_complete_diagnostics, trim_trailing_line_whitespace,
    },
};
use hir::analysis::{
    analysis_pass::AnalysisPassManager, diagnostics::DiagnosticVoucher, initialize_analysis_pass,
    semantic::SemanticBorrowAnalysisPass,
};
use hir::{
    Ingot,
    hir_def::{HirIngot, TopLevelMod},
    lower::{map_file_to_mod, module_tree},
};

use crate::diagnostics::ToCsDiag;

define_input_db!(DriverDataBase);

impl DriverDataBase {
    /// Recreate only compiler inputs in a fresh Salsa database.
    ///
    /// Large multi-artifact builds can use this boundary to give each emitted
    /// artifact a bounded query lifetime. Source files, the resolved dependency
    /// graph, compiler options, and compilation settings are reproduced, while
    /// parsed HIR, type-system queries, MIR packages, and backend state are not.
    pub fn replicate_inputs(&self) -> Self {
        let files = self
            .workspace()
            .all_files(self)
            .iter()
            .map(|(url, file)| (url, file.text(self).clone()))
            .collect::<Vec<_>>();
        let dependency_graph = self.dependency_graph();
        let recovery_mode = self.compiler_options().recovery_mode(self);
        let profile = self.compilation_settings().profile(self);

        let mut replica = Self::default();
        for (url, text) in files {
            replica.workspace().update(&mut replica, url, text);
        }
        replica.graph = Some(dependency_graph.replicate_into(self, &replica));
        replica.options = Some(common::options::CompilerOptions::new(
            &replica,
            recovery_mode,
        ));
        replica.settings = Some(common::compilation::CompilationSettings::new(
            &replica, profile,
        ));
        replica
    }

    // TODO: An temporary implementation for ui testing.
    pub fn run_on_top_mod<'db>(&'db self, top_mod: TopLevelMod<'db>) -> DiagnosticsCollection<'db> {
        self.run_on_file_with_pass_manager(top_mod, initialize_analysis_pass())
    }

    pub fn run_on_file_with_pass_manager<'db>(
        &'db self,
        top_mod: TopLevelMod<'db>,
        mut pass_manager: hir::analysis::analysis_pass::AnalysisPassManager,
    ) -> DiagnosticsCollection<'db> {
        DiagnosticsCollection(pass_manager.run_on_module(self, top_mod))
    }

    pub fn run_on_ingot<'db>(&'db self, ingot: Ingot<'db>) -> DiagnosticsCollection<'db> {
        self.run_on_ingot_with_pass_manager(ingot, initialize_analysis_pass())
    }

    pub fn run_on_ingot_with_pass_manager<'db>(
        &'db self,
        ingot: Ingot<'db>,
        mut pass_manager: hir::analysis::analysis_pass::AnalysisPassManager,
    ) -> DiagnosticsCollection<'db> {
        let tree = module_tree(self, ingot);
        DiagnosticsCollection(pass_manager.run_on_module_tree(self, tree))
    }

    pub fn top_mod(&self, input: File) -> TopLevelMod<'_> {
        map_file_to_mod(self, input)
    }

    /// Deterministic source closure structurally participating in a root
    /// compilation: every module in the root ingot module tree and its resolved
    /// dependency ingots.
    ///
    /// This is intentionally conservative. It does not claim item-level
    /// reachability, but unlike import-text scanning every edge is owned by the
    /// compiler database.
    pub fn source_dependency_urls(&self, root: File) -> Vec<String> {
        fn visit(
            db: &DriverDataBase,
            ingot: Ingot<'_>,
            visited_ingots: &mut std::collections::BTreeSet<String>,
            urls: &mut std::collections::BTreeSet<String>,
        ) {
            let identity = ingot.base(db).to_string();
            if !visited_ingots.insert(identity) {
                return;
            }
            for module in ingot.module_tree(db).all_modules() {
                if let Some(url) = module.source_file(db).url(db) {
                    urls.insert(url.to_string());
                }
            }
            for (_, dependency) in ingot.resolved_external_ingots(db) {
                visit(db, *dependency, visited_ingots, urls);
            }
        }

        let mut visited_ingots = std::collections::BTreeSet::new();
        let mut urls = std::collections::BTreeSet::new();
        let top_mod = self.top_mod(root);
        visit(self, top_mod.ingot(self), &mut visited_ingots, &mut urls);
        urls.into_iter().collect()
    }

    pub fn mir_diagnostics_for_top_mod<'db>(
        &'db self,
        top_mod: TopLevelMod<'db>,
    ) -> Vec<CompleteDiagnostic> {
        let mut pass_manager = initialize_mir_diagnostics_pass();
        let mut diagnostics: Vec<_> = pass_manager
            .run_on_module(self, top_mod)
            .into_iter()
            .map(|diag| diag.to_complete(self))
            .collect();
        sort_and_dedup_complete_diagnostics(&mut diagnostics);
        diagnostics
    }

    pub fn mir_diagnostics_for_ingot<'db>(&'db self, ingot: Ingot<'db>) -> Vec<CompleteDiagnostic> {
        // Empty ingots (e.g. deleted during incremental workspace changes)
        // have no root module to analyze.
        if ingot.module_tree(self).root_data().is_none() {
            return Vec::new();
        };
        if self.run_on_ingot(ingot).has_errors(self) {
            return Vec::new();
        }
        let mut pass_manager = initialize_mir_diagnostics_pass();
        let mut diagnostics: Vec<_> = pass_manager
            .run_on_module_tree(self, ingot.module_tree(self))
            .into_iter()
            .map(|diag| diag.to_complete(self))
            .collect();
        sort_and_dedup_complete_diagnostics(&mut diagnostics);
        diagnostics
    }

    pub fn emit_complete_diagnostics(&self, diagnostics: &[CompleteDiagnostic]) {
        let writer = BufferWriter::stderr(ColorChoice::Auto);
        let mut buffer = writer.buffer();
        let config = term::Config::default();
        let mut diagnostics = diagnostics.to_vec();
        sort_and_dedup_complete_diagnostics(&mut diagnostics);

        for diag in diagnostics {
            term::emit(&mut buffer, &config, &CsDbWrapper(self), &diag.to_cs(self)).unwrap();
        }

        writer
            .print(&buffer)
            .expect("Failed to write diagnostics to stderr");
    }

    pub fn format_complete_diagnostics(&self, diagnostics: &[CompleteDiagnostic]) -> String {
        let writer = BufferWriter::stderr(ColorChoice::Never);
        let mut buffer = writer.buffer();
        let config = term::Config::default();
        let mut diagnostics = diagnostics.to_vec();
        sort_and_dedup_complete_diagnostics(&mut diagnostics);

        for diag in diagnostics {
            term::emit(&mut buffer, &config, &CsDbWrapper(self), &diag.to_cs(self)).unwrap();
        }

        trim_trailing_line_whitespace(std::str::from_utf8(buffer.as_slice()).unwrap())
    }
}

fn initialize_mir_diagnostics_pass() -> AnalysisPassManager {
    let mut pass_manager = AnalysisPassManager::new();
    pass_manager.add_module_pass("SemanticBorrow", Box::new(SemanticBorrowAnalysisPass));
    pass_manager
}

pub struct DiagnosticsCollection<'db>(Vec<Box<dyn DiagnosticVoucher + 'db>>);
impl DiagnosticsCollection<'_> {
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn has_errors(&self, db: &DriverDataBase) -> bool {
        self.complete(db)
            .iter()
            .any(|d| d.severity == Severity::Error)
    }

    pub fn emit(&self, db: &DriverDataBase) {
        let writer = BufferWriter::stderr(ColorChoice::Auto);
        let mut buffer = writer.buffer();
        let config = term::Config::default();

        for diag in self.complete(db) {
            term::emit(&mut buffer, &config, &CsDbWrapper(db), &diag.to_cs(db)).unwrap();
        }

        writer
            .print(&buffer)
            .expect("Failed to write diagnostics to stderr");
    }

    /// Format the accumulated diagnostics to a string.
    pub fn format_diags(&self, db: &DriverDataBase) -> String {
        let writer = BufferWriter::stderr(ColorChoice::Never);
        let mut buffer = writer.buffer();
        let config = term::Config::default();

        for diag in self.complete(db) {
            term::emit(&mut buffer, &config, &CsDbWrapper(db), &diag.to_cs(db)).unwrap();
        }

        trim_trailing_line_whitespace(std::str::from_utf8(buffer.as_slice()).unwrap())
    }

    /// Convert deferred diagnostics into stable, sorted diagnostic data.
    ///
    /// Presentation (terminal, LSP, browser, JSON) belongs to the caller.
    pub fn complete(&self, db: &DriverDataBase) -> Vec<CompleteDiagnostic> {
        let mut diags: Vec<_> = self.0.iter().map(|d| d.as_ref().to_complete(db)).collect();
        sort_and_dedup_complete_diagnostics(&mut diags);
        diags
    }
}

fn sort_complete_diagnostics(diags: &mut [CompleteDiagnostic]) {
    diags.sort_by(cmp_complete_diagnostics);
}

fn sort_and_dedup_complete_diagnostics(diags: &mut Vec<CompleteDiagnostic>) {
    sort_complete_diagnostics(diags);
    diags.dedup();
}

#[cfg(test)]
mod tests {
    use super::*;
    use salsa::Setter;

    #[test]
    fn replicated_inputs_preserve_configuration_without_sharing_files() {
        let mut source = DriverDataBase::default();
        let url: url::Url = "file:///isolated-stage/src/lib.fe".parse().unwrap();
        source.workspace().update(
            &mut source,
            url.clone(),
            "pub fn shade() -> u32 { 7 }".to_owned(),
        );
        source
            .compiler_options()
            .set_recovery_mode(&mut source)
            .to(false);
        source
            .compilation_settings()
            .set_profile(&mut source)
            .to("release".into());

        let mut replica = source.replicate_inputs();
        let replicated_file = replica.workspace().get(&replica, &url).unwrap();
        assert_eq!(
            replicated_file.text(&replica),
            "pub fn shade() -> u32 { 7 }"
        );
        assert!(!replica.compiler_options().recovery_mode(&replica));
        assert_eq!(replica.compilation_settings().profile(&replica), "release");

        replica.workspace().update(
            &mut replica,
            url.clone(),
            "pub fn shade() -> u32 { 9 }".to_owned(),
        );
        let source_file = source.workspace().get(&source, &url).unwrap();
        assert_eq!(source_file.text(&source), "pub fn shade() -> u32 { 7 }");
    }
}
