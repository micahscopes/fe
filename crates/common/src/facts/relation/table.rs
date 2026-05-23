use serde::{Deserialize, Deserializer, Serialize, Serializer, de, ser::SerializeStruct};

use crate::facts::{
    TypedFactRelationColumnName, TypedFactRelationError, TypedFactRelationName,
    relation::validation::{validate_typed_fact_relation, validate_typed_fact_relation_typed},
};

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
