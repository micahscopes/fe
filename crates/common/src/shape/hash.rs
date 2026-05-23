use std::ops::{Index, IndexMut};

use super::{ShapeDimension, ShapeGraph, ShapeNode, ShapeNodeId};

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

pub(super) fn hash_graph(graph: &ShapeGraph) -> ShapeGraphHashes {
    let local = graph.nodes().iter().map(local_digests).collect::<Vec<_>>();
    let mut tree = vec![None; graph.nodes().len()];
    let mut visiting = vec![false; graph.nodes().len()];
    for idx in 0..graph.nodes().len() {
        tree_digests(
            graph,
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
    let graph = graph_digests(graph, &node_hashes);
    ShapeGraphHashes { node_hashes, graph }
}

fn tree_digests(
    graph: &ShapeGraph,
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
    let children = graph.nodes()[idx].children().to_vec();
    for dimension in ShapeDimension::ALL {
        let mut hasher = StableHasher::default();
        hasher.write_str("shape.tree");
        hasher.write_str(dimension.as_str());
        hasher.write_digest(local[idx][dimension]);
        for child in children.iter() {
            if dimension == ShapeDimension::Structure {
                hasher.write_str(child.label());
            }
            let child_tree = tree_digests(graph, child.child(), local, tree, visiting);
            hasher.write_digest(child_tree[dimension]);
        }
        result[dimension] = hasher.finish();
    }

    visiting[idx] = false;
    tree[idx] = Some(result);
    result
}

fn graph_digests(graph: &ShapeGraph, node_hashes: &[ShapeNodeHashes]) -> DimensionDigests {
    let mut sorted_nodes = graph
        .nodes()
        .iter()
        .enumerate()
        .map(|(idx, node)| (node.stable_key(), idx))
        .collect::<Vec<_>>();
    sorted_nodes.sort_unstable_by(|lhs, rhs| lhs.0.cmp(rhs.0));

    let mut sorted_edges = graph.edges().iter().collect::<Vec<_>>();
    sorted_edges.sort_unstable_by(|lhs, rhs| {
        (
            graph.nodes()[lhs.from().index()].stable_key(),
            lhs.label(),
            graph.nodes()[lhs.to().index()].stable_key(),
        )
            .cmp(&(
                graph.nodes()[rhs.from().index()].stable_key(),
                rhs.label(),
                graph.nodes()[rhs.to().index()].stable_key(),
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
                let from = &graph.nodes()[edge.from().index()];
                let to = &graph.nodes()[edge.to().index()];
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
