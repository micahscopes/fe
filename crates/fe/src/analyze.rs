use std::collections::HashSet;

use camino::Utf8PathBuf;
use codegen::OptLevel;
use common::{
    InputDb,
    config::{Config, WorkspaceConfig},
    facts::{
        ShapeHashDigest, SourceSpanFileCount, TypedFactRelationCount, TypedFactRelationIndex,
        TypedFactRelationSet, TypedFactSet, shape_graph_facts,
    },
    file::IngotFileKind,
    shape::{ShapeBuilder, ShapeDescribe, ShapeDimension, ShapeGraph, ShapeNodeId},
};
use driver::DriverDataBase;
use driver::cli_target::{CliTarget, resolve_cli_target};
use hir::{
    Ingot,
    hir_def::{HirIngot, ItemKind, TopLevelMod},
};
use mir::{
    RuntimeOriginFactOwnerKeys, RuntimeOriginFactTargetKey, build_runtime_package,
    build_test_runtime_package, runtime_package_origin_facts, runtime_package_origins,
};
use salsa::Setter;
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

mod codegen_reports;
mod render;
mod report;

use codegen_reports::{summarize_runtime_codegen_reports, summarize_test_codegen_reports};
use render::render_report;
use report::{
    ANALYZE_REPORT_SCHEMA_VERSION, AnalyzeBodyReport, AnalyzeOriginFactReport, AnalyzePackageKind,
    AnalyzeReport, AnalyzeShapeHashReport, AnalyzeShapeReport, AnalyzeTargetReport,
    ORIGIN_PATH_WITNESS_LIMIT, ORIGIN_PATH_WITNESS_PRIORITY, OriginCount,
};
#[cfg(test)]
use report::{ANALYZE_SOURCE_MAP_ALL_SECTIONS, AnalyzeSourceMapReport, OriginCountError};

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
        schema_version: ANALYZE_REPORT_SCHEMA_VERSION,
        profile: options.profile.to_string(),
        package_kind: if options.include_tests {
            AnalyzePackageKind::Tests
        } else {
            AnalyzePackageKind::Runtime
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
        report
            .validate()
            .map_err(|err| format!("invalid analyze report: {err}"))?;
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
        scope: scope.to_string(),
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
                dimension,
                digest_hex: ShapeHashDigest::new(graph_hashes.digest(dimension).to_hex()),
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

    let mut query_errors = Vec::new();
    let path_witnesses = match relation_index.representative_path_exports_with_priority(
        ORIGIN_PATH_WITNESS_PRIORITY.iter().copied(),
        ORIGIN_PATH_WITNESS_LIMIT,
    ) {
        Ok(path_witnesses) => path_witnesses,
        Err(err) => {
            query_errors.push(format!("{err:?}"));
            Vec::new()
        }
    };
    let source_path_witnesses = match relation_index
        .representative_source_path_exports_with_priority(
            ORIGIN_PATH_WITNESS_PRIORITY.iter().copied(),
            ORIGIN_PATH_WITNESS_LIMIT,
        ) {
        Ok(source_path_witnesses) => source_path_witnesses,
        Err(err) => {
            query_errors.push(format!("{err:?}"));
            Vec::new()
        }
    };

    AnalyzeOriginFactReport {
        scope: scope.to_string(),
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
        source_path_witnesses,
        query_error: (!query_errors.is_empty()).then(|| query_errors.join("; ")),
        facts: facts.to_owned_export(),
    }
}

fn relation_index_for_export(export: &TypedFactRelationSet) -> TypedFactRelationIndex<'_> {
    TypedFactRelationIndex::new(export)
        .expect("typed fact relation export should build a query index")
}

fn relation_counts_from_relation_index(
    index: &TypedFactRelationIndex<'_>,
) -> Vec<TypedFactRelationCount> {
    index
        .relation_counts()
        .expect("typed fact relation export should contain declared relations")
}

fn source_span_file_counts_from_relation_index(
    index: &TypedFactRelationIndex<'_>,
) -> Vec<SourceSpanFileCount> {
    index
        .source_span_file_counts()
        .expect("typed fact relation export should contain source_span relation")
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
mod tests;
