use crate::facts::{
    DataFlowFact, OriginLinkFact, OriginNodeFact, ShapeChildFact, ShapeEdgeFact, ShapeFieldFact,
    ShapeHashFact, ShapeNodeFact, SourceSpanFact, TraceEventFact, TypedFact, TypedFactSet,
};

macro_rules! typed_fact_iterators {
    ($($method:ident => $variant:ident($fact:ty)),+ $(,)?) => {
        impl TypedFactSet {
            $(
                pub fn $method(&self) -> impl Iterator<Item = &$fact> {
                    self.facts.iter().filter_map(|fact| match fact {
                        TypedFact::$variant(fact) => Some(fact),
                        _ => None,
                    })
                }
            )+
        }
    };
}

typed_fact_iterators! {
    origin_nodes => OriginNode(OriginNodeFact),
    origin_links => OriginLink(OriginLinkFact),
    source_spans => SourceSpan(SourceSpanFact),
    shape_nodes => ShapeNode(ShapeNodeFact),
    shape_fields => ShapeField(ShapeFieldFact),
    shape_children => ShapeChild(ShapeChildFact),
    shape_edges => ShapeEdge(ShapeEdgeFact),
    trace_events => TraceEvent(TraceEventFact),
    data_flows => DataFlow(DataFlowFact),
    shape_hashes => ShapeHash(ShapeHashFact),
}
