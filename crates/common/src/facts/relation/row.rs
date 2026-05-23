use crate::facts::{TypedFactRelationColumnName, TypedFactRelationError, TypedFactRelationName};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TypedFactRelationRow<'a> {
    relation: TypedFactRelationName,
    row: &'a [String],
}

impl<'a> TypedFactRelationRow<'a> {
    pub(in crate::facts) const fn new(relation: TypedFactRelationName, row: &'a [String]) -> Self {
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
