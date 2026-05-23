use std::{collections::BTreeSet, fmt};

use serde::{Deserialize, Deserializer, Serialize, Serializer, de, ser::SerializeStruct};

use super::{
    OwnedTypedFactSetExport, TypedFactRelationColumnName, TypedFactRelationName,
    relation_schema::{
        columns_match, typed_fact_relation_schema_for_raw_name, typed_fact_relation_schemas,
    },
};

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct TypedFactRelationSet {
    schema_version: u32,
    relations: Vec<TypedFactRelation>,
}

impl TypedFactRelationSet {
    pub const SCHEMA_VERSION: u32 = OwnedTypedFactSetExport::SCHEMA_VERSION;

    pub fn new(relations: Vec<TypedFactRelation>) -> Result<Self, TypedFactRelationError> {
        let relation_set = Self {
            schema_version: Self::SCHEMA_VERSION,
            relations,
        };
        validate_typed_fact_relation_set(relation_set.schema_version(), relation_set.relations())?;
        Ok(relation_set)
    }

    pub const fn schema_version(&self) -> u32 {
        self.schema_version
    }

    pub fn relations(&self) -> &[TypedFactRelation] {
        &self.relations
    }

    pub fn relation(&self, name: TypedFactRelationName) -> Option<&TypedFactRelation> {
        self.relations
            .iter()
            .find(|relation| relation.relation_name() == name)
    }
}

impl<'de> Deserialize<'de> for TypedFactRelationSet {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct RawRelationSet {
            schema_version: u32,
            relations: Vec<TypedFactRelation>,
        }

        let raw = RawRelationSet::deserialize(deserializer)?;
        validate_typed_fact_relation_set(raw.schema_version, &raw.relations)
            .map_err(de::Error::custom)?;

        Ok(Self {
            schema_version: raw.schema_version,
            relations: raw.relations,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TypedFactRelation {
    name: TypedFactRelationName,
    rows: Vec<Vec<String>>,
}

impl TypedFactRelation {
    pub fn new(
        name: TypedFactRelationName,
        rows: Vec<Vec<String>>,
    ) -> Result<Self, TypedFactRelationError> {
        let relation = Self { name, rows };
        validate_typed_fact_relation_typed(relation.relation_name(), relation.rows())?;
        Ok(relation)
    }

    pub const fn relation_name(&self) -> TypedFactRelationName {
        self.name
    }

    pub const fn name(&self) -> &'static str {
        self.name.as_str()
    }

    pub fn typed_columns(&self) -> &[TypedFactRelationColumnName] {
        self.name.schema().columns()
    }

    pub fn column_names(&self) -> impl Iterator<Item = &'static str> {
        self.name.schema().column_names()
    }

    pub fn columns(&self) -> Vec<String> {
        self.column_names().map(str::to_string).collect()
    }

    pub fn rows(&self) -> &[Vec<String>] {
        &self.rows
    }

    pub fn row_count(&self) -> usize {
        self.rows.len()
    }
}

impl Serialize for TypedFactRelation {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut out = serializer.serialize_struct("TypedFactRelation", 3)?;
        out.serialize_field("name", &self.name)?;
        out.serialize_field("columns", self.typed_columns())?;
        out.serialize_field("rows", &self.rows)?;
        out.end()
    }
}

impl<'de> Deserialize<'de> for TypedFactRelation {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct RawRelation {
            name: String,
            columns: Vec<String>,
            rows: Vec<Vec<String>>,
        }

        let raw = RawRelation::deserialize(deserializer)?;
        validate_typed_fact_relation(&raw.name, &raw.columns, &raw.rows)
            .map_err(de::Error::custom)?;
        let name = TypedFactRelationName::from_str(&raw.name)
            .expect("validated typed fact relation should have a known relation name");

        Ok(Self {
            name,
            rows: raw.rows,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct TypedFactRelationCount {
    relation: TypedFactRelationName,
    rows: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TypedFactRelationCountError {
    ZeroRows,
}

impl fmt::Display for TypedFactRelationCountError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ZeroRows => write!(
                f,
                "typed fact relation count rows must be greater than zero"
            ),
        }
    }
}

impl std::error::Error for TypedFactRelationCountError {}

impl TypedFactRelationCount {
    pub fn new(relation: TypedFactRelationName, rows: usize) -> Self {
        Self::try_new(relation, rows).unwrap_or_else(|err| panic!("{err}"))
    }

    pub fn try_new(
        relation: TypedFactRelationName,
        rows: usize,
    ) -> Result<Self, TypedFactRelationCountError> {
        if rows == 0 {
            return Err(TypedFactRelationCountError::ZeroRows);
        }
        Ok(Self { relation, rows })
    }

    pub const fn relation(&self) -> TypedFactRelationName {
        self.relation
    }

    pub const fn relation_name(&self) -> &'static str {
        self.relation.as_str()
    }

    pub const fn rows(&self) -> usize {
        self.rows
    }
}

impl<'de> Deserialize<'de> for TypedFactRelationCount {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct RawCount {
            relation: TypedFactRelationName,
            rows: usize,
        }

        let raw = RawCount::deserialize(deserializer)?;
        Self::try_new(raw.relation, raw.rows).map_err(de::Error::custom)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TypedFactRelationRow<'a> {
    relation: TypedFactRelationName,
    row: &'a [String],
}

impl<'a> TypedFactRelationRow<'a> {
    pub(super) const fn new(relation: TypedFactRelationName, row: &'a [String]) -> Self {
        Self { relation, row }
    }

    pub const fn relation(&self) -> TypedFactRelationName {
        self.relation
    }

    pub const fn relation_name(&self) -> &'static str {
        self.relation.as_str()
    }

    pub fn cells(&self) -> &'a [String] {
        self.row
    }

    pub fn cell(
        &self,
        column: TypedFactRelationColumnName,
    ) -> Result<&'a str, TypedFactRelationError> {
        let index = self.relation.column_index(column)?;
        Ok(self.row[index].as_str())
    }
}

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

impl std::error::Error for TypedFactRelationError {}

pub(super) fn validate_typed_fact_relation_set(
    schema_version: u32,
    relations: &[TypedFactRelation],
) -> Result<(), TypedFactRelationError> {
    if schema_version != TypedFactRelationSet::SCHEMA_VERSION {
        return Err(TypedFactRelationError::UnsupportedSchemaVersion {
            actual: schema_version,
            expected: TypedFactRelationSet::SCHEMA_VERSION,
        });
    }

    let mut seen = BTreeSet::new();
    for relation in relations {
        let relation_name = relation.relation_name();
        validate_typed_fact_relation_typed(relation_name, relation.rows())?;
        if !seen.insert(relation_name) {
            return Err(TypedFactRelationError::DuplicateRelation {
                relation: relation_name.as_str().to_string(),
            });
        }
    }
    for schema in typed_fact_relation_schemas() {
        let relation_name = schema.name();
        if !seen.contains(&relation_name) {
            return Err(TypedFactRelationError::MissingRelation {
                relation: relation_name.as_str().to_string(),
            });
        }
    }

    Ok(())
}

fn validate_typed_fact_relation_typed(
    name: TypedFactRelationName,
    rows: &[Vec<String>],
) -> Result<(), TypedFactRelationError> {
    let expected_columns = name.schema().columns();
    validate_typed_fact_relation_rows(name.as_str(), expected_columns.len(), rows)
}

fn validate_typed_fact_relation(
    name: &str,
    columns: &[String],
    rows: &[Vec<String>],
) -> Result<(), TypedFactRelationError> {
    let Some(schema) = typed_fact_relation_schema_for_raw_name(name) else {
        return Err(TypedFactRelationError::UnknownRelation {
            relation: name.to_string(),
        });
    };
    let expected_columns = schema.columns();
    if !columns_match(columns, expected_columns) {
        return Err(TypedFactRelationError::WrongColumns {
            relation: name.to_string(),
            actual: columns.to_vec(),
            expected: expected_columns
                .iter()
                .map(|column| column.as_str().to_string())
                .collect(),
        });
    }

    validate_typed_fact_relation_rows(name, expected_columns.len(), rows)
}

fn validate_typed_fact_relation_rows(
    name: &str,
    expected_columns: usize,
    rows: &[Vec<String>],
) -> Result<(), TypedFactRelationError> {
    for (idx, row) in rows.iter().enumerate() {
        if row.len() != expected_columns {
            return Err(TypedFactRelationError::WrongRowWidth {
                relation: name.to_string(),
                row: idx,
                actual: row.len(),
                expected: expected_columns,
            });
        }
    }

    Ok(())
}
