use std::fmt::Write;

use codegen::debug::{BytecodeSourceMapEntry, BytecodeSourceMapEntryKind};
use common::facts::{
    OriginPathWitnessExport, OriginReachabilitySummary, OriginSourcePathWitnessExport,
    SourceSpanFileCount, TypedFactRelationCount,
};

use crate::AnalyzeFormat;

use super::report::{AnalyzeReport, AnalyzeSourceMapReport, OriginCount};

pub(super) fn render_report(
    report: &AnalyzeReport,
    format: AnalyzeFormat,
) -> Result<String, String> {
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
                let post_opt_coverage = source_map
                    .post_opt_origin_coverage
                    .as_ref()
                    .map(|coverage| {
                        format!(
                            " post_opt_origins={} same_inst_id={} created_or_unmatched_after_preopt_snapshot={} pre_opt_snapshot_losses={}",
                            coverage.total(),
                            coverage.same_inst_id(),
                            coverage.created_or_unmatched_after_preopt_snapshot(),
                            coverage.pre_opt_snapshot_losses(),
                        )
                    })
                    .unwrap_or_default();
                writeln!(
                    out,
                    "    {} {} {}:{} total={} source={} debug_locations={} debug_line_table_files={} debug_line_table_rows={} non_source={}{}{}",
                    source_map.scope,
                    source_map.label,
                    source_map.object,
                    source_map.section,
                    source_map.total,
                    source_map.source,
                    source_map.debug_locations,
                    source_map.debug_line_table_files,
                    source_map.debug_line_table_rows,
                    source_map.non_source,
                    bytecode_coverage,
                    post_opt_coverage,
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
                let source_path_witnesses = (!origin_facts.source_path_witnesses.is_empty())
                    .then(|| {
                        format!(
                            " source_path_witnesses={}",
                            origin_facts.source_path_witnesses.len()
                        )
                    })
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
                    "    {} {}{} total={} origin_nodes={} origin_links={} source_spans={}{}{}{}{}{}",
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
                    source_path_witnesses,
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
                if !origin_facts.source_path_witnesses.is_empty() {
                    writeln!(out, "      source paths:").unwrap();
                    for witness in &origin_facts.source_path_witnesses {
                        write_origin_source_path_witness(&mut out, witness);
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
                    .map(|hash| format!("{}={}", hash.dimension.as_str(), hash.digest_hex))
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

fn write_relation_counts(out: &mut String, counts: &[TypedFactRelationCount]) {
    let relation_counts = counts
        .iter()
        .map(|count| format!("{}={}", count.relation_name(), count.rows()))
        .collect::<Vec<_>>()
        .join(" ");
    writeln!(out, "      relation counts: {relation_counts}").unwrap();
}

fn write_source_span_file_counts(out: &mut String, counts: &[SourceSpanFileCount]) {
    let source_span_files = counts
        .iter()
        .map(|count| format!("{:?}={}", count.file(), count.spans()))
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
    write_origin_path_nodes(out, witness);
}

fn write_origin_source_path_witness(out: &mut String, witness: &OriginSourcePathWitnessExport) {
    let span = witness.source_span();
    let path = witness.path();
    writeln!(
        out,
        "        {} -> {} source span_kind={} file={:?} bytes={}..{} lines={}:{}..{}:{}:",
        path.from_kind().as_str(),
        path.to_kind().as_str(),
        span.span_kind().as_str(),
        span.file(),
        span.start_byte(),
        span.end_byte(),
        span.start_line(),
        span.start_col(),
        span.end_line(),
        span.end_col(),
    )
    .unwrap();
    write_origin_path_nodes(out, path);
}

fn write_origin_path_nodes(out: &mut String, witness: &OriginPathWitnessExport) {
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
