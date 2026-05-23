use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, de};

use crate::shape::{ShapeDimension, ShapeNodeId};

use super::{FactId, FactNamespace, FactNamespaceError, ids::validated_fact_namespace};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ShapeFactTextError {
    Empty { field: &'static str },
    WrongNamespace(FactNamespaceError),
}

impl fmt::Display for ShapeFactTextError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty { field } => write!(f, "{field} must not be empty"),
            Self::WrongNamespace(err) => err.fmt(f),
        }
    }
}

impl std::error::Error for ShapeFactTextError {}

impl From<FactNamespaceError> for ShapeFactTextError {
    fn from(err: FactNamespaceError) -> Self {
        Self::WrongNamespace(err)
    }
}

fn validated_shape_node_id(id: FactId) -> Result<FactId, ShapeFactTextError> {
    validated_fact_namespace(id, FactNamespace::ShapeNode).map_err(Into::into)
}

fn non_empty_shape_fact_text(
    field: &'static str,
    value: impl Into<String>,
) -> Result<String, ShapeFactTextError> {
    let value = value.into();
    if value.is_empty() {
        return Err(ShapeFactTextError::Empty { field });
    }
    Ok(value)
}

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
