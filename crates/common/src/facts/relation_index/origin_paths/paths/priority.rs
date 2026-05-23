use std::collections::BTreeSet;

use crate::{
    facts::{OriginPathWitnessExport, TypedFactRelationError},
    origin::OriginExportKind,
};

use super::super::{super::TypedFactRelationIndex, graph::OriginRelationGraph};
use super::origin_path_export;
use super::representative::representative_path_export_for_kind_pair_from_graph;

impl<'a> TypedFactRelationIndex<'a> {
    pub fn representative_path_exports_with_priority(
        &self,
        priority_kind_pairs: impl IntoIterator<Item = (OriginExportKind, OriginExportKind)>,
        limit: usize,
    ) -> Result<Vec<OriginPathWitnessExport>, TypedFactRelationError> {
        let graph = OriginRelationGraph::from_index(self)?;
        let mut seen_pairs = BTreeSet::new();
        let mut exports = Vec::new();
        if limit == 0 {
            return Ok(exports);
        }

        for (from_kind, to_kind) in priority_kind_pairs {
            if !seen_pairs.insert((from_kind, to_kind)) {
                continue;
            }
            let Some(export) =
                representative_path_export_for_kind_pair_from_graph(from_kind, to_kind, &graph)?
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
                let pair = (start_key.kind(), end_key.kind());
                if !seen_pairs.insert(pair) {
                    continue;
                }
                let Some(export) = origin_path_export(start_id, end_id, pair.0, pair.1, &graph)
                else {
                    continue;
                };
                exports.push(export);
                if exports.len() >= limit {
                    return Ok(exports);
                }
            }
        }

        Ok(exports)
    }
}
