use serde::{Deserialize, Deserializer, Serialize, Serializer, de, ser::SerializeStruct};

use crate::{
    origin::{OriginExportKey, OriginLinkKind},
    shape::{ShapeDimension, ShapeNodeId},
};

use super::{
    DataFlowFact, FactId, OriginFactIndex, OriginLinkFact, OriginNodeFact, ShapeChildFact,
    ShapeEdgeFact, ShapeFactIndex, ShapeFieldFact, ShapeHashFact, ShapeHashScope, ShapeNodeFact,
    SourceSpanFact, SourceSpanKind, TraceEventFact, TypedFactSet,
};

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct OwnedTypedFactSetExport {
    schema_version: u32,
    facts: Vec<TypedFact>,
}

impl OwnedTypedFactSetExport {
    pub const SCHEMA_VERSION: u32 = 1;

    pub(super) fn from_facts(facts: Vec<TypedFact>) -> Self {
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
    pub(super) const fn new(facts: &'a [TypedFact]) -> Self {
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TypedFact {
    OriginNode(OriginNodeFact),
    OriginLink(OriginLinkFact),
    SourceSpan(SourceSpanFact),
    ShapeNode(ShapeNodeFact),
    ShapeField(ShapeFieldFact),
    ShapeChild(ShapeChildFact),
    ShapeEdge(ShapeEdgeFact),
    TraceEvent(TraceEventFact),
    DataFlow(DataFlowFact),
    ShapeHash(ShapeHashFact),
}

impl Serialize for TypedFact {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::OriginNode(fact) => {
                let mut out = serializer.serialize_struct("TypedFact", 3)?;
                out.serialize_field("type", "origin_node")?;
                out.serialize_field("id", &fact.id())?;
                out.serialize_field("key", fact.key())?;
                out.end()
            }
            Self::OriginLink(fact) => {
                let mut out = serializer.serialize_struct("TypedFact", 4)?;
                out.serialize_field("type", "origin_link")?;
                out.serialize_field("from", &fact.from())?;
                out.serialize_field("to", &fact.to())?;
                out.serialize_field("kind", &fact.kind())?;
                out.end()
            }
            Self::SourceSpan(fact) => {
                let mut out = serializer.serialize_struct("TypedFact", 10)?;
                out.serialize_field("type", "source_span")?;
                out.serialize_field("origin", &fact.origin())?;
                out.serialize_field("span_kind", &fact.span_kind())?;
                out.serialize_field("file", fact.file())?;
                out.serialize_field("start_byte", &fact.start_byte())?;
                out.serialize_field("end_byte", &fact.end_byte())?;
                out.serialize_field("start_line", &fact.start_line())?;
                out.serialize_field("start_col", &fact.start_col())?;
                out.serialize_field("end_line", &fact.end_line())?;
                out.serialize_field("end_col", &fact.end_col())?;
                out.end()
            }
            Self::ShapeNode(fact) => {
                let mut out = serializer.serialize_struct("TypedFact", 5)?;
                out.serialize_field("type", "shape_node")?;
                out.serialize_field("id", &fact.id())?;
                out.serialize_field("source_id", &fact.source_id())?;
                out.serialize_field("stable_key", fact.stable_key())?;
                out.serialize_field("kind", fact.kind())?;
                out.end()
            }
            Self::ShapeField(fact) => {
                let mut out = serializer.serialize_struct("TypedFact", 5)?;
                out.serialize_field("type", "shape_field")?;
                out.serialize_field("node", &fact.node())?;
                out.serialize_field("dimension", &fact.dimension())?;
                out.serialize_field("name", fact.name())?;
                out.serialize_field("value", fact.value())?;
                out.end()
            }
            Self::ShapeChild(fact) => {
                let mut out = serializer.serialize_struct("TypedFact", 5)?;
                out.serialize_field("type", "shape_child")?;
                out.serialize_field("parent", &fact.parent())?;
                out.serialize_field("child", &fact.child())?;
                out.serialize_field("label", fact.label())?;
                out.serialize_field("order", &fact.order())?;
                out.end()
            }
            Self::ShapeEdge(fact) => {
                let mut out = serializer.serialize_struct("TypedFact", 4)?;
                out.serialize_field("type", "shape_edge")?;
                out.serialize_field("from", &fact.from())?;
                out.serialize_field("to", &fact.to())?;
                out.serialize_field("label", fact.label())?;
                out.end()
            }
            Self::TraceEvent(fact) => {
                let mut out = serializer.serialize_struct("TypedFact", 4)?;
                out.serialize_field("type", "trace_event")?;
                out.serialize_field("node", &fact.node())?;
                out.serialize_field("event_kind", fact.event_kind())?;
                out.serialize_field("value", fact.value())?;
                out.end()
            }
            Self::DataFlow(fact) => {
                let mut out = serializer.serialize_struct("TypedFact", 4)?;
                out.serialize_field("type", "data_flow")?;
                out.serialize_field("source", &fact.source())?;
                out.serialize_field("target", &fact.target())?;
                out.serialize_field("kind", fact.kind())?;
                out.end()
            }
            Self::ShapeHash(fact) => {
                let mut out = serializer.serialize_struct("TypedFact", 5)?;
                out.serialize_field("type", "shape_hash")?;
                out.serialize_field("node", &fact.node())?;
                out.serialize_field("scope", &fact.scope())?;
                out.serialize_field("dimension", &fact.dimension())?;
                out.serialize_field("digest_hex", fact.digest())?;
                out.end()
            }
        }
    }
}

impl<'de> Deserialize<'de> for TypedFact {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(tag = "type", deny_unknown_fields)]
        enum RawTypedFact {
            #[serde(rename = "origin_node")]
            OriginNode { id: FactId, key: OriginExportKey },
            #[serde(rename = "origin_link")]
            OriginLink {
                from: FactId,
                to: FactId,
                kind: OriginLinkKind,
            },
            #[serde(rename = "source_span")]
            SourceSpan {
                origin: FactId,
                span_kind: SourceSpanKind,
                file: String,
                start_byte: usize,
                end_byte: usize,
                start_line: usize,
                start_col: usize,
                end_line: usize,
                end_col: usize,
            },
            #[serde(rename = "shape_node")]
            ShapeNode {
                id: FactId,
                source_id: ShapeNodeId,
                stable_key: String,
                kind: String,
            },
            #[serde(rename = "shape_field")]
            ShapeField {
                node: FactId,
                dimension: ShapeDimension,
                name: String,
                value: String,
            },
            #[serde(rename = "shape_child")]
            ShapeChild {
                parent: FactId,
                child: FactId,
                label: String,
                order: u32,
            },
            #[serde(rename = "shape_edge")]
            ShapeEdge {
                from: FactId,
                to: FactId,
                label: String,
            },
            #[serde(rename = "trace_event")]
            TraceEvent {
                node: FactId,
                event_kind: String,
                value: String,
            },
            #[serde(rename = "data_flow")]
            DataFlow {
                source: FactId,
                target: FactId,
                kind: String,
            },
            #[serde(rename = "shape_hash")]
            ShapeHash {
                node: Option<FactId>,
                scope: ShapeHashScope,
                dimension: ShapeDimension,
                digest_hex: String,
            },
        }

        match RawTypedFact::deserialize(deserializer)? {
            RawTypedFact::OriginNode { id, key } => Ok(Self::OriginNode(
                OriginNodeFact::try_new(id, key).map_err(de::Error::custom)?,
            )),
            RawTypedFact::OriginLink { from, to, kind } => Ok(Self::OriginLink(
                OriginLinkFact::try_new(from, to, kind).map_err(de::Error::custom)?,
            )),
            RawTypedFact::SourceSpan {
                origin,
                span_kind,
                file,
                start_byte,
                end_byte,
                start_line,
                start_col,
                end_line,
                end_col,
            } => Ok(Self::SourceSpan(
                SourceSpanFact::try_new(
                    origin, span_kind, file, start_byte, end_byte, start_line, start_col, end_line,
                    end_col,
                )
                .map_err(de::Error::custom)?,
            )),
            RawTypedFact::ShapeNode {
                id,
                source_id,
                stable_key,
                kind,
            } => Ok(Self::ShapeNode(
                ShapeNodeFact::try_new(id, source_id, stable_key, kind)
                    .map_err(de::Error::custom)?,
            )),
            RawTypedFact::ShapeField {
                node,
                dimension,
                name,
                value,
            } => Ok(Self::ShapeField(
                ShapeFieldFact::try_new(node, dimension, name, value).map_err(de::Error::custom)?,
            )),
            RawTypedFact::ShapeChild {
                parent,
                child,
                label,
                order,
            } => Ok(Self::ShapeChild(
                ShapeChildFact::try_new(parent, child, label, order).map_err(de::Error::custom)?,
            )),
            RawTypedFact::ShapeEdge { from, to, label } => Ok(Self::ShapeEdge(
                ShapeEdgeFact::try_new(from, to, label).map_err(de::Error::custom)?,
            )),
            RawTypedFact::TraceEvent {
                node,
                event_kind,
                value,
            } => Ok(Self::TraceEvent(
                TraceEventFact::try_new(node, event_kind, value).map_err(de::Error::custom)?,
            )),
            RawTypedFact::DataFlow {
                source,
                target,
                kind,
            } => Ok(Self::DataFlow(
                DataFlowFact::try_new(source, target, kind).map_err(de::Error::custom)?,
            )),
            RawTypedFact::ShapeHash {
                node,
                scope,
                dimension,
                digest_hex,
            } => Ok(Self::ShapeHash(
                ShapeHashFact::try_from_digest_hex(node, scope, dimension, digest_hex)
                    .map_err(de::Error::custom)?,
            )),
        }
    }
}
