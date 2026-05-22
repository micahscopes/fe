use std::{
    fmt::Write as _,
    ops::{Index, IndexMut},
};

use serde::{Deserialize, Serialize};

pub use fe_shape_derive::ShapeDescribe;

/// Stable FNV-1a digest used by shape hashing.
///
/// This is intentionally small for now: the important invariant is that shape
/// hashing is deterministic and explicit, not tied to process-randomized hash
/// maps or callback order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, salsa::Update)]
pub struct StableDigest(u64);

impl StableDigest {
    pub const EMPTY: Self = Self(0xcbf29ce484222325);

    pub const fn as_u64(self) -> u64 {
        self.0
    }

    pub fn to_hex(self) -> String {
        format!("{:016x}", self.0)
    }
}

#[derive(Clone, Debug)]
struct StableHasher {
    state: u64,
}

impl Default for StableHasher {
    fn default() -> Self {
        Self {
            state: StableDigest::EMPTY.0,
        }
    }
}

impl StableHasher {
    fn write_u64(&mut self, value: u64) {
        self.write_bytes(&value.to_le_bytes());
    }

    fn write_str(&mut self, value: &str) {
        self.write_u64(value.len() as u64);
        self.write_bytes(value.as_bytes());
    }

    fn write_digest(&mut self, digest: StableDigest) {
        self.write_u64(digest.0);
    }

    fn write_bytes(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.state ^= u64::from(*byte);
            self.state = self.state.wrapping_mul(0x100000001b3);
        }
    }

    fn finish(self) -> StableDigest {
        StableDigest(self.state)
    }
}

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

    const fn index(self) -> usize {
        match self {
            Self::Structure => 0,
            Self::Names => 1,
            Self::Constants => 2,
            Self::Types => 3,
            Self::TraceEvents => 4,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, salsa::Update)]
pub struct DimensionDigests {
    digests: [StableDigest; 5],
}

impl DimensionDigests {
    pub const EMPTY: Self = Self {
        digests: [StableDigest::EMPTY; 5],
    };

    pub fn digest(self, dimension: ShapeDimension) -> StableDigest {
        self[dimension]
    }

    pub fn exact(self) -> StableDigest {
        let mut hasher = StableHasher::default();
        hasher.write_str("shape.dimension.exact");
        for dimension in ShapeDimension::ALL {
            hasher.write_str(dimension.as_str());
            hasher.write_digest(self[dimension]);
        }
        hasher.finish()
    }
}

impl Default for DimensionDigests {
    fn default() -> Self {
        Self::EMPTY
    }
}

impl Index<ShapeDimension> for DimensionDigests {
    type Output = StableDigest;

    fn index(&self, index: ShapeDimension) -> &Self::Output {
        &self.digests[index.index()]
    }
}

impl IndexMut<ShapeDimension> for DimensionDigests {
    fn index_mut(&mut self, index: ShapeDimension) -> &mut Self::Output {
        &mut self.digests[index.index()]
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
        let local = self.nodes.iter().map(local_digests).collect::<Vec<_>>();
        let mut tree = vec![None; self.nodes.len()];
        let mut visiting = vec![false; self.nodes.len()];
        for idx in 0..self.nodes.len() {
            self.tree_digests(
                ShapeNodeId::from_u32(idx as u32),
                &local,
                &mut tree,
                &mut visiting,
            );
        }
        let tree = tree
            .into_iter()
            .map(|digest| digest.expect("tree digest should be computed"))
            .collect::<Vec<_>>();
        let node_hashes = local
            .into_iter()
            .zip(tree.iter().copied())
            .map(|(local, tree)| ShapeNodeHashes { local, tree })
            .collect::<Vec<_>>();
        let graph = self.graph_digests(&node_hashes);
        ShapeGraphHashes { node_hashes, graph }
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

    fn tree_digests(
        &self,
        node: ShapeNodeId,
        local: &[DimensionDigests],
        tree: &mut [Option<DimensionDigests>],
        visiting: &mut [bool],
    ) -> DimensionDigests {
        let idx = node.index();
        if let Some(digest) = tree[idx] {
            return digest;
        }
        assert!(!visiting[idx], "shape child graph contains a cycle");
        visiting[idx] = true;

        let mut result = DimensionDigests::EMPTY;
        let children = self.nodes[idx].children.clone();
        for dimension in ShapeDimension::ALL {
            let mut hasher = StableHasher::default();
            hasher.write_str("shape.tree");
            hasher.write_str(dimension.as_str());
            hasher.write_digest(local[idx][dimension]);
            for child in children.iter() {
                if dimension == ShapeDimension::Structure {
                    hasher.write_str(child.label());
                }
                let child_tree = self.tree_digests(child.child(), local, tree, visiting);
                hasher.write_digest(child_tree[dimension]);
            }
            result[dimension] = hasher.finish();
        }

        visiting[idx] = false;
        tree[idx] = Some(result);
        result
    }

    fn graph_digests(&self, node_hashes: &[ShapeNodeHashes]) -> DimensionDigests {
        let mut sorted_nodes = self
            .nodes
            .iter()
            .enumerate()
            .map(|(idx, node)| (node.stable_key(), idx))
            .collect::<Vec<_>>();
        sorted_nodes.sort_unstable_by(|lhs, rhs| lhs.0.cmp(rhs.0));

        let mut sorted_edges = self.edges.iter().collect::<Vec<_>>();
        sorted_edges.sort_unstable_by(|lhs, rhs| {
            (
                self.nodes[lhs.from().index()].stable_key(),
                lhs.label(),
                self.nodes[lhs.to().index()].stable_key(),
            )
                .cmp(&(
                    self.nodes[rhs.from().index()].stable_key(),
                    rhs.label(),
                    self.nodes[rhs.to().index()].stable_key(),
                ))
        });

        let mut result = DimensionDigests::EMPTY;
        for dimension in ShapeDimension::ALL {
            let mut hasher = StableHasher::default();
            hasher.write_str("shape.graph");
            hasher.write_str(dimension.as_str());

            for (stable_key, idx) in sorted_nodes.iter().copied() {
                hasher.write_str(stable_key);
                hasher.write_digest(node_hashes[idx].tree()[dimension]);
            }

            if dimension == ShapeDimension::Structure {
                for edge in sorted_edges.iter().copied() {
                    let from = &self.nodes[edge.from().index()];
                    let to = &self.nodes[edge.to().index()];
                    hasher.write_str(from.stable_key());
                    hasher.write_str(edge.label());
                    hasher.write_str(to.stable_key());
                    hasher.write_digest(
                        node_hashes[edge.from().index()]
                            .tree()
                            .digest(ShapeDimension::Structure),
                    );
                    hasher.write_digest(
                        node_hashes[edge.to().index()]
                            .tree()
                            .digest(ShapeDimension::Structure),
                    );
                }
            }

            result[dimension] = hasher.finish();
        }
        result
    }
}

fn assert_non_empty_shape_text(field: &'static str, value: &str) {
    assert!(!value.is_empty(), "{field} must not be empty");
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ShapeBuilder {
    graph: ShapeGraph,
    next_auto_key: u32,
}

impl Default for ShapeBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl ShapeBuilder {
    pub const fn new() -> Self {
        Self {
            graph: ShapeGraph::new(),
            next_auto_key: 0,
        }
    }

    pub fn into_graph(self) -> ShapeGraph {
        self.graph
    }

    pub fn add_described_node(
        &mut self,
        kind: impl Into<String>,
        stable_key: Option<String>,
    ) -> ShapeNodeId {
        let kind = kind.into();
        let stable_key = stable_key.unwrap_or_else(|| {
            let key = format!("auto:{:08}:{kind}", self.next_auto_key);
            self.next_auto_key += 1;
            key
        });
        self.graph.add_node(stable_key, kind)
    }

    pub fn add_field_value<V: ShapeFieldValue + ?Sized>(
        &mut self,
        node: ShapeNodeId,
        dimension: ShapeDimension,
        name: impl Into<String>,
        value: &V,
    ) {
        self.graph
            .add_field(node, dimension, name, value.shape_field_value());
    }

    pub fn add_child_node<T: ShapeDescribe + ?Sized>(
        &mut self,
        parent: ShapeNodeId,
        label: impl Into<String>,
        child: &T,
    ) -> ShapeNodeId {
        let child_node = child.describe_shape(self);
        self.graph.add_child(parent, label, child_node);
        child_node
    }
}

pub trait ShapeDescribe {
    fn describe_shape(&self, builder: &mut ShapeBuilder) -> ShapeNodeId;

    fn shape_graph(&self) -> ShapeGraph
    where
        Self: Sized,
    {
        let mut builder = ShapeBuilder::new();
        self.describe_shape(&mut builder);
        builder.into_graph()
    }

    fn shape_hashes(&self) -> ShapeGraphHashes
    where
        Self: Sized,
    {
        self.shape_graph().hashes()
    }
}

impl<T: ShapeDescribe + ?Sized> ShapeDescribe for Box<T> {
    fn describe_shape(&self, builder: &mut ShapeBuilder) -> ShapeNodeId {
        (**self).describe_shape(builder)
    }
}

impl<T: ShapeDescribe> ShapeDescribe for Option<T> {
    fn describe_shape(&self, builder: &mut ShapeBuilder) -> ShapeNodeId {
        let node = builder.add_described_node("Option", None);
        match self {
            Some(value) => {
                builder.add_field_value(node, ShapeDimension::Structure, "variant", "Some");
                builder.add_child_node(node, "some", value);
            }
            None => builder.add_field_value(node, ShapeDimension::Structure, "variant", "None"),
        }
        node
    }
}

impl<T: ShapeDescribe> ShapeDescribe for Vec<T> {
    fn describe_shape(&self, builder: &mut ShapeBuilder) -> ShapeNodeId {
        self.as_slice().describe_shape(builder)
    }
}

impl<T: ShapeDescribe> ShapeDescribe for [T] {
    fn describe_shape(&self, builder: &mut ShapeBuilder) -> ShapeNodeId {
        let node = builder.add_described_node("slice", None);
        for (idx, child) in self.iter().enumerate() {
            builder.add_child_node(node, format!("item:{idx}"), child);
        }
        node
    }
}

impl<A: ShapeDescribe, B: ShapeDescribe> ShapeDescribe for (A, B) {
    fn describe_shape(&self, builder: &mut ShapeBuilder) -> ShapeNodeId {
        let node = builder.add_described_node("tuple2", None);
        builder.add_child_node(node, "0", &self.0);
        builder.add_child_node(node, "1", &self.1);
        node
    }
}

pub trait ShapeFieldValue {
    fn shape_field_value(&self) -> String;
}

impl<T: ShapeFieldValue + ?Sized> ShapeFieldValue for &T {
    fn shape_field_value(&self) -> String {
        (*self).shape_field_value()
    }
}

macro_rules! impl_display_shape_field_value {
    ($($ty:ty),* $(,)?) => {
        $(
            impl ShapeFieldValue for $ty {
                fn shape_field_value(&self) -> String {
                    self.to_string()
                }
            }
        )*
    };
}

impl_display_shape_field_value!(
    bool, u8, u16, u32, u64, u128, usize, i8, i16, i32, i64, i128, isize
);

impl ShapeFieldValue for str {
    fn shape_field_value(&self) -> String {
        self.to_owned()
    }
}

impl ShapeFieldValue for String {
    fn shape_field_value(&self) -> String {
        self.clone()
    }
}

impl ShapeFieldValue for [u8] {
    fn shape_field_value(&self) -> String {
        bytes_to_hex(self)
    }
}

impl ShapeFieldValue for Vec<u8> {
    fn shape_field_value(&self) -> String {
        bytes_to_hex(self)
    }
}

impl ShapeFieldValue for Box<[u8]> {
    fn shape_field_value(&self) -> String {
        bytes_to_hex(self)
    }
}

macro_rules! impl_scalar_shape_describe {
    ($($ty:ty),* $(,)?) => {
        $(
            impl ShapeDescribe for $ty {
                fn describe_shape(&self, builder: &mut ShapeBuilder) -> ShapeNodeId {
                    let node = builder.add_described_node(stringify!($ty), None);
                    builder.add_field_value(node, ShapeDimension::Constants, "value", self);
                    node
                }
            }
        )*
    };
}

impl_scalar_shape_describe!(
    bool, u8, u16, u32, u64, u128, usize, i8, i16, i32, i64, i128, isize
);

fn bytes_to_hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        write!(&mut out, "{byte:02x}").expect("writing to String should not fail");
    }
    out
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, salsa::Update)]
pub struct ShapeNodeHashes {
    local: DimensionDigests,
    tree: DimensionDigests,
}

impl ShapeNodeHashes {
    pub const fn local(&self) -> DimensionDigests {
        self.local
    }

    pub const fn tree(&self) -> DimensionDigests {
        self.tree
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, salsa::Update)]
pub struct ShapeGraphHashes {
    node_hashes: Vec<ShapeNodeHashes>,
    graph: DimensionDigests,
}

impl ShapeGraphHashes {
    pub fn node(&self, node: ShapeNodeId) -> Option<&ShapeNodeHashes> {
        self.node_hashes.get(node.index())
    }

    pub fn graph(&self) -> DimensionDigests {
        self.graph
    }
}

fn local_digests(node: &ShapeNode) -> DimensionDigests {
    let mut result = DimensionDigests::EMPTY;
    for dimension in ShapeDimension::ALL {
        let mut hasher = StableHasher::default();
        hasher.write_str("shape.local");
        hasher.write_str(dimension.as_str());
        if dimension == ShapeDimension::Structure {
            hasher.write_str(node.kind());
        }
        let mut fields = node
            .fields()
            .iter()
            .filter(|field| field.dimension() == dimension)
            .collect::<Vec<_>>();
        fields
            .sort_unstable_by(|lhs, rhs| (lhs.name(), lhs.value()).cmp(&(rhs.name(), rhs.value())));
        for field in fields {
            hasher.write_str(field.name());
            hasher.write_str(field.value());
        }
        result[dimension] = hasher.finish();
    }
    result
}

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
