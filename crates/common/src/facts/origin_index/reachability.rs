use std::collections::{BTreeMap, BTreeSet, VecDeque};

use super::OriginFactIndex;
use crate::facts::{FactId, OriginReachabilitySummary};
use crate::origin::{OriginExportKey, OriginExportKind, OriginLinkKind};

impl<'a> OriginFactIndex<'a> {
    pub fn reachable_from(&self, start: FactId) -> BTreeSet<FactId> {
        self.reachable_from_with_kinds(start, |_| true)
    }

    pub fn reachable_from_with_kinds(
        &self,
        start: FactId,
        mut include_kind: impl FnMut(OriginLinkKind) -> bool,
    ) -> BTreeSet<FactId> {
        let mut seen = BTreeSet::new();
        let mut queue = VecDeque::new();
        queue.push_back(start);

        while let Some(current) = queue.pop_front() {
            for link in self.outgoing(current) {
                if !include_kind(link.kind()) {
                    continue;
                }
                if seen.insert(link.to()) {
                    queue.push_back(link.to());
                }
            }
        }

        seen
    }

    pub fn reachable_keys_from(&self, start: FactId) -> Vec<&OriginExportKey> {
        self.reachable_from(start)
            .into_iter()
            .filter_map(|id| self.origin_key(id))
            .collect()
    }

    pub fn has_path(&self, from: FactId, to: FactId) -> bool {
        self.reachable_from(from).contains(&to)
    }

    pub fn has_reachable_kind_pair(
        &self,
        from_kind: OriginExportKind,
        to_kind: OriginExportKind,
    ) -> bool {
        self.representative_path_for_kind_pair(from_kind, to_kind)
            .is_some()
    }

    pub fn reachability_summary(&self) -> OriginReachabilitySummary {
        let mut pair_counts = BTreeMap::new();

        for (start_id, start_node) in &self.nodes_by_id {
            for end_id in self.reachable_from(*start_id) {
                let Some(end_node) = self.origin_node(end_id) else {
                    continue;
                };
                *pair_counts
                    .entry((start_node.key().kind(), end_node.key().kind()))
                    .or_insert(0) += 1;
            }
        }

        OriginReachabilitySummary::from_pair_counts(pair_counts)
    }
}
