use crate::diagnostics::CsDbWrapper;
use codespan_reporting::term::{
    self,
    termcolor::{BufferWriter, ColorChoice},
};
use common::file::File;
use common::{
    compilation::CompilationSettings,
    dependencies::DependencyGraph,
    diagnostics::{
        CompleteDiagnostic, Severity, sort_and_coalesce_complete_diagnostics,
        trim_trailing_line_whitespace,
    },
    file::Workspace,
    impl_input_db,
    options::CompilerOptions,
};
use hir::analysis::{
    analysis_pass::AnalysisPassManager,
    diagnostics::DiagnosticVoucher,
    initialize_analysis_pass,
    semantic::{LayoutEvidenceAnalysisPass, SemanticBorrowAnalysisPass},
};
use hir::{
    Ingot,
    core::meta::{FrozenGeneration, GeneratedModuleInput, GenerationManifest},
    hir_def::{HirIngot, TopLevelMod},
    lower::{map_file_to_mod, module_tree},
};

use crate::diagnostics::ToCsDiag;

#[derive(Clone)]
#[salsa::db]
pub struct DriverDataBase {
    storage: salsa::Storage<Self>,
    index: Option<Workspace>,
    graph: Option<DependencyGraph>,
    options: Option<CompilerOptions>,
    settings: Option<CompilationSettings>,
    salsa_event_counters: Option<std::sync::Arc<std::sync::Mutex<common::SalsaEventCounters>>>,
    generation: Option<GenerationManifest>,
}

#[salsa::db]
impl salsa::Database for DriverDataBase {
    fn salsa_event(&self, event: &dyn Fn() -> salsa::Event) {
        if let Some(counters) = &self.salsa_event_counters
            && let Ok(mut counters) = counters.lock()
        {
            counters.record_event(&event());
        }
    }
}

impl_input_db!(DriverDataBase);

#[salsa::db]
impl hir::HirDb for DriverDataBase {
    fn generated_module(&self, url: &url::Url) -> Option<GeneratedModuleInput> {
        self.generation?.modules(self).get(url).copied()
    }
}

impl Default for DriverDataBase {
    fn default() -> Self {
        let mut db = Self {
            storage: salsa::Storage::default(),
            index: None,
            graph: None,
            options: None,
            settings: None,
            salsa_event_counters: None,
            generation: None,
        };
        db.index = Some(Workspace::default(&db));
        db.graph = Some(DependencyGraph::default(&db));
        db.options = Some(CompilerOptions::default(&db));
        db.settings = Some(CompilationSettings::default(&db));
        db.generation = Some(GenerationManifest::new(&db, Default::default()));
        common::stdlib::HasBuiltinCore::initialize_builtin_core(&mut db);
        common::stdlib::HasBuiltinStd::initialize_builtin_std(&mut db);
        db
    }
}

impl DriverDataBase {
    pub fn set_salsa_event_counters(
        &mut self,
        counters: Option<std::sync::Arc<std::sync::Mutex<common::SalsaEventCounters>>>,
    ) {
        self.salsa_event_counters = counters;
    }

    /// Publish a completed generation pass as tracked, lifetime-free input.
    pub(crate) fn publish_generation(&mut self, requests: Vec<FrozenGeneration>) {
        use salsa::Setter;

        let mut by_module = std::collections::BTreeMap::new();
        for request in requests {
            by_module
                .entry(request.target.file.clone())
                .or_insert_with(Vec::new)
                .push(request);
        }
        let modules = by_module
            .into_iter()
            .map(|(url, requests)| (url, GeneratedModuleInput::new(self, requests)))
            .collect();
        self.generation
            .expect("generation manifest initialized")
            .set_modules(self)
            .to(modules);
    }

    /// Create an independent input view for explicit elaboration.
    ///
    /// Unlike cloning a Salsa database handle, this creates new input identities
    /// and does not share mutable inputs or query results. Resolve items again in
    /// the returned database; only URLs and authored/configuration data cross.
    /// No filesystem discovery or dependency resolution is repeated here.
    pub fn fork_inputs(&self) -> Self {
        use common::InputDb;
        use salsa::Setter;

        let mut target = Self::default();
        // Default construction installs builtin files. Replace the complete set
        // so this view also preserves removals and in-memory builtin edits.
        let defaults: Vec<_> = target
            .workspace()
            .all_files(&target)
            .iter()
            .map(|(url, _)| url.clone())
            .collect();
        for url in defaults {
            target.workspace().remove(&mut target, &url);
        }
        for (url, file) in self.workspace().all_files(self).iter() {
            target
                .workspace()
                .update(&mut target, url.clone(), file.text(self).clone());
        }
        self.dependency_graph().copy_into(self, &mut target);
        target
            .compiler_options()
            .set_recovery_mode(&mut target)
            .to(self.compiler_options().recovery_mode(self));
        target
            .compilation_settings()
            .set_profile(&mut target)
            .to(self.compilation_settings().profile(self));
        target
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
        sort_and_coalesce_complete_diagnostics(&mut diagnostics);
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
        sort_and_coalesce_complete_diagnostics(&mut diagnostics);
        diagnostics
    }

    pub fn emit_complete_diagnostics(&self, diagnostics: &[CompleteDiagnostic]) {
        let writer = BufferWriter::stderr(stderr_color_choice());
        let mut buffer = writer.buffer();
        let config = term::Config::default();
        let mut diagnostics = diagnostics.to_vec();
        sort_and_coalesce_complete_diagnostics(&mut diagnostics);

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
        sort_and_coalesce_complete_diagnostics(&mut diagnostics);

        for diag in diagnostics {
            term::emit(&mut buffer, &config, &CsDbWrapper(self), &diag.to_cs(self)).unwrap();
        }

        trim_trailing_line_whitespace(std::str::from_utf8(buffer.as_slice()).unwrap())
    }
}

/// The `--color` preference for diagnostics written to standard error.
fn stderr_color_choice() -> ColorChoice {
    if common::color::should_colorize(common::color::ColorTarget::Stderr) {
        ColorChoice::Always
    } else {
        ColorChoice::Never
    }
}

fn initialize_mir_diagnostics_pass() -> AnalysisPassManager {
    let mut pass_manager = AnalysisPassManager::new();
    pass_manager.add_module_pass("SemanticBorrow", Box::new(SemanticBorrowAnalysisPass));
    pass_manager.add_module_pass("LayoutEvidence", Box::new(LayoutEvidenceAnalysisPass));
    pass_manager
}

pub struct DiagnosticsCollection<'db>(Vec<Box<dyn DiagnosticVoucher + 'db>>);
impl DiagnosticsCollection<'_> {
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn has_errors(&self, db: &DriverDataBase) -> bool {
        self.complete_diags(db)
            .iter()
            .any(|d| d.severity == Severity::Error)
    }

    pub fn emit(&self, db: &DriverDataBase) {
        let writer = BufferWriter::stderr(stderr_color_choice());
        let mut buffer = writer.buffer();
        let config = term::Config::default();

        for diag in self.complete_diags(db) {
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

        for diag in self.complete_diags(db) {
            term::emit(&mut buffer, &config, &CsDbWrapper(db), &diag.to_cs(db)).unwrap();
        }

        trim_trailing_line_whitespace(std::str::from_utf8(buffer.as_slice()).unwrap())
    }

    pub fn complete_diags(&self, db: &DriverDataBase) -> Vec<CompleteDiagnostic> {
        let mut diags: Vec<_> = self.0.iter().map(|d| d.as_ref().to_complete(db)).collect();
        sort_and_coalesce_complete_diagnostics(&mut diags);
        diags
    }
}
