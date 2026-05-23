use serde::{Deserialize, Deserializer, Serialize, de};

use super::fact::TypedFact;
use crate::facts::{OriginFactIndex, ShapeFactIndex, TypedFactSet};

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct OwnedTypedFactSetExport {
    schema_version: u32,
    facts: Vec<TypedFact>,
}

impl OwnedTypedFactSetExport {
    pub const SCHEMA_VERSION: u32 = 1;

    pub(in crate::facts) fn from_facts(facts: Vec<TypedFact>) -> Self {
        Self {
            schema_version: Self::SCHEMA_VERSION,
            facts,
        }
    }

    pub const fn schema_version(&self) -> u32 {
        self.schema_version
    }

    pub fn facts(&self) -> &[TypedFact] {
        &self.facts
    }
}

impl<'de> Deserialize<'de> for OwnedTypedFactSetExport {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct RawExport {
            schema_version: u32,
            facts: Vec<TypedFact>,
        }

        let raw = RawExport::deserialize(deserializer)?;
        if raw.schema_version != Self::SCHEMA_VERSION {
            return Err(de::Error::custom(format!(
                "unsupported typed fact schema_version {}; expected {}",
                raw.schema_version,
                Self::SCHEMA_VERSION
            )));
        }

        let facts = TypedFactSet::new(raw.facts);
        OriginFactIndex::new(&facts).map_err(|err| {
            de::Error::custom(format!("invalid origin facts in typed fact export: {err}"))
        })?;
        ShapeFactIndex::new(&facts).map_err(|err| {
            de::Error::custom(format!("invalid shape facts in typed fact export: {err}"))
        })?;

        Ok(Self {
            schema_version: raw.schema_version,
            facts: facts.into_facts(),
        })
    }
}

#[derive(Clone, Copy, Debug, Serialize)]
pub struct TypedFactSetExport<'a> {
    schema_version: u32,
    facts: &'a [TypedFact],
}

impl<'a> TypedFactSetExport<'a> {
    pub(in crate::facts) const fn new(facts: &'a [TypedFact]) -> Self {
        Self {
            schema_version: OwnedTypedFactSetExport::SCHEMA_VERSION,
            facts,
        }
    }

    pub const fn schema_version(self) -> u32 {
        self.schema_version
    }

    pub const fn facts(self) -> &'a [TypedFact] {
        self.facts
    }
}
