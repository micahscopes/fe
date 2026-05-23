use std::fmt;

use super::TypedFactRelationError;

impl fmt::Display for TypedFactRelationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedSchemaVersion { actual, expected } => write!(
                f,
                "unsupported typed fact relation schema_version {actual}; expected {expected}"
            ),
            Self::UnknownRelation { relation } => {
                write!(f, "unknown typed fact relation `{relation}`")
            }
            Self::MissingRelation { relation } => {
                write!(f, "missing typed fact relation `{relation}`")
            }
            Self::DuplicateRelation { relation } => {
                write!(f, "duplicate typed fact relation `{relation}`")
            }
            Self::DuplicateRelationId { relation, value } => {
                write!(
                    f,
                    "duplicate id `{value}` in typed fact relation `{relation}`"
                )
            }
            Self::DuplicateRelationKey {
                relation,
                columns,
                values,
            } => write!(
                f,
                "duplicate key {values:?} for columns {columns:?} in typed fact relation `{relation}`"
            ),
            Self::WrongColumns {
                relation,
                actual,
                expected,
            } => write!(
                f,
                "typed fact relation `{relation}` has columns {actual:?}; expected {expected:?}"
            ),
            Self::WrongRowWidth {
                relation,
                row,
                actual,
                expected,
            } => write!(
                f,
                "typed fact relation `{relation}` row {row} has {actual} columns; expected {expected}"
            ),
            Self::UnknownColumn { relation, column } => write!(
                f,
                "unknown typed fact relation column `{column}` for relation `{relation}`"
            ),
            Self::InvalidRelationValue {
                relation,
                column,
                value,
            } => write!(
                f,
                "typed fact relation `{relation}` column `{column}` has invalid value `{value}`"
            ),
            Self::InvalidSourceSpanRange {
                origin,
                start_byte,
                end_byte,
            } => write!(
                f,
                "typed fact relation `source_span` row for origin `{origin}` has invalid byte range {start_byte}..{end_byte}"
            ),
            Self::InvalidSourceSpanPosition {
                origin,
                start_line,
                start_col,
                end_line,
                end_col,
            } => write!(
                f,
                "typed fact relation `source_span` row for origin `{origin}` has invalid line/column range {start_line}:{start_col}..{end_line}:{end_col}"
            ),
            Self::InvalidSourceSpanFile { origin } => write!(
                f,
                "typed fact relation `source_span` row for origin `{origin}` has empty file"
            ),
            Self::MissingRelationReference {
                relation,
                column,
                value,
                target_relation,
            } => write!(
                f,
                "typed fact relation `{relation}` column `{column}` references missing `{target_relation}` id `{value}`"
            ),
            Self::InvalidShapeHashNode { node, scope } => write!(
                f,
                "typed fact relation `shape_hash` has invalid node `{node}` for scope `{scope}`"
            ),
            Self::InvalidShapeHashDigest {
                node,
                scope,
                dimension,
            } => write!(
                f,
                "typed fact relation `shape_hash` has invalid digest for node `{node}` scope `{scope}` dimension `{dimension}`"
            ),
            Self::DuplicateShapeHash {
                node,
                scope,
                dimension,
            } => write!(
                f,
                "typed fact relation `shape_hash` has duplicate hash for node `{node}` scope `{scope}` dimension `{dimension}`"
            ),
            Self::MissingShapeHash {
                node,
                scope,
                dimension,
            } => write!(
                f,
                "typed fact relation `shape_hash` is missing hash for node `{node}` scope `{scope}` dimension `{dimension}`"
            ),
        }
    }
}
