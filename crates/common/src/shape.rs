mod describe;
mod field_value;
mod graph;
mod hash;

pub use describe::{ShapeBuilder, ShapeDescribe};
pub use fe_shape_derive::ShapeDescribe;
pub use field_value::ShapeFieldValue;
pub use graph::{
    ShapeChild, ShapeDimension, ShapeEdge, ShapeField, ShapeGraph, ShapeNode, ShapeNodeId,
};
pub use hash::{DimensionDigests, ShapeGraphHashes, ShapeNodeHashes, StableDigest};

#[cfg(test)]
mod tests {
    use super::{ShapeDescribe, ShapeDimension, ShapeGraph};

    #[derive(ShapeDescribe)]
    enum DerivedExpr {
        Lit {
            #[shape(field = Constants)]
            value: u32,
        },
        Var {
            #[shape(field = Names)]
            name: String,
        },
        Let {
            #[shape(field = Names)]
            binding: String,
            #[shape(child)]
            value: Box<DerivedExpr>,
            #[shape(child)]
            body: Box<DerivedExpr>,
        },
    }

    #[derive(ShapeDescribe)]
    #[shape(kind = "stable.struct", stable_key = stable_struct_key)]
    struct DerivedStableStruct {
        #[shape(skip = "identity is represented by the derived stable key")]
        id: u32,
        #[shape(field = Constants)]
        value: u32,
    }

    #[derive(ShapeDescribe)]
    #[shape(stable_key = stable_enum_key)]
    enum DerivedStableEnum {
        #[shape(kind = "stable.variant.named", stable_key = stable_named_variant_key)]
        Named {
            #[shape(skip = "identity is represented by the derived stable key")]
            id: u32,
            #[shape(field = Names)]
            name: String,
        },
        Unnamed(
            #[shape(skip = "identity is represented by the derived stable key")] u32,
            #[shape(field = Constants)] u32,
        ),
    }

    fn stable_struct_key(value: &DerivedStableStruct) -> String {
        format!("stable-struct:{}", value.id)
    }

    fn stable_enum_key(value: &DerivedStableEnum) -> String {
        match value {
            DerivedStableEnum::Named { id, .. } => format!("enum-container:{id}"),
            DerivedStableEnum::Unnamed(id, _) => format!("enum-container:{id}"),
        }
    }

    fn stable_named_variant_key(value: &DerivedStableEnum) -> String {
        match value {
            DerivedStableEnum::Named { id, .. } => format!("enum-named:{id}"),
            DerivedStableEnum::Unnamed(id, _) => format!("enum-named:{id}"),
        }
    }

    fn graph_with_child_constant(value: &str, edge_label: &str) -> ShapeGraph {
        let mut graph = ShapeGraph::new();
        let stmt = graph.add_node("stmt:0", "stmt");
        let expr = graph.add_node("expr:0", "literal");
        graph.add_field(expr, ShapeDimension::Constants, "value", value);
        graph.add_child(stmt, "expr", expr);
        graph.add_edge(stmt, expr, edge_label);
        graph
    }

    #[test]
    #[should_panic(expected = "shape stable key must not be empty")]
    fn shape_graph_rejects_empty_stable_keys() {
        let mut graph = ShapeGraph::new();
        graph.add_node("", "stmt");
    }

    #[test]
    #[should_panic(expected = "shape node kind must not be empty")]
    fn shape_graph_rejects_empty_node_kinds() {
        let mut graph = ShapeGraph::new();
        graph.add_node("stmt:0", "");
    }

    #[test]
    #[should_panic(expected = "shape field name must not be empty")]
    fn shape_graph_rejects_empty_field_names() {
        let mut graph = ShapeGraph::new();
        let node = graph.add_node("stmt:0", "stmt");
        graph.add_field(node, ShapeDimension::Names, "", "main");
    }

    #[test]
    #[should_panic(expected = "shape child label must not be empty")]
    fn shape_graph_rejects_empty_child_labels() {
        let mut graph = ShapeGraph::new();
        let parent = graph.add_node("stmt:0", "stmt");
        let child = graph.add_node("expr:0", "literal");
        graph.add_child(parent, "", child);
    }

    #[test]
    #[should_panic(expected = "shape edge label must not be empty")]
    fn shape_graph_rejects_empty_edge_labels() {
        let mut graph = ShapeGraph::new();
        let from = graph.add_node("stmt:0", "stmt");
        let to = graph.add_node("expr:0", "literal");
        graph.add_edge(from, to, "");
    }

    #[test]
    fn graph_edges_do_not_suppress_child_content_hashing() {
        let first = graph_with_child_constant("1", "cfg:stmt-to-expr");
        let second = graph_with_child_constant("2", "cfg:stmt-to-expr");

        let first_hashes = first.hashes();
        let second_hashes = second.hashes();

        assert_ne!(
            first_hashes
                .node(super::ShapeNodeId::from_u32(0))
                .unwrap()
                .tree()
                .digest(ShapeDimension::Constants),
            second_hashes
                .node(super::ShapeNodeId::from_u32(0))
                .unwrap()
                .tree()
                .digest(ShapeDimension::Constants)
        );
        assert_ne!(first_hashes.graph().exact(), second_hashes.graph().exact());
    }

    #[test]
    fn graph_edges_do_not_pollute_structure_with_endpoint_content_dimensions() {
        let first = graph_with_child_constant("1", "cfg:stmt-to-expr");
        let second = graph_with_child_constant("2", "cfg:stmt-to-expr");

        let first_hashes = first.hashes();
        let second_hashes = second.hashes();

        assert_eq!(
            first_hashes.graph().digest(ShapeDimension::Structure),
            second_hashes.graph().digest(ShapeDimension::Structure),
            "constant-only endpoint changes must not alter the structure projection"
        );
        assert_ne!(
            first_hashes.graph().digest(ShapeDimension::Constants),
            second_hashes.graph().digest(ShapeDimension::Constants),
            "constant endpoint changes must still affect the constants projection"
        );
        assert_ne!(
            first_hashes.graph().exact(),
            second_hashes.graph().exact(),
            "exact graph digest should still include all dimension changes"
        );
    }

    #[test]
    fn graph_digest_observes_edge_label_changes_without_tree_changes() {
        let first = graph_with_child_constant("1", "cfg:then");
        let second = graph_with_child_constant("1", "cfg:else");

        let first_hashes = first.hashes();
        let second_hashes = second.hashes();

        assert_eq!(
            first_hashes
                .node(super::ShapeNodeId::from_u32(0))
                .unwrap()
                .tree()
                .exact(),
            second_hashes
                .node(super::ShapeNodeId::from_u32(0))
                .unwrap()
                .tree()
                .exact()
        );
        assert_ne!(
            first_hashes.graph().digest(ShapeDimension::Structure),
            second_hashes.graph().digest(ShapeDimension::Structure)
        );
    }

    #[test]
    fn graph_digest_uses_full_edge_labels() {
        let common_prefix = "cfg:edge-label-with-a-long-common-prefix-that-must-not-be-truncated:";
        let first = graph_with_child_constant("1", &format!("{common_prefix}left"));
        let second = graph_with_child_constant("1", &format!("{common_prefix}right"));

        assert_ne!(
            first.hashes().graph().digest(ShapeDimension::Structure),
            second.hashes().graph().digest(ShapeDimension::Structure)
        );
    }

    #[test]
    fn local_field_hashes_do_not_depend_on_insertion_order() {
        let mut first = ShapeGraph::new();
        let first_node = first.add_node("expr:0", "literal");
        first.add_field(first_node, ShapeDimension::Names, "identifier", "value");
        first.add_field(first_node, ShapeDimension::Constants, "literal", "1");
        first.add_field(first_node, ShapeDimension::Types, "ty", "u256");

        let mut second = ShapeGraph::new();
        let second_node = second.add_node("expr:0", "literal");
        second.add_field(second_node, ShapeDimension::Types, "ty", "u256");
        second.add_field(second_node, ShapeDimension::Constants, "literal", "1");
        second.add_field(second_node, ShapeDimension::Names, "identifier", "value");

        assert_eq!(
            first.hashes().graph().exact(),
            second.hashes().graph().exact(),
            "shape fields are unordered metadata; use children for ordered content"
        );
    }

    #[test]
    fn child_label_changes_tree_structure_only() {
        let mut first = ShapeGraph::new();
        let first_parent = first.add_node("stmt:0", "stmt");
        let first_child = first.add_node("expr:0", "literal");
        first.add_field(first_child, ShapeDimension::Constants, "value", "1");
        first.add_child(first_parent, "then", first_child);

        let mut second = ShapeGraph::new();
        let second_parent = second.add_node("stmt:0", "stmt");
        let second_child = second.add_node("expr:0", "literal");
        second.add_field(second_child, ShapeDimension::Constants, "value", "1");
        second.add_child(second_parent, "else", second_child);

        let first_hashes = first.hashes();
        let second_hashes = second.hashes();

        assert_ne!(
            first_hashes
                .node(super::ShapeNodeId::from_u32(0))
                .unwrap()
                .tree()
                .digest(ShapeDimension::Structure),
            second_hashes
                .node(super::ShapeNodeId::from_u32(0))
                .unwrap()
                .tree()
                .digest(ShapeDimension::Structure)
        );
        assert_eq!(
            first_hashes
                .node(super::ShapeNodeId::from_u32(0))
                .unwrap()
                .tree()
                .digest(ShapeDimension::Constants),
            second_hashes
                .node(super::ShapeNodeId::from_u32(0))
                .unwrap()
                .tree()
                .digest(ShapeDimension::Constants)
        );
    }

    #[test]
    fn child_order_remains_part_of_tree_hashes() {
        let mut first = ShapeGraph::new();
        let first_parent = first.add_node("tuple:0", "tuple");
        let first_left = first.add_node("literal:0", "literal");
        let first_right = first.add_node("literal:1", "literal");
        first.add_field(first_left, ShapeDimension::Constants, "value", "1");
        first.add_field(first_right, ShapeDimension::Constants, "value", "2");
        first.add_child(first_parent, "item", first_left);
        first.add_child(first_parent, "item", first_right);

        let mut second = ShapeGraph::new();
        let second_parent = second.add_node("tuple:0", "tuple");
        let second_left = second.add_node("literal:0", "literal");
        let second_right = second.add_node("literal:1", "literal");
        second.add_field(second_left, ShapeDimension::Constants, "value", "1");
        second.add_field(second_right, ShapeDimension::Constants, "value", "2");
        second.add_child(second_parent, "item", second_right);
        second.add_child(second_parent, "item", second_left);

        assert_ne!(
            first
                .hashes()
                .node(first_parent)
                .unwrap()
                .tree()
                .digest(ShapeDimension::Constants),
            second
                .hashes()
                .node(second_parent)
                .unwrap()
                .tree()
                .digest(ShapeDimension::Constants),
            "ordered content belongs in children, not unordered fields"
        );
    }

    #[test]
    fn dimension_projection_keeps_names_and_constants_separate() {
        let mut first = ShapeGraph::new();
        let first_node = first.add_node("expr:0", "path");
        first.add_field(first_node, ShapeDimension::Names, "identifier", "alice");
        first.add_field(first_node, ShapeDimension::Constants, "literal", "1");

        let mut second = ShapeGraph::new();
        let second_node = second.add_node("expr:0", "path");
        second.add_field(second_node, ShapeDimension::Names, "identifier", "bob");
        second.add_field(second_node, ShapeDimension::Constants, "literal", "1");

        let first_hashes = first.hashes();
        let second_hashes = second.hashes();

        assert_ne!(
            first_hashes.graph().digest(ShapeDimension::Names),
            second_hashes.graph().digest(ShapeDimension::Names)
        );
        assert_eq!(
            first_hashes.graph().digest(ShapeDimension::Constants),
            second_hashes.graph().digest(ShapeDimension::Constants)
        );
    }

    #[test]
    fn derived_shape_keeps_constants_out_of_structure() {
        let first = DerivedExpr::Lit { value: 1 }.shape_hashes();
        let second = DerivedExpr::Lit { value: 2 }.shape_hashes();

        assert_eq!(
            first.graph().digest(ShapeDimension::Structure),
            second.graph().digest(ShapeDimension::Structure)
        );
        assert_ne!(
            first.graph().digest(ShapeDimension::Constants),
            second.graph().digest(ShapeDimension::Constants)
        );
    }

    #[test]
    fn derived_shape_child_content_reaches_parent_tree() {
        let first = DerivedExpr::Let {
            binding: "x".to_string(),
            value: Box::new(DerivedExpr::Lit { value: 1 }),
            body: Box::new(DerivedExpr::Var {
                name: "x".to_string(),
            }),
        }
        .shape_hashes();
        let second = DerivedExpr::Let {
            binding: "x".to_string(),
            value: Box::new(DerivedExpr::Lit { value: 2 }),
            body: Box::new(DerivedExpr::Var {
                name: "x".to_string(),
            }),
        }
        .shape_hashes();

        assert_ne!(
            first
                .node(super::ShapeNodeId::from_u32(0))
                .unwrap()
                .tree()
                .digest(ShapeDimension::Constants),
            second
                .node(super::ShapeNodeId::from_u32(0))
                .unwrap()
                .tree()
                .digest(ShapeDimension::Constants)
        );
    }

    #[test]
    fn derived_shape_uses_declared_stable_keys() {
        let graph = DerivedStableStruct { id: 7, value: 1 }.shape_graph();
        let root = graph.node(super::ShapeNodeId::from_u32(0)).unwrap();

        assert_eq!(root.kind(), "stable.struct");
        assert_eq!(root.stable_key(), "stable-struct:7");
    }

    #[test]
    fn derived_enum_variant_stable_key_overrides_container_key() {
        let named = DerivedStableEnum::Named {
            id: 11,
            name: "alice".to_string(),
        }
        .shape_graph();
        let named_root = named.node(super::ShapeNodeId::from_u32(0)).unwrap();
        assert_eq!(named_root.kind(), "stable.variant.named");
        assert_eq!(named_root.stable_key(), "enum-named:11");

        let unnamed = DerivedStableEnum::Unnamed(13, 2).shape_graph();
        let unnamed_root = unnamed.node(super::ShapeNodeId::from_u32(0)).unwrap();
        assert_eq!(unnamed_root.kind(), "DerivedStableEnum::Unnamed");
        assert_eq!(unnamed_root.stable_key(), "enum-container:13");
    }
}
