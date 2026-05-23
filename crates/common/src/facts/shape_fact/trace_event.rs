use serde::{Deserialize, Deserializer, Serialize, de};

use crate::facts::FactId;

use super::text::{ShapeFactTextError, non_empty_shape_fact_text, validated_shape_node_id};

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct TraceEventFact {
    node: FactId,
    event_kind: String,
    value: String,
}

impl TraceEventFact {
    pub fn new(node: FactId, event_kind: impl Into<String>, value: impl Into<String>) -> Self {
        Self::try_new(node, event_kind, value).unwrap_or_else(|err| panic!("{err}"))
    }

    pub fn try_new(
        node: FactId,
        event_kind: impl Into<String>,
        value: impl Into<String>,
    ) -> Result<Self, ShapeFactTextError> {
        Ok(Self {
            node: validated_shape_node_id(node)?,
            event_kind: non_empty_shape_fact_text("trace event kind", event_kind)?,
            value: value.into(),
        })
    }

    pub const fn node(&self) -> FactId {
        self.node
    }

    pub fn event_kind(&self) -> &str {
        &self.event_kind
    }

    pub fn value(&self) -> &str {
        &self.value
    }
}

impl<'de> Deserialize<'de> for TraceEventFact {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct RawTraceEvent {
            node: FactId,
            event_kind: String,
            value: String,
        }

        let raw = RawTraceEvent::deserialize(deserializer)?;
        Self::try_new(raw.node, raw.event_kind, raw.value).map_err(de::Error::custom)
    }
}
