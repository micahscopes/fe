use crate::{
    facts::{FactId, ShapeFactIndex, ShapeHashFact, ShapeHashFactKey, ShapeNodeFact},
    shape::{ShapeDimension, ShapeNodeId},
};

impl<'a> ShapeFactIndex<'a> {
    pub fn shape_id_by_source_id(&self, source_id: ShapeNodeId) -> Option<FactId> {
        self.ids_by_source_id.get(&source_id).copied()
    }

    pub fn shape_id_by_stable_key(&self, stable_key: &str) -> Option<FactId> {
        self.ids_by_stable_key.get(stable_key).copied()
    }

    pub fn shape_node(&self, id: FactId) -> Option<&ShapeNodeFact> {
        self.nodes_by_id.get(&id).copied()
    }

    pub fn shape_hash(&self, key: ShapeHashFactKey) -> Option<&ShapeHashFact> {
        self.hashes_by_key.get(&key).copied()
    }

    pub fn graph_hash(&self, dimension: ShapeDimension) -> Option<&ShapeHashFact> {
        self.shape_hash(ShapeHashFactKey::graph(dimension))
    }

    pub fn local_hash(&self, node: FactId, dimension: ShapeDimension) -> Option<&ShapeHashFact> {
        self.shape_hash(ShapeHashFactKey::local(node, dimension))
    }

    pub fn tree_hash(&self, node: FactId, dimension: ShapeDimension) -> Option<&ShapeHashFact> {
        self.shape_hash(ShapeHashFactKey::tree(node, dimension))
    }
}
