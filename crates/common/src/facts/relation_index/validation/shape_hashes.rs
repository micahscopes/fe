use std::collections::BTreeSet;

use crate::{
    facts::{
        ShapeHashDigest, ShapeHashScope, TypedFactRelationColumnName, TypedFactRelationError,
        TypedFactRelationName,
    },
    shape::ShapeDimension,
};

use super::super::TypedFactRelationIndex;

impl<'a> TypedFactRelationIndex<'a> {
    pub(in crate::facts::relation_index) fn validate_shape_hash_rows(
        &self,
        shape_ids: &BTreeSet<String>,
    ) -> Result<(), TypedFactRelationError> {
        let relation_table = self.relation(TypedFactRelationName::ShapeHash)?;
        let node_column = self.column_index(
            TypedFactRelationName::ShapeHash,
            TypedFactRelationColumnName::Node,
        )?;
        let scope_column = self.column_index(
            TypedFactRelationName::ShapeHash,
            TypedFactRelationColumnName::Scope,
        )?;
        let dimension_column = self.column_index(
            TypedFactRelationName::ShapeHash,
            TypedFactRelationColumnName::Dimension,
        )?;
        let digest_column = self.column_index(
            TypedFactRelationName::ShapeHash,
            TypedFactRelationColumnName::DigestHex,
        )?;
        let mut shape_hash_keys = BTreeSet::new();

        for row in relation_table.rows() {
            let node = &row[node_column];
            let scope_raw = &row[scope_column];
            let Some(scope) = ShapeHashScope::from_str(scope_raw) else {
                return Err(TypedFactRelationError::InvalidRelationValue {
                    relation: TypedFactRelationName::ShapeHash.as_str().to_string(),
                    column: TypedFactRelationColumnName::Scope.as_str().to_string(),
                    value: scope_raw.clone(),
                });
            };
            let Some(dimension) = ShapeDimension::from_str(&row[dimension_column]) else {
                return Err(TypedFactRelationError::InvalidRelationValue {
                    relation: TypedFactRelationName::ShapeHash.as_str().to_string(),
                    column: TypedFactRelationColumnName::Dimension.as_str().to_string(),
                    value: row[dimension_column].clone(),
                });
            };
            match (scope, node.as_str()) {
                (ShapeHashScope::Graph, "graph") => {}
                (ShapeHashScope::Local | ShapeHashScope::Tree, node)
                    if shape_ids.contains(node) => {}
                _ => {
                    return Err(TypedFactRelationError::InvalidShapeHashNode {
                        node: node.clone(),
                        scope: scope.as_str().to_string(),
                    });
                }
            }
            if ShapeHashDigest::try_new(row[digest_column].as_str()).is_err() {
                return Err(TypedFactRelationError::InvalidShapeHashDigest {
                    node: node.clone(),
                    scope: scope.as_str().to_string(),
                    dimension: dimension.as_str().to_string(),
                });
            }

            let key = (node.clone(), scope, dimension);
            if !shape_hash_keys.insert(key) {
                return Err(TypedFactRelationError::DuplicateShapeHash {
                    node: node.clone(),
                    scope: scope.as_str().to_string(),
                    dimension: dimension.as_str().to_string(),
                });
            }
        }

        if !shape_ids.is_empty() || !shape_hash_keys.is_empty() {
            for dimension in ShapeDimension::ALL {
                require_shape_hash_relation_row(
                    &shape_hash_keys,
                    "graph",
                    ShapeHashScope::Graph,
                    dimension,
                )?;

                for node in shape_ids {
                    require_shape_hash_relation_row(
                        &shape_hash_keys,
                        node,
                        ShapeHashScope::Local,
                        dimension,
                    )?;
                    require_shape_hash_relation_row(
                        &shape_hash_keys,
                        node,
                        ShapeHashScope::Tree,
                        dimension,
                    )?;
                }
            }
        }

        Ok(())
    }
}

fn require_shape_hash_relation_row(
    shape_hash_keys: &BTreeSet<(String, ShapeHashScope, ShapeDimension)>,
    node: &str,
    scope: ShapeHashScope,
    dimension: ShapeDimension,
) -> Result<(), TypedFactRelationError> {
    let key = (node.to_string(), scope, dimension);
    if shape_hash_keys.contains(&key) {
        Ok(())
    } else {
        Err(TypedFactRelationError::MissingShapeHash {
            node: key.0,
            scope: key.1.as_str().to_string(),
            dimension: key.2.as_str().to_string(),
        })
    }
}
