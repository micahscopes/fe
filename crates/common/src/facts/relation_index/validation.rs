use crate::{
    origin::{OriginExportKind, OriginLinkKind},
    shape::ShapeDimension,
};

use super::super::{
    SourceSpanKind, TypedFactRelationColumnName, TypedFactRelationError, TypedFactRelationName,
};
use super::TypedFactRelationIndex;

mod helpers;
mod origin_keys;
mod shape_hashes;
mod source_spans;

pub(super) use origin_keys::invalid_origin_export_key_part;

impl<'a> TypedFactRelationIndex<'a> {
    pub(super) fn validate_semantics(self) -> Result<Self, TypedFactRelationError> {
        use TypedFactRelationColumnName as Column;
        use TypedFactRelationName as Relation;

        self.validate_column_values(
            Relation::OriginNode,
            Column::Kind,
            OriginExportKind::from_str,
        )?;
        self.validate_column_values(Relation::OriginLink, Column::Kind, OriginLinkKind::from_str)?;
        self.validate_column_values(
            Relation::SourceSpan,
            Column::SpanKind,
            SourceSpanKind::from_str,
        )?;
        self.validate_column_values(
            Relation::ShapeField,
            Column::Dimension,
            ShapeDimension::from_str,
        )?;
        self.validate_numeric_column::<u32>(Relation::ShapeNode, Column::SourceId)?;
        self.validate_numeric_column::<u32>(Relation::ShapeChild, Column::Order)?;
        self.validate_non_empty_column(Relation::ShapeNode, Column::StableKey)?;
        self.validate_non_empty_column(Relation::ShapeNode, Column::Kind)?;
        self.validate_non_empty_column(Relation::ShapeField, Column::Name)?;
        self.validate_non_empty_column(Relation::ShapeChild, Column::Label)?;
        self.validate_non_empty_column(Relation::ShapeEdge, Column::Label)?;
        self.validate_non_empty_column(Relation::TraceEvent, Column::EventKind)?;
        self.validate_non_empty_column(Relation::DataFlow, Column::Kind)?;
        self.validate_origin_export_key_rows()?;

        self.validate_unique_columns(
            Relation::OriginNode,
            &[Column::Kind, Column::OwnerKey, Column::LocalKey],
        )?;
        self.validate_unique_columns(
            Relation::OriginLink,
            &[Column::From, Column::To, Column::Kind],
        )?;
        self.validate_unique_columns(Relation::ShapeNode, &[Column::SourceId])?;
        self.validate_unique_columns(Relation::ShapeNode, &[Column::StableKey])?;

        let origin_ids = self.relation_id_set(Relation::OriginNode, Column::Id)?;
        let shape_ids = self.relation_id_set(Relation::ShapeNode, Column::Id)?;

        self.validate_relation_references(
            Relation::OriginLink,
            [
                (Column::From, &origin_ids, Relation::OriginNode),
                (Column::To, &origin_ids, Relation::OriginNode),
            ],
        )?;
        self.validate_relation_references(
            Relation::SourceSpan,
            [(Column::Origin, &origin_ids, Relation::OriginNode)],
        )?;
        self.validate_relation_references(
            Relation::ShapeField,
            [(Column::Node, &shape_ids, Relation::ShapeNode)],
        )?;
        self.validate_relation_references(
            Relation::ShapeChild,
            [
                (Column::Parent, &shape_ids, Relation::ShapeNode),
                (Column::Child, &shape_ids, Relation::ShapeNode),
            ],
        )?;
        self.validate_relation_references(
            Relation::ShapeEdge,
            [
                (Column::From, &shape_ids, Relation::ShapeNode),
                (Column::To, &shape_ids, Relation::ShapeNode),
            ],
        )?;
        self.validate_relation_references(
            Relation::TraceEvent,
            [(Column::Node, &shape_ids, Relation::ShapeNode)],
        )?;
        self.validate_relation_references(
            Relation::DataFlow,
            [
                (Column::Source, &shape_ids, Relation::ShapeNode),
                (Column::Target, &shape_ids, Relation::ShapeNode),
            ],
        )?;
        self.validate_source_span_rows()?;
        self.validate_shape_hash_rows(&shape_ids)?;

        Ok(self)
    }
}
