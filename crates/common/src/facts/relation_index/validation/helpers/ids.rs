use std::collections::BTreeSet;

use crate::facts::{TypedFactRelationColumnName, TypedFactRelationError, TypedFactRelationName};

use super::super::super::TypedFactRelationIndex;

impl<'a> TypedFactRelationIndex<'a> {
    pub(in crate::facts::relation_index) fn relation_id_set(
        &self,
        relation: TypedFactRelationName,
        column: TypedFactRelationColumnName,
    ) -> Result<BTreeSet<String>, TypedFactRelationError> {
        let relation_table = self.relation(relation)?;
        let column_idx = self.column_index(relation, column)?;
        let mut ids = BTreeSet::new();
        for row in relation_table.rows() {
            self.validate_fact_id_cell(relation, column, &row[column_idx])?;
            if !ids.insert(row[column_idx].clone()) {
                return Err(TypedFactRelationError::DuplicateRelationId {
                    relation: relation.as_str().to_string(),
                    value: row[column_idx].clone(),
                });
            }
        }
        Ok(ids)
    }

    fn validate_fact_id_cell(
        &self,
        relation: TypedFactRelationName,
        column: TypedFactRelationColumnName,
        value: &str,
    ) -> Result<(), TypedFactRelationError> {
        let Some(ordinal) = value
            .strip_prefix(relation.as_str())
            .and_then(|rest| rest.strip_prefix(':'))
        else {
            return Err(TypedFactRelationError::InvalidRelationValue {
                relation: relation.as_str().to_string(),
                column: column.as_str().to_string(),
                value: value.to_string(),
            });
        };
        if ordinal.parse::<u64>().is_err() {
            return Err(TypedFactRelationError::InvalidRelationValue {
                relation: relation.as_str().to_string(),
                column: column.as_str().to_string(),
                value: value.to_string(),
            });
        }
        Ok(())
    }
}
