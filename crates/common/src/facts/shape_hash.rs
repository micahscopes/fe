use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, de};

use crate::shape::ShapeDimension;

use super::{FactId, FactNamespace, FactNamespaceError, ids::validated_fact_namespace};

crate::define_closed_string_enum! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
    pub enum ShapeHashScope {
        Local => "local",
        Tree => "tree",
        Graph => "graph",
    }
}

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

fn validate_shape_hash_node_scope(
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

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ShapeHashFact {
    node: Option<FactId>,
    scope: ShapeHashScope,
    dimension: ShapeDimension,
    digest_hex: ShapeHashDigest,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct ShapeHashDigest(String);

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ShapeHashDigestError {
    InvalidDigest { digest_hex: String },
}

impl fmt::Display for ShapeHashDigestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidDigest { digest_hex } => write!(
                f,
                "shape hash digest `{digest_hex}` must be canonical 16-character lowercase hex"
            ),
        }
    }
}

impl std::error::Error for ShapeHashDigestError {}

impl ShapeHashDigest {
    pub fn new(digest_hex: impl Into<String>) -> Self {
        Self::try_new(digest_hex).unwrap_or_else(|err| panic!("{err}"))
    }

    pub fn try_new(digest_hex: impl Into<String>) -> Result<Self, ShapeHashDigestError> {
        let digest_hex = digest_hex.into();
        if is_canonical_shape_hash_digest(&digest_hex) {
            Ok(Self(digest_hex))
        } else {
            Err(ShapeHashDigestError::InvalidDigest { digest_hex })
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn into_string(self) -> String {
        self.0
    }
}

impl fmt::Display for ShapeHashDigest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for ShapeHashDigest {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let digest_hex = String::deserialize(deserializer)?;
        Self::try_new(digest_hex).map_err(de::Error::custom)
    }
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

fn is_canonical_shape_hash_digest(digest_hex: &str) -> bool {
    digest_hex.len() == 16
        && digest_hex
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}
