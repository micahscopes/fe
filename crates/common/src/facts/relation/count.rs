use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, de};

use crate::facts::TypedFactRelationName;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct TypedFactRelationCount {
    relation: TypedFactRelationName,
    rows: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TypedFactRelationCountError {
    ZeroRows,
}

impl fmt::Display for TypedFactRelationCountError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ZeroRows => write!(
                f,
                "typed fact relation count rows must be greater than zero"
            ),
        }
    }
}

impl std::error::Error for TypedFactRelationCountError {}

impl TypedFactRelationCount {
    pub fn new(relation: TypedFactRelationName, rows: usize) -> Self {
        Self::try_new(relation, rows).unwrap_or_else(|err| panic!("{err}"))
    }

    pub fn try_new(
        relation: TypedFactRelationName,
        rows: usize,
    ) -> Result<Self, TypedFactRelationCountError> {
        if rows == 0 {
            return Err(TypedFactRelationCountError::ZeroRows);
        }
        Ok(Self { relation, rows })
    }

    pub const fn relation(&self) -> TypedFactRelationName {
        self.relation
    }

    pub const fn relation_name(&self) -> &'static str {
        self.relation.as_str()
    }

    pub const fn rows(&self) -> usize {
        self.rows
    }
}

impl<'de> Deserialize<'de> for TypedFactRelationCount {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct RawCount {
            relation: TypedFactRelationName,
            rows: usize,
        }

        let raw = RawCount::deserialize(deserializer)?;
        Self::try_new(raw.relation, raw.rows).map_err(de::Error::custom)
    }
}
