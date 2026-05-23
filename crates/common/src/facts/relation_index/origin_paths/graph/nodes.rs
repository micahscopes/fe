use std::collections::BTreeMap;

use crate::{
    facts::{TypedFactRelationColumnName, TypedFactRelationError, TypedFactRelationName},
    origin::{OriginExportKey, OriginExportKind},
};

use super::{super::super::TypedFactRelationIndex, origin_node_id_ordinal};
use crate::facts::relation_index::validation::invalid_origin_export_key_part;

pub(super) fn origin_node_ids_in_fact_order<'a>(
    index: &TypedFactRelationIndex<'a>,
) -> Result<Vec<&'a str>, TypedFactRelationError> {
    let relation_table = index.relation(TypedFactRelationName::OriginNode)?;
    let id_column = index.column_index(
        TypedFactRelationName::OriginNode,
        TypedFactRelationColumnName::Id,
    )?;
    let mut ids = Vec::new();
    for row in relation_table.rows() {
        let id = row[id_column].as_str();
        ids.push((
            origin_node_id_ordinal(
                TypedFactRelationName::OriginNode,
                TypedFactRelationColumnName::Id,
                id,
            )?,
            id,
        ));
    }
    ids.sort_by_key(|(ordinal, _)| *ordinal);
    Ok(ids.into_iter().map(|(_, id)| id).collect())
}

pub(super) fn origin_node_keys_by_id<'a>(
    index: &TypedFactRelationIndex<'a>,
) -> Result<BTreeMap<&'a str, OriginExportKey>, TypedFactRelationError> {
    let relation_table = index.relation(TypedFactRelationName::OriginNode)?;
    let id_column = index.column_index(
        TypedFactRelationName::OriginNode,
        TypedFactRelationColumnName::Id,
    )?;
    let kind_column = index.column_index(
        TypedFactRelationName::OriginNode,
        TypedFactRelationColumnName::Kind,
    )?;
    let owner_column = index.column_index(
        TypedFactRelationName::OriginNode,
        TypedFactRelationColumnName::OwnerKey,
    )?;
    let local_column = index.column_index(
        TypedFactRelationName::OriginNode,
        TypedFactRelationColumnName::LocalKey,
    )?;
    let mut keys_by_id = BTreeMap::new();

    for row in relation_table.rows() {
        let Some(kind) = OriginExportKind::from_str(&row[kind_column]) else {
            return Err(TypedFactRelationError::InvalidRelationValue {
                relation: TypedFactRelationName::OriginNode.as_str().to_string(),
                column: TypedFactRelationColumnName::Kind.as_str().to_string(),
                value: row[kind_column].clone(),
            });
        };
        let key = OriginExportKey::try_from_raw_parts(
            kind,
            row[owner_column].clone(),
            row[local_column].clone(),
        )
        .map_err(|err| {
            let column = invalid_origin_export_key_part(err);
            let idx = match column {
                "owner_key" => owner_column,
                "local_key" => local_column,
                _ => owner_column,
            };
            TypedFactRelationError::InvalidRelationValue {
                relation: TypedFactRelationName::OriginNode.as_str().to_string(),
                column: column.to_string(),
                value: row[idx].clone(),
            }
        })?;
        keys_by_id.insert(row[id_column].as_str(), key);
    }

    Ok(keys_by_id)
}
