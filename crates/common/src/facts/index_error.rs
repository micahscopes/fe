use std::fmt;

use crate::{
    origin::{OriginExportKey, OriginLinkKind},
    shape::ShapeDimension,
};

use super::{FactId, FactNamespace, ShapeHashScope};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FactIndexError {
    WrongFactNamespace {
        id: FactId,
        expected: FactNamespace,
    },
    DuplicateOriginKey,
    DuplicateOriginId,
    DuplicateOriginLink {
        from: FactId,
        to: FactId,
        kind: OriginLinkKind,
    },
    OriginLinkMissingEndpoint {
        endpoint: FactId,
    },
    SourceSpanMissingOrigin {
        origin: FactId,
    },
    InvalidSourceSpanRange {
        origin: FactId,
        start_byte: usize,
        end_byte: usize,
    },
    InvalidSourceSpanPosition {
        origin: FactId,
        start_line: usize,
        start_col: usize,
        end_line: usize,
        end_col: usize,
    },
    InvalidSourceSpanFile {
        origin: FactId,
    },
    DuplicateShapeId,
    DuplicateShapeSourceId,
    DuplicateShapeStableKey,
    InvalidShapeText {
        field: &'static str,
    },
    ShapeFactMissingNode {
        node: FactId,
    },
    ShapeHashNodeScopeMismatch {
        scope: ShapeHashScope,
        node: Option<FactId>,
    },
    DuplicateShapeHash {
        scope: ShapeHashScope,
        node: Option<FactId>,
        dimension: ShapeDimension,
    },
    MissingShapeHash {
        scope: ShapeHashScope,
        node: Option<FactId>,
        dimension: ShapeDimension,
    },
}

impl fmt::Display for FactIndexError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WrongFactNamespace { id, expected } => write!(
                f,
                "fact id {} has namespace {}, expected {}",
                id.stable_key(),
                id.namespace().as_str(),
                expected.as_str()
            ),
            Self::DuplicateOriginKey => write!(f, "duplicate origin export key"),
            Self::DuplicateOriginId => write!(f, "duplicate origin fact id"),
            Self::DuplicateOriginLink { from, to, kind } => write!(
                f,
                "duplicate origin link {} -> {} ({})",
                from.stable_key(),
                to.stable_key(),
                kind.as_str()
            ),
            Self::OriginLinkMissingEndpoint { endpoint } => write!(
                f,
                "origin link references missing endpoint {}",
                endpoint.stable_key()
            ),
            Self::SourceSpanMissingOrigin { origin } => {
                write!(
                    f,
                    "source span references missing origin {}",
                    origin.stable_key()
                )
            }
            Self::InvalidSourceSpanRange {
                origin,
                start_byte,
                end_byte,
            } => write!(
                f,
                "source span for origin {} has invalid byte range {}..{}",
                origin.stable_key(),
                start_byte,
                end_byte
            ),
            Self::InvalidSourceSpanPosition {
                origin,
                start_line,
                start_col,
                end_line,
                end_col,
            } => write!(
                f,
                "source span for origin {} has invalid line/column range {}:{}..{}:{}",
                origin.stable_key(),
                start_line,
                start_col,
                end_line,
                end_col
            ),
            Self::InvalidSourceSpanFile { origin } => write!(
                f,
                "source span for origin {} has empty file",
                origin.stable_key()
            ),
            Self::DuplicateShapeId => write!(f, "duplicate shape fact id"),
            Self::DuplicateShapeSourceId => write!(f, "duplicate shape source id"),
            Self::DuplicateShapeStableKey => write!(f, "duplicate shape stable key"),
            Self::InvalidShapeText { field } => write!(f, "{field} must not be empty"),
            Self::ShapeFactMissingNode { node } => {
                write!(
                    f,
                    "shape fact references missing node {}",
                    node.stable_key()
                )
            }
            Self::ShapeHashNodeScopeMismatch { scope, node } => {
                let node = node
                    .map(FactId::stable_key)
                    .unwrap_or_else(|| "none".to_string());
                write!(
                    f,
                    "shape hash scope {} has invalid node reference {}",
                    scope.as_str(),
                    node
                )
            }
            Self::DuplicateShapeHash {
                scope,
                node,
                dimension,
            } => {
                let node = shape_hash_node_label(*node);
                write!(
                    f,
                    "duplicate shape hash for scope {} dimension {} at node {}",
                    scope.as_str(),
                    dimension.as_str(),
                    node
                )
            }
            Self::MissingShapeHash {
                scope,
                node,
                dimension,
            } => {
                let node = shape_hash_node_label(*node);
                write!(
                    f,
                    "missing shape hash for scope {} dimension {} at node {}",
                    scope.as_str(),
                    dimension.as_str(),
                    node
                )
            }
        }
    }
}

impl std::error::Error for FactIndexError {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SourceSpanFactError {
    InvalidFacts(FactIndexError),
    MissingOriginKey(OriginExportKey),
}

impl fmt::Display for SourceSpanFactError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidFacts(err) => write!(f, "invalid origin facts: {err}"),
            Self::MissingOriginKey(key) => write!(
                f,
                "source span references missing origin key {}:{}:{}",
                key.kind().as_str(),
                key.owner_key(),
                key.local_key()
            ),
        }
    }
}

impl std::error::Error for SourceSpanFactError {}

pub(super) fn require_fact_namespace(
    id: FactId,
    expected: FactNamespace,
) -> Result<(), FactIndexError> {
    if id.namespace() == expected {
        Ok(())
    } else {
        Err(FactIndexError::WrongFactNamespace { id, expected })
    }
}

pub(super) fn require_non_empty_shape_fact_text(
    field: &'static str,
    value: &str,
) -> Result<(), FactIndexError> {
    if value.is_empty() {
        Err(FactIndexError::InvalidShapeText { field })
    } else {
        Ok(())
    }
}

fn shape_hash_node_label(node: Option<FactId>) -> String {
    node.map(FactId::stable_key)
        .unwrap_or_else(|| "graph".to_string())
}
