use std::collections::BTreeSet;

use crate::{
    facts::{OriginKindPathWitness, OriginPathWitnessExport},
    origin::{OriginExportKey, OriginExportKind},
};

use super::super::OriginFactIndex;

impl<'a> OriginFactIndex<'a> {
    pub fn path_export_between_keys(
        &self,
        from_key: &OriginExportKey,
        to_key: &OriginExportKey,
    ) -> Option<OriginPathWitnessExport> {
        let path = self.shortest_path_between_keys(from_key, to_key)?;
        export_path_witness(
            self,
            OriginKindPathWitness::new(from_key.kind(), to_key.kind(), path),
        )
    }

    pub fn representative_path_export_for_kind_pair(
        &self,
        from_kind: OriginExportKind,
        to_kind: OriginExportKind,
    ) -> Option<OriginPathWitnessExport> {
        self.representative_path_for_kind_pair(from_kind, to_kind)
            .and_then(|witness| export_path_witness(self, witness))
    }

    pub fn representative_path_exports(&self, limit: usize) -> Vec<OriginPathWitnessExport> {
        self.representative_paths_by_kind(limit)
            .into_iter()
            .filter_map(|witness| export_path_witness(self, witness))
            .collect()
    }

    pub fn representative_path_exports_with_priority(
        &self,
        priority_kind_pairs: impl IntoIterator<Item = (OriginExportKind, OriginExportKind)>,
        limit: usize,
    ) -> Vec<OriginPathWitnessExport> {
        let mut seen_pairs = BTreeSet::new();
        let mut exports = Vec::new();
        if limit == 0 {
            return exports;
        }

        for (from_kind, to_kind) in priority_kind_pairs {
            if !seen_pairs.insert((from_kind, to_kind)) {
                continue;
            }
            let Some(export) = self.representative_path_export_for_kind_pair(from_kind, to_kind)
            else {
                continue;
            };
            exports.push(export);
            if exports.len() >= limit {
                return exports;
            }
        }

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
                let witness = OriginKindPathWitness::new(pair.0, pair.1, path);
                let Some(export) = export_path_witness(self, witness) else {
                    continue;
                };
                exports.push(export);
                if exports.len() >= limit {
                    return exports;
                }
            }
        }

        exports
    }
}

fn export_path_witness(
    index: &OriginFactIndex<'_>,
    witness: OriginKindPathWitness,
) -> Option<OriginPathWitnessExport> {
    let nodes = witness
        .path()
        .nodes()
        .iter()
        .map(|id| index.origin_key(*id).cloned())
        .collect::<Option<Vec<_>>>()?;
    Some(OriginPathWitnessExport::new(
        witness.from_kind(),
        witness.to_kind(),
        nodes,
        witness.path().links().to_vec(),
    ))
}
