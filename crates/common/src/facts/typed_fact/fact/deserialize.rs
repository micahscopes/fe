mod construct;
mod raw;

use serde::{Deserialize, Deserializer};

use super::TypedFact;
use raw::RawTypedFact;

impl<'de> Deserialize<'de> for TypedFact {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        RawTypedFact::deserialize(deserializer)?.into_typed_fact()
    }
}
