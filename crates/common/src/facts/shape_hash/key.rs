use std::fmt;

use crate::{facts::FactId, shape::ShapeDimension};

use super::ShapeHashScope;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ShapeHashFactKey {
    node: Option<FactId>,
    scope: ShapeHashScope,
    dimension: ShapeDimension,
}

impl ShapeHashFactKey {
    pub fn new(node: Option<FactId>, scope: ShapeHashScope, dimension: ShapeDimension) -> Self {
        Self::try_new(node, scope, dimension).unwrap_or_else(|err| panic!("{err}"))
    }

    pub fn try_new(
        node: Option<FactId>,
        scope: ShapeHashScope,
        dimension: ShapeDimension,
    ) -> Result<Self, ShapeHashNodeScopeError> {
        validate_shape_hash_node_scope(scope, node)?;
        Ok(Self {
            node,
            scope,
            dimension,
        })
    }

    pub const fn local(node: FactId, dimension: ShapeDimension) -> Self {
        Self {
            node: Some(node),
            scope: ShapeHashScope::Local,
            dimension,
        }
    }

    pub const fn tree(node: FactId, dimension: ShapeDimension) -> Self {
        Self {
            node: Some(node),
            scope: ShapeHashScope::Tree,
            dimension,
        }
    }

    pub const fn graph(dimension: ShapeDimension) -> Self {
        Self {
            node: None,
            scope: ShapeHashScope::Graph,
            dimension,
        }
    }

    pub const fn node(self) -> Option<FactId> {
        self.node
    }

    pub const fn scope(self) -> ShapeHashScope {
        self.scope
    }

    pub const fn dimension(self) -> ShapeDimension {
        self.dimension
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShapeHashNodeScopeError {
    scope: ShapeHashScope,
    node: Option<FactId>,
}

impl ShapeHashNodeScopeError {
    pub const fn new(scope: ShapeHashScope, node: Option<FactId>) -> Self {
        Self { scope, node }
    }

    pub const fn scope(&self) -> ShapeHashScope {
        self.scope
    }

    pub const fn node(&self) -> Option<FactId> {
        self.node
    }
}

impl fmt::Display for ShapeHashNodeScopeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let node = self
            .node
            .map(FactId::stable_key)
            .unwrap_or_else(|| "none".to_string());
        write!(
            f,
            "shape hash scope {} has invalid node reference {}",
            self.scope.as_str(),
            node
        )
    }
}

impl std::error::Error for ShapeHashNodeScopeError {}

pub(in crate::facts::shape_hash) fn validate_shape_hash_node_scope(
    scope: ShapeHashScope,
    node: Option<FactId>,
) -> Result<(), ShapeHashNodeScopeError> {
    match (scope, node) {
        (ShapeHashScope::Local | ShapeHashScope::Tree, Some(_)) | (ShapeHashScope::Graph, None) => {
            Ok(())
        }
        (scope, node) => Err(ShapeHashNodeScopeError::new(scope, node)),
    }
}
