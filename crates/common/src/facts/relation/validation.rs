use std::collections::BTreeSet;

use crate::facts::{
    TypedFactRelation, TypedFactRelationError, TypedFactRelationName, TypedFactRelationSet,
    relation_schema::{
        columns_match, typed_fact_relation_schema_for_raw_name, typed_fact_relation_schemas,
    },
};

pub(in crate::facts) fn validate_typed_fact_relation_set(
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

pub(in crate::facts::relation) fn validate_typed_fact_relation_typed(
    name: TypedFactRelationName,
    rows: &[Vec<String>],
) -> Result<(), TypedFactRelationError> {
    let expected_columns = name.schema().columns();
    validate_typed_fact_relation_rows(name.as_str(), expected_columns.len(), rows)
}

pub(in crate::facts::relation) fn validate_typed_fact_relation(
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
