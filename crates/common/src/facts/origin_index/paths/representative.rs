use std::collections::BTreeSet;

use crate::{facts::OriginKindPathWitness, origin::OriginExportKind};

use super::super::OriginFactIndex;

impl<'a> OriginFactIndex<'a> {
    pub fn representative_path_for_kind_pair(
        &self,
        from_kind: OriginExportKind,
        to_kind: OriginExportKind,
    ) -> Option<OriginKindPathWitness> {
        for (start_id, start_node) in &self.nodes_by_id {
            if start_node.key().kind() != from_kind {
                continue;
            }

            for end_id in self.reachable_from(*start_id) {
                let Some(end_node) = self.origin_node(end_id) else {
                    continue;
                };
                if end_node.key().kind() != to_kind {
                    continue;
                }
                let path = self.shortest_path(*start_id, end_id)?;
                return Some(OriginKindPathWitness::new(from_kind, to_kind, path));
            }
        }

        None
    }

    pub fn representative_paths_by_kind(&self, limit: usize) -> Vec<OriginKindPathWitness> {
        let mut seen_pairs = BTreeSet::new();
        let mut witnesses = Vec::new();

        for (start_id, start_node) in &self.nodes_by_id {
            for end_id in self.reachable_from(*start_id) {
                let Some(end_node) = self.origin_node(end_id) else {
                    continue;
                };
                let pair = (start_node.key().kind(), end_node.key().kind());
                if !seen_pairs.insert(pair) {
                    continue;
                }
                let Some(path) = self.shortest_path(*start_id, end_id) else {
                    continue;
                };
                witnesses.push(OriginKindPathWitness::new(pair.0, pair.1, path));
                if witnesses.len() >= limit {
                    return witnesses;
                }
            }
        }

        witnesses
    }
}
