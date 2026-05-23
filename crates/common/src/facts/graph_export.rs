use std::{collections::BTreeMap, convert::Infallible};

use crate::{
    origin::{OriginExportKey, OriginGraph},
    shape::{ShapeDimension, ShapeGraph, ShapeNodeId},
};

use super::{
    DataFlowFact, FactIdAllocator, FactNamespace, OriginLinkFact, OriginNodeFact, ShapeChildFact,
    ShapeEdgeFact, ShapeFieldFact, ShapeHashDigest, ShapeHashFact, ShapeHashScope, ShapeNodeFact,
    TraceEventFact, TypedFact, TypedFactSet,
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

pub fn shape_graph_facts(graph: &ShapeGraph) -> TypedFactSet {
    let hashes = graph.hashes();
    let mut allocator = FactIdAllocator::new();
    let mut node_ids = BTreeMap::new();
    let mut sorted_nodes = graph
        .nodes()
        .iter()
        .enumerate()
        .map(|(idx, node)| (node.stable_key(), ShapeNodeId::from_u32(idx as u32), node))
        .collect::<Vec<_>>();
    sorted_nodes.sort_unstable_by(|lhs, rhs| lhs.0.cmp(rhs.0));

    let mut facts = Vec::new();

    for (_, source_id, node) in sorted_nodes.iter().copied() {
        let id = allocator.get_or_alloc(FactNamespace::ShapeNode, node.stable_key());
        node_ids.insert(source_id, id);
        facts.push(TypedFact::ShapeNode(ShapeNodeFact::new(
            id,
            source_id,
            node.stable_key(),
            node.kind(),
        )));
    }

    for (_, source_id, node) in sorted_nodes.iter().copied() {
        let node_id = node_ids[&source_id];
        for field in node.fields() {
            facts.push(TypedFact::ShapeField(ShapeFieldFact::new(
                node_id,
                field.dimension(),
                field.name(),
                field.value(),
            )));
            if field.dimension() == ShapeDimension::TraceEvents {
                facts.push(TypedFact::TraceEvent(TraceEventFact::new(
                    node_id,
                    field.name(),
                    field.value(),
                )));
            }
        }

        for (order, child) in node.children().iter().enumerate() {
            facts.push(TypedFact::ShapeChild(ShapeChildFact::new(
                node_id,
                node_ids[&child.child()],
                child.label(),
                order as u32,
            )));
        }

        let node_hashes = hashes
            .node(source_id)
            .expect("shape hashes should exist for every shape node");
        for dimension in ShapeDimension::ALL {
            facts.push(TypedFact::ShapeHash(ShapeHashFact::new(
                Some(node_id),
                ShapeHashScope::Local,
                dimension,
                ShapeHashDigest::new(node_hashes.local().digest(dimension).to_hex()),
            )));
            facts.push(TypedFact::ShapeHash(ShapeHashFact::new(
                Some(node_id),
                ShapeHashScope::Tree,
                dimension,
                ShapeHashDigest::new(node_hashes.tree().digest(dimension).to_hex()),
            )));
        }
    }

    let mut sorted_edges = graph.edges().iter().collect::<Vec<_>>();
    sorted_edges.sort_unstable_by(|lhs, rhs| {
        (
            graph.node(lhs.from()).unwrap().stable_key(),
            lhs.label(),
            graph.node(lhs.to()).unwrap().stable_key(),
        )
            .cmp(&(
                graph.node(rhs.from()).unwrap().stable_key(),
                rhs.label(),
                graph.node(rhs.to()).unwrap().stable_key(),
            ))
    });

    for edge in sorted_edges {
        facts.push(TypedFact::ShapeEdge(ShapeEdgeFact::new(
            node_ids[&edge.from()],
            node_ids[&edge.to()],
            edge.label(),
        )));
        facts.push(TypedFact::DataFlow(DataFlowFact::new(
            node_ids[&edge.from()],
            node_ids[&edge.to()],
            edge.label(),
        )));
    }

    for dimension in ShapeDimension::ALL {
        facts.push(TypedFact::ShapeHash(ShapeHashFact::new(
            None,
            ShapeHashScope::Graph,
            dimension,
            ShapeHashDigest::new(hashes.graph().digest(dimension).to_hex()),
        )));
    }

    TypedFactSet::new(facts)
}
