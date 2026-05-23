use serde::{Deserialize, Deserializer, Serialize, de};

use crate::{facts::FactId, shape::ShapeNodeId};

use super::text::{ShapeFactTextError, non_empty_shape_fact_text, validated_shape_node_id};

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ShapeNodeFact {
    id: FactId,
    source_id: ShapeNodeId,
    stable_key: String,
    kind: String,
}

impl ShapeNodeFact {
    pub fn new(
        id: FactId,
        source_id: ShapeNodeId,
        stable_key: impl Into<String>,
        kind: impl Into<String>,
    ) -> Self {
        Self::try_new(id, source_id, stable_key, kind).unwrap_or_else(|err| panic!("{err}"))
    }

    pub fn try_new(
        id: FactId,
        source_id: ShapeNodeId,
        stable_key: impl Into<String>,
        kind: impl Into<String>,
    ) -> Result<Self, ShapeFactTextError> {
        Ok(Self {
            id: validated_shape_node_id(id)?,
            source_id,
            stable_key: non_empty_shape_fact_text("shape stable key", stable_key)?,
            kind: non_empty_shape_fact_text("shape node kind", kind)?,
        })
    }

    pub const fn id(&self) -> FactId {
        self.id
    }

    pub const fn source_id(&self) -> ShapeNodeId {
        self.source_id
    }

    pub fn stable_key(&self) -> &str {
        &self.stable_key
    }

    pub fn kind(&self) -> &str {
        &self.kind
    }
}

impl<'de> Deserialize<'de> for ShapeNodeFact {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct RawShapeNode {
            id: FactId,
            source_id: ShapeNodeId,
            stable_key: String,
            kind: String,
        }

        let raw = RawShapeNode::deserialize(deserializer)?;
        Self::try_new(raw.id, raw.source_id, raw.stable_key, raw.kind).map_err(de::Error::custom)
    }
}
