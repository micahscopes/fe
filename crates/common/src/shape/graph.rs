use serde::{Deserialize, Serialize};

use super::ShapeGraphHashes;

crate::define_closed_string_enum! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, salsa::Update)]
    pub enum ShapeDimension {
        Structure => "structure",
        Names => "names",
        Constants => "constants",
        Types => "types",
        TraceEvents => "trace_events",
    }
}

impl ShapeDimension {
    pub const ALL: [Self; 5] = [
        Self::Structure,
        Self::Names,
        Self::Constants,
        Self::Types,
        Self::TraceEvents,
    ];

    pub(super) const fn index(self) -> usize {
        match self {
            Self::Structure => 0,
            Self::Names => 1,
            Self::Constants => 2,
            Self::Types => 3,
            Self::TraceEvents => 4,
        }
    }
}

#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, salsa::Update, Serialize, Deserialize,
)]
#[serde(transparent)]
pub struct ShapeNodeId(u32);

impl ShapeNodeId {
    pub const fn from_u32(raw: u32) -> Self {
        Self(raw)
    }

    pub const fn as_u32(self) -> u32 {
        self.0
    }

    pub const fn index(self) -> usize {
        self.0 as usize
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, salsa::Update)]
pub struct ShapeField {
    dimension: ShapeDimension,
    name: String,
    value: String,
}

impl ShapeField {
    pub fn new(
        dimension: ShapeDimension,
        name: impl Into<String>,
        value: impl Into<String>,
    ) -> Self {
        let name = name.into();
        assert_non_empty_shape_text("shape field name", &name);
        Self {
            dimension,
            name,
            value: value.into(),
        }
    }

    pub const fn dimension(&self) -> ShapeDimension {
        self.dimension
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn value(&self) -> &str {
        &self.value
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, salsa::Update)]
pub struct ShapeNode {
    stable_key: String,
    kind: String,
    fields: Vec<ShapeField>,
    children: Vec<ShapeChild>,
}

impl ShapeNode {
    pub fn new(stable_key: impl Into<String>, kind: impl Into<String>) -> Self {
        let stable_key = stable_key.into();
        let kind = kind.into();
        assert_non_empty_shape_text("shape stable key", &stable_key);
        assert_non_empty_shape_text("shape node kind", &kind);
        Self {
            stable_key,
            kind,
            fields: Vec::new(),
            children: Vec::new(),
        }
    }

    pub fn stable_key(&self) -> &str {
        &self.stable_key
    }

    pub fn kind(&self) -> &str {
        &self.kind
    }

    pub fn fields(&self) -> &[ShapeField] {
        &self.fields
    }

    pub fn children(&self) -> &[ShapeChild] {
        &self.children
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, salsa::Update)]
pub struct ShapeChild {
    label: String,
    child: ShapeNodeId,
}

impl ShapeChild {
    pub fn new(label: impl Into<String>, child: ShapeNodeId) -> Self {
        let label = label.into();
        assert_non_empty_shape_text("shape child label", &label);
        Self { label, child }
    }

    pub fn label(&self) -> &str {
        &self.label
    }

    pub const fn child(&self) -> ShapeNodeId {
        self.child
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, salsa::Update)]
pub struct ShapeEdge {
    from: ShapeNodeId,
    to: ShapeNodeId,
    label: String,
}

impl ShapeEdge {
    pub fn new(from: ShapeNodeId, to: ShapeNodeId, label: impl Into<String>) -> Self {
        let label = label.into();
        assert_non_empty_shape_text("shape edge label", &label);
        Self { from, to, label }
    }

    pub const fn from(&self) -> ShapeNodeId {
        self.from
    }

    pub const fn to(&self) -> ShapeNodeId {
        self.to
    }

    pub fn label(&self) -> &str {
        &self.label
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, salsa::Update)]
pub struct ShapeGraph {
    nodes: Vec<ShapeNode>,
    edges: Vec<ShapeEdge>,
}

impl Default for ShapeGraph {
    fn default() -> Self {
        Self::new()
    }
}

impl ShapeGraph {
    pub const fn new() -> Self {
        Self {
            nodes: Vec::new(),
            edges: Vec::new(),
        }
    }

    pub fn add_node(
        &mut self,
        stable_key: impl Into<String>,
        kind: impl Into<String>,
    ) -> ShapeNodeId {
        let stable_key = stable_key.into();
        let kind = kind.into();
        assert_non_empty_shape_text("shape stable key", &stable_key);
        assert_non_empty_shape_text("shape node kind", &kind);
        assert!(
            self.nodes
                .iter()
                .all(|node| node.stable_key() != stable_key),
            "duplicate shape stable key: {stable_key}"
        );
        let id = ShapeNodeId::from_u32(self.nodes.len() as u32);
        self.nodes.push(ShapeNode::new(stable_key, kind));
        id
    }

    pub fn add_field(
        &mut self,
        node: ShapeNodeId,
        dimension: ShapeDimension,
        name: impl Into<String>,
        value: impl Into<String>,
    ) {
        self.node_mut(node)
            .fields
            .push(ShapeField::new(dimension, name, value));
    }

    pub fn add_child(&mut self, parent: ShapeNodeId, label: impl Into<String>, child: ShapeNodeId) {
        self.assert_node(child);
        self.node_mut(parent)
            .children
            .push(ShapeChild::new(label, child));
    }

    pub fn add_edge(&mut self, from: ShapeNodeId, to: ShapeNodeId, label: impl Into<String>) {
        self.assert_node(from);
        self.assert_node(to);
        self.edges.push(ShapeEdge::new(from, to, label));
    }

    pub fn nodes(&self) -> &[ShapeNode] {
        &self.nodes
    }

    pub fn edges(&self) -> &[ShapeEdge] {
        &self.edges
    }

    pub fn node(&self, node: ShapeNodeId) -> Option<&ShapeNode> {
        self.nodes.get(node.index())
    }

    pub fn hashes(&self) -> ShapeGraphHashes {
        super::hash::hash_graph(self)
    }

    fn node_mut(&mut self, node: ShapeNodeId) -> &mut ShapeNode {
        self.assert_node(node);
        &mut self.nodes[node.index()]
    }

    fn assert_node(&self, node: ShapeNodeId) {
        assert!(
            node.index() < self.nodes.len(),
            "shape node id out of bounds: {}",
            node.as_u32()
        );
    }
}

fn assert_non_empty_shape_text(field: &'static str, value: &str) {
    assert!(!value.is_empty(), "{field} must not be empty");
}
