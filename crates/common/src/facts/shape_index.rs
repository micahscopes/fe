use std::collections::BTreeMap;

use crate::shape::{ShapeDimension, ShapeNodeId};

use super::index_error::{require_fact_namespace, require_non_empty_shape_fact_text};
use super::{
    FactId, FactIndexError, FactNamespace, ShapeHashFact, ShapeHashFactKey, ShapeHashScope,
    ShapeNodeFact, TypedFactSet,
};

#[derive(Clone, Debug)]
pub struct ShapeFactIndex<'a> {
    nodes_by_id: BTreeMap<FactId, &'a ShapeNodeFact>,
    ids_by_source_id: BTreeMap<ShapeNodeId, FactId>,
    ids_by_stable_key: BTreeMap<String, FactId>,
    hashes_by_key: BTreeMap<ShapeHashFactKey, &'a ShapeHashFact>,
}

impl<'a> ShapeFactIndex<'a> {
    pub fn new(facts: &'a TypedFactSet) -> Result<Self, FactIndexError> {
        let mut nodes_by_id = BTreeMap::new();
        let mut ids_by_source_id = BTreeMap::new();
        let mut ids_by_stable_key = BTreeMap::new();

        for node in facts.shape_nodes() {
            require_fact_namespace(node.id(), FactNamespace::ShapeNode)?;
            require_non_empty_shape_fact_text("shape stable key", node.stable_key())?;
            require_non_empty_shape_fact_text("shape node kind", node.kind())?;
            if nodes_by_id.insert(node.id(), node).is_some() {
                return Err(FactIndexError::DuplicateShapeId);
            }
            if ids_by_source_id
                .insert(node.source_id(), node.id())
                .is_some()
            {
                return Err(FactIndexError::DuplicateShapeSourceId);
            }
            if ids_by_stable_key
                .insert(node.stable_key().to_string(), node.id())
                .is_some()
            {
                return Err(FactIndexError::DuplicateShapeStableKey);
            }
        }

        let mut index = Self {
            nodes_by_id,
            ids_by_source_id,
            ids_by_stable_key,
            hashes_by_key: BTreeMap::new(),
        };

        for field in facts.shape_fields() {
            index.require_shape_node(field.node())?;
            require_non_empty_shape_fact_text("shape field name", field.name())?;
        }
        for child in facts.shape_children() {
            index.require_shape_node(child.parent())?;
            index.require_shape_node(child.child())?;
            require_non_empty_shape_fact_text("shape child label", child.label())?;
        }
        for edge in facts.shape_edges() {
            index.require_shape_node(edge.from())?;
            index.require_shape_node(edge.to())?;
            require_non_empty_shape_fact_text("shape edge label", edge.label())?;
        }
        for event in facts.trace_events() {
            index.require_shape_node(event.node())?;
            require_non_empty_shape_fact_text("trace event kind", event.event_kind())?;
        }
        for flow in facts.data_flows() {
            index.require_shape_node(flow.source())?;
            index.require_shape_node(flow.target())?;
            require_non_empty_shape_fact_text("data flow kind", flow.kind())?;
        }

        for hash in facts.shape_hashes() {
            match (hash.scope(), hash.node()) {
                (ShapeHashScope::Local | ShapeHashScope::Tree, Some(node)) => {
                    index.require_shape_node(node)?;
                }
                (ShapeHashScope::Graph, None) => {}
                (scope, node) => {
                    return Err(FactIndexError::ShapeHashNodeScopeMismatch { scope, node });
                }
            }

            let key = ShapeHashFactKey::new(hash.node(), hash.scope(), hash.dimension());
            if index.hashes_by_key.insert(key, hash).is_some() {
                return Err(FactIndexError::DuplicateShapeHash {
                    scope: hash.scope(),
                    node: hash.node(),
                    dimension: hash.dimension(),
                });
            }
        }

        if !index.nodes_by_id.is_empty() || !index.hashes_by_key.is_empty() {
            for dimension in ShapeDimension::ALL {
                require_shape_hash(&index.hashes_by_key, ShapeHashFactKey::graph(dimension))?;

                for node in index.nodes_by_id.keys().copied() {
                    require_shape_hash(
                        &index.hashes_by_key,
                        ShapeHashFactKey::local(node, dimension),
                    )?;
                    require_shape_hash(
                        &index.hashes_by_key,
                        ShapeHashFactKey::tree(node, dimension),
                    )?;
                }
            }
        }

        Ok(index)
    }

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

    fn require_shape_node(&self, node: FactId) -> Result<(), FactIndexError> {
        require_fact_namespace(node, FactNamespace::ShapeNode)?;
        if self.nodes_by_id.contains_key(&node) {
            Ok(())
        } else {
            Err(FactIndexError::ShapeFactMissingNode { node })
        }
    }
}

fn require_shape_hash(
    hashes_by_key: &BTreeMap<ShapeHashFactKey, &ShapeHashFact>,
    key: ShapeHashFactKey,
) -> Result<(), FactIndexError> {
    if hashes_by_key.contains_key(&key) {
        Ok(())
    } else {
        Err(FactIndexError::MissingShapeHash {
            scope: key.scope(),
            node: key.node(),
            dimension: key.dimension(),
        })
    }
}
