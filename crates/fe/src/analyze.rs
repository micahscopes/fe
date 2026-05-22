use std::{collections::HashSet, fmt::Write};

use camino::Utf8PathBuf;
use codegen::{
    OptLevel, SonatinaContractBytecode, SonatinaTestOptions, TestMetadata,
    debug::{
        BytecodeOriginCoverageExport, BytecodeSourceMapEntry, BytecodeSourceMapEntryKind,
        BytecodeSourceMapSummary, bytecode_source_map_entries_summary,
    },
    emit_runtime_package_sonatina_bytecode_with_source_maps, emit_test_module_sonatina,
    origin::BytecodeOriginCoverage,
};
use common::{
    InputDb,
    config::{Config, WorkspaceConfig},
    facts::{
        OriginPathWitnessExport, OriginReachabilitySummary, OwnedTypedFactSetExport,
        TypedFactRelationIndex, TypedFactRelationSet, TypedFactSet, shape_graph_facts,
    },
    file::IngotFileKind,
    origin::OriginExportKind,
    shape::{ShapeBuilder, ShapeDescribe, ShapeDimension, ShapeGraph, ShapeNodeId},
};
use driver::DriverDataBase;
use driver::cli_target::{CliTarget, resolve_cli_target};
use hir::{
    Ingot,
    hir_def::{HirIngot, ItemKind, TopLevelMod},
};
use mir::{
    RuntimeOriginFactOwnerKeys, RuntimeOriginFactTargetKey, RuntimeOriginSource,
    build_runtime_package, build_test_runtime_package, runtime_package_origin_facts,
    runtime_package_origins,
};
use salsa::Setter;
use serde::Serialize;
use url::Url;

use crate::{
    AnalyzeFormat,
    dependency_diagnostics::DependencyIssues,
    workspace_ingot::{
        INGOT_REQUIRES_WORKSPACE_ROOT, WorkspaceMemberRef, select_workspace_member_paths,
    },
};

#[derive(Debug)]
pub(crate) struct AnalyzeOutcome {
    pub(crate) has_errors: bool,
    pub(crate) output: String,
}

#[derive(Debug, Serialize)]
struct AnalyzeReport {
    schema_version: u32,
    profile: String,
    package_kind: &'static str,
    targets: Vec<AnalyzeTargetReport>,
}

#[derive(Debug, Serialize)]
struct AnalyzeTargetReport {
    label: String,
    runtime_bodies: usize,
    runtime_statements: OriginCount,
    runtime_terminators: OriginCount,
    bodies: Vec<AnalyzeBodyReport>,
    source_maps: Vec<AnalyzeSourceMapReport>,
    origin_facts: Vec<AnalyzeOriginFactReport>,
    shapes: Vec<AnalyzeShapeReport>,
}

#[derive(Debug, Serialize)]
struct AnalyzeBodyReport {
    symbol: String,
    statements: OriginCount,
    terminators: OriginCount,
}

#[derive(Debug, Serialize)]
struct AnalyzeSourceMapReport {
    scope: &'static str,
    label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    test: Option<String>,
    object: String,
    section: String,
    total: usize,
    source: usize,
    non_source: usize,
    source_span_invalid: usize,
    semantic_span_missing: usize,
    runtime_stmt_missing: usize,
    runtime_terminator_missing: usize,
    runtime_synthetic: usize,
    sonatina_synthetic: usize,
    sonatina_unmapped: usize,
    post_preopt_snapshot_gap: usize,
    bytecode_unmapped: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    bytecode_origin_coverage: Option<BytecodeOriginCoverageExport>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    entries: Vec<BytecodeSourceMapEntry>,
}

impl AnalyzeSourceMapReport {
    fn from_summary(
        scope: &'static str,
        label: String,
        test: Option<String>,
        object: String,
        section: String,
        summary: &BytecodeSourceMapSummary,
        bytecode_origin_coverage: Option<BytecodeOriginCoverage>,
        entries: Vec<BytecodeSourceMapEntry>,
    ) -> Self {
        Self {
            scope,
            label,
            test,
            object,
            section,
            total: summary.total(),
            source: summary.source(),
            non_source: summary.non_source(),
            source_span_invalid: summary.source_span_invalid(),
            semantic_span_missing: summary.semantic_span_missing(),
            runtime_stmt_missing: summary.runtime_stmt_missing(),
            runtime_terminator_missing: summary.runtime_terminator_missing(),
            runtime_synthetic: summary.runtime_synthetic(),
            sonatina_synthetic: summary.sonatina_synthetic(),
            sonatina_unmapped: summary.sonatina_unmapped(),
            post_preopt_snapshot_gap: summary.post_preopt_snapshot_gap(),
            bytecode_unmapped: summary.bytecode_unmapped(),
            bytecode_origin_coverage: bytecode_origin_coverage
                .map(BytecodeOriginCoverageExport::from),
            entries,
        }
    }
}

#[derive(Debug, Serialize)]
struct AnalyzeOriginFactReport {
    scope: &'static str,
    label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    object: Option<String>,
    total: usize,
    origin_nodes: usize,
    origin_links: usize,
    source_spans: usize,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    source_span_files: Vec<AnalyzeSourceSpanFileCount>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    relation_counts: Vec<AnalyzeFactRelationCount>,
    #[serde(skip_serializing_if = "Option::is_none")]
    relation_tables: Option<TypedFactRelationSet>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reachability: Option<OriginReachabilitySummary>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    path_witnesses: Vec<OriginPathWitnessExport>,
    #[serde(skip_serializing_if = "Option::is_none")]
    query_error: Option<String>,
    facts: OwnedTypedFactSetExport,
}

#[derive(Debug, Serialize)]
struct AnalyzeFactRelationCount {
    relation: String,
    rows: usize,
}

#[derive(Debug, Serialize)]
struct AnalyzeSourceSpanFileCount {
    file: String,
    spans: usize,
}

#[derive(Debug, Serialize)]
struct AnalyzeShapeReport {
    scope: &'static str,
    label: String,
    shape_nodes: usize,
    shape_fields: usize,
    shape_children: usize,
    shape_edges: usize,
    trace_events: usize,
    data_flows: usize,
    graph_hashes: Vec<AnalyzeShapeHashReport>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    relation_counts: Vec<AnalyzeFactRelationCount>,
    #[serde(skip_serializing_if = "Option::is_none")]
    facts: Option<OwnedTypedFactSetExport>,
    #[serde(skip_serializing_if = "Option::is_none")]
    relation_tables: Option<TypedFactRelationSet>,
}

#[derive(Debug, Serialize)]
struct AnalyzeShapeHashReport {
    dimension: &'static str,
    digest_hex: String,
}

const ORIGIN_PATH_WITNESS_LIMIT: usize = 12;
const ORIGIN_PATH_WITNESS_PRIORITY: &[(OriginExportKind, OriginExportKind)] = &[
    (OriginExportKind::Semantic, OriginExportKind::RuntimeStmt),
    (
        OriginExportKind::Semantic,
        OriginExportKind::RuntimeTerminator,
    ),
    (OriginExportKind::Semantic, OriginExportKind::BytecodePc),
    (
        OriginExportKind::RuntimeStmt,
        OriginExportKind::SonatinaInst,
    ),
    (
        OriginExportKind::RuntimeTerminator,
        OriginExportKind::SonatinaInst,
    ),
    (OriginExportKind::RuntimeStmt, OriginExportKind::BytecodePc),
    (
        OriginExportKind::RuntimeTerminator,
        OriginExportKind::BytecodePc,
    ),
    (OriginExportKind::SonatinaInst, OriginExportKind::BytecodePc),
    (
        OriginExportKind::BytecodeUnmapped,
        OriginExportKind::BytecodePc,
    ),
];

#[derive(Clone, Copy, Debug, Default, Serialize)]
struct OriginCount {
    total: usize,
    semantic: usize,
    synthetic: usize,
}

impl OriginCount {
    fn push(&mut self, source: RuntimeOriginSource<'_>) {
        self.total += 1;
        match source {
            RuntimeOriginSource::Semantic(_) => self.semantic += 1,
            RuntimeOriginSource::Synthetic => self.synthetic += 1,
        }
    }

    fn extend(&mut self, other: Self) {
        self.total += other.total;
        self.semantic += other.semantic;
        self.synthetic += other.synthetic;
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct AnalyzeOptions<'a> {
    profile: &'a str,
    format: AnalyzeFormat,
    include_tests: bool,
    include_source_maps: bool,
    include_source_map_entries: bool,
    include_origin_facts: bool,
    include_fact_relation_tables: bool,
    include_shape_hashes: bool,
    include_shape_facts: bool,
    opt_level: OptLevel,
    recovery_mode: bool,
}

impl<'a> AnalyzeOptions<'a> {
    fn new(
        profile: &'a str,
        format: AnalyzeFormat,
        include_tests: bool,
        include_source_maps: bool,
        include_source_map_entries: bool,
        include_origin_facts: bool,
        include_fact_relation_tables: bool,
        include_shape_hashes: bool,
        include_shape_facts: bool,
        opt_level: OptLevel,
        recovery_mode: bool,
    ) -> Self {
        Self {
            profile,
            format,
            include_tests,
            include_source_maps,
            include_source_map_entries,
            include_origin_facts,
            include_fact_relation_tables,
            include_shape_hashes,
            include_shape_facts,
            opt_level,
            recovery_mode,
        }
    }

    fn validate(self) -> Result<Self, String> {
        if self.include_source_map_entries && !self.include_source_maps {
            return Err("`fe analyze --source-map-entries` requires `--source-maps`".to_string());
        }
        if self.include_fact_relation_tables && self.format != AnalyzeFormat::Json {
            return Err("`fe analyze --fact-relation-tables` requires `--format json`".to_string());
        }
        if self.include_fact_relation_tables
            && !(self.include_origin_facts || self.include_shape_facts)
        {
            return Err(
                "`fe analyze --fact-relation-tables` requires `--origin-facts` or `--shape-facts`"
                    .to_string(),
            );
        }
        Ok(self)
    }
}

pub fn analyze(
    path: &Utf8PathBuf,
    ingot: Option<&str>,
    force_standalone: bool,
    profile: &str,
    format: AnalyzeFormat,
    include_tests: bool,
    include_source_maps: bool,
    include_source_map_entries: bool,
    include_origin_facts: bool,
    include_fact_relation_tables: bool,
    include_shape_hashes: bool,
    include_shape_facts: bool,
    opt_level: OptLevel,
    recovery_mode: bool,
) -> Result<bool, String> {
    let options = AnalyzeOptions::new(
        profile,
        format,
        include_tests,
        include_source_maps,
        include_source_map_entries,
        include_origin_facts,
        include_fact_relation_tables,
        include_shape_hashes,
        include_shape_facts,
        opt_level,
        recovery_mode,
    );
    let outcome = analyze_to_string(path, ingot, force_standalone, options)?;
    if !outcome.output.is_empty() {
        print!("{}", outcome.output);
    }
    Ok(outcome.has_errors)
}

pub(crate) fn analyze_to_string(
    path: &Utf8PathBuf,
    ingot: Option<&str>,
    force_standalone: bool,
    options: AnalyzeOptions<'_>,
) -> Result<AnalyzeOutcome, String> {
    let options = options.validate()?;

    let mut db = DriverDataBase::default();
    db.compiler_options()
        .set_recovery_mode(&mut db)
        .to(options.recovery_mode);
    db.compilation_settings()
        .set_profile(&mut db)
        .to(options.profile.into());

    let mut report = AnalyzeReport {
        schema_version: 1,
        profile: options.profile.to_string(),
        package_kind: if options.include_tests {
            "tests"
        } else {
            "runtime"
        },
        targets: Vec::new(),
    };

    let target = resolve_cli_target(&mut db, path, force_standalone)?;
    let has_errors = match target {
        CliTarget::StandaloneFile(file_path) => {
            if ingot.is_some() {
                eprintln!("Error: {INGOT_REQUIRES_WORKSPACE_ROOT}");
                true
            } else {
                analyze_single_file(&mut db, &file_path, options, &mut report)
            }
        }
        CliTarget::Directory(dir_path) => {
            analyze_directory(&mut db, &dir_path, ingot, options, &mut report)
        }
    };

    report
        .targets
        .sort_by(|left, right| left.label.cmp(&right.label));
    let output = if has_errors {
        String::new()
    } else {
        render_report(&report, options.format)?
    };
    Ok(AnalyzeOutcome { has_errors, output })
}

fn analyze_single_file(
    db: &mut DriverDataBase,
    file_path: &Utf8PathBuf,
    options: AnalyzeOptions<'_>,
    report: &mut AnalyzeReport,
) -> bool {
    let canonical = match file_path.canonicalize_utf8() {
        Ok(path) => path,
        Err(err) => {
            eprintln!("Error: Cannot canonicalize {file_path}: {err}");
            return true;
        }
    };
    let file_url = match Url::from_file_path(&canonical) {
        Ok(url) => url,
        Err(_) => {
            eprintln!("Error: Invalid file path: {file_path}");
            return true;
        }
    };
    let content = match std::fs::read_to_string(file_path) {
        Ok(content) => content,
        Err(err) => {
            eprintln!("Error: Failed to read file {file_path}: {err}");
            return true;
        }
    };

    db.workspace().touch(db, file_url.clone(), Some(content));
    let Some(file) = db.workspace().get(db, &file_url) else {
        eprintln!("Error: Could not process file {file_path}");
        return true;
    };
    let top_mod = db.top_mod(file);
    analyze_top_mod(db, file_path.as_str(), top_mod, options, report)
}

fn analyze_directory(
    db: &mut DriverDataBase,
    dir_path: &Utf8PathBuf,
    ingot: Option<&str>,
    options: AnalyzeOptions<'_>,
    report: &mut AnalyzeReport,
) -> bool {
    let ingot_url = match dir_url(dir_path) {
        Ok(url) => url,
        Err(message) => {
            eprintln!("{message}");
            return true;
        }
    };

    let had_init_diagnostics = driver::init_ingot(db, &ingot_url);
    if had_init_diagnostics {
        return true;
    }

    let config = match config_from_db(db, &ingot_url) {
        Ok(Some(config)) => config,
        Ok(None) => {
            if ingot.is_some() {
                eprintln!("Error: {INGOT_REQUIRES_WORKSPACE_ROOT}");
                return true;
            }
            eprintln!("Error: No fe.toml file found in the root directory");
            return true;
        }
        Err(err) => {
            eprintln!("Error: {err}");
            return true;
        }
    };

    match config {
        Config::Workspace(workspace) => {
            analyze_workspace(db, dir_path, *workspace, ingot, options, report)
        }
        Config::Ingot(_) => {
            if ingot.is_some() {
                eprintln!("Error: {INGOT_REQUIRES_WORKSPACE_ROOT}");
                return true;
            }
            analyze_ingot_url(db, &ingot_url, options, report)
        }
    }
}

fn analyze_workspace(
    db: &mut DriverDataBase,
    dir_path: &Utf8PathBuf,
    workspace_config: WorkspaceConfig,
    ingot: Option<&str>,
    options: AnalyzeOptions<'_>,
    report: &mut AnalyzeReport,
) -> bool {
    let workspace_url = match dir_url(dir_path) {
        Ok(url) => url,
        Err(message) => {
            eprintln!("{message}");
            return true;
        }
    };

    let members = match driver::workspace_members(&workspace_config.workspace, &workspace_url) {
        Ok(members) => members,
        Err(err) => {
            eprintln!("Error: Failed to resolve workspace members: {err}");
            return true;
        }
    };

    let selected_member_paths = match select_workspace_member_paths(
        dir_path,
        dir_path,
        members
            .iter()
            .map(|member| WorkspaceMemberRef::new(member.path.as_path(), member.name.as_deref())),
        ingot,
    ) {
        Ok(paths) => paths.into_iter().collect::<HashSet<_>>(),
        Err(err) => {
            eprintln!("Error: {err}");
            return true;
        }
    };

    let mut seen = HashSet::new();
    let mut has_errors = false;
    for member in members {
        let member_path = dir_path.join(member.path.as_str());
        if !selected_member_paths.contains(&member_path) {
            continue;
        }
        has_errors |= analyze_ingot_and_dependencies(db, &member.url, options, report, &mut seen);
    }
    has_errors
}

fn analyze_ingot_url(
    db: &mut DriverDataBase,
    ingot_url: &Url,
    options: AnalyzeOptions<'_>,
    report: &mut AnalyzeReport,
) -> bool {
    let mut seen = HashSet::new();
    analyze_ingot_and_dependencies(db, ingot_url, options, report, &mut seen)
}

fn analyze_ingot_and_dependencies(
    db: &mut DriverDataBase,
    ingot_url: &Url,
    options: AnalyzeOptions<'_>,
    report: &mut AnalyzeReport,
    seen: &mut HashSet<Url>,
) -> bool {
    if !seen.insert(ingot_url.clone()) {
        return false;
    }

    let Some(ingot) = db.workspace().containing_ingot(db, ingot_url.clone()) else {
        eprintln!("Error: Could not resolve ingot {ingot_url}");
        return true;
    };

    if !ingot_has_source_files(db, ingot) {
        eprintln!("Error: Could not find source files for ingot {ingot_url}");
        return true;
    }

    let label = ingot_label(db, ingot, ingot_url);
    let mut has_errors = analyze_ingot_diagnostics(db, ingot, &label);

    let dependency_errors = DependencyIssues::collect(db, ingot_url, seen);
    if !dependency_errors.is_empty() {
        has_errors = true;
        eprint!("{}", dependency_errors.format(db));
    }

    if has_errors {
        return true;
    }

    if options.include_tests {
        analyze_ingot_test_modules(db, &label, ingot, options, report)
    } else {
        analyze_top_mod(db, &label, ingot.root_mod(db), options, report)
    }
}

fn analyze_top_mod(
    db: &DriverDataBase,
    label: &str,
    top_mod: TopLevelMod<'_>,
    options: AnalyzeOptions<'_>,
    report: &mut AnalyzeReport,
) -> bool {
    if analyze_top_mod_diagnostics(db, top_mod, label) {
        return true;
    }

    let package = if options.include_tests {
        match build_test_runtime_package(db, top_mod, None) {
            Ok(package) => package,
            Err(err) => {
                eprintln!("Error: failed to build test runtime package for {label}: {err}");
                return true;
            }
        }
    } else {
        match build_runtime_package(db, top_mod) {
            Ok(package) => package,
            Err(err) => {
                eprintln!("Error: failed to build runtime package for {label}: {err}");
                return true;
            }
        }
    };
    let origins = runtime_package_origins(db, package);
    let mut target = summarize_runtime_origins(label, &origins);
    if options.include_shape_hashes || options.include_shape_facts {
        target.shapes = summarize_const_region_shapes(
            db,
            package,
            options.include_shape_facts,
            options.include_fact_relation_tables,
        );
        target.shapes.extend(summarize_runtime_body_shapes(
            db,
            package,
            options.include_shape_facts,
            options.include_fact_relation_tables,
        ));
    }
    if options.include_origin_facts
        && let Some(facts) =
            runtime_origin_fact_report(label, &origins, options.include_fact_relation_tables)
    {
        target.origin_facts.push(facts);
    }
    if options.include_tests && (options.include_source_maps || options.include_origin_facts) {
        let codegen_reports = match summarize_test_codegen_reports(db, top_mod, options) {
            Ok(codegen_reports) => codegen_reports,
            Err(err) => {
                eprintln!("Error: failed to build Sonatina analysis reports for {label}: {err}");
                return true;
            }
        };
        target.source_maps = codegen_reports.source_maps;
        target.origin_facts.extend(codegen_reports.origin_facts);
    } else if options.include_source_maps {
        let codegen_reports = match summarize_runtime_codegen_reports(db, package, options) {
            Ok(codegen_reports) => codegen_reports,
            Err(err) => {
                eprintln!("Error: failed to build Sonatina analysis reports for {label}: {err}");
                return true;
            }
        };
        target.source_maps = codegen_reports.source_maps;
        target.origin_facts.extend(codegen_reports.origin_facts);
    }
    report.targets.push(target);
    false
}

fn analyze_ingot_test_modules(
    db: &DriverDataBase,
    label: &str,
    ingot: Ingot<'_>,
    options: AnalyzeOptions<'_>,
    report: &mut AnalyzeReport,
) -> bool {
    let mut top_mods = ingot.all_modules(db).to_vec();
    top_mods.sort_by(|left, right| left.name(db).cmp(&right.name(db)));

    let mut has_errors = false;
    for top_mod in top_mods {
        if !has_test_functions(db, top_mod) {
            continue;
        }
        let module_label = format!("{label}::{}", top_mod.name(db).data(db));
        has_errors |= analyze_top_mod(db, &module_label, top_mod, options, report);
    }

    has_errors
}

fn has_test_functions(db: &DriverDataBase, top_mod: TopLevelMod<'_>) -> bool {
    top_mod.all_funcs(db).iter().any(|func| {
        ItemKind::from(*func)
            .attrs(db)
            .is_some_and(|attrs| attrs.has_attr(db, "test"))
    })
}

struct AnalyzeTestCodegenReports {
    source_maps: Vec<AnalyzeSourceMapReport>,
    origin_facts: Vec<AnalyzeOriginFactReport>,
}

fn summarize_runtime_codegen_reports(
    db: &DriverDataBase,
    package: mir::RuntimePackage<'_>,
    options: AnalyzeOptions<'_>,
) -> Result<AnalyzeTestCodegenReports, codegen::LowerError> {
    let outputs = emit_runtime_package_sonatina_bytecode_with_source_maps(
        db,
        &package,
        codegen::EVM_LAYOUT,
        options.opt_level,
    )?;
    let source_maps = options
        .include_source_maps
        .then(|| {
            outputs
                .iter()
                .filter_map(|(contract, output)| {
                    source_map_report_for_runtime_contract(
                        contract,
                        output,
                        options.include_source_map_entries,
                    )
                })
                .collect()
        })
        .unwrap_or_default();
    let origin_facts = options
        .include_origin_facts
        .then(|| {
            outputs
                .iter()
                .flat_map(|(contract, output)| {
                    origin_fact_reports_for_runtime_contract(
                        contract,
                        output,
                        options.include_fact_relation_tables,
                    )
                })
                .collect()
        })
        .unwrap_or_default();

    Ok(AnalyzeTestCodegenReports {
        source_maps,
        origin_facts,
    })
}

fn summarize_test_codegen_reports(
    db: &DriverDataBase,
    top_mod: TopLevelMod<'_>,
    options: AnalyzeOptions<'_>,
) -> Result<AnalyzeTestCodegenReports, codegen::LowerError> {
    let output = emit_test_module_sonatina(
        db,
        top_mod,
        options.opt_level,
        SonatinaTestOptions {
            emit_observability: true,
        },
        None,
    )?;
    let source_maps = options
        .include_source_maps
        .then(|| {
            output
                .tests
                .iter()
                .filter_map(|test| {
                    source_map_report_for_test(test, options.include_source_map_entries)
                })
                .collect()
        })
        .unwrap_or_default();
    let origin_facts = options
        .include_origin_facts
        .then(|| {
            output
                .tests
                .iter()
                .flat_map(|test| {
                    origin_fact_reports_for_test(test, options.include_fact_relation_tables)
                })
                .collect()
        })
        .unwrap_or_default();

    Ok(AnalyzeTestCodegenReports {
        source_maps,
        origin_facts,
    })
}

fn source_map_report_for_test(
    test: &TestMetadata,
    include_source_map_entries: bool,
) -> Option<AnalyzeSourceMapReport> {
    let summary = test.sonatina_source_map_summary.as_ref()?;
    let entries = include_source_map_entries
        .then(|| test.sonatina_source_map_entries.clone())
        .unwrap_or_default();
    Some(source_map_report_from_summary(
        &test.display_name,
        &test.object_name,
        summary,
        test.sonatina_bytecode_origin_coverage,
        entries,
    ))
}

fn source_map_report_for_runtime_contract(
    contract: &str,
    output: &SonatinaContractBytecode,
    include_source_map_entries: bool,
) -> Option<AnalyzeSourceMapReport> {
    source_map_report_from_entries(
        "runtime_bytecode",
        contract.to_string(),
        contract,
        &output.source_map_entries,
        output.bytecode_origin_coverage,
        include_source_map_entries,
    )
}

fn source_map_report_from_summary(
    test: &str,
    fallback_object: &str,
    summary: &BytecodeSourceMapSummary,
    bytecode_origin_coverage: Option<BytecodeOriginCoverage>,
    entries: Vec<BytecodeSourceMapEntry>,
) -> AnalyzeSourceMapReport {
    AnalyzeSourceMapReport::from_summary(
        "test_bytecode",
        test.to_string(),
        Some(test.to_string()),
        summary.object().unwrap_or(fallback_object).to_string(),
        summary.section().unwrap_or("<all>").to_string(),
        summary,
        bytecode_origin_coverage,
        entries,
    )
}

fn source_map_report_from_entries(
    scope: &'static str,
    label: String,
    fallback_object: &str,
    all_entries: &[BytecodeSourceMapEntry],
    bytecode_origin_coverage: Option<BytecodeOriginCoverage>,
    include_entries: bool,
) -> Option<AnalyzeSourceMapReport> {
    let summary = bytecode_source_map_entries_summary(all_entries, None, None)?;

    Some(AnalyzeSourceMapReport::from_summary(
        scope,
        label,
        None,
        all_entries
            .first()
            .map(|entry| entry.object().to_string())
            .unwrap_or_else(|| fallback_object.to_string()),
        "<all>".to_string(),
        &summary,
        bytecode_origin_coverage,
        include_entries
            .then(|| all_entries.to_vec())
            .unwrap_or_default(),
    ))
}

fn summarize_const_region_shapes<'db>(
    db: &'db DriverDataBase,
    package: mir::RuntimePackage<'db>,
    include_facts: bool,
    include_relation_tables: bool,
) -> Vec<AnalyzeShapeReport> {
    package
        .const_regions(db)
        .into_iter()
        .enumerate()
        .map(|(idx, region)| {
            let data = region.data(db);
            let graph = data.value.shape_graph();
            shape_report_for_graph(
                "const_region",
                format!("const_region:{idx}"),
                graph,
                include_facts,
                include_relation_tables,
            )
        })
        .collect()
}

fn summarize_runtime_body_shapes<'db>(
    db: &'db DriverDataBase,
    package: mir::RuntimePackage<'db>,
    include_facts: bool,
    include_relation_tables: bool,
) -> Vec<AnalyzeShapeReport> {
    package
        .functions(db)
        .into_iter()
        .map(|function| {
            let body = function.instance(db).body(db);
            let graph = RuntimeBodyShape { body: &body }.shape_graph();
            shape_report_for_graph(
                "runtime_body",
                function.symbol(db),
                graph,
                include_facts,
                include_relation_tables,
            )
        })
        .collect()
}

fn shape_report_for_graph(
    scope: &'static str,
    label: String,
    graph: ShapeGraph,
    include_facts: bool,
    include_relation_tables: bool,
) -> AnalyzeShapeReport {
    let graph_hashes = graph.hashes().graph();
    let shape_nodes = graph.nodes().len();
    let shape_fields = graph.nodes().iter().map(|node| node.fields().len()).sum();
    let shape_children = graph.nodes().iter().map(|node| node.children().len()).sum();
    let shape_edges = graph.edges().len();
    let trace_events = graph
        .nodes()
        .iter()
        .flat_map(|node| node.fields())
        .filter(|field| field.dimension() == ShapeDimension::TraceEvents)
        .count();
    let data_flows = graph.edges().len();
    let facts = include_facts.then(|| shape_graph_facts(&graph));
    let relation_export = facts.as_ref().map(TypedFactSet::relation_export);
    let relation_counts = relation_export
        .as_ref()
        .map(|export| {
            let relation_index = relation_index_for_export(export);
            relation_counts_from_relation_index(&relation_index)
        })
        .unwrap_or_default();
    let relation_tables = include_relation_tables.then_some(relation_export).flatten();
    let facts = facts.map(|facts| facts.to_owned_export());

    AnalyzeShapeReport {
        scope,
        label,
        shape_nodes,
        shape_fields,
        shape_children,
        shape_edges,
        trace_events,
        data_flows,
        graph_hashes: ShapeDimension::ALL
            .into_iter()
            .map(|dimension| AnalyzeShapeHashReport {
                dimension: dimension.as_str(),
                digest_hex: graph_hashes.digest(dimension).to_hex(),
            })
            .collect(),
        relation_counts,
        facts,
        relation_tables,
    }
}

struct RuntimeBodyShape<'a, 'db> {
    body: &'a mir::RuntimeBody<'db>,
}

impl ShapeDescribe for RuntimeBodyShape<'_, '_> {
    fn describe_shape(&self, builder: &mut ShapeBuilder) -> ShapeNodeId {
        let node = builder.add_described_node("RuntimeBody", None);
        builder.add_field_value(
            node,
            ShapeDimension::Structure,
            "locals",
            &self.body.locals.len(),
        );
        builder.add_field_value(
            node,
            ShapeDimension::Structure,
            "blocks",
            &self.body.blocks.len(),
        );
        let statement_count = self
            .body
            .blocks
            .iter()
            .map(|block| block.stmts.len())
            .sum::<usize>();
        builder.add_field_value(
            node,
            ShapeDimension::Structure,
            "statements",
            &statement_count,
        );
        for (idx, block) in self.body.blocks.iter().enumerate() {
            builder.add_child_node(node, format!("block:{idx}"), block);
        }
        node
    }
}

fn origin_fact_reports_for_test(
    test: &TestMetadata,
    include_relation_tables: bool,
) -> Vec<AnalyzeOriginFactReport> {
    let mut reports = Vec::new();
    if let Some(facts) = test.sonatina_origin_facts.as_ref() {
        reports.push(origin_fact_report(
            "test_bytecode",
            test.display_name.clone(),
            Some(test.object_name.clone()),
            facts,
            include_relation_tables,
        ));
    }
    if let Some(facts) = test.sonatina_snapshot_origin_facts.as_ref() {
        reports.push(origin_fact_report(
            "test_sonatina_snapshot",
            test.display_name.clone(),
            Some(test.object_name.clone()),
            facts,
            include_relation_tables,
        ));
    }
    reports
}

fn origin_fact_reports_for_runtime_contract(
    contract: &str,
    output: &SonatinaContractBytecode,
    include_relation_tables: bool,
) -> Vec<AnalyzeOriginFactReport> {
    let mut reports = Vec::new();
    if let Some(facts) = output.origin_facts.as_ref() {
        reports.push(origin_fact_report(
            "runtime_bytecode",
            contract.to_string(),
            Some(contract.to_string()),
            facts,
            include_relation_tables,
        ));
    }
    if let Some(facts) = output.snapshot_origin_facts.as_ref() {
        reports.push(origin_fact_report(
            "runtime_sonatina_snapshot",
            contract.to_string(),
            Some(contract.to_string()),
            facts,
            include_relation_tables,
        ));
    }
    reports
}

fn runtime_origin_fact_report<'db>(
    label: &str,
    origins: &mir::RuntimePackageOrigins<'db>,
    include_relation_tables: bool,
) -> Option<AnalyzeOriginFactReport> {
    let target_key = RuntimeOriginFactTargetKey::new(label);
    let facts = runtime_package_origin_facts(origins, |body| {
        RuntimeOriginFactOwnerKeys::for_body(&target_key, body.symbol_key())
    });
    if facts.facts().is_empty() {
        return None;
    }
    Some(origin_fact_report(
        "runtime",
        label.to_string(),
        None,
        &facts,
        include_relation_tables,
    ))
}

fn origin_fact_report(
    scope: &'static str,
    label: String,
    object: Option<String>,
    facts: &TypedFactSet,
    include_relation_tables: bool,
) -> AnalyzeOriginFactReport {
    let relation_export = facts.relation_export();
    let relation_index = relation_index_for_export(&relation_export);
    let reachability = Some(
        relation_index
            .origin_reachability_summary()
            .expect("typed fact relation export should answer origin reachability"),
    );
    let relation_counts = relation_counts_from_relation_index(&relation_index);
    let source_span_files = source_span_file_counts_from_relation_index(&relation_index);

    let (path_witnesses, query_error) = match relation_index
        .representative_path_exports_with_priority(
            ORIGIN_PATH_WITNESS_PRIORITY.iter().copied(),
            ORIGIN_PATH_WITNESS_LIMIT,
        ) {
        Ok(path_witnesses) => (path_witnesses, None),
        Err(err) => (Vec::new(), Some(format!("{err:?}"))),
    };

    AnalyzeOriginFactReport {
        scope,
        label,
        object,
        total: facts.facts().len(),
        origin_nodes: facts.origin_nodes().count(),
        origin_links: facts.origin_links().count(),
        source_spans: facts.source_spans().count(),
        source_span_files,
        relation_counts,
        relation_tables: include_relation_tables.then_some(relation_export),
        reachability,
        path_witnesses,
        query_error,
        facts: facts.to_owned_export(),
    }
}

fn relation_index_for_export(export: &TypedFactRelationSet) -> TypedFactRelationIndex<'_> {
    TypedFactRelationIndex::new(export)
        .expect("typed fact relation export should build a query index")
}

fn relation_counts_from_relation_index(
    index: &TypedFactRelationIndex<'_>,
) -> Vec<AnalyzeFactRelationCount> {
    index
        .relation_counts()
        .expect("typed fact relation export should contain declared relations")
        .into_iter()
        .map(|count| AnalyzeFactRelationCount {
            relation: count.relation().to_string(),
            rows: count.rows(),
        })
        .collect()
}

fn source_span_file_counts_from_relation_index(
    index: &TypedFactRelationIndex<'_>,
) -> Vec<AnalyzeSourceSpanFileCount> {
    index
        .source_span_file_counts()
        .expect("typed fact relation export should contain source_span relation")
        .into_iter()
        .map(|count| AnalyzeSourceSpanFileCount {
            file: count.file().to_string(),
            spans: count.spans(),
        })
        .collect()
}

fn analyze_top_mod_diagnostics(db: &DriverDataBase, top_mod: TopLevelMod<'_>, label: &str) -> bool {
    let hir_diags = db.run_on_top_mod(top_mod);
    let mut has_errors = false;
    let hir_has_errors = hir_diags.has_errors(db);

    if !hir_diags.is_empty() {
        eprintln!("errors in {label}");
        eprintln!();
        hir_diags.emit(db);
        has_errors = true;
    }

    let mir_diags = if hir_has_errors {
        Vec::new()
    } else {
        db.mir_diagnostics_for_top_mod(top_mod)
    };
    if !mir_diags.is_empty() {
        if !has_errors {
            eprintln!("errors in {label}");
            eprintln!();
        }
        db.emit_complete_diagnostics(&mir_diags);
        has_errors = true;
    }

    has_errors
}

fn analyze_ingot_diagnostics(db: &DriverDataBase, ingot: Ingot<'_>, label: &str) -> bool {
    let hir_diags = db.run_on_ingot(ingot);
    let mut has_errors = false;
    let hir_has_errors = hir_diags.has_errors(db);

    if !hir_diags.is_empty() {
        eprintln!("errors in {label}");
        eprintln!();
        hir_diags.emit(db);
        has_errors = true;
    }

    let mir_diags = if hir_has_errors {
        Vec::new()
    } else {
        db.mir_diagnostics_for_ingot(ingot)
    };
    if !mir_diags.is_empty() {
        if !has_errors {
            eprintln!("errors in {label}");
            eprintln!();
        }
        db.emit_complete_diagnostics(&mir_diags);
        has_errors = true;
    }

    has_errors
}

fn summarize_runtime_origins(
    label: &str,
    origins: &mir::RuntimePackageOrigins<'_>,
) -> AnalyzeTargetReport {
    let mut runtime_statements = OriginCount::default();
    let mut runtime_terminators = OriginCount::default();
    let mut bodies = Vec::new();

    for body in origins.bodies() {
        let mut statements = OriginCount::default();
        for record in body.origins().stmt_origins() {
            statements.push(record.source());
        }
        let mut terminators = OriginCount::default();
        for record in body.origins().terminator_origins() {
            terminators.push(record.source());
        }
        runtime_statements.extend(statements);
        runtime_terminators.extend(terminators);
        bodies.push(AnalyzeBodyReport {
            symbol: body.symbol().to_string(),
            statements,
            terminators,
        });
    }

    bodies.sort_by(|left, right| left.symbol.cmp(&right.symbol));

    AnalyzeTargetReport {
        label: label.to_string(),
        runtime_bodies: bodies.len(),
        runtime_statements,
        runtime_terminators,
        bodies,
        source_maps: Vec::new(),
        origin_facts: Vec::new(),
        shapes: Vec::new(),
    }
}

fn render_report(report: &AnalyzeReport, format: AnalyzeFormat) -> Result<String, String> {
    match format {
        AnalyzeFormat::Text => Ok(render_text_report(report)),
        AnalyzeFormat::Json => serde_json::to_string_pretty(report)
            .map(|mut json| {
                json.push('\n');
                json
            })
            .map_err(|err| format!("failed to render analyze JSON: {err}")),
    }
}

fn render_text_report(report: &AnalyzeReport) -> String {
    let mut out = String::new();
    writeln!(out, "Fe Origin Analysis").unwrap();
    writeln!(out, "profile: {}", report.profile).unwrap();
    writeln!(out, "package_kind: {}", report.package_kind).unwrap();
    writeln!(out, "targets: {}", report.targets.len()).unwrap();
    writeln!(out).unwrap();

    for target in &report.targets {
        writeln!(out, "{}", target.label).unwrap();
        writeln!(out, "  runtime bodies: {}", target.runtime_bodies).unwrap();
        write_origin_count(&mut out, "  runtime statements", target.runtime_statements);
        write_origin_count(
            &mut out,
            "  runtime terminators",
            target.runtime_terminators,
        );
        if !target.bodies.is_empty() {
            writeln!(out, "  bodies:").unwrap();
            for body in &target.bodies {
                writeln!(out, "    {}", body.symbol).unwrap();
                write_origin_count(&mut out, "      statements", body.statements);
                write_origin_count(&mut out, "      terminators", body.terminators);
            }
        }
        if !target.source_maps.is_empty() {
            writeln!(out, "  source maps:").unwrap();
            for source_map in &target.source_maps {
                let bytecode_coverage = source_map
                    .bytecode_origin_coverage
                    .as_ref()
                    .map(|coverage| {
                        format!(
                            " bytecode_origins={} post_opt={} backend_prepared={} unmapped={}",
                            coverage.total(),
                            coverage.sonatina_post_opt(),
                            coverage.sonatina_backend_prepared(),
                            coverage.unmapped()
                        )
                    })
                    .unwrap_or_default();
                writeln!(
                    out,
                    "    {} {} {}:{} total={} source={} non_source={}{}",
                    source_map.scope,
                    source_map.label,
                    source_map.object,
                    source_map.section,
                    source_map.total,
                    source_map.source,
                    source_map.non_source,
                    bytecode_coverage,
                )
                .unwrap();
                write_source_map_breakdown(&mut out, source_map);
                if !source_map.entries.is_empty() {
                    writeln!(out, "      entries:").unwrap();
                    for entry in &source_map.entries {
                        write_source_map_entry(&mut out, entry);
                    }
                }
            }
        }
        if !target.origin_facts.is_empty() {
            writeln!(out, "  origin facts:").unwrap();
            for origin_facts in &target.origin_facts {
                let reachable_pairs = origin_facts
                    .reachability
                    .as_ref()
                    .map(|summary| format!(" reachable_pairs={}", summary.reachable_pairs()))
                    .unwrap_or_default();
                let path_witnesses = (!origin_facts.path_witnesses.is_empty())
                    .then(|| format!(" path_witnesses={}", origin_facts.path_witnesses.len()))
                    .unwrap_or_default();
                let query_error = origin_facts
                    .query_error
                    .as_ref()
                    .map(|err| format!(" query_error={err}"))
                    .unwrap_or_default();
                let relations = (!origin_facts.relation_counts.is_empty())
                    .then(|| format!(" relations={}", origin_facts.relation_counts.len()))
                    .unwrap_or_default();
                writeln!(
                    out,
                    "    {} {}{} total={} origin_nodes={} origin_links={} source_spans={}{}{}{}{}",
                    origin_facts.scope,
                    origin_facts.label,
                    origin_facts
                        .object
                        .as_ref()
                        .map(|object| format!(" object={object}"))
                        .unwrap_or_default(),
                    origin_facts.total,
                    origin_facts.origin_nodes,
                    origin_facts.origin_links,
                    origin_facts.source_spans,
                    reachable_pairs,
                    path_witnesses,
                    query_error,
                    relations,
                )
                .unwrap();
                if let Some(reachability) = origin_facts.reachability.as_ref()
                    && !reachability.reachable_pairs_by_kind().is_empty()
                {
                    write_origin_reachability_pairs(&mut out, reachability);
                }
                if !origin_facts.relation_counts.is_empty() {
                    write_relation_counts(&mut out, &origin_facts.relation_counts);
                }
                if !origin_facts.source_span_files.is_empty() {
                    write_source_span_file_counts(&mut out, &origin_facts.source_span_files);
                }
                if !origin_facts.path_witnesses.is_empty() {
                    writeln!(out, "      paths:").unwrap();
                    for witness in &origin_facts.path_witnesses {
                        write_origin_path_witness(&mut out, witness);
                    }
                }
            }
        }
        if !target.shapes.is_empty() {
            writeln!(out, "  shapes:").unwrap();
            for shape in &target.shapes {
                let relations = (!shape.relation_counts.is_empty())
                    .then(|| format!(" relations={}", shape.relation_counts.len()))
                    .unwrap_or_default();
                let graph_hashes = shape
                    .graph_hashes
                    .iter()
                    .map(|hash| format!("{}={}", hash.dimension, hash.digest_hex))
                    .collect::<Vec<_>>()
                    .join(" ");
                writeln!(
                    out,
                    "    {} {} nodes={} fields={} children={} edges={} trace_events={} data_flows={}{} graph_hashes=[{}]",
                    shape.scope,
                    shape.label,
                    shape.shape_nodes,
                    shape.shape_fields,
                    shape.shape_children,
                    shape.shape_edges,
                    shape.trace_events,
                    shape.data_flows,
                    relations,
                    graph_hashes,
                )
                .unwrap();
                if !shape.relation_counts.is_empty() {
                    write_relation_counts(&mut out, &shape.relation_counts);
                }
            }
        }
        writeln!(out).unwrap();
    }

    out
}

fn write_source_map_breakdown(out: &mut String, source_map: &AnalyzeSourceMapReport) {
    writeln!(
        out,
        "      classification: source={} source_span_invalid={} semantic_span_missing={} runtime_stmt_missing={} runtime_terminator_missing={} runtime_synthetic={} sonatina_synthetic={} sonatina_unmapped={} post_preopt_snapshot_gap={} bytecode_unmapped={}",
        source_map.source,
        source_map.source_span_invalid,
        source_map.semantic_span_missing,
        source_map.runtime_stmt_missing,
        source_map.runtime_terminator_missing,
        source_map.runtime_synthetic,
        source_map.sonatina_synthetic,
        source_map.sonatina_unmapped,
        source_map.post_preopt_snapshot_gap,
        source_map.bytecode_unmapped,
    )
    .unwrap();
}

fn write_source_map_entry(out: &mut String, entry: &BytecodeSourceMapEntry) {
    write!(
        out,
        "        {}:{} {}..{} kind={}",
        entry.object(),
        entry.section(),
        entry.pc_start(),
        entry.pc_end(),
        entry.kind().kind_name(),
    )
    .unwrap();

    match entry.kind() {
        BytecodeSourceMapEntryKind::Source {
            span_kind,
            file,
            start_byte,
            end_byte,
            start_line,
            start_col,
            end_line,
            end_col,
            snippet,
        } => {
            write!(
                out,
                " span_kind={} file={file:?} bytes={}..{} lines={}:{}..{}:{} snippet={:?}",
                (*span_kind).as_str(),
                start_byte,
                end_byte,
                start_line,
                start_col,
                end_line,
                end_col,
                compact_source_snippet(snippet),
            )
            .unwrap();
        }
        kind => {
            if let Some(reason) = kind.reason() {
                write!(out, " reason={reason}").unwrap();
            }
        }
    }

    writeln!(out).unwrap();
}

fn compact_source_snippet(snippet: &str) -> String {
    const MAX_SNIPPET_CHARS: usize = 80;

    let mut compact = snippet.split_whitespace().collect::<Vec<_>>().join(" ");
    if compact.chars().count() > MAX_SNIPPET_CHARS {
        compact = compact.chars().take(MAX_SNIPPET_CHARS - 3).collect();
        compact.push_str("...");
    }
    compact
}

fn write_origin_reachability_pairs(out: &mut String, summary: &OriginReachabilitySummary) {
    let pairs = summary
        .reachable_pairs_by_kind()
        .iter()
        .map(|pair| {
            format!(
                "{}->{}={}",
                pair.from_kind().as_str(),
                pair.to_kind().as_str(),
                pair.reachable_pairs()
            )
        })
        .collect::<Vec<_>>()
        .join(" ");
    writeln!(out, "      reachable kind pairs: {pairs}").unwrap();
}

fn write_relation_counts(out: &mut String, counts: &[AnalyzeFactRelationCount]) {
    let relation_counts = counts
        .iter()
        .map(|count| format!("{}={}", count.relation, count.rows))
        .collect::<Vec<_>>()
        .join(" ");
    writeln!(out, "      relation counts: {relation_counts}").unwrap();
}

fn write_source_span_file_counts(out: &mut String, counts: &[AnalyzeSourceSpanFileCount]) {
    let source_span_files = counts
        .iter()
        .map(|count| format!("{:?}={}", count.file, count.spans))
        .collect::<Vec<_>>()
        .join(" ");
    writeln!(out, "      source span files: {source_span_files}").unwrap();
}

fn write_origin_path_witness(out: &mut String, witness: &OriginPathWitnessExport) {
    writeln!(
        out,
        "        {} -> {}:",
        witness.from_kind().as_str(),
        witness.to_kind().as_str()
    )
    .unwrap();
    let Some((first, rest)) = witness.nodes().split_first() else {
        writeln!(out, "          <empty>").unwrap();
        return;
    };

    write!(out, "          {}", first.display_label()).unwrap();
    for (idx, node) in rest.iter().enumerate() {
        let link = witness
            .links()
            .get(idx)
            .map(|kind| kind.as_str())
            .unwrap_or("<missing-link>");
        write!(out, " --{}--> {}", link, node.display_label()).unwrap();
    }
    writeln!(out).unwrap();
}

fn write_origin_count(out: &mut String, label: &str, count: OriginCount) {
    writeln!(
        out,
        "{label}: {} (semantic {}, synthetic {})",
        count.total, count.semantic, count.synthetic
    )
    .unwrap();
}

fn ingot_has_source_files(db: &DriverDataBase, ingot: Ingot<'_>) -> bool {
    ingot
        .files(db)
        .iter()
        .any(|(_, file)| matches!(file.kind(db), Some(IngotFileKind::Source)))
}

fn ingot_label(db: &DriverDataBase, ingot: Ingot<'_>, fallback: &Url) -> String {
    ingot
        .config(db)
        .and_then(|config| config.metadata.name)
        .map(|name| name.to_string())
        .unwrap_or_else(|| fallback.to_string())
}

fn config_from_db(db: &DriverDataBase, ingot_url: &Url) -> Result<Option<Config>, String> {
    let config_url = ingot_url
        .join("fe.toml")
        .map_err(|_| format!("Failed to locate fe.toml for {ingot_url}"))?;
    let Some(file) = db.workspace().get(db, &config_url) else {
        return Ok(None);
    };
    let config = Config::parse(file.text(db))
        .map_err(|err| format!("Failed to parse {config_url}: {err}"))?;
    Ok(Some(config))
}

fn dir_url(path: &Utf8PathBuf) -> Result<Url, String> {
    let canonical_path = match path.canonicalize_utf8() {
        Ok(path) => path,
        Err(_) => {
            let cwd = std::env::current_dir()
                .map_err(|err| format!("Failed to read current directory: {err}"))?;
            let cwd = Utf8PathBuf::from_path_buf(cwd)
                .map_err(|_| "Current directory is not valid UTF-8".to_string())?;
            cwd.join(path)
        }
    };
    Url::from_directory_path(canonical_path.as_str())
        .map_err(|_| format!("Error: invalid or non-existent directory path: {path}"))
}

#[cfg(test)]
mod tests {
    use std::fs;

    use camino::Utf8PathBuf;
    use codegen::OptLevel;
    use serde_json::Value;
    use tempfile::tempdir;

    use super::{AnalyzeOptions, analyze_to_string};
    use crate::AnalyzeFormat;

    fn json_options(
        include_tests: bool,
        include_source_maps: bool,
        include_source_map_entries: bool,
        include_origin_facts: bool,
        include_shape_hashes: bool,
        include_shape_facts: bool,
    ) -> AnalyzeOptions<'static> {
        AnalyzeOptions::new(
            "dev",
            AnalyzeFormat::Json,
            include_tests,
            include_source_maps,
            include_source_map_entries,
            include_origin_facts,
            false,
            include_shape_hashes,
            include_shape_facts,
            OptLevel::O0,
            false,
        )
    }

    fn has_reachable_kind_pair(report: &Value, from_kind: &str, to_kind: &str) -> bool {
        report["reachability"]["reachable_pairs_by_kind"]
            .as_array()
            .is_some_and(|pairs| {
                pairs.iter().any(|pair| {
                    pair["from_kind"] == from_kind
                        && pair["to_kind"] == to_kind
                        && pair["reachable_pairs"]
                            .as_u64()
                            .is_some_and(|count| count > 0)
                })
            })
    }

    fn has_path_witness(report: &Value, from_kind: &str, to_kind: &str) -> bool {
        report["path_witnesses"]
            .as_array()
            .is_some_and(|witnesses| {
                witnesses.iter().any(|witness| {
                    witness["from_kind"] == from_kind
                        && witness["to_kind"] == to_kind
                        && witness["links"]
                            .as_array()
                            .is_some_and(|links| !links.is_empty())
                        && witness["nodes"].as_array().is_some_and(|nodes| {
                            nodes.len() >= 2
                                && nodes.first().is_some_and(|node| node["kind"] == from_kind)
                                && nodes.last().is_some_and(|node| node["kind"] == to_kind)
                        })
                })
            })
    }

    fn has_relation_count(report: &Value, relation: &str) -> bool {
        report["relation_counts"].as_array().is_some_and(|counts| {
            counts.iter().any(|count| {
                count["relation"] == relation && count["rows"].as_u64().is_some_and(|rows| rows > 0)
            })
        })
    }

    fn has_relation_table(report: &Value, relation: &str) -> bool {
        report["relation_tables"]["relations"]
            .as_array()
            .is_some_and(|relations| {
                relations.iter().any(|table| {
                    table["name"] == relation
                        && table["rows"]
                            .as_array()
                            .is_some_and(|rows| !rows.is_empty())
                })
            })
    }

    fn has_source_span_file_count(report: &Value) -> bool {
        report["source_span_files"].as_array().is_some_and(|files| {
            files.iter().any(|file| {
                file["file"].as_str().is_some_and(|path| !path.is_empty())
                    && file["spans"].as_u64().is_some_and(|spans| spans > 0)
            })
        })
    }

    fn has_partitioned_bytecode_origin_coverage(report: &Value) -> bool {
        let coverage = &report["bytecode_origin_coverage"];
        let Some(total) = coverage["total"].as_u64() else {
            return false;
        };
        total > 0
            && coverage["sonatina_post_opt"].as_u64().unwrap_or_default()
                + coverage["sonatina_backend_prepared"]
                    .as_u64()
                    .unwrap_or_default()
                + coverage["unmapped"].as_u64().unwrap_or_default()
                == total
    }

    #[test]
    fn analyze_fact_relation_tables_validate_option_dependencies() {
        let missing_facts = AnalyzeOptions::new(
            "dev",
            AnalyzeFormat::Json,
            false,
            false,
            false,
            false,
            true,
            false,
            false,
            OptLevel::O0,
            false,
        )
        .validate()
        .expect_err("relation tables require emitted typed facts");
        assert!(
            missing_facts.contains("requires `--origin-facts` or `--shape-facts`"),
            "{missing_facts}"
        );

        let text_format = AnalyzeOptions::new(
            "dev",
            AnalyzeFormat::Text,
            false,
            false,
            false,
            true,
            true,
            false,
            false,
            OptLevel::O0,
            false,
        )
        .validate()
        .expect_err("relation tables are JSON-only");
        assert!(
            text_format.contains("requires `--format json`"),
            "{text_format}"
        );
    }

    #[test]
    fn analyze_standalone_file_reports_runtime_origin_summary_json() {
        let temp = tempdir().expect("tempdir");
        let file_path = Utf8PathBuf::from_path_buf(temp.path().join("sample.fe")).unwrap();
        fs::write(
            file_path.as_std_path(),
            r#"
fn sample() -> u256 {
    1
}
"#,
        )
        .expect("write fixture");

        let outcome = analyze_to_string(
            &file_path,
            None,
            true,
            json_options(false, false, false, false, false, false),
        )
        .expect("analyze succeeds");

        assert!(!outcome.has_errors);
        let value = serde_json::from_str::<Value>(&outcome.output).expect("valid JSON");
        assert_eq!(value["schema_version"], 1);
        assert_eq!(value["targets"][0]["label"], file_path.as_str());
        assert!(
            value["targets"][0]["runtime_statements"]["total"]
                .as_u64()
                .is_some_and(|total| total > 0),
            "expected runtime statement origins in {value:#?}"
        );
    }

    #[test]
    fn analyze_origin_facts_reports_runtime_origin_facts_without_tests() {
        let temp = tempdir().expect("tempdir");
        let file_path = Utf8PathBuf::from_path_buf(temp.path().join("sample.fe")).unwrap();
        fs::write(
            file_path.as_std_path(),
            r#"
fn sample() -> u256 {
    let x: u256 = 1
    x
}
"#,
        )
        .expect("write fixture");

        let outcome = analyze_to_string(
            &file_path,
            None,
            true,
            json_options(false, false, false, true, false, false),
        )
        .expect("analyze succeeds");

        assert!(!outcome.has_errors);
        let value = serde_json::from_str::<Value>(&outcome.output).expect("valid JSON");
        let origin_facts = value["targets"][0]["origin_facts"]
            .as_array()
            .expect("origin facts should be an array");
        assert!(
            origin_facts.iter().any(|report| {
                report["scope"] == "runtime"
                    && report["origin_nodes"]
                        .as_u64()
                        .is_some_and(|count| count > 0)
                    && report["origin_links"]
                        .as_u64()
                        .is_some_and(|count| count > 0)
                    && report["facts"]["schema_version"] == 1
                    && report["reachability"]["reachable_pairs"]
                        .as_u64()
                        .is_some_and(|count| count > 0)
                    && (has_reachable_kind_pair(report, "semantic", "runtime.stmt")
                        || has_reachable_kind_pair(report, "semantic", "runtime.terminator"))
                    && (has_path_witness(report, "semantic", "runtime.stmt")
                        || has_path_witness(report, "semantic", "runtime.terminator"))
                    && has_relation_count(report, "origin_node")
                    && has_relation_count(report, "origin_link")
                    && report["facts"]["facts"].as_array().is_some_and(|facts| {
                        facts.iter().any(|fact| {
                            fact["type"] == "origin_node"
                                && (fact["key"]["kind"] == "runtime.stmt"
                                    || fact["key"]["kind"] == "runtime.terminator")
                        }) && facts
                            .iter()
                            .any(|fact| fact["type"] == "origin_link" && fact["kind"] == "lowered")
                    })
            }),
            "expected typed runtime origin facts in {value:#?}"
        );
    }

    #[test]
    fn analyze_origin_facts_text_renders_path_witnesses() {
        let temp = tempdir().expect("tempdir");
        let file_path = Utf8PathBuf::from_path_buf(temp.path().join("sample.fe")).unwrap();
        fs::write(
            file_path.as_std_path(),
            r#"
fn sample() -> u256 {
    let x: u256 = 1
    x
}
"#,
        )
        .expect("write fixture");

        let outcome = analyze_to_string(
            &file_path,
            None,
            true,
            AnalyzeOptions::new(
                "dev",
                AnalyzeFormat::Text,
                false,
                false,
                false,
                true,
                false,
                false,
                false,
                OptLevel::O0,
                false,
            ),
        )
        .expect("analyze succeeds");

        assert!(!outcome.has_errors);
        assert!(
            outcome.output.contains("      paths:\n"),
            "{}",
            outcome.output
        );
        assert!(
            outcome.output.contains("semantic -> runtime."),
            "{}",
            outcome.output
        );
        assert!(
            outcome.output.contains("--lowered-->"),
            "{}",
            outcome.output
        );
        assert!(
            outcome.output.contains("      relation counts:"),
            "{}",
            outcome.output
        );
        assert!(
            outcome.output.contains("origin_node="),
            "{}",
            outcome.output
        );
        assert!(
            outcome.output.contains("origin_link="),
            "{}",
            outcome.output
        );
        assert!(
            outcome.output.contains("      reachable kind pairs:"),
            "{}",
            outcome.output
        );
        assert!(
            outcome.output.contains("semantic->runtime."),
            "{}",
            outcome.output
        );
    }

    #[test]
    fn analyze_origin_fact_relation_tables_reports_engine_agnostic_rows() {
        let temp = tempdir().expect("tempdir");
        let file_path = Utf8PathBuf::from_path_buf(temp.path().join("sample.fe")).unwrap();
        fs::write(
            file_path.as_std_path(),
            r#"
fn sample() -> u256 {
    let x: u256 = 1
    x
}
"#,
        )
        .expect("write fixture");

        let outcome = analyze_to_string(
            &file_path,
            None,
            true,
            AnalyzeOptions::new(
                "dev",
                AnalyzeFormat::Json,
                false,
                false,
                false,
                true,
                true,
                false,
                false,
                OptLevel::O0,
                false,
            ),
        )
        .expect("analyze succeeds");

        assert!(!outcome.has_errors);
        let value = serde_json::from_str::<Value>(&outcome.output).expect("valid JSON");
        let origin_facts = value["targets"][0]["origin_facts"]
            .as_array()
            .expect("origin facts should be an array");
        assert!(
            origin_facts.iter().any(|report| {
                report["scope"] == "runtime"
                    && report["relation_tables"]["schema_version"] == 1
                    && has_relation_table(report, "origin_node")
                    && has_relation_table(report, "origin_link")
            }),
            "expected runtime origin relation tables in {value:#?}"
        );
    }

    #[test]
    fn analyze_shape_hashes_reports_const_region_shapes() {
        let temp = tempdir().expect("tempdir");
        let file_path = Utf8PathBuf::from_path_buf(temp.path().join("sample.fe")).unwrap();
        fs::write(
            file_path.as_std_path(),
            r#"
const C: [u256; 3] = [10, 20, 30]

fn sample() -> u256 {
    C[1]
}
"#,
        )
        .expect("write fixture");

        let outcome = analyze_to_string(
            &file_path,
            None,
            true,
            json_options(false, false, false, false, true, true),
        )
        .expect("analyze succeeds");

        assert!(!outcome.has_errors);
        let value = serde_json::from_str::<Value>(&outcome.output).expect("valid JSON");
        let shapes = value["targets"][0]["shapes"]
            .as_array()
            .expect("shapes should be an array");
        assert!(
            shapes.iter().any(|shape| {
                shape["scope"] == "const_region"
                    && shape["shape_nodes"].as_u64().is_some_and(|count| count > 0)
                    && shape["trace_events"].as_u64().is_some()
                    && shape["data_flows"].as_u64().is_some()
                    && shape["graph_hashes"].as_array().is_some_and(|hashes| {
                        hashes.iter().any(|hash| hash["dimension"] == "constants")
                            && hashes.iter().any(|hash| hash["dimension"] == "types")
                    })
                    && shape["facts"]["schema_version"] == 1
                    && has_relation_count(shape, "shape_node")
                    && has_relation_count(shape, "shape_hash")
                    && shape["facts"]["facts"].as_array().is_some_and(|facts| {
                        facts.iter().any(|fact| fact["type"] == "shape_node")
                            && facts.iter().any(|fact| fact["type"] == "shape_hash")
                    })
            }),
            "expected const-region shape hashes and facts in {value:#?}"
        );
        assert!(
            shapes.iter().any(|shape| {
                shape["scope"] == "runtime_body"
                    && shape["shape_nodes"].as_u64().is_some_and(|count| count > 0)
                    && shape["trace_events"].as_u64().is_some()
                    && shape["data_flows"].as_u64().is_some()
                    && shape["graph_hashes"].as_array().is_some_and(|hashes| {
                        hashes.iter().any(|hash| hash["dimension"] == "structure")
                            && hashes.iter().any(|hash| hash["dimension"] == "constants")
                    })
                    && shape["facts"]["schema_version"] == 1
                    && has_relation_count(shape, "shape_node")
                    && has_relation_count(shape, "shape_hash")
                    && shape["facts"]["facts"].as_array().is_some_and(|facts| {
                        facts.iter().any(|fact| fact["type"] == "shape_node")
                            && facts.iter().any(|fact| fact["type"] == "shape_hash")
                    })
            }),
            "expected runtime body shape hashes and facts in {value:#?}"
        );
    }

    #[test]
    fn analyze_shape_hashes_text_renders_all_graph_dimensions() {
        let temp = tempdir().expect("tempdir");
        let file_path = Utf8PathBuf::from_path_buf(temp.path().join("sample.fe")).unwrap();
        fs::write(
            file_path.as_std_path(),
            r#"
const C: [u256; 3] = [10, 20, 30]

fn sample() -> u256 {
    C[1]
}
"#,
        )
        .expect("write fixture");

        let outcome = analyze_to_string(
            &file_path,
            None,
            true,
            AnalyzeOptions::new(
                "dev",
                AnalyzeFormat::Text,
                false,
                false,
                false,
                false,
                false,
                true,
                true,
                OptLevel::O0,
                false,
            ),
        )
        .expect("analyze succeeds");

        assert!(!outcome.has_errors);
        for dimension in ["structure", "names", "constants", "types", "trace_events"] {
            assert!(
                outcome.output.contains(&format!("{dimension}=")),
                "{}",
                outcome.output
            );
        }
        assert!(
            outcome.output.contains("      relation counts:"),
            "{}",
            outcome.output
        );
        assert!(outcome.output.contains("shape_node="), "{}", outcome.output);
        assert!(outcome.output.contains("shape_hash="), "{}", outcome.output);
    }

    #[test]
    fn analyze_shape_fact_relation_tables_reports_engine_agnostic_rows() {
        let temp = tempdir().expect("tempdir");
        let file_path = Utf8PathBuf::from_path_buf(temp.path().join("sample.fe")).unwrap();
        fs::write(
            file_path.as_std_path(),
            r#"
const C: [u256; 3] = [10, 20, 30]

fn sample() -> u256 {
    C[1]
}
"#,
        )
        .expect("write fixture");

        let outcome = analyze_to_string(
            &file_path,
            None,
            true,
            AnalyzeOptions::new(
                "dev",
                AnalyzeFormat::Json,
                false,
                false,
                false,
                false,
                true,
                true,
                true,
                OptLevel::O0,
                false,
            ),
        )
        .expect("analyze succeeds");

        assert!(!outcome.has_errors);
        let value = serde_json::from_str::<Value>(&outcome.output).expect("valid JSON");
        let shapes = value["targets"][0]["shapes"]
            .as_array()
            .expect("shapes should be an array");
        assert!(
            shapes.iter().any(|shape| {
                shape["scope"] == "const_region"
                    && shape["relation_tables"]["schema_version"] == 1
                    && has_relation_count(shape, "shape_node")
                    && has_relation_count(shape, "shape_hash")
                    && has_relation_table(shape, "shape_node")
                    && has_relation_table(shape, "shape_hash")
            }),
            "expected shape relation tables in {value:#?}"
        );
    }

    #[test]
    fn analyze_file_inside_ingot_uses_ingot_context_by_default() {
        let temp = tempdir().expect("tempdir");
        let root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
        let src = root.join("src");
        fs::create_dir_all(src.as_std_path()).expect("create src");
        fs::write(
            root.join("fe.toml").as_std_path(),
            "[ingot]\nname = \"analyze_app\"\nversion = \"0.1.0\"\n",
        )
        .expect("write config");
        let file_path = src.join("lib.fe");
        fs::write(
            file_path.as_std_path(),
            r#"
fn sample() -> u256 {
    1
}
"#,
        )
        .expect("write source");

        let outcome = analyze_to_string(
            &file_path,
            None,
            false,
            json_options(false, false, false, false, false, false),
        )
        .expect("analyze succeeds");

        assert!(!outcome.has_errors);
        let value = serde_json::from_str::<Value>(&outcome.output).expect("valid JSON");
        assert_eq!(value["targets"][0]["label"], "analyze_app");
    }

    #[test]
    fn analyze_tests_mode_reports_test_runtime_origin_summary() {
        let temp = tempdir().expect("tempdir");
        let root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
        let src = root.join("src");
        fs::create_dir_all(src.as_std_path()).expect("create src");
        fs::write(
            root.join("fe.toml").as_std_path(),
            "[ingot]\nname = \"analyze_tests_app\"\nversion = \"0.1.0\"\n",
        )
        .expect("write config");
        let file_path = src.join("lib.fe");
        fs::write(
            file_path.as_std_path(),
            r#"
#[test]
fn test_sample() {
    let x: u256 = 1
}
"#,
        )
        .expect("write source");

        let outcome = analyze_to_string(
            &file_path,
            None,
            false,
            json_options(true, false, false, false, false, false),
        )
        .expect("analyze succeeds");

        assert!(!outcome.has_errors);
        let value = serde_json::from_str::<Value>(&outcome.output).expect("valid JSON");
        assert_eq!(value["package_kind"], "tests");
        assert!(
            value["targets"][0]["runtime_statements"]["total"]
                .as_u64()
                .is_some_and(|total| total > 0),
            "expected test runtime statement origins in {value:#?}"
        );
    }

    #[test]
    fn analyze_source_maps_reports_typed_test_bytecode_summary() {
        let temp = tempdir().expect("tempdir");
        let root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
        let src = root.join("src");
        fs::create_dir_all(src.as_std_path()).expect("create src");
        fs::write(
            root.join("fe.toml").as_std_path(),
            "[ingot]\nname = \"analyze_source_maps_app\"\nversion = \"0.1.0\"\n",
        )
        .expect("write config");
        let file_path = src.join("lib.fe");
        fs::write(
            file_path.as_std_path(),
            r#"
#[test]
fn test_source_map() {
    let x: u256 = 1
    let y: u256 = x + 2
}
"#,
        )
        .expect("write source");

        let outcome = analyze_to_string(
            &file_path,
            None,
            false,
            json_options(true, true, true, false, false, false),
        )
        .expect("analyze succeeds");

        assert!(!outcome.has_errors);
        let value = serde_json::from_str::<Value>(&outcome.output).expect("valid JSON");
        let source_maps = value["targets"][0]["source_maps"]
            .as_array()
            .expect("source maps should be an array");
        assert!(
            source_maps.iter().any(|source_map| {
                source_map["total"].as_u64().is_some_and(|total| total > 0)
                    && source_map["scope"] == "test_bytecode"
                    && source_map["label"] == "test_source_map"
                    && source_map["source"]
                        .as_u64()
                        .is_some_and(|source| source > 0)
                    && has_partitioned_bytecode_origin_coverage(source_map)
                    && source_map["entries"].as_array().is_some_and(|entries| {
                        entries.iter().any(|entry| {
                            entry["kind"] == "source"
                                && entry.get("pc_start").is_some()
                                && entry.get("pc_end").is_some()
                                && entry.get("file").is_some()
                                && entry.get("start_byte").is_some()
                                && entry["snippet"]
                                    .as_str()
                                    .is_some_and(|snippet| !snippet.trim().is_empty())
                        })
                    })
                    && source_map["test"] == "test_source_map"
            }),
            "expected source-map summary with full source entries in {value:#?}"
        );
    }

    #[test]
    fn analyze_source_maps_reports_runtime_bytecode_summary_without_tests() {
        let temp = tempdir().expect("tempdir");
        let file_path = Utf8PathBuf::from_path_buf(temp.path().join("sample.fe")).unwrap();
        fs::write(
            file_path.as_std_path(),
            r#"
msg FooMsg {
    #[selector = 0x12345678]
    Ping -> u256,
}

pub contract Foo {
    recv FooMsg {
        Ping -> u256 {
            let x: u256 = 1
            return x + 2
        }
    }
}
"#,
        )
        .expect("write fixture");

        let outcome = analyze_to_string(
            &file_path,
            None,
            true,
            json_options(false, true, true, true, false, false),
        )
        .expect("analyze succeeds");

        assert!(!outcome.has_errors);
        let value = serde_json::from_str::<Value>(&outcome.output).expect("valid JSON");
        let source_maps = value["targets"][0]["source_maps"]
            .as_array()
            .expect("source maps should be an array");
        assert!(
            source_maps.iter().any(|source_map| {
                source_map["scope"] == "runtime_bytecode"
                    && source_map["label"] == "Foo"
                    && source_map["object"] == "Foo"
                    && source_map["total"].as_u64().is_some_and(|total| total > 0)
                    && has_partitioned_bytecode_origin_coverage(source_map)
                    && source_map["entries"].as_array().is_some_and(|entries| {
                        entries.iter().any(|entry| {
                            entry["kind"] == "source"
                                && entry.get("pc_start").is_some()
                                && entry.get("pc_end").is_some()
                                && entry["snippet"]
                                    .as_str()
                                    .is_some_and(|snippet| !snippet.trim().is_empty())
                        })
                    })
            }),
            "expected runtime bytecode source-map report in {value:#?}"
        );

        let origin_facts = value["targets"][0]["origin_facts"]
            .as_array()
            .expect("origin facts should be an array");
        assert!(
            origin_facts.iter().any(|report| {
                report["scope"] == "runtime_bytecode"
                    && report["label"] == "Foo"
                    && report["object"] == "Foo"
                    && has_path_witness(report, "semantic", "bytecode.pc")
                    && (has_reachable_kind_pair(report, "runtime.stmt", "bytecode.pc")
                        || has_reachable_kind_pair(report, "runtime.terminator", "bytecode.pc"))
                    && (has_path_witness(report, "runtime.stmt", "bytecode.pc")
                        || has_path_witness(report, "runtime.terminator", "bytecode.pc"))
                    && has_source_span_file_count(report)
                    && report["facts"]["schema_version"] == 1
            }),
            "expected runtime bytecode origin facts in {value:#?}"
        );
        assert!(
            origin_facts.iter().any(|report| {
                report["scope"] == "runtime_sonatina_snapshot"
                    && report["label"] == "Foo"
                    && report["object"] == "Foo"
                    && report["facts"]["schema_version"] == 1
            }),
            "expected runtime Sonatina snapshot origin facts in {value:#?}"
        );
    }

    #[test]
    fn analyze_source_maps_text_renders_classification_and_entries() {
        let temp = tempdir().expect("tempdir");
        let file_path = Utf8PathBuf::from_path_buf(temp.path().join("sample.fe")).unwrap();
        fs::write(
            file_path.as_std_path(),
            r#"
msg FooMsg {
    #[selector = 0x12345678]
    Ping -> u256,
}

pub contract Foo {
    recv FooMsg {
        Ping -> u256 {
            let x: u256 = 1
            return x + 2
        }
    }
}
"#,
        )
        .expect("write fixture");

        let outcome = analyze_to_string(
            &file_path,
            None,
            true,
            AnalyzeOptions::new(
                "dev",
                AnalyzeFormat::Text,
                false,
                true,
                true,
                true,
                false,
                false,
                false,
                OptLevel::O0,
                false,
            ),
        )
        .expect("analyze succeeds");

        assert!(!outcome.has_errors);
        for expected in [
            "  source maps:\n",
            "classification: source=",
            "source_span_invalid=",
            "semantic_span_missing=",
            "runtime_stmt_missing=",
            "runtime_terminator_missing=",
            "sonatina_unmapped=",
            "      entries:\n",
            "kind=source",
            "snippet=",
            "      source span files:",
        ] {
            assert!(outcome.output.contains(expected), "{}", outcome.output);
        }
    }

    #[test]
    fn analyze_origin_facts_reports_typed_test_bytecode_facts() {
        let temp = tempdir().expect("tempdir");
        let root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
        let src = root.join("src");
        fs::create_dir_all(src.as_std_path()).expect("create src");
        fs::write(
            root.join("fe.toml").as_std_path(),
            "[ingot]\nname = \"analyze_origin_facts_app\"\nversion = \"0.1.0\"\n",
        )
        .expect("write config");
        let file_path = src.join("lib.fe");
        fs::write(
            file_path.as_std_path(),
            r#"
#[test]
fn test_origin_facts() {
    let x: u256 = 1
    let y: u256 = x + 2
}
"#,
        )
        .expect("write source");

        let outcome = analyze_to_string(
            &file_path,
            None,
            false,
            json_options(true, false, false, true, false, false),
        )
        .expect("analyze succeeds");

        assert!(!outcome.has_errors);
        let value = serde_json::from_str::<Value>(&outcome.output).expect("valid JSON");
        let origin_facts = value["targets"][0]["origin_facts"]
            .as_array()
            .expect("origin facts should be an array");
        assert!(
            origin_facts.iter().any(|report| {
                report["scope"] == "runtime"
                    && report["origin_links"]
                        .as_u64()
                        .is_some_and(|count| count > 0)
            }),
            "expected runtime origin facts alongside test bytecode facts in {value:#?}"
        );
        assert!(
            origin_facts.iter().any(|report| {
                report["scope"] == "test_bytecode"
                    && report["label"] == "test_origin_facts"
                    && report["origin_nodes"]
                        .as_u64()
                        .is_some_and(|count| count > 0)
                    && report["origin_links"]
                        .as_u64()
                        .is_some_and(|count| count > 0)
                    && report["source_spans"]
                        .as_u64()
                        .is_some_and(|count| count > 0)
                    && has_source_span_file_count(report)
                    && has_relation_count(report, "origin_node")
                    && has_relation_count(report, "origin_link")
                    && has_relation_count(report, "source_span")
                    && (has_reachable_kind_pair(report, "runtime.stmt", "bytecode.pc")
                        || has_reachable_kind_pair(report, "runtime.terminator", "bytecode.pc"))
                    && (has_path_witness(report, "runtime.stmt", "bytecode.pc")
                        || has_path_witness(report, "runtime.terminator", "bytecode.pc"))
                    && report["facts"]["schema_version"] == 1
                    && report["facts"]["facts"].as_array().is_some_and(|facts| {
                        facts.iter().any(|fact| {
                            fact["type"] == "origin_node" && fact["key"]["kind"] == "bytecode.pc"
                        }) && facts.iter().any(|fact| {
                            fact["type"] == "origin_link"
                                && (fact["kind"] == "lowered" || fact["kind"] == "transformed")
                        }) && facts.iter().any(|fact| fact["type"] == "source_span")
                    })
            }),
            "expected typed origin facts for test bytecode in {value:#?}"
        );
        assert!(
            origin_facts.iter().any(|report| {
                report["scope"] == "test_sonatina_snapshot"
                    && report["label"] == "test_origin_facts"
                    && report["object"] == "test_origin_facts"
                    && report["origin_nodes"]
                        .as_u64()
                        .is_some_and(|count| count > 0)
                    && report["origin_links"]
                        .as_u64()
                        .is_some_and(|count| count > 0)
                    && report["facts"]["schema_version"] == 1
                    && report["facts"]["facts"].as_array().is_some_and(|facts| {
                        facts.iter().any(|fact| {
                            fact["type"] == "origin_node" && fact["key"]["kind"] == "sonatina.inst"
                        }) && facts
                            .iter()
                            .any(|fact| fact["type"] == "origin_link" && fact["kind"] == "alias")
                    })
            }),
            "expected typed Sonatina snapshot origin facts for test bytecode in {value:#?}"
        );
    }
}
