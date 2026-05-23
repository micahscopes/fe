use serde::de;

use crate::facts::{
    DataFlowFact, OriginLinkFact, OriginNodeFact, ShapeChildFact, ShapeEdgeFact, ShapeFieldFact,
    ShapeHashFact, ShapeNodeFact, SourceSpanFact, TraceEventFact,
};

use super::super::TypedFact;
use super::raw::RawTypedFact;

impl RawTypedFact {
    pub(super) fn into_typed_fact<E: de::Error>(self) -> Result<TypedFact, E> {
        match self {
            Self::OriginNode { id, key } => Ok(TypedFact::OriginNode(
                OriginNodeFact::try_new(id, key).map_err(E::custom)?,
            )),
            Self::OriginLink { from, to, kind } => Ok(TypedFact::OriginLink(
                OriginLinkFact::try_new(from, to, kind).map_err(E::custom)?,
            )),
            Self::SourceSpan {
                origin,
                span_kind,
                file,
                start_byte,
                end_byte,
                start_line,
                start_col,
                end_line,
                end_col,
            } => Ok(TypedFact::SourceSpan(
                SourceSpanFact::try_new(
                    origin, span_kind, file, start_byte, end_byte, start_line, start_col, end_line,
                    end_col,
                )
                .map_err(E::custom)?,
            )),
            Self::ShapeNode {
                id,
                source_id,
                stable_key,
                kind,
            } => Ok(TypedFact::ShapeNode(
                ShapeNodeFact::try_new(id, source_id, stable_key, kind).map_err(E::custom)?,
            )),
            Self::ShapeField {
                node,
                dimension,
                name,
                value,
            } => Ok(TypedFact::ShapeField(
                ShapeFieldFact::try_new(node, dimension, name, value).map_err(E::custom)?,
            )),
            Self::ShapeChild {
                parent,
                child,
                label,
                order,
            } => Ok(TypedFact::ShapeChild(
                ShapeChildFact::try_new(parent, child, label, order).map_err(E::custom)?,
            )),
            Self::ShapeEdge { from, to, label } => Ok(TypedFact::ShapeEdge(
                ShapeEdgeFact::try_new(from, to, label).map_err(E::custom)?,
            )),
            Self::TraceEvent {
                node,
                event_kind,
                value,
            } => Ok(TypedFact::TraceEvent(
                TraceEventFact::try_new(node, event_kind, value).map_err(E::custom)?,
            )),
            Self::DataFlow {
                source,
                target,
                kind,
            } => Ok(TypedFact::DataFlow(
                DataFlowFact::try_new(source, target, kind).map_err(E::custom)?,
            )),
            Self::ShapeHash {
                node,
                scope,
                dimension,
                digest_hex,
            } => Ok(TypedFact::ShapeHash(
                ShapeHashFact::try_from_digest_hex(node, scope, dimension, digest_hex)
                    .map_err(E::custom)?,
            )),
        }
    }
}
