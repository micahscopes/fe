use serde::{Deserialize, Deserializer, Serialize, de};

use crate::facts::{
    OwnedTypedFactSetExport, TypedFactRelation, TypedFactRelationError,
    relation::validation::validate_typed_fact_relation_set,
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

    pub fn relation(
        &self,
        name: crate::facts::TypedFactRelationName,
    ) -> Option<&TypedFactRelation> {
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
