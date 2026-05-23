use std::{collections::BTreeMap, convert::Infallible};

use crate::origin::{OriginExportKey, OriginGraph};

use super::super::{
    FactIdAllocator, FactNamespace, OriginLinkFact, OriginNodeFact, TypedFact, TypedFactSet,
};

pub fn origin_graph_facts<Node>(
    graph: &OriginGraph<Node>,
    mut export_key: impl FnMut(&Node) -> OriginExportKey,
) -> TypedFactSet {
    match try_origin_graph_facts(graph, |node| Ok::<_, Infallible>(export_key(node))) {
        Ok(facts) => facts,
        Err(never) => match never {},
    }
}

pub fn try_origin_graph_facts<Node, E>(
    graph: &OriginGraph<Node>,
    mut export_key: impl FnMut(&Node) -> Result<OriginExportKey, E>,
) -> Result<TypedFactSet, E> {
    let mut keys = Vec::new();
    let mut links = Vec::new();

    for link in graph.links() {
        let from_key = export_key(link.from())?;
        let to_key = export_key(link.to())?;
        keys.push(from_key.clone());
        keys.push(to_key.clone());
        links.push((from_key, to_key, link.kind()));
    }

    keys.sort();
    keys.dedup();
    links.sort();
    links.dedup();

    let mut allocator = FactIdAllocator::new();
    let mut ids = BTreeMap::new();
    let mut facts = Vec::new();

    for key in keys {
        let id = allocator.get_or_alloc(FactNamespace::OriginNode, key.canonical_storage_key());
        ids.insert(key.clone(), id);
        facts.push(TypedFact::OriginNode(OriginNodeFact::new(id, key)));
    }

    for (from_key, to_key, kind) in links {
        let from = ids[&from_key];
        let to = ids[&to_key];
        facts.push(TypedFact::OriginLink(OriginLinkFact::new(from, to, kind)));
    }

    Ok(TypedFactSet::new(facts))
}
