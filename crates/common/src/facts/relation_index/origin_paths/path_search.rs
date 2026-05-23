use std::collections::{BTreeMap, BTreeSet, VecDeque};

use crate::origin::{OriginExportKey, OriginLinkKind};

pub(super) fn shortest_origin_relation_path<'a>(
    from: &'a str,
    to: &'a str,
    keys_by_id: &BTreeMap<&'a str, OriginExportKey>,
    outgoing: &BTreeMap<&'a str, Vec<(&'a str, OriginLinkKind)>>,
) -> Option<(Vec<&'a str>, Vec<OriginLinkKind>)> {
    if !keys_by_id.contains_key(from) || !keys_by_id.contains_key(to) {
        return None;
    }
    if from == to {
        return Some((vec![from], Vec::new()));
    }

    let mut seen = BTreeSet::new();
    let mut predecessor = BTreeMap::new();
    let mut queue = VecDeque::new();
    seen.insert(from);
    queue.push_back(from);

    while let Some(current) = queue.pop_front() {
        for (target, kind) in outgoing.get(current).into_iter().flatten() {
            if !seen.insert(*target) {
                continue;
            }
            predecessor.insert(*target, (current, *kind));
            if *target == to {
                return reconstruct_origin_relation_path(from, to, predecessor);
            }
            queue.push_back(*target);
        }
    }

    None
}

fn reconstruct_origin_relation_path<'a>(
    from: &'a str,
    to: &'a str,
    predecessor: BTreeMap<&'a str, (&'a str, OriginLinkKind)>,
) -> Option<(Vec<&'a str>, Vec<OriginLinkKind>)> {
    let mut nodes = vec![to];
    let mut links = Vec::new();
    let mut current = to;

    while current != from {
        let (previous, kind) = predecessor.get(current).copied()?;
        links.push(kind);
        nodes.push(previous);
        current = previous;
    }

    nodes.reverse();
    links.reverse();
    Some((nodes, links))
}
