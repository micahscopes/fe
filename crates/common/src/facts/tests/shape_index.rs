use super::*;

#[test]
fn shape_graph_export_preserves_fields_children_edges_and_hash_dimensions() {
    let mut graph = ShapeGraph::new();
    let stmt = graph.add_node("stmt:0", "stmt");
    let expr = graph.add_node("expr:0", "literal");
    graph.add_field(expr, ShapeDimension::Constants, "value", "1");
    graph.add_field(
        stmt,
        ShapeDimension::TraceEvents,
        "runtime_code_region",
        "runtime_code_region_ref",
    );
    graph.add_child(stmt, "expr", expr);
    graph.add_edge(stmt, expr, "data-flow:full-label");

    let facts = shape_graph_facts(&graph);

    assert_eq!(facts.shape_nodes().count(), 2);
    assert_eq!(facts.shape_fields().count(), 2);
    let child = facts
        .shape_children()
        .next()
        .expect("child fact should be exported");
    assert_eq!(child.label(), "expr");
    assert_eq!(child.order(), 0);
    assert_eq!(child.parent().namespace(), FactNamespace::ShapeNode);
    assert_eq!(child.child().namespace(), FactNamespace::ShapeNode);

    let edge = facts
        .shape_edges()
        .next()
        .expect("edge fact should be exported");
    assert_eq!(edge.label(), "data-flow:full-label");
    assert_eq!(edge.from().namespace(), FactNamespace::ShapeNode);
    assert_eq!(edge.to().namespace(), FactNamespace::ShapeNode);

    let trace_event = facts
        .trace_events()
        .next()
        .expect("trace event fact should be exported");
    assert_eq!(trace_event.event_kind(), "runtime_code_region");
    assert_eq!(trace_event.value(), "runtime_code_region_ref");
    assert_eq!(trace_event.node().namespace(), FactNamespace::ShapeNode);

    let data_flow = facts
        .data_flows()
        .next()
        .expect("data-flow fact should be exported");
    assert_eq!(data_flow.kind(), "data-flow:full-label");
    assert_eq!(data_flow.source().namespace(), FactNamespace::ShapeNode);
    assert_eq!(data_flow.target().namespace(), FactNamespace::ShapeNode);

    for dimension in ShapeDimension::ALL {
        assert!(facts.shape_hashes().any(|hash| {
            hash.scope() == ShapeHashScope::Graph && hash.dimension() == dimension
        }));
        assert_eq!(
            facts
                .shape_hashes()
                .filter(|hash| {
                    hash.scope() == ShapeHashScope::Local
                        && hash.dimension() == dimension
                        && hash.node().is_some()
                })
                .count(),
            2
        );
        assert_eq!(
            facts
                .shape_hashes()
                .filter(|hash| {
                    hash.scope() == ShapeHashScope::Tree
                        && hash.dimension() == dimension
                        && hash.node().is_some()
                })
                .count(),
            2
        );
    }
}

#[test]
fn shape_fact_index_answers_stable_and_source_key_lookups() {
    let mut graph = ShapeGraph::new();
    let root = graph.add_node("root", "block");
    let leaf = graph.add_node("leaf", "literal");
    graph.add_child(root, "expr", leaf);

    let facts = shape_graph_facts(&graph);
    let index = ShapeFactIndex::new(&facts).expect("shape facts should index");
    let root_id = index
        .shape_id_by_stable_key("root")
        .expect("root stable key should be indexed");
    let leaf_id = index
        .shape_id_by_source_id(leaf)
        .expect("leaf source id should be indexed");

    assert_eq!(
        index
            .shape_node(root_id)
            .expect("root id should resolve")
            .kind(),
        "block"
    );
    assert_eq!(
        index
            .shape_node(leaf_id)
            .expect("leaf id should resolve")
            .stable_key(),
        "leaf"
    );
}

#[test]
fn shape_fact_index_answers_hash_lookups_without_row_scans() {
    let mut graph = ShapeGraph::new();
    let root = graph.add_node("root", "block");
    let leaf = graph.add_node("leaf", "literal");
    graph.add_field(leaf, ShapeDimension::Constants, "value", "7");
    graph.add_child(root, "expr", leaf);

    let facts = shape_graph_facts(&graph);
    let index = ShapeFactIndex::new(&facts).expect("shape facts should index");
    let root_id = index
        .shape_id_by_stable_key("root")
        .expect("root stable key should be indexed");

    for dimension in ShapeDimension::ALL {
        let graph_hash = index
            .graph_hash(dimension)
            .expect("graph hash should be indexed");
        assert_eq!(graph_hash.node(), None);
        assert_eq!(graph_hash.scope(), ShapeHashScope::Graph);
        assert_eq!(graph_hash.dimension(), dimension);

        let local_hash = index
            .local_hash(root_id, dimension)
            .expect("local hash should be indexed");
        let direct_hash = index
            .shape_hash(ShapeHashFactKey::local(root_id, dimension))
            .expect("direct key lookup should return the same hash");
        assert_eq!(local_hash, direct_hash);
        assert_eq!(local_hash.scope(), ShapeHashScope::Local);
        assert_eq!(local_hash.digest_hex().len(), 16);

        let tree_hash = index
            .tree_hash(root_id, dimension)
            .expect("tree hash should be indexed");
        assert_eq!(tree_hash.scope(), ShapeHashScope::Tree);
    }

    assert!(
        index
            .shape_hash(ShapeHashFactKey::graph(ShapeDimension::Structure))
            .is_some()
    );
}

#[test]
fn shape_fact_export_has_exact_synthetic_oracle_rows() {
    let mut graph = ShapeGraph::new();
    let root = graph.add_node("root", "block");
    let leaf = graph.add_node("leaf", "name");
    graph.add_field(leaf, ShapeDimension::Names, "identifier", "alice");
    graph.add_child(root, "binding", leaf);

    let facts = shape_graph_facts(&graph).into_facts();

    assert!(facts.iter().any(|fact| matches!(
        fact,
        TypedFact::ShapeNode(node)
            if node.stable_key() == "root" && node.kind() == "block"
    )));
    assert!(facts.iter().any(|fact| matches!(
        fact,
        TypedFact::ShapeField(field)
            if field.dimension() == ShapeDimension::Names
                && field.name() == "identifier"
                && field.value() == "alice"
    )));
    assert!(facts.iter().any(|fact| matches!(
        fact,
        TypedFact::ShapeChild(child)
            if child.label() == "binding" && child.order() == 0
    )));
}
