use std::collections::BTreeMap;

use crate::{
    facts::{
        SourceSpanExport, SourceSpanKind, TypedFactRelationColumnName, TypedFactRelationError,
        TypedFactRelationName,
    },
    origin::OriginExportKey,
};

use super::super::{super::TypedFactRelationIndex, graph::origin_node_id_ordinal};
use super::columns::SourceSpanRelationColumns;

pub(in crate::facts::relation_index::origin_paths) fn source_spans_by_origin_id<'a>(
    index: &TypedFactRelationIndex<'a>,
    keys_by_id: &BTreeMap<&'a str, OriginExportKey>,
) -> Result<BTreeMap<&'a str, Vec<SourceSpanExport>>, TypedFactRelationError> {
    let relation_table = index.relation(TypedFactRelationName::SourceSpan)?;
    let columns = SourceSpanRelationColumns::from_index(index)?;
    let mut source_spans_by_id = BTreeMap::<&'a str, Vec<SourceSpanExport>>::new();

    for row in relation_table.rows() {
        let origin_id = row[columns.origin].as_str();
        origin_node_id_ordinal(
            TypedFactRelationName::SourceSpan,
            TypedFactRelationColumnName::Origin,
            origin_id,
        )?;
        let Some(origin_key) = keys_by_id.get(origin_id) else {
            return Err(TypedFactRelationError::MissingRelationReference {
                relation: TypedFactRelationName::SourceSpan.as_str().to_string(),
                column: TypedFactRelationColumnName::Origin.as_str().to_string(),
                value: origin_id.to_string(),
                target_relation: TypedFactRelationName::OriginNode.as_str().to_string(),
            });
        };
        let Some(span_kind) = SourceSpanKind::from_str(&row[columns.span_kind]) else {
            return Err(TypedFactRelationError::InvalidRelationValue {
                relation: TypedFactRelationName::SourceSpan.as_str().to_string(),
                column: TypedFactRelationColumnName::SpanKind.as_str().to_string(),
                value: row[columns.span_kind].clone(),
            });
        };

        source_spans_by_id
            .entry(origin_id)
            .or_default()
            .push(SourceSpanExport::new(
                origin_key.clone(),
                span_kind,
                row[columns.file].clone(),
                parse_source_span_number(
                    index,
                    TypedFactRelationColumnName::StartByte,
                    &row[columns.start_byte],
                )?,
                parse_source_span_number(
                    index,
                    TypedFactRelationColumnName::EndByte,
                    &row[columns.end_byte],
                )?,
                parse_source_span_number(
                    index,
                    TypedFactRelationColumnName::StartLine,
                    &row[columns.start_line],
                )?,
                parse_source_span_number(
                    index,
                    TypedFactRelationColumnName::StartCol,
                    &row[columns.start_col],
                )?,
                parse_source_span_number(
                    index,
                    TypedFactRelationColumnName::EndLine,
                    &row[columns.end_line],
                )?,
                parse_source_span_number(
                    index,
                    TypedFactRelationColumnName::EndCol,
                    &row[columns.end_col],
                )?,
            ));
    }
    for spans in source_spans_by_id.values_mut() {
        spans.sort();
    }

    Ok(source_spans_by_id)
}

fn parse_source_span_number<'a>(
    index: &TypedFactRelationIndex<'a>,
    column: TypedFactRelationColumnName,
    value: &str,
) -> Result<usize, TypedFactRelationError> {
    index.parse_relation_number(TypedFactRelationName::SourceSpan, column, value)
}
