use std::collections::BTreeMap;

use crate::facts::{OriginReachabilitySummary, TypedFactRelationError};

use super::{super::TypedFactRelationIndex, graph::OriginRelationGraph};

impl<'a> TypedFactRelationIndex<'a> {
    pub fn origin_reachability_summary(
        &self,
    ) -> Result<OriginReachabilitySummary, TypedFactRelationError> {
        let graph = OriginRelationGraph::from_index(self)?;

        let mut pair_counts = BTreeMap::new();
        for &start_id in graph.node_ids() {
            let Some(start_key) = graph.keys_by_id().get(start_id) else {
                continue;
            };
            for end_id in graph.reachable_origin_ids_from(start_id)? {
                if let Some(end_key) = graph.keys_by_id().get(end_id) {
                    *pair_counts
                        .entry((start_key.kind(), end_key.kind()))
                        .or_insert(0) += 1;
                }
            }
        }

        Ok(OriginReachabilitySummary::from_pair_counts(pair_counts))
    }
}
