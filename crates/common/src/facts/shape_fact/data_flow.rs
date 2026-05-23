use serde::{Deserialize, Deserializer, Serialize, de};

use crate::facts::FactId;

use super::text::{ShapeFactTextError, non_empty_shape_fact_text, validated_shape_node_id};

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct DataFlowFact {
    source: FactId,
    target: FactId,
    kind: String,
}

impl DataFlowFact {
    pub fn new(source: FactId, target: FactId, kind: impl Into<String>) -> Self {
        Self::try_new(source, target, kind).unwrap_or_else(|err| panic!("{err}"))
    }

    pub fn try_new(
        source: FactId,
        target: FactId,
        kind: impl Into<String>,
    ) -> Result<Self, ShapeFactTextError> {
        Ok(Self {
            source: validated_shape_node_id(source)?,
            target: validated_shape_node_id(target)?,
            kind: non_empty_shape_fact_text("data flow kind", kind)?,
        })
    }

    pub const fn source(&self) -> FactId {
        self.source
    }

    pub const fn target(&self) -> FactId {
        self.target
    }

    pub fn kind(&self) -> &str {
        &self.kind
    }
}

impl<'de> Deserialize<'de> for DataFlowFact {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct RawDataFlow {
            source: FactId,
            target: FactId,
            kind: String,
        }

        let raw = RawDataFlow::deserialize(deserializer)?;
        Self::try_new(raw.source, raw.target, raw.kind).map_err(de::Error::custom)
    }
}
