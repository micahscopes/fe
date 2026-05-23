mod graph_export;
mod ids;
mod index_error;
mod origin_fact;
mod origin_index;
mod origin_path;
mod relation;
mod relation_export;
mod relation_index;
mod relation_schema;
mod shape_fact;
mod shape_hash;
mod shape_index;
mod source_span;
mod typed_fact;
mod typed_fact_set;

pub use graph_export::{origin_graph_facts, shape_graph_facts, try_origin_graph_facts};
pub use ids::{FactId, FactIdAllocator, FactNamespace, FactNamespaceError};
pub use index_error::{FactIndexError, SourceSpanFactError};
pub use origin_fact::{OriginLinkFact, OriginNodeFact};
pub use origin_index::OriginFactIndex;
pub use origin_path::{
    OriginKindPathWitness, OriginPath, OriginPathError, OriginPathWitnessExport,
    OriginPathWitnessExportError, OriginReachabilitySummary, OriginReachabilitySummaryError,
    OriginReachableKindPairSummary, OriginSourcePathWitnessExport,
    OriginSourcePathWitnessExportError,
};
pub use relation::{
    TypedFactRelation, TypedFactRelationCount, TypedFactRelationCountError, TypedFactRelationError,
    TypedFactRelationRow, TypedFactRelationSet,
};
pub use relation_index::TypedFactRelationIndex;
pub use relation_schema::{
    TypedFactRelationColumnName, TypedFactRelationName, TypedFactRelationSchema,
    typed_fact_relation_schemas,
};
pub use shape_fact::{
    DataFlowFact, ShapeChildFact, ShapeEdgeFact, ShapeFactTextError, ShapeFieldFact, ShapeNodeFact,
    TraceEventFact,
};
pub use shape_hash::{
    ShapeHashDigest, ShapeHashDigestError, ShapeHashFact, ShapeHashFactError, ShapeHashFactKey,
    ShapeHashNodeScopeError, ShapeHashScope,
};
pub use shape_index::ShapeFactIndex;
pub use source_span::{
    SourceSpanExport, SourceSpanExportError, SourceSpanFact, SourceSpanFactBuildError,
    SourceSpanFileCount, SourceSpanFileCountError, SourceSpanKind,
};
pub use typed_fact::{OwnedTypedFactSetExport, TypedFact, TypedFactSetExport};
pub use typed_fact_set::TypedFactSet;

#[cfg(test)]
mod tests;
