use std::collections::BTreeSet;

use crate::facts::{TypedFactRelationColumnName, TypedFactRelationError, TypedFactRelationName};

use super::super::super::TypedFactRelationIndex;

impl<'a> TypedFactRelationIndex<'a> {
    pub(in crate::facts::relation_index) fn validate_relation_references<'b>(
        &self,
        relation: TypedFactRelationName,
        references: impl IntoIterator<
            Item = (
                TypedFactRelationColumnName,
                &'b BTreeSet<String>,
                TypedFactRelationName,
            ),
        >,
    ) -> Result<(), TypedFactRelationError> {
        let relation_table = self.relation(relation)?;
        let references = references
            .into_iter()
            .map(|(column, target_ids, target_relation)| {
                self.column_index(relation, column)
                    .map(|idx| (column, idx, target_ids, target_relation))
            })
            .collect::<Result<Vec<_>, _>>()?;

        for row in relation_table.rows() {
            for (column, idx, target_ids, target_relation) in &references {
                if !target_ids.contains(&row[*idx]) {
                    return Err(TypedFactRelationError::MissingRelationReference {
                        relation: relation.as_str().to_string(),
                        column: column.as_str().to_string(),
                        value: row[*idx].clone(),
                        target_relation: target_relation.as_str().to_string(),
                    });
                }
            }
        }

        Ok(())
    }
}
