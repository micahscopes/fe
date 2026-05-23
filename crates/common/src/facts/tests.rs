use std::collections::BTreeSet;

use crate::{
    facts::{
        DataFlowFact, FactId, FactIndexError, FactNamespace, FactNamespaceError, OriginFactIndex,
        OriginLinkFact, OriginLinkFact as OriginLinkFactRow, OriginNodeFact, OriginPath,
        OriginPathError, OriginPathWitnessExport, OriginPathWitnessExportError,
        OriginReachabilitySummary, OriginReachabilitySummaryError, OriginReachableKindPairSummary,
        OriginSourcePathWitnessExport, OriginSourcePathWitnessExportError, OwnedTypedFactSetExport,
        ShapeChildFact, ShapeEdgeFact, ShapeFactIndex, ShapeFactTextError, ShapeFieldFact,
        ShapeHashDigest, ShapeHashDigestError, ShapeHashFact, ShapeHashFactError, ShapeHashFactKey,
        ShapeHashNodeScopeError, ShapeHashScope, ShapeNodeFact, SourceSpanExport,
        SourceSpanExportError, SourceSpanFact, SourceSpanFactBuildError, SourceSpanFileCount,
        SourceSpanFileCountError, SourceSpanKind, TraceEventFact, TypedFact, TypedFactRelation,
        TypedFactRelationColumnName, TypedFactRelationCount, TypedFactRelationCountError,
        TypedFactRelationError, TypedFactRelationIndex, TypedFactRelationName,
        TypedFactRelationSet, TypedFactSet, origin_graph_facts, shape_graph_facts,
        try_origin_graph_facts, typed_fact_relation_schemas,
    },
    origin::{OriginExportKey, OriginExportKind, OriginGraph, OriginLinkKind},
    shape::{ShapeDimension, ShapeGraph, ShapeNodeId},
};

fn relation_rows_mut<'a>(
    value: &'a mut serde_json::Value,
    relation_name: &str,
) -> &'a mut Vec<serde_json::Value> {
    value["relations"]
        .as_array_mut()
        .expect("relations should be an array")
        .iter_mut()
        .find(|relation| relation["name"] == relation_name)
        .expect("relation should exist")["rows"]
        .as_array_mut()
        .expect("relation rows should be an array")
}

fn relation_cell(row: &serde_json::Value, column: usize) -> String {
    row.as_array().expect("relation row should be an array")[column]
        .as_str()
        .expect("relation cell should be a string")
        .to_string()
}

fn origin_key(kind: OriginExportKind, owner: &str, local: &str) -> OriginExportKey {
    OriginExportKey::try_from_raw_parts(kind, owner, local).unwrap()
}

mod fact_dtos;
mod graph_and_source_span;
mod origin_paths;
mod relation_export;
mod relation_index;
mod relation_json;
mod shape_index;
mod typed_fact_json;
