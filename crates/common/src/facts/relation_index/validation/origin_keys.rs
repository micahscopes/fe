use crate::{
    facts::{TypedFactRelationColumnName, TypedFactRelationError, TypedFactRelationName},
    origin::{OriginExportKey, OriginExportKeyError, OriginExportKind},
};

use super::super::TypedFactRelationIndex;

impl<'a> TypedFactRelationIndex<'a> {
    pub(in crate::facts::relation_index) fn validate_origin_export_key_rows(
        &self,
    ) -> Result<(), TypedFactRelationError> {
        let relation = self.relation(TypedFactRelationName::OriginNode)?;
        let kind_idx = self.column_index(
            TypedFactRelationName::OriginNode,
            TypedFactRelationColumnName::Kind,
        )?;
        let owner_idx = self.column_index(
            TypedFactRelationName::OriginNode,
            TypedFactRelationColumnName::OwnerKey,
        )?;
        let local_idx = self.column_index(
            TypedFactRelationName::OriginNode,
            TypedFactRelationColumnName::LocalKey,
        )?;

        for row in relation.rows() {
            let Some(kind) = OriginExportKind::from_str(&row[kind_idx]) else {
                return Err(TypedFactRelationError::InvalidRelationValue {
                    relation: TypedFactRelationName::OriginNode.as_str().to_string(),
                    column: TypedFactRelationColumnName::Kind.as_str().to_string(),
                    value: row[kind_idx].clone(),
                });
            };
            if let Err(err) = OriginExportKey::try_from_raw_parts(
                kind,
                row[owner_idx].clone(),
                row[local_idx].clone(),
            ) {
                let column = invalid_origin_export_key_part(err);
                let idx = match column {
                    "owner_key" => owner_idx,
                    "local_key" => local_idx,
                    _ => owner_idx,
                };
                return Err(TypedFactRelationError::InvalidRelationValue {
                    relation: TypedFactRelationName::OriginNode.as_str().to_string(),
                    column: column.to_string(),
                    value: row[idx].clone(),
                });
            }
        }

        Ok(())
    }
}

pub(in crate::facts::relation_index) fn invalid_origin_export_key_part(
    err: OriginExportKeyError,
) -> &'static str {
    match err {
        OriginExportKeyError::EmptyOwnerKey => "owner_key",
        OriginExportKeyError::EmptyLocalKey => "local_key",
        OriginExportKeyError::ReservedStorageSeparator { field } => match field {
            "owner_key" => "owner_key",
            "local_key" => "local_key",
            _ => "owner_key",
        },
    }
}
