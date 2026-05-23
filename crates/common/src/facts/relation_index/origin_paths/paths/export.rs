use crate::{facts::OriginPathWitnessExport, origin::OriginExportKind};

use super::super::{graph::OriginRelationGraph, path_search::shortest_origin_relation_path};

pub(in crate::facts::relation_index::origin_paths) fn origin_path_export<'a>(
    from_id: &'a str,
    to_id: &'a str,
    from_kind: OriginExportKind,
    to_kind: OriginExportKind,
    graph: &OriginRelationGraph<'a>,
) -> Option<OriginPathWitnessExport> {
    let (node_ids, links) =
        shortest_origin_relation_path(from_id, to_id, graph.keys_by_id(), graph.outgoing())?;
    let nodes = node_ids
        .into_iter()
        .map(|id| graph.keys_by_id().get(id).cloned())
        .collect::<Option<Vec<_>>>()?;
    Some(OriginPathWitnessExport::new(
        from_kind, to_kind, nodes, links,
    ))
}
