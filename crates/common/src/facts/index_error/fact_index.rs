mod display;

use crate::{
    facts::{FactId, FactNamespace, ShapeHashScope},
    origin::OriginLinkKind,
    shape::ShapeDimension,
};

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

impl std::error::Error for FactIndexError {}
