use crate::{
    facts::{OriginPathWitnessExport, TypedFactRelationError},
    origin::OriginExportKey,
};

use super::super::{super::TypedFactRelationIndex, graph::OriginRelationGraph};
use super::origin_path_export;

impl<'a> TypedFactRelationIndex<'a> {
    pub fn path_export_between_keys(
        &self,
        from_key: &OriginExportKey,
        to_key: &OriginExportKey,
    ) -> Result<Option<OriginPathWitnessExport>, TypedFactRelationError> {
        let graph = OriginRelationGraph::from_index(self)?;
        let Some(from_id) = graph.id_for_key(from_key) else {
            return Ok(None);
        };
        let Some(to_id) = graph.id_for_key(to_key) else {
            return Ok(None);
        };

        Ok(origin_path_export(
            from_id,
            to_id,
            from_key.kind(),
            to_key.kind(),
            &graph,
        ))
    }
}
