use serde::{Deserialize, Deserializer, Serialize, de};

use crate::facts::FactId;

use super::text::{ShapeFactTextError, non_empty_shape_fact_text, validated_shape_node_id};

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ShapeChildFact {
    parent: FactId,
    child: FactId,
    label: String,
    order: u32,
}

impl ShapeChildFact {
    pub fn new(parent: FactId, child: FactId, label: impl Into<String>, order: u32) -> Self {
        Self::try_new(parent, child, label, order).unwrap_or_else(|err| panic!("{err}"))
    }

    pub fn try_new(
        parent: FactId,
        child: FactId,
        label: impl Into<String>,
        order: u32,
    ) -> Result<Self, ShapeFactTextError> {
        Ok(Self {
            parent: validated_shape_node_id(parent)?,
            child: validated_shape_node_id(child)?,
            label: non_empty_shape_fact_text("shape child label", label)?,
            order,
        })
    }

    pub const fn parent(&self) -> FactId {
        self.parent
    }

    pub const fn child(&self) -> FactId {
        self.child
    }

    pub fn label(&self) -> &str {
        &self.label
    }

    pub const fn order(&self) -> u32 {
        self.order
    }
}

impl<'de> Deserialize<'de> for ShapeChildFact {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct RawShapeChild {
            parent: FactId,
            child: FactId,
            label: String,
            order: u32,
        }

        let raw = RawShapeChild::deserialize(deserializer)?;
        Self::try_new(raw.parent, raw.child, raw.label, raw.order).map_err(de::Error::custom)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ShapeEdgeFact {
    from: FactId,
    to: FactId,
    label: String,
}

impl ShapeEdgeFact {
    pub fn new(from: FactId, to: FactId, label: impl Into<String>) -> Self {
        Self::try_new(from, to, label).unwrap_or_else(|err| panic!("{err}"))
    }

    pub fn try_new(
        from: FactId,
        to: FactId,
        label: impl Into<String>,
    ) -> Result<Self, ShapeFactTextError> {
        Ok(Self {
            from: validated_shape_node_id(from)?,
            to: validated_shape_node_id(to)?,
            label: non_empty_shape_fact_text("shape edge label", label)?,
        })
    }

    pub const fn from(&self) -> FactId {
        self.from
    }

    pub const fn to(&self) -> FactId {
        self.to
    }

    pub fn label(&self) -> &str {
        &self.label
    }
}

impl<'de> Deserialize<'de> for ShapeEdgeFact {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct RawShapeEdge {
            from: FactId,
            to: FactId,
            label: String,
        }

        let raw = RawShapeEdge::deserialize(deserializer)?;
        Self::try_new(raw.from, raw.to, raw.label).map_err(de::Error::custom)
    }
}
