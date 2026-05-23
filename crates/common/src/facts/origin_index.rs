use std::collections::{BTreeMap, BTreeSet, VecDeque};

use crate::origin::{OriginExportKey, OriginExportKind, OriginLinkKind};

use super::index_error::require_fact_namespace;
use super::{
    FactId, FactIndexError, FactNamespace, OriginKindPathWitness, OriginLinkFact, OriginNodeFact,
    OriginPath, OriginPathWitnessExport, OriginReachabilitySummary, SourceSpanFact, TypedFactSet,
};

#[derive(Clone, Debug)]
pub struct OriginFactIndex<'a> {
    nodes_by_id: BTreeMap<FactId, &'a OriginNodeFact>,
    ids_by_key: BTreeMap<OriginExportKey, FactId>,
    outgoing: BTreeMap<FactId, Vec<&'a OriginLinkFact>>,
    source_spans_by_origin: BTreeMap<FactId, Vec<&'a SourceSpanFact>>,
}

impl<'a> OriginFactIndex<'a> {
    pub fn new(facts: &'a TypedFactSet) -> Result<Self, FactIndexError> {
        let mut nodes_by_id = BTreeMap::new();
        let mut ids_by_key = BTreeMap::new();

        for node in facts.origin_nodes() {
            require_fact_namespace(node.id(), FactNamespace::OriginNode)?;
            if nodes_by_id.insert(node.id(), node).is_some() {
                return Err(FactIndexError::DuplicateOriginId);
            }
            if ids_by_key.insert(node.key().clone(), node.id()).is_some() {
                return Err(FactIndexError::DuplicateOriginKey);
            }
        }

        let mut seen_links = BTreeSet::new();
        let mut outgoing: BTreeMap<FactId, Vec<&OriginLinkFact>> = BTreeMap::new();
        for link in facts.origin_links() {
            require_fact_namespace(link.from(), FactNamespace::OriginNode)?;
            require_fact_namespace(link.to(), FactNamespace::OriginNode)?;
            if !nodes_by_id.contains_key(&link.from()) {
                return Err(FactIndexError::OriginLinkMissingEndpoint {
                    endpoint: link.from(),
                });
            }
            if !nodes_by_id.contains_key(&link.to()) {
                return Err(FactIndexError::OriginLinkMissingEndpoint {
                    endpoint: link.to(),
                });
            }
            let link_key = (link.from(), link.to(), link.kind());
            if !seen_links.insert(link_key) {
                return Err(FactIndexError::DuplicateOriginLink {
                    from: link.from(),
                    to: link.to(),
                    kind: link.kind(),
                });
            }
            outgoing.entry(link.from()).or_default().push(link);
        }
        for links in outgoing.values_mut() {
            links.sort_by_key(|link| (link.to(), link.kind()));
        }

        let mut source_spans_by_origin: BTreeMap<FactId, Vec<&SourceSpanFact>> = BTreeMap::new();
        for span in facts.source_spans() {
            require_fact_namespace(span.origin(), FactNamespace::OriginNode)?;
            if !nodes_by_id.contains_key(&span.origin()) {
                return Err(FactIndexError::SourceSpanMissingOrigin {
                    origin: span.origin(),
                });
            }
            if span.file().is_empty() {
                return Err(FactIndexError::InvalidSourceSpanFile {
                    origin: span.origin(),
                });
            }
            if span.start_byte() > span.end_byte() {
                return Err(FactIndexError::InvalidSourceSpanRange {
                    origin: span.origin(),
                    start_byte: span.start_byte(),
                    end_byte: span.end_byte(),
                });
            }
            if span.start_line() > span.end_line()
                || (span.start_line() == span.end_line() && span.start_col() > span.end_col())
            {
                return Err(FactIndexError::InvalidSourceSpanPosition {
                    origin: span.origin(),
                    start_line: span.start_line(),
                    start_col: span.start_col(),
                    end_line: span.end_line(),
                    end_col: span.end_col(),
                });
            }
            source_spans_by_origin
                .entry(span.origin())
                .or_default()
                .push(span);
        }
        for spans in source_spans_by_origin.values_mut() {
            spans.sort_by_key(|span| {
                (
                    span.file(),
                    span.start_byte(),
                    span.end_byte(),
                    span.start_line(),
                    span.start_col(),
                    span.end_line(),
                    span.end_col(),
                    span.span_kind(),
                )
            });
        }

        Ok(Self {
            nodes_by_id,
            ids_by_key,
            outgoing,
            source_spans_by_origin,
        })
    }

    pub fn origin_id(&self, key: &OriginExportKey) -> Option<FactId> {
        self.ids_by_key.get(key).copied()
    }

    pub fn origin_key(&self, id: FactId) -> Option<&OriginExportKey> {
        self.nodes_by_id.get(&id).map(|node| node.key())
    }

    pub fn origin_node(&self, id: FactId) -> Option<&OriginNodeFact> {
        self.nodes_by_id.get(&id).copied()
    }

    pub fn outgoing(&self, id: FactId) -> impl Iterator<Item = &'a OriginLinkFact> + '_ {
        self.outgoing
            .get(&id)
            .into_iter()
            .flat_map(|links| links.iter().copied())
    }

    pub fn source_spans_for_origin(
        &self,
        id: FactId,
    ) -> impl Iterator<Item = &'a SourceSpanFact> + '_ {
        self.source_spans_by_origin
            .get(&id)
            .into_iter()
            .flat_map(|spans| spans.iter().copied())
    }

    pub fn source_spans_for_key(
        &self,
        key: &OriginExportKey,
    ) -> impl Iterator<Item = &'a SourceSpanFact> + '_ {
        self.origin_id(key)
            .into_iter()
            .flat_map(|id| self.source_spans_for_origin(id))
    }

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

    pub fn has_path_between_keys(
        &self,
        from_key: &OriginExportKey,
        to_key: &OriginExportKey,
    ) -> bool {
        self.shortest_path_between_keys(from_key, to_key).is_some()
    }

    pub fn has_reachable_kind_pair(
        &self,
        from_kind: OriginExportKind,
        to_kind: OriginExportKind,
    ) -> bool {
        self.representative_path_for_kind_pair(from_kind, to_kind)
            .is_some()
    }

    pub fn shortest_path(&self, from: FactId, to: FactId) -> Option<OriginPath> {
        if !self.nodes_by_id.contains_key(&from) || !self.nodes_by_id.contains_key(&to) {
            return None;
        }
        if from == to {
            return Some(OriginPath::new(vec![from], Vec::new()));
        }

        let mut seen = BTreeSet::new();
        let mut predecessor = BTreeMap::new();
        let mut queue = VecDeque::new();
        seen.insert(from);
        queue.push_back(from);

        while let Some(current) = queue.pop_front() {
            for link in self.outgoing(current) {
                if !seen.insert(link.to()) {
                    continue;
                }
                predecessor.insert(link.to(), (current, link.kind()));
                if link.to() == to {
                    return Some(reconstruct_origin_path(from, to, predecessor));
                }
                queue.push_back(link.to());
            }
        }

        None
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

    pub fn shortest_path_between_keys(
        &self,
        from_key: &OriginExportKey,
        to_key: &OriginExportKey,
    ) -> Option<OriginPath> {
        let from = self.origin_id(from_key)?;
        let to = self.origin_id(to_key)?;
        self.shortest_path(from, to)
    }

    pub fn path_export_between_keys(
        &self,
        from_key: &OriginExportKey,
        to_key: &OriginExportKey,
    ) -> Option<OriginPathWitnessExport> {
        let path = self.shortest_path_between_keys(from_key, to_key)?;
        self.export_path_witness(OriginKindPathWitness::new(
            from_key.kind(),
            to_key.kind(),
            path,
        ))
    }

    pub fn representative_path_export_for_kind_pair(
        &self,
        from_kind: OriginExportKind,
        to_kind: OriginExportKind,
    ) -> Option<OriginPathWitnessExport> {
        self.representative_path_for_kind_pair(from_kind, to_kind)
            .and_then(|witness| self.export_path_witness(witness))
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

    pub fn representative_path_exports(&self, limit: usize) -> Vec<OriginPathWitnessExport> {
        self.representative_paths_by_kind(limit)
            .into_iter()
            .filter_map(|witness| self.export_path_witness(witness))
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
                let Some(export) = self.export_path_witness(witness) else {
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

    fn export_path_witness(
        &self,
        witness: OriginKindPathWitness,
    ) -> Option<OriginPathWitnessExport> {
        let nodes = witness
            .path()
            .nodes()
            .iter()
            .map(|id| self.origin_key(*id).cloned())
            .collect::<Option<Vec<_>>>()?;
        Some(OriginPathWitnessExport::new(
            witness.from_kind(),
            witness.to_kind(),
            nodes,
            witness.path().links().to_vec(),
        ))
    }
}

fn reconstruct_origin_path(
    from: FactId,
    to: FactId,
    predecessor: BTreeMap<FactId, (FactId, OriginLinkKind)>,
) -> OriginPath {
    let mut nodes = vec![to];
    let mut links = Vec::new();
    let mut current = to;

    while current != from {
        let (previous, kind) = predecessor[&current];
        links.push(kind);
        nodes.push(previous);
        current = previous;
    }

    nodes.reverse();
    links.reverse();
    OriginPath::new(nodes, links)
}
