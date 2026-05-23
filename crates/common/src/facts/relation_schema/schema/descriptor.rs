use crate::facts::TypedFactRelationError;

use super::super::{TypedFactRelationColumnName, TypedFactRelationName};
use super::catalog::typed_fact_relation_schema_for_name;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TypedFactRelationSchema {
    name: TypedFactRelationName,
    columns: &'static [TypedFactRelationColumnName],
}

impl TypedFactRelationSchema {
    pub const fn new(
        name: TypedFactRelationName,
        columns: &'static [TypedFactRelationColumnName],
    ) -> Self {
        Self { name, columns }
    }

    pub const fn name(self) -> TypedFactRelationName {
        self.name
    }

    pub const fn columns(self) -> &'static [TypedFactRelationColumnName] {
        self.columns
    }

    pub fn column_names(self) -> impl Iterator<Item = &'static str> {
        self.columns.iter().map(|column| column.as_str())
    }
}

impl TypedFactRelationName {
    pub fn schema(self) -> TypedFactRelationSchema {
        typed_fact_relation_schema_for_name(self)
    }

    pub fn column_index(
        self,
        column: TypedFactRelationColumnName,
    ) -> Result<usize, TypedFactRelationError> {
        self.schema()
            .columns()
            .iter()
            .position(|candidate| *candidate == column)
            .ok_or_else(|| TypedFactRelationError::UnknownColumn {
                relation: self.as_str().to_string(),
                column: column.as_str().to_string(),
            })
    }
}
