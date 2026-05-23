use crate::facts::{TypedFactRelationColumnName, TypedFactRelationError, TypedFactRelationName};

use super::super::super::TypedFactRelationIndex;

impl<'a> TypedFactRelationIndex<'a> {
    pub(in crate::facts::relation_index) fn validate_numeric_column<T>(
        &self,
        relation: TypedFactRelationName,
        column: TypedFactRelationColumnName,
    ) -> Result<(), TypedFactRelationError>
    where
        T: std::str::FromStr,
    {
        let relation_table = self.relation(relation)?;
        let column_idx = self.column_index(relation, column)?;
        for row in relation_table.rows() {
            self.parse_relation_number::<T>(relation, column, &row[column_idx])?;
        }
        Ok(())
    }

    pub(in crate::facts::relation_index) fn validate_non_empty_column(
        &self,
        relation: TypedFactRelationName,
        column: TypedFactRelationColumnName,
    ) -> Result<(), TypedFactRelationError> {
        let relation_table = self.relation(relation)?;
        let column_idx = self.column_index(relation, column)?;
        for row in relation_table.rows() {
            if row[column_idx].is_empty() {
                return Err(TypedFactRelationError::InvalidRelationValue {
                    relation: relation.as_str().to_string(),
                    column: column.as_str().to_string(),
                    value: row[column_idx].clone(),
                });
            }
        }
        Ok(())
    }

    pub(in crate::facts::relation_index) fn parse_relation_number<T>(
        &self,
        relation: TypedFactRelationName,
        column: TypedFactRelationColumnName,
        value: &str,
    ) -> Result<T, TypedFactRelationError>
    where
        T: std::str::FromStr,
    {
        value
            .parse::<T>()
            .map_err(|_| TypedFactRelationError::InvalidRelationValue {
                relation: relation.as_str().to_string(),
                column: column.as_str().to_string(),
                value: value.to_string(),
            })
    }

    pub(in crate::facts::relation_index) fn validate_column_values<T>(
        &self,
        relation: TypedFactRelationName,
        column: TypedFactRelationColumnName,
        mut parse: impl FnMut(&str) -> Option<T>,
    ) -> Result<(), TypedFactRelationError> {
        let relation_table = self.relation(relation)?;
        let column_idx = self.column_index(relation, column)?;
        for row in relation_table.rows() {
            if parse(&row[column_idx]).is_none() {
                return Err(TypedFactRelationError::InvalidRelationValue {
                    relation: relation.as_str().to_string(),
                    column: column.as_str().to_string(),
                    value: row[column_idx].clone(),
                });
            }
        }
        Ok(())
    }
}
