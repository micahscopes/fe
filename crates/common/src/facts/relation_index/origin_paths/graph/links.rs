use std::collections::BTreeMap;

use crate::{
    facts::{TypedFactRelationColumnName, TypedFactRelationError, TypedFactRelationName},
    origin::OriginLinkKind,
};

use super::{super::super::TypedFactRelationIndex, origin_node_id_ordinal};

pub(super) fn origin_outgoing_by_id<'a>(
    index: &TypedFactRelationIndex<'a>,
) -> Result<BTreeMap<&'a str, Vec<(&'a str, OriginLinkKind)>>, TypedFactRelationError> {
    let relation_table = index.relation(TypedFactRelationName::OriginLink)?;
    let from_column = index.column_index(
        TypedFactRelationName::OriginLink,
        TypedFactRelationColumnName::From,
    )?;
    let to_column = index.column_index(
        TypedFactRelationName::OriginLink,
        TypedFactRelationColumnName::To,
    )?;
    let kind_column = index.column_index(
        TypedFactRelationName::OriginLink,
        TypedFactRelationColumnName::Kind,
    )?;
    let mut outgoing = BTreeMap::<&str, Vec<(u64, &str, OriginLinkKind)>>::new();

    for row in relation_table.rows() {
        let from = row[from_column].as_str();
        let to = row[to_column].as_str();
        origin_node_id_ordinal(
            TypedFactRelationName::OriginLink,
            TypedFactRelationColumnName::From,
            from,
        )?;
        let to_ordinal = origin_node_id_ordinal(
            TypedFactRelationName::OriginLink,
            TypedFactRelationColumnName::To,
            to,
        )?;
        let Some(kind) = OriginLinkKind::from_str(&row[kind_column]) else {
            return Err(TypedFactRelationError::InvalidRelationValue {
                relation: TypedFactRelationName::OriginLink.as_str().to_string(),
                column: TypedFactRelationColumnName::Kind.as_str().to_string(),
                value: row[kind_column].clone(),
            });
        };
        outgoing
            .entry(from)
            .or_default()
            .push((to_ordinal, to, kind));
    }

    Ok(outgoing
        .into_iter()
        .map(|(from, mut targets)| {
            targets.sort_by_key(|(to_ordinal, _, kind)| (*to_ordinal, *kind));
            (
                from,
                targets
                    .into_iter()
                    .map(|(_, to, kind)| (to, kind))
                    .collect(),
            )
        })
        .collect())
}
