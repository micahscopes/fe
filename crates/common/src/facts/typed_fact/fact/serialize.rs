use serde::{Serialize, Serializer, ser::SerializeStruct};

use super::TypedFact;

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
