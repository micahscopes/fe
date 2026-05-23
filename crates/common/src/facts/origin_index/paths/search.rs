use std::collections::{BTreeMap, BTreeSet, VecDeque};

use crate::{
    facts::{FactId, OriginPath},
    origin::{OriginExportKey, OriginLinkKind},
};

use super::super::OriginFactIndex;

impl<'a> OriginFactIndex<'a> {
    pub fn has_path_between_keys(
        &self,
        from_key: &OriginExportKey,
        to_key: &OriginExportKey,
    ) -> bool {
        self.shortest_path_between_keys(from_key, to_key).is_some()
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

    pub fn shortest_path_between_keys(
        &self,
        from_key: &OriginExportKey,
        to_key: &OriginExportKey,
    ) -> Option<OriginPath> {
        let from = self.origin_id(from_key)?;
        let to = self.origin_id(to_key)?;
        self.shortest_path(from, to)
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
