use crate::{
    facts::{OriginPathWitnessExport, TypedFactRelationError},
    origin::OriginExportKind,
};

use super::super::{super::TypedFactRelationIndex, graph::OriginRelationGraph};
use super::origin_path_export;

impl<'a> TypedFactRelationIndex<'a> {
    pub fn representative_path_export_for_kind_pair(
        &self,
        from_kind: OriginExportKind,
        to_kind: OriginExportKind,
    ) -> Result<Option<OriginPathWitnessExport>, TypedFactRelationError> {
        let graph = OriginRelationGraph::from_index(self)?;
        representative_path_export_for_kind_pair_from_graph(from_kind, to_kind, &graph)
    }
}

pub(super) fn representative_path_export_for_kind_pair_from_graph<'a>(
    from_kind: OriginExportKind,
    to_kind: OriginExportKind,
    graph: &OriginRelationGraph<'a>,
) -> Result<Option<OriginPathWitnessExport>, TypedFactRelationError> {
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
            return Ok(origin_path_export(
                start_id, end_id, from_kind, to_kind, graph,
            ));
        }
    }

    Ok(None)
}
