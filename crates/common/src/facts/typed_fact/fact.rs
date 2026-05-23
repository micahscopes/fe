mod deserialize;
mod serialize;

use crate::facts::{
    DataFlowFact, OriginLinkFact, OriginNodeFact, ShapeChildFact, ShapeEdgeFact, ShapeFieldFact,
    ShapeHashFact, ShapeNodeFact, SourceSpanFact, TraceEventFact,
};

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
