use std::collections::BTreeSet;

use crate::{
    origin::{OriginExportKey, OriginExportKeyError, OriginExportKind, OriginLinkKind},
    shape::ShapeDimension,
};

use super::super::{
    ShapeHashDigest, ShapeHashScope, SourceSpanKind, TypedFactRelationColumnName,
    TypedFactRelationError, TypedFactRelationName,
};
use super::TypedFactRelationIndex;

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

    fn relation_id_set(
        &self,
        relation: TypedFactRelationName,
        column: TypedFactRelationColumnName,
    ) -> Result<BTreeSet<String>, TypedFactRelationError> {
        let relation_table = self.relation(relation)?;
        let column_idx = self.column_index(relation, column)?;
        let mut ids = BTreeSet::new();
        for row in relation_table.rows() {
            self.validate_fact_id_cell(relation, column, &row[column_idx])?;
            if !ids.insert(row[column_idx].clone()) {
                return Err(TypedFactRelationError::DuplicateRelationId {
                    relation: relation.as_str().to_string(),
                    value: row[column_idx].clone(),
                });
            }
        }
        Ok(ids)
    }

    fn validate_fact_id_cell(
        &self,
        relation: TypedFactRelationName,
        column: TypedFactRelationColumnName,
        value: &str,
    ) -> Result<(), TypedFactRelationError> {
        let Some(ordinal) = value
            .strip_prefix(relation.as_str())
            .and_then(|rest| rest.strip_prefix(':'))
        else {
            return Err(TypedFactRelationError::InvalidRelationValue {
                relation: relation.as_str().to_string(),
                column: column.as_str().to_string(),
                value: value.to_string(),
            });
        };
        if ordinal.parse::<u64>().is_err() {
            return Err(TypedFactRelationError::InvalidRelationValue {
                relation: relation.as_str().to_string(),
                column: column.as_str().to_string(),
                value: value.to_string(),
            });
        }
        Ok(())
    }

    fn validate_unique_columns(
        &self,
        relation: TypedFactRelationName,
        columns: &[TypedFactRelationColumnName],
    ) -> Result<(), TypedFactRelationError> {
        let relation_table = self.relation(relation)?;
        let column_indexes = columns
            .iter()
            .map(|column| self.column_index(relation, *column))
            .collect::<Result<Vec<_>, _>>()?;
        let mut seen = BTreeSet::new();
        for row in relation_table.rows() {
            let values = column_indexes
                .iter()
                .map(|idx| row[*idx].clone())
                .collect::<Vec<_>>();
            if !seen.insert(values.clone()) {
                return Err(TypedFactRelationError::DuplicateRelationKey {
                    relation: relation.as_str().to_string(),
                    columns: columns
                        .iter()
                        .map(|column| column.as_str().to_string())
                        .collect(),
                    values,
                });
            }
        }
        Ok(())
    }

    fn validate_origin_export_key_rows(&self) -> Result<(), TypedFactRelationError> {
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

    fn validate_relation_references<'b>(
        &self,
        relation: TypedFactRelationName,
        references: impl IntoIterator<
            Item = (
                TypedFactRelationColumnName,
                &'b BTreeSet<String>,
                TypedFactRelationName,
            ),
        >,
    ) -> Result<(), TypedFactRelationError> {
        let relation_table = self.relation(relation)?;
        let references = references
            .into_iter()
            .map(|(column, target_ids, target_relation)| {
                self.column_index(relation, column)
                    .map(|idx| (column, idx, target_ids, target_relation))
            })
            .collect::<Result<Vec<_>, _>>()?;

        for row in relation_table.rows() {
            for (column, idx, target_ids, target_relation) in &references {
                if !target_ids.contains(&row[*idx]) {
                    return Err(TypedFactRelationError::MissingRelationReference {
                        relation: relation.as_str().to_string(),
                        column: column.as_str().to_string(),
                        value: row[*idx].clone(),
                        target_relation: target_relation.as_str().to_string(),
                    });
                }
            }
        }

        Ok(())
    }

    fn validate_numeric_column<T>(
        &self,
        relation: TypedFactRelationName,
        column: TypedFactRelationColumnName,
    ) -> Result<(), TypedFactRelationError>
    where
        T: std::str::FromStr,
    {
        let relation_table = self.relation(relation)?;
        let column_idx = self.column_index(relation, column)?;
        for row in relation_table.rows() {
            self.parse_relation_number::<T>(relation, column, &row[column_idx])?;
        }
        Ok(())
    }

    fn validate_non_empty_column(
        &self,
        relation: TypedFactRelationName,
        column: TypedFactRelationColumnName,
    ) -> Result<(), TypedFactRelationError> {
        let relation_table = self.relation(relation)?;
        let column_idx = self.column_index(relation, column)?;
        for row in relation_table.rows() {
            if row[column_idx].is_empty() {
                return Err(TypedFactRelationError::InvalidRelationValue {
                    relation: relation.as_str().to_string(),
                    column: column.as_str().to_string(),
                    value: row[column_idx].clone(),
                });
            }
        }
        Ok(())
    }

    pub(super) fn parse_relation_number<T>(
        &self,
        relation: TypedFactRelationName,
        column: TypedFactRelationColumnName,
        value: &str,
    ) -> Result<T, TypedFactRelationError>
    where
        T: std::str::FromStr,
    {
        value
            .parse::<T>()
            .map_err(|_| TypedFactRelationError::InvalidRelationValue {
                relation: relation.as_str().to_string(),
                column: column.as_str().to_string(),
                value: value.to_string(),
            })
    }

    fn validate_column_values<T>(
        &self,
        relation: TypedFactRelationName,
        column: TypedFactRelationColumnName,
        mut parse: impl FnMut(&str) -> Option<T>,
    ) -> Result<(), TypedFactRelationError> {
        let relation_table = self.relation(relation)?;
        let column_idx = self.column_index(relation, column)?;
        for row in relation_table.rows() {
            if parse(&row[column_idx]).is_none() {
                return Err(TypedFactRelationError::InvalidRelationValue {
                    relation: relation.as_str().to_string(),
                    column: column.as_str().to_string(),
                    value: row[column_idx].clone(),
                });
            }
        }
        Ok(())
    }

    fn validate_source_span_rows(&self) -> Result<(), TypedFactRelationError> {
        let relation_table = self.relation(TypedFactRelationName::SourceSpan)?;
        let origin_column = self.column_index(
            TypedFactRelationName::SourceSpan,
            TypedFactRelationColumnName::Origin,
        )?;
        let file_column = self.column_index(
            TypedFactRelationName::SourceSpan,
            TypedFactRelationColumnName::File,
        )?;
        let start_byte_column = self.column_index(
            TypedFactRelationName::SourceSpan,
            TypedFactRelationColumnName::StartByte,
        )?;
        let end_byte_column = self.column_index(
            TypedFactRelationName::SourceSpan,
            TypedFactRelationColumnName::EndByte,
        )?;
        let start_line_column = self.column_index(
            TypedFactRelationName::SourceSpan,
            TypedFactRelationColumnName::StartLine,
        )?;
        let start_col_column = self.column_index(
            TypedFactRelationName::SourceSpan,
            TypedFactRelationColumnName::StartCol,
        )?;
        let end_line_column = self.column_index(
            TypedFactRelationName::SourceSpan,
            TypedFactRelationColumnName::EndLine,
        )?;
        let end_col_column = self.column_index(
            TypedFactRelationName::SourceSpan,
            TypedFactRelationColumnName::EndCol,
        )?;

        for row in relation_table.rows() {
            let origin = &row[origin_column];
            if row[file_column].is_empty() {
                return Err(TypedFactRelationError::InvalidSourceSpanFile {
                    origin: origin.clone(),
                });
            }
            let start_byte = self.parse_relation_number::<usize>(
                TypedFactRelationName::SourceSpan,
                TypedFactRelationColumnName::StartByte,
                &row[start_byte_column],
            )?;
            let end_byte = self.parse_relation_number::<usize>(
                TypedFactRelationName::SourceSpan,
                TypedFactRelationColumnName::EndByte,
                &row[end_byte_column],
            )?;
            let start_line = self.parse_relation_number::<usize>(
                TypedFactRelationName::SourceSpan,
                TypedFactRelationColumnName::StartLine,
                &row[start_line_column],
            )?;
            let start_col = self.parse_relation_number::<usize>(
                TypedFactRelationName::SourceSpan,
                TypedFactRelationColumnName::StartCol,
                &row[start_col_column],
            )?;
            let end_line = self.parse_relation_number::<usize>(
                TypedFactRelationName::SourceSpan,
                TypedFactRelationColumnName::EndLine,
                &row[end_line_column],
            )?;
            let end_col = self.parse_relation_number::<usize>(
                TypedFactRelationName::SourceSpan,
                TypedFactRelationColumnName::EndCol,
                &row[end_col_column],
            )?;

            if start_byte > end_byte {
                return Err(TypedFactRelationError::InvalidSourceSpanRange {
                    origin: origin.clone(),
                    start_byte,
                    end_byte,
                });
            }
            if start_line > end_line || (start_line == end_line && start_col > end_col) {
                return Err(TypedFactRelationError::InvalidSourceSpanPosition {
                    origin: origin.clone(),
                    start_line,
                    start_col,
                    end_line,
                    end_col,
                });
            }
        }

        Ok(())
    }

    fn validate_shape_hash_rows(
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

pub(super) fn invalid_origin_export_key_part(err: OriginExportKeyError) -> &'static str {
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
