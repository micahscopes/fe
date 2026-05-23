use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, de};

use crate::{
    facts::{FactId, FactNamespace, FactNamespaceError, ids::validated_fact_namespace},
    shape::ShapeDimension,
};

use super::{
    ShapeHashDigest, ShapeHashDigestError, ShapeHashNodeScopeError, ShapeHashScope,
    key::validate_shape_hash_node_scope,
};

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ShapeHashFact {
    node: Option<FactId>,
    scope: ShapeHashScope,
    dimension: ShapeDimension,
    digest_hex: ShapeHashDigest,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ShapeHashFactError {
    InvalidDigest(ShapeHashDigestError),
    InvalidNodeScope(ShapeHashNodeScopeError),
    WrongNamespace(FactNamespaceError),
}

impl fmt::Display for ShapeHashFactError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidDigest(err) => err.fmt(f),
            Self::InvalidNodeScope(err) => err.fmt(f),
            Self::WrongNamespace(err) => err.fmt(f),
        }
    }
}

impl std::error::Error for ShapeHashFactError {}

impl From<FactNamespaceError> for ShapeHashFactError {
    fn from(err: FactNamespaceError) -> Self {
        Self::WrongNamespace(err)
    }
}

impl From<ShapeHashDigestError> for ShapeHashFactError {
    fn from(err: ShapeHashDigestError) -> Self {
        Self::InvalidDigest(err)
    }
}

impl From<ShapeHashNodeScopeError> for ShapeHashFactError {
    fn from(err: ShapeHashNodeScopeError) -> Self {
        Self::InvalidNodeScope(err)
    }
}

impl ShapeHashFact {
    pub fn new(
        node: Option<FactId>,
        scope: ShapeHashScope,
        dimension: ShapeDimension,
        digest: ShapeHashDigest,
    ) -> Self {
        Self::try_new(node, scope, dimension, digest).unwrap_or_else(|err| panic!("{err}"))
    }

    pub fn try_new(
        node: Option<FactId>,
        scope: ShapeHashScope,
        dimension: ShapeDimension,
        digest: ShapeHashDigest,
    ) -> Result<Self, ShapeHashFactError> {
        let node = match node {
            Some(node) => Some(validated_fact_namespace(node, FactNamespace::ShapeNode)?),
            None => None,
        };
        validate_shape_hash_node_scope(scope, node)?;
        Ok(Self {
            node,
            scope,
            dimension,
            digest_hex: digest,
        })
    }

    pub fn try_from_digest_hex(
        node: Option<FactId>,
        scope: ShapeHashScope,
        dimension: ShapeDimension,
        digest_hex: impl Into<String>,
    ) -> Result<Self, ShapeHashFactError> {
        let digest = ShapeHashDigest::try_new(digest_hex)?;
        Self::try_new(node, scope, dimension, digest)
    }

    pub const fn node(&self) -> Option<FactId> {
        self.node
    }

    pub const fn scope(&self) -> ShapeHashScope {
        self.scope
    }

    pub const fn dimension(&self) -> ShapeDimension {
        self.dimension
    }

    pub const fn digest(&self) -> &ShapeHashDigest {
        &self.digest_hex
    }

    pub fn digest_hex(&self) -> &str {
        self.digest_hex.as_str()
    }
}

impl<'de> Deserialize<'de> for ShapeHashFact {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct RawShapeHash {
            node: Option<FactId>,
            scope: ShapeHashScope,
            dimension: ShapeDimension,
            digest_hex: String,
        }

        let raw = RawShapeHash::deserialize(deserializer)?;
        Self::try_from_digest_hex(raw.node, raw.scope, raw.dimension, raw.digest_hex)
            .map_err(de::Error::custom)
    }
}
