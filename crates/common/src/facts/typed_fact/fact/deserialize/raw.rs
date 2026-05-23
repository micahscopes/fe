use serde::Deserialize;

use crate::{
    facts::{FactId, ShapeHashScope, SourceSpanKind},
    origin::{OriginExportKey, OriginLinkKind},
    shape::{ShapeDimension, ShapeNodeId},
};

#[derive(Deserialize)]
#[serde(tag = "type", deny_unknown_fields)]
pub(super) enum RawTypedFact {
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
