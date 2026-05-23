use std::collections::BTreeMap;

use super::relation::validate_typed_fact_relation_set;
use super::{
    TypedFactRelation, TypedFactRelationColumnName, TypedFactRelationCount, TypedFactRelationError,
    TypedFactRelationName, TypedFactRelationRow, TypedFactRelationSet, typed_fact_relation_schemas,
};

mod origin_paths;
mod validation;

#[derive(Clone, Debug)]
pub struct TypedFactRelationIndex<'a> {
    relations_by_name: BTreeMap<TypedFactRelationName, &'a TypedFactRelation>,
}

impl<'a> TypedFactRelationIndex<'a> {
    pub fn new(relations: &'a TypedFactRelationSet) -> Result<Self, TypedFactRelationError> {
        validate_typed_fact_relation_set(relations.schema_version(), relations.relations())?;

        let mut relations_by_name = BTreeMap::new();
        for relation in relations.relations() {
            relations_by_name.insert(relation.relation_name(), relation);
        }

        Ok(Self { relations_by_name }.validate_semantics()?)
    }

    pub fn relation(
        &self,
        name: TypedFactRelationName,
    ) -> Result<&'a TypedFactRelation, TypedFactRelationError> {
        self.relations_by_name.get(&name).copied().ok_or_else(|| {
            TypedFactRelationError::UnknownRelation {
                relation: name.as_str().to_string(),
            }
        })
    }

    pub fn row_count(
        &self,
        relation: TypedFactRelationName,
    ) -> Result<usize, TypedFactRelationError> {
        Ok(self.relation(relation)?.row_count())
    }

    pub fn rows(
        &self,
        relation: TypedFactRelationName,
    ) -> Result<Vec<TypedFactRelationRow<'a>>, TypedFactRelationError> {
        let relation_table = self.relation(relation)?;
        Ok(relation_table
            .rows()
            .iter()
            .map(|row| TypedFactRelationRow::new(relation, row.as_slice()))
            .collect())
    }

    pub fn rows_where(
        &self,
        relation: TypedFactRelationName,
        column: TypedFactRelationColumnName,
        value: &str,
    ) -> Result<Vec<TypedFactRelationRow<'a>>, TypedFactRelationError> {
        let relation_table = self.relation(relation)?;
        let column = self.column_index(relation, column)?;
        Ok(relation_table
            .rows()
            .iter()
            .filter(|row| row[column] == value)
            .map(|row| TypedFactRelationRow::new(relation, row.as_slice()))
            .collect())
    }

    pub fn relation_counts(&self) -> Result<Vec<TypedFactRelationCount>, TypedFactRelationError> {
        typed_fact_relation_schemas()
            .iter()
            .filter_map(|schema| {
                let relation = match self.relation(schema.name()) {
                    Ok(relation) => relation,
                    Err(err) => return Some(Err(err)),
                };
                (relation.row_count() > 0).then(|| {
                    Ok(TypedFactRelationCount::new(
                        schema.name(),
                        relation.row_count(),
                    ))
                })
            })
            .collect()
    }

    pub fn column_index(
        &self,
        relation: TypedFactRelationName,
        column: TypedFactRelationColumnName,
    ) -> Result<usize, TypedFactRelationError> {
        self.relation(relation)?;
        relation.column_index(column)
    }
}
