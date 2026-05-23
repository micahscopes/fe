use serde::{Deserialize, Deserializer, Serialize, de};

use crate::{facts::FactId, shape::ShapeDimension};

use super::text::{ShapeFactTextError, non_empty_shape_fact_text, validated_shape_node_id};

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ShapeFieldFact {
    node: FactId,
    dimension: ShapeDimension,
    name: String,
    value: String,
}

impl ShapeFieldFact {
    pub fn new(
        node: FactId,
        dimension: ShapeDimension,
        name: impl Into<String>,
        value: impl Into<String>,
    ) -> Self {
        Self::try_new(node, dimension, name, value).unwrap_or_else(|err| panic!("{err}"))
    }

    pub fn try_new(
        node: FactId,
        dimension: ShapeDimension,
        name: impl Into<String>,
        value: impl Into<String>,
    ) -> Result<Self, ShapeFactTextError> {
        Ok(Self {
            node: validated_shape_node_id(node)?,
            dimension,
            name: non_empty_shape_fact_text("shape field name", name)?,
            value: value.into(),
        })
    }

    pub const fn node(&self) -> FactId {
        self.node
    }

    pub const fn dimension(&self) -> ShapeDimension {
        self.dimension
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn value(&self) -> &str {
        &self.value
    }
}

impl<'de> Deserialize<'de> for ShapeFieldFact {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct RawShapeField {
            node: FactId,
            dimension: ShapeDimension,
            name: String,
            value: String,
        }

        let raw = RawShapeField::deserialize(deserializer)?;
        Self::try_new(raw.node, raw.dimension, raw.name, raw.value).map_err(de::Error::custom)
    }
}
