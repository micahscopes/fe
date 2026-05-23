mod display;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TypedFactRelationError {
    UnsupportedSchemaVersion {
        actual: u32,
        expected: u32,
    },
    UnknownRelation {
        relation: String,
    },
    MissingRelation {
        relation: String,
    },
    DuplicateRelation {
        relation: String,
    },
    DuplicateRelationId {
        relation: String,
        value: String,
    },
    DuplicateRelationKey {
        relation: String,
        columns: Vec<String>,
        values: Vec<String>,
    },
    WrongColumns {
        relation: String,
        actual: Vec<String>,
        expected: Vec<String>,
    },
    WrongRowWidth {
        relation: String,
        row: usize,
        actual: usize,
        expected: usize,
    },
    UnknownColumn {
        relation: String,
        column: String,
    },
    InvalidRelationValue {
        relation: String,
        column: String,
        value: String,
    },
    InvalidSourceSpanRange {
        origin: String,
        start_byte: usize,
        end_byte: usize,
    },
    InvalidSourceSpanPosition {
        origin: String,
        start_line: usize,
        start_col: usize,
        end_line: usize,
        end_col: usize,
    },
    InvalidSourceSpanFile {
        origin: String,
    },
    MissingRelationReference {
        relation: String,
        column: String,
        value: String,
        target_relation: String,
    },
    InvalidShapeHashNode {
        node: String,
        scope: String,
    },
    InvalidShapeHashDigest {
        node: String,
        scope: String,
        dimension: String,
    },
    DuplicateShapeHash {
        node: String,
        scope: String,
        dimension: String,
    },
    MissingShapeHash {
        node: String,
        scope: String,
        dimension: String,
    },
}

impl std::error::Error for TypedFactRelationError {}
