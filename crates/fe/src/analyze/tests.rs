use std::fs;

use camino::Utf8PathBuf;
use codegen::{
    OptLevel,
    debug::{
        BytecodeSourceMapEntry, BytecodeSourceMapEntryKind, bytecode_source_map_entries_summary,
    },
};
use common::{
    facts::{
        OriginReachabilitySummary, OwnedTypedFactSetExport, TypedFactRelationCount,
        TypedFactRelationName, TypedFactRelationSet, TypedFactSet,
    },
    origin::{OriginExportKind, OriginLinkKind},
    shape::ShapeDimension,
};
use tempfile::tempdir;

use super::{
    ANALYZE_REPORT_SCHEMA_VERSION, ANALYZE_SOURCE_MAP_ALL_SECTIONS, AnalyzeBodyReport,
    AnalyzeOptions, AnalyzeOriginFactReport, AnalyzePackageKind, AnalyzeReport,
    AnalyzeShapeHashReport, AnalyzeShapeReport, AnalyzeSourceMapReport, OriginCount,
    OriginCountError, analyze_to_string,
};
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

fn analyze_report(output: &str) -> AnalyzeReport {
    let report =
        serde_json::from_str::<AnalyzeReport>(output).expect("analyze report should match schema");
    assert_eq!(report.schema_version, ANALYZE_REPORT_SCHEMA_VERSION);
    report
}

fn relation_tables_value_for_report(report: &serde_json::Value) -> serde_json::Value {
    let export = serde_json::from_value::<OwnedTypedFactSetExport>(report["facts"].clone())
        .expect("test facts should decode");
    let facts = typed_facts_from_export(&export);
    serde_json::to_value(facts.relation_export()).expect("relation tables should serialize")
}

fn relation_rows_mut<'a>(
    relation_tables: &'a mut serde_json::Value,
    relation_name: &str,
) -> &'a mut Vec<serde_json::Value> {
    let relation = relation_tables["relations"]
        .as_array_mut()
        .expect("relation tables should contain relations")
        .iter_mut()
        .find(|relation| relation["name"].as_str() == Some(relation_name))
        .expect("test relation table should exist");
    relation["rows"]
        .as_array_mut()
        .expect("relation table should contain rows")
}

fn typed_facts_from_export(export: &OwnedTypedFactSetExport) -> TypedFactSet {
    assert_eq!(
        export.schema_version(),
        OwnedTypedFactSetExport::SCHEMA_VERSION
    );
    TypedFactSet::new(export.facts().to_vec())
}

fn typed_origin_facts(report: &AnalyzeOriginFactReport) -> TypedFactSet {
    typed_facts_from_export(&report.facts)
}

fn shape_facts(report: &AnalyzeShapeReport) -> TypedFactSet {
    let facts = report
        .facts
        .as_ref()
        .expect("shape facts should be emitted");
    typed_facts_from_export(facts)
}

fn has_origin_node_kind(facts: &TypedFactSet, kind: OriginExportKind) -> bool {
    facts.origin_nodes().any(|fact| fact.key().kind() == kind)
}

fn has_origin_link_kind(facts: &TypedFactSet, kind: OriginLinkKind) -> bool {
    facts.origin_links().any(|fact| fact.kind() == kind)
}

fn has_source_span_fact(facts: &TypedFactSet) -> bool {
    facts.source_spans().next().is_some()
}

fn has_shape_node_fact(facts: &TypedFactSet) -> bool {
    facts.shape_nodes().next().is_some()
}

fn has_shape_hash_fact(facts: &TypedFactSet) -> bool {
    facts.shape_hashes().next().is_some()
}

fn origin_kind(raw: &str) -> OriginExportKind {
    OriginExportKind::from_str(raw).expect("test should use a known origin kind")
}

fn typed_reachability(report: &AnalyzeOriginFactReport) -> &OriginReachabilitySummary {
    report
        .reachability
        .as_ref()
        .expect("report reachability should match typed schema")
}

fn has_reachable_kind_pair(
    report: &AnalyzeOriginFactReport,
    from_kind: &str,
    to_kind: &str,
) -> bool {
    typed_reachability(report).pair_count(origin_kind(from_kind), origin_kind(to_kind)) > 0
}

fn has_path_witness(report: &AnalyzeOriginFactReport, from_kind: &str, to_kind: &str) -> bool {
    let from_kind = origin_kind(from_kind);
    let to_kind = origin_kind(to_kind);
    report.path_witnesses.iter().any(|witness| {
        witness.from_kind() == from_kind
            && witness.to_kind() == to_kind
            && !witness.links().is_empty()
            && witness.nodes().len() >= 2
            && witness
                .nodes()
                .first()
                .is_some_and(|node| node.kind() == from_kind)
            && witness
                .nodes()
                .last()
                .is_some_and(|node| node.kind() == to_kind)
    })
}

fn has_source_path_witness(
    report: &AnalyzeOriginFactReport,
    from_kind: &str,
    to_kind: &str,
) -> bool {
    let from_kind = origin_kind(from_kind);
    let to_kind = origin_kind(to_kind);
    report.source_path_witnesses.iter().any(|witness| {
        let path = witness.path();
        let span = witness.source_span();
        path.from_kind() == from_kind
            && path.to_kind() == to_kind
            && !path.links().is_empty()
            && path.nodes().len() >= 2
            && path
                .nodes()
                .first()
                .is_some_and(|node| node.kind() == from_kind)
            && path
                .nodes()
                .last()
                .is_some_and(|node| node.kind() == to_kind)
            && span.origin_key().kind() == to_kind
            && !span.file().is_empty()
            && span.start_byte() <= span.end_byte()
    })
}

fn has_relation_count(counts: &[TypedFactRelationCount], relation: &str) -> bool {
    counts
        .iter()
        .any(|count| count.relation_name() == relation && count.rows() > 0)
}

fn has_relation_table(
    relation_tables: &Option<TypedFactRelationSet>,
    relation: TypedFactRelationName,
) -> bool {
    let relation_tables = relation_tables
        .as_ref()
        .expect("report relation tables should match typed schema");
    assert_eq!(
        relation_tables.schema_version(),
        TypedFactRelationSet::SCHEMA_VERSION
    );
    relation_tables
        .relation(relation)
        .is_some_and(|table| !table.rows().is_empty())
}

fn has_source_span_file_count(report: &AnalyzeOriginFactReport) -> bool {
    report
        .source_span_files
        .iter()
        .any(|file| !file.file().is_empty() && file.spans() > 0)
}

fn has_typed_source_entry(entries: &[BytecodeSourceMapEntry]) -> bool {
    entries.iter().any(|entry| {
        entry.pc_start() < entry.pc_end()
            && matches!(
                entry.kind(),
                BytecodeSourceMapEntryKind::Source { snippet, .. }
                    if !snippet.trim().is_empty()
            )
    })
}

fn has_partitioned_bytecode_origin_coverage(report: &AnalyzeSourceMapReport) -> bool {
    let coverage = report
        .bytecode_origin_coverage
        .as_ref()
        .expect("bytecode origin coverage should match typed schema");
    coverage.total() > 0 && coverage.classified_total() == coverage.total()
}

fn has_partitioned_post_opt_origin_coverage(report: &AnalyzeSourceMapReport) -> bool {
    let coverage = report
        .post_opt_origin_coverage
        .as_ref()
        .expect("post-opt origin coverage should match typed schema");
    coverage.total() > 0
        && coverage.post_opt_classified_total() == coverage.total()
        && coverage.computed_observed_pre_opt_total() == coverage.observed_pre_opt_total()
}

mod cli_basic;
mod cli_origin_facts;
mod cli_shape;
mod cli_source_maps;
mod origin_fact_report;
mod report_schema;
mod shape_report;
mod source_map_report;
