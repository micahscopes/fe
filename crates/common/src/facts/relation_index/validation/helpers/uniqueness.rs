use std::collections::BTreeSet;

use crate::facts::{TypedFactRelationColumnName, TypedFactRelationError, TypedFactRelationName};

use super::super::super::TypedFactRelationIndex;

impl<'a> TypedFactRelationIndex<'a> {
    pub(in crate::facts::relation_index) fn validate_unique_columns(
        &self,
        relation: TypedFactRelationName,
        columns: &[TypedFactRelationColumnName],
    ) -> Result<(), TypedFactRelationError> {
        let relation_table = self.relation(relation)?;
        let column_indexes = columns
            .iter()
            .map(|column| self.column_index(relation, *column))
            .collect::<Result<Vec<_>, _>>()?;
        let mut seen = BTreeSet::new();
        for row in relation_table.rows() {
            let values = column_indexes
                .iter()
                .map(|idx| row[*idx].clone())
                .collect::<Vec<_>>();
            if !seen.insert(values.clone()) {
                return Err(TypedFactRelationError::DuplicateRelationKey {
                    relation: relation.as_str().to_string(),
                    columns: columns
                        .iter()
                        .map(|column| column.as_str().to_string())
                        .collect(),
                    values,
                });
            }
        }
        Ok(())
    }
}
