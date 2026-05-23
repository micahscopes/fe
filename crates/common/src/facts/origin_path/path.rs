use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, de};

use crate::{
    facts::{FactId, FactNamespace, FactNamespaceError},
    origin::{OriginExportKind, OriginLinkKind},
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OriginPathError {
    EmptyPath,
    LengthMismatch { nodes: usize, links: usize },
    WrongNamespace(FactNamespaceError),
}

impl From<FactNamespaceError> for OriginPathError {
    fn from(err: FactNamespaceError) -> Self {
        Self::WrongNamespace(err)
    }
}

impl fmt::Display for OriginPathError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyPath => write!(f, "origin path must contain at least one node"),
            Self::LengthMismatch { nodes, links } => write!(
                f,
                "origin path has {nodes} nodes and {links} links; expected exactly one more node than link"
            ),
            Self::WrongNamespace(err) => err.fmt(f),
        }
    }
}

impl std::error::Error for OriginPathError {}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct OriginPath {
    nodes: Vec<FactId>,
    links: Vec<OriginLinkKind>,
}

impl OriginPath {
    pub fn new(nodes: Vec<FactId>, links: Vec<OriginLinkKind>) -> Self {
        Self::try_new(nodes, links).unwrap_or_else(|err| panic!("{err}"))
    }

    pub fn try_new(
        nodes: Vec<FactId>,
        links: Vec<OriginLinkKind>,
    ) -> Result<Self, OriginPathError> {
        if nodes.is_empty() {
            return Err(OriginPathError::EmptyPath);
        }
        if nodes.len() != links.len() + 1 {
            return Err(OriginPathError::LengthMismatch {
                nodes: nodes.len(),
                links: links.len(),
            });
        }
        for node in &nodes {
            validate_origin_node_fact_id(*node)?;
        }
        Ok(Self { nodes, links })
    }

    pub fn nodes(&self) -> &[FactId] {
        &self.nodes
    }

    pub fn links(&self) -> &[OriginLinkKind] {
        &self.links
    }
}

impl<'de> Deserialize<'de> for OriginPath {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct RawPath {
            nodes: Vec<FactId>,
            links: Vec<OriginLinkKind>,
        }

        let raw = RawPath::deserialize(deserializer)?;
        Self::try_new(raw.nodes, raw.links).map_err(de::Error::custom)
    }
}

fn validate_origin_node_fact_id(id: FactId) -> Result<FactId, FactNamespaceError> {
    if id.namespace() == FactNamespace::OriginNode {
        Ok(id)
    } else {
        Err(FactNamespaceError::WrongNamespace {
            id,
            expected: FactNamespace::OriginNode,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OriginKindPathWitness {
    from_kind: OriginExportKind,
    to_kind: OriginExportKind,
    path: OriginPath,
}

impl OriginKindPathWitness {
    pub fn new(from_kind: OriginExportKind, to_kind: OriginExportKind, path: OriginPath) -> Self {
        Self {
            from_kind,
            to_kind,
            path,
        }
    }

    pub const fn from_kind(&self) -> OriginExportKind {
        self.from_kind
    }

    pub const fn to_kind(&self) -> OriginExportKind {
        self.to_kind
    }

    pub fn path(&self) -> &OriginPath {
        &self.path
    }
}
