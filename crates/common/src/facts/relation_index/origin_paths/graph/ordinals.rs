use crate::facts::{TypedFactRelationColumnName, TypedFactRelationError, TypedFactRelationName};

pub(in crate::facts::relation_index::origin_paths) fn origin_node_id_ordinal(
    relation: TypedFactRelationName,
    column: TypedFactRelationColumnName,
    value: &str,
) -> Result<u64, TypedFactRelationError> {
    value
        .strip_prefix("origin_node:")
        .and_then(|ordinal| ordinal.parse::<u64>().ok())
        .ok_or_else(|| TypedFactRelationError::InvalidRelationValue {
            relation: relation.as_str().to_string(),
            column: column.as_str().to_string(),
            value: value.to_string(),
        })
}
