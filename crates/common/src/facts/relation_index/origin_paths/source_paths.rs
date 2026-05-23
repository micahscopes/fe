use std::collections::{BTreeMap, BTreeSet};

use crate::{
    facts::{OriginSourcePathWitnessExport, SourceSpanExport, TypedFactRelationError},
    origin::OriginExportKind,
};

use super::{
    super::TypedFactRelationIndex, graph::OriginRelationGraph, paths::origin_path_export,
    source_spans::source_spans_by_origin_id,
};

impl<'a> TypedFactRelationIndex<'a> {
    pub fn representative_source_path_exports_with_priority(
        &self,
        priority_kind_pairs: impl IntoIterator<Item = (OriginExportKind, OriginExportKind)>,
        limit: usize,
    ) -> Result<Vec<OriginSourcePathWitnessExport>, TypedFactRelationError> {
        let graph = OriginRelationGraph::from_index(self)?;
        let source_spans_by_id = source_spans_by_origin_id(self, graph.keys_by_id())?;
        let mut seen_pairs = BTreeSet::new();
        let mut exports = Vec::new();
        if limit == 0 || source_spans_by_id.is_empty() {
            return Ok(exports);
        }

        for (from_kind, to_kind) in priority_kind_pairs {
            if !seen_pairs.insert((from_kind, to_kind)) {
                continue;
            }
            let Some(export) = representative_source_path_export_for_kind_pair_from_graph(
                from_kind,
                to_kind,
                &graph,
                &source_spans_by_id,
            )?
            else {
                continue;
            };
            exports.push(export);
            if exports.len() >= limit {
                return Ok(exports);
            }
        }

        for &start_id in graph.node_ids() {
            let Some(start_key) = graph.keys_by_id().get(start_id) else {
                continue;
            };
            for end_id in graph.reachable_origin_ids_from(start_id)? {
                let Some(end_key) = graph.keys_by_id().get(end_id) else {
                    continue;
                };
                let Some(source_span) = source_spans_by_id
                    .get(end_id)
                    .and_then(|spans| spans.first())
                else {
                    continue;
                };
                let pair = (start_key.kind(), end_key.kind());
                if !seen_pairs.insert(pair) {
                    continue;
                }
                let Some(path) = origin_path_export(start_id, end_id, pair.0, pair.1, &graph)
                else {
                    continue;
                };
                exports.push(OriginSourcePathWitnessExport::new(
                    path,
                    source_span.clone(),
                ));
                if exports.len() >= limit {
                    return Ok(exports);
                }
            }
        }

        Ok(exports)
    }
}

fn representative_source_path_export_for_kind_pair_from_graph<'a>(
    from_kind: OriginExportKind,
    to_kind: OriginExportKind,
    graph: &OriginRelationGraph<'a>,
    source_spans_by_id: &BTreeMap<&'a str, Vec<SourceSpanExport>>,
) -> Result<Option<OriginSourcePathWitnessExport>, TypedFactRelationError> {
    for &start_id in graph.node_ids() {
        let Some(start_key) = graph.keys_by_id().get(start_id) else {
            continue;
        };
        if start_key.kind() != from_kind {
            continue;
        }

        for end_id in graph.reachable_origin_ids_from(start_id)? {
            let Some(end_key) = graph.keys_by_id().get(end_id) else {
                continue;
            };
            if end_key.kind() != to_kind {
                continue;
            }
            let Some(source_span) = source_spans_by_id
                .get(end_id)
                .and_then(|spans| spans.first())
            else {
                continue;
            };
            let Some(path) = origin_path_export(start_id, end_id, from_kind, to_kind, graph) else {
                continue;
            };
            return Ok(Some(OriginSourcePathWitnessExport::new(
                path,
                source_span.clone(),
            )));
        }
    }

    Ok(None)
}
