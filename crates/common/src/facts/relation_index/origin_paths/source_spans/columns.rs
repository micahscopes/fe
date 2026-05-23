use crate::facts::{TypedFactRelationColumnName, TypedFactRelationError, TypedFactRelationName};

use super::super::super::TypedFactRelationIndex;

pub(super) struct SourceSpanRelationColumns {
    pub(super) origin: usize,
    pub(super) span_kind: usize,
    pub(super) file: usize,
    pub(super) start_byte: usize,
    pub(super) end_byte: usize,
    pub(super) start_line: usize,
    pub(super) start_col: usize,
    pub(super) end_line: usize,
    pub(super) end_col: usize,
}

impl SourceSpanRelationColumns {
    pub(super) fn from_index<'a>(
        index: &TypedFactRelationIndex<'a>,
    ) -> Result<Self, TypedFactRelationError> {
        Ok(Self {
            origin: source_span_column(index, TypedFactRelationColumnName::Origin)?,
            span_kind: source_span_column(index, TypedFactRelationColumnName::SpanKind)?,
            file: source_span_column(index, TypedFactRelationColumnName::File)?,
            start_byte: source_span_column(index, TypedFactRelationColumnName::StartByte)?,
            end_byte: source_span_column(index, TypedFactRelationColumnName::EndByte)?,
            start_line: source_span_column(index, TypedFactRelationColumnName::StartLine)?,
            start_col: source_span_column(index, TypedFactRelationColumnName::StartCol)?,
            end_line: source_span_column(index, TypedFactRelationColumnName::EndLine)?,
            end_col: source_span_column(index, TypedFactRelationColumnName::EndCol)?,
        })
    }
}

fn source_span_column<'a>(
    index: &TypedFactRelationIndex<'a>,
    column: TypedFactRelationColumnName,
) -> Result<usize, TypedFactRelationError> {
    index.column_index(TypedFactRelationName::SourceSpan, column)
}
