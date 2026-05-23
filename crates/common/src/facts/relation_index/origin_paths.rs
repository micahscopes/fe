use std::collections::{BTreeMap, BTreeSet, VecDeque};

use crate::origin::{OriginExportKey, OriginExportKind, OriginLinkKind};

use super::super::{
    OriginPathWitnessExport, OriginReachabilitySummary, OriginSourcePathWitnessExport,
    SourceSpanExport, SourceSpanFileCount, SourceSpanKind, TypedFactRelationColumnName,
    TypedFactRelationError, TypedFactRelationName,
};
use super::{TypedFactRelationIndex, validation::invalid_origin_export_key_part};

impl<'a> TypedFactRelationIndex<'a> {
    pub fn origin_reachability_summary(
        &self,
    ) -> Result<OriginReachabilitySummary, TypedFactRelationError> {
        let node_ids = self.origin_node_ids_in_fact_order()?;
        let keys_by_id = self.origin_node_keys_by_id()?;
        let outgoing = self.origin_outgoing_by_id()?;

        let mut pair_counts = BTreeMap::new();
        for start_id in node_ids {
            let Some(start_key) = keys_by_id.get(start_id) else {
                continue;
            };
            for end_id in self.reachable_origin_ids_from(start_id, &outgoing)? {
                if let Some(end_key) = keys_by_id.get(end_id) {
                    *pair_counts
                        .entry((start_key.kind(), end_key.kind()))
                        .or_insert(0) += 1;
                }
            }
        }

        Ok(OriginReachabilitySummary::from_pair_counts(pair_counts))
    }

    pub fn representative_path_export_for_kind_pair(
        &self,
        from_kind: OriginExportKind,
        to_kind: OriginExportKind,
    ) -> Result<Option<OriginPathWitnessExport>, TypedFactRelationError> {
        let node_ids = self.origin_node_ids_in_fact_order()?;
        let keys_by_id = self.origin_node_keys_by_id()?;
        let outgoing = self.origin_outgoing_by_id()?;
        self.representative_path_export_for_kind_pair_from_graph(
            from_kind,
            to_kind,
            &node_ids,
            &keys_by_id,
            &outgoing,
        )
    }

    pub fn path_export_between_keys(
        &self,
        from_key: &OriginExportKey,
        to_key: &OriginExportKey,
    ) -> Result<Option<OriginPathWitnessExport>, TypedFactRelationError> {
        let keys_by_id = self.origin_node_keys_by_id()?;
        let outgoing = self.origin_outgoing_by_id()?;
        let ids_by_key = keys_by_id
            .iter()
            .map(|(id, key)| (key.clone(), *id))
            .collect::<BTreeMap<_, _>>();
        let Some(from_id) = ids_by_key.get(from_key).copied() else {
            return Ok(None);
        };
        let Some(to_id) = ids_by_key.get(to_key).copied() else {
            return Ok(None);
        };

        Ok(self.origin_path_export(
            from_id,
            to_id,
            from_key.kind(),
            to_key.kind(),
            &keys_by_id,
            &outgoing,
        ))
    }

    pub fn representative_path_exports_with_priority(
        &self,
        priority_kind_pairs: impl IntoIterator<Item = (OriginExportKind, OriginExportKind)>,
        limit: usize,
    ) -> Result<Vec<OriginPathWitnessExport>, TypedFactRelationError> {
        let node_ids = self.origin_node_ids_in_fact_order()?;
        let keys_by_id = self.origin_node_keys_by_id()?;
        let outgoing = self.origin_outgoing_by_id()?;
        let mut seen_pairs = BTreeSet::new();
        let mut exports = Vec::new();
        if limit == 0 {
            return Ok(exports);
        }

        for (from_kind, to_kind) in priority_kind_pairs {
            if !seen_pairs.insert((from_kind, to_kind)) {
                continue;
            }
            let Some(export) = self.representative_path_export_for_kind_pair_from_graph(
                from_kind,
                to_kind,
                &node_ids,
                &keys_by_id,
                &outgoing,
            )?
            else {
                continue;
            };
            exports.push(export);
            if exports.len() >= limit {
                return Ok(exports);
            }
        }

        for start_id in &node_ids {
            let Some(start_key) = keys_by_id.get(start_id) else {
                continue;
            };
            for end_id in self.reachable_origin_ids_from(start_id, &outgoing)? {
                let Some(end_key) = keys_by_id.get(end_id) else {
                    continue;
                };
                let pair = (start_key.kind(), end_key.kind());
                if !seen_pairs.insert(pair) {
                    continue;
                }
                let Some(export) = self.origin_path_export(
                    start_id,
                    end_id,
                    pair.0,
                    pair.1,
                    &keys_by_id,
                    &outgoing,
                ) else {
                    continue;
                };
                exports.push(export);
                if exports.len() >= limit {
                    return Ok(exports);
                }
            }
        }

        Ok(exports)
    }

    pub fn representative_source_path_exports_with_priority(
        &self,
        priority_kind_pairs: impl IntoIterator<Item = (OriginExportKind, OriginExportKind)>,
        limit: usize,
    ) -> Result<Vec<OriginSourcePathWitnessExport>, TypedFactRelationError> {
        let node_ids = self.origin_node_ids_in_fact_order()?;
        let keys_by_id = self.origin_node_keys_by_id()?;
        let outgoing = self.origin_outgoing_by_id()?;
        let source_spans_by_id = self.source_spans_by_origin_id(&keys_by_id)?;
        let mut seen_pairs = BTreeSet::new();
        let mut exports = Vec::new();
        if limit == 0 || source_spans_by_id.is_empty() {
            return Ok(exports);
        }

        for (from_kind, to_kind) in priority_kind_pairs {
            if !seen_pairs.insert((from_kind, to_kind)) {
                continue;
            }
            let Some(export) = self.representative_source_path_export_for_kind_pair_from_graph(
                from_kind,
                to_kind,
                &node_ids,
                &keys_by_id,
                &outgoing,
                &source_spans_by_id,
            )?
            else {
                continue;
            };
            exports.push(export);
            if exports.len() >= limit {
                return Ok(exports);
            }
        }

        for start_id in &node_ids {
            let Some(start_key) = keys_by_id.get(start_id) else {
                continue;
            };
            for end_id in self.reachable_origin_ids_from(start_id, &outgoing)? {
                let Some(end_key) = keys_by_id.get(end_id) else {
                    continue;
                };
                let Some(source_span) = source_spans_by_id
                    .get(end_id)
                    .and_then(|spans| spans.first())
                else {
                    continue;
                };
                let pair = (start_key.kind(), end_key.kind());
                if !seen_pairs.insert(pair) {
                    continue;
                }
                let Some(path) = self.origin_path_export(
                    start_id,
                    end_id,
                    pair.0,
                    pair.1,
                    &keys_by_id,
                    &outgoing,
                ) else {
                    continue;
                };
                exports.push(OriginSourcePathWitnessExport::new(
                    path,
                    source_span.clone(),
                ));
                if exports.len() >= limit {
                    return Ok(exports);
                }
            }
        }

        Ok(exports)
    }

    pub fn source_span_file_counts(
        &self,
    ) -> Result<Vec<SourceSpanFileCount>, TypedFactRelationError> {
        let relation_table = self.relation(TypedFactRelationName::SourceSpan)?;
        let file_column = self.column_index(
            TypedFactRelationName::SourceSpan,
            TypedFactRelationColumnName::File,
        )?;
        let mut counts = BTreeMap::<String, usize>::new();

        for row in relation_table.rows() {
            *counts.entry(row[file_column].clone()).or_default() += 1;
        }

        Ok(counts
            .into_iter()
            .map(|(file, spans)| SourceSpanFileCount::new(file, spans))
            .collect())
    }

    fn representative_path_export_for_kind_pair_from_graph(
        &self,
        from_kind: OriginExportKind,
        to_kind: OriginExportKind,
        node_ids: &[&'a str],
        keys_by_id: &BTreeMap<&'a str, OriginExportKey>,
        outgoing: &BTreeMap<&'a str, Vec<(&'a str, OriginLinkKind)>>,
    ) -> Result<Option<OriginPathWitnessExport>, TypedFactRelationError> {
        for start_id in node_ids {
            let Some(start_key) = keys_by_id.get(start_id) else {
                continue;
            };
            if start_key.kind() != from_kind {
                continue;
            }

            for end_id in self.reachable_origin_ids_from(start_id, outgoing)? {
                let Some(end_key) = keys_by_id.get(end_id) else {
                    continue;
                };
                if end_key.kind() != to_kind {
                    continue;
                }
                return Ok(self.origin_path_export(
                    start_id, end_id, from_kind, to_kind, keys_by_id, outgoing,
                ));
            }
        }

        Ok(None)
    }

    fn representative_source_path_export_for_kind_pair_from_graph(
        &self,
        from_kind: OriginExportKind,
        to_kind: OriginExportKind,
        node_ids: &[&'a str],
        keys_by_id: &BTreeMap<&'a str, OriginExportKey>,
        outgoing: &BTreeMap<&'a str, Vec<(&'a str, OriginLinkKind)>>,
        source_spans_by_id: &BTreeMap<&'a str, Vec<SourceSpanExport>>,
    ) -> Result<Option<OriginSourcePathWitnessExport>, TypedFactRelationError> {
        for start_id in node_ids {
            let Some(start_key) = keys_by_id.get(start_id) else {
                continue;
            };
            if start_key.kind() != from_kind {
                continue;
            }

            for end_id in self.reachable_origin_ids_from(start_id, outgoing)? {
                let Some(end_key) = keys_by_id.get(end_id) else {
                    continue;
                };
                if end_key.kind() != to_kind {
                    continue;
                }
                let Some(source_span) = source_spans_by_id
                    .get(end_id)
                    .and_then(|spans| spans.first())
                else {
                    continue;
                };
                let Some(path) = self
                    .origin_path_export(start_id, end_id, from_kind, to_kind, keys_by_id, outgoing)
                else {
                    continue;
                };
                return Ok(Some(OriginSourcePathWitnessExport::new(
                    path,
                    source_span.clone(),
                )));
            }
        }

        Ok(None)
    }

    fn origin_path_export(
        &self,
        from_id: &'a str,
        to_id: &'a str,
        from_kind: OriginExportKind,
        to_kind: OriginExportKind,
        keys_by_id: &BTreeMap<&'a str, OriginExportKey>,
        outgoing: &BTreeMap<&'a str, Vec<(&'a str, OriginLinkKind)>>,
    ) -> Option<OriginPathWitnessExport> {
        let (node_ids, links) =
            shortest_origin_relation_path(from_id, to_id, keys_by_id, outgoing)?;
        let nodes = node_ids
            .into_iter()
            .map(|id| keys_by_id.get(id).cloned())
            .collect::<Option<Vec<_>>>()?;
        Some(OriginPathWitnessExport::new(
            from_kind, to_kind, nodes, links,
        ))
    }

    fn origin_node_ids_in_fact_order(&self) -> Result<Vec<&'a str>, TypedFactRelationError> {
        let relation_table = self.relation(TypedFactRelationName::OriginNode)?;
        let id_column = self.column_index(
            TypedFactRelationName::OriginNode,
            TypedFactRelationColumnName::Id,
        )?;
        let mut ids = Vec::new();
        for row in relation_table.rows() {
            let id = row[id_column].as_str();
            ids.push((
                self.origin_node_id_ordinal(
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

    fn origin_node_keys_by_id(
        &self,
    ) -> Result<BTreeMap<&'a str, OriginExportKey>, TypedFactRelationError> {
        let relation_table = self.relation(TypedFactRelationName::OriginNode)?;
        let id_column = self.column_index(
            TypedFactRelationName::OriginNode,
            TypedFactRelationColumnName::Id,
        )?;
        let kind_column = self.column_index(
            TypedFactRelationName::OriginNode,
            TypedFactRelationColumnName::Kind,
        )?;
        let owner_column = self.column_index(
            TypedFactRelationName::OriginNode,
            TypedFactRelationColumnName::OwnerKey,
        )?;
        let local_column = self.column_index(
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

    fn source_spans_by_origin_id(
        &self,
        keys_by_id: &BTreeMap<&'a str, OriginExportKey>,
    ) -> Result<BTreeMap<&'a str, Vec<SourceSpanExport>>, TypedFactRelationError> {
        let relation_table = self.relation(TypedFactRelationName::SourceSpan)?;
        let origin_column = self.column_index(
            TypedFactRelationName::SourceSpan,
            TypedFactRelationColumnName::Origin,
        )?;
        let span_kind_column = self.column_index(
            TypedFactRelationName::SourceSpan,
            TypedFactRelationColumnName::SpanKind,
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
        let mut source_spans_by_id = BTreeMap::<&'a str, Vec<SourceSpanExport>>::new();

        for row in relation_table.rows() {
            let origin_id = row[origin_column].as_str();
            self.origin_node_id_ordinal(
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
            let Some(span_kind) = SourceSpanKind::from_str(&row[span_kind_column]) else {
                return Err(TypedFactRelationError::InvalidRelationValue {
                    relation: TypedFactRelationName::SourceSpan.as_str().to_string(),
                    column: TypedFactRelationColumnName::SpanKind.as_str().to_string(),
                    value: row[span_kind_column].clone(),
                });
            };

            source_spans_by_id
                .entry(origin_id)
                .or_default()
                .push(SourceSpanExport::new(
                    origin_key.clone(),
                    span_kind,
                    row[file_column].clone(),
                    self.parse_relation_number(
                        TypedFactRelationName::SourceSpan,
                        TypedFactRelationColumnName::StartByte,
                        &row[start_byte_column],
                    )?,
                    self.parse_relation_number(
                        TypedFactRelationName::SourceSpan,
                        TypedFactRelationColumnName::EndByte,
                        &row[end_byte_column],
                    )?,
                    self.parse_relation_number(
                        TypedFactRelationName::SourceSpan,
                        TypedFactRelationColumnName::StartLine,
                        &row[start_line_column],
                    )?,
                    self.parse_relation_number(
                        TypedFactRelationName::SourceSpan,
                        TypedFactRelationColumnName::StartCol,
                        &row[start_col_column],
                    )?,
                    self.parse_relation_number(
                        TypedFactRelationName::SourceSpan,
                        TypedFactRelationColumnName::EndLine,
                        &row[end_line_column],
                    )?,
                    self.parse_relation_number(
                        TypedFactRelationName::SourceSpan,
                        TypedFactRelationColumnName::EndCol,
                        &row[end_col_column],
                    )?,
                ));
        }
        for spans in source_spans_by_id.values_mut() {
            spans.sort();
        }

        Ok(source_spans_by_id)
    }

    fn origin_outgoing_by_id(
        &self,
    ) -> Result<BTreeMap<&'a str, Vec<(&'a str, OriginLinkKind)>>, TypedFactRelationError> {
        let relation_table = self.relation(TypedFactRelationName::OriginLink)?;
        let from_column = self.column_index(
            TypedFactRelationName::OriginLink,
            TypedFactRelationColumnName::From,
        )?;
        let to_column = self.column_index(
            TypedFactRelationName::OriginLink,
            TypedFactRelationColumnName::To,
        )?;
        let kind_column = self.column_index(
            TypedFactRelationName::OriginLink,
            TypedFactRelationColumnName::Kind,
        )?;
        let mut outgoing = BTreeMap::<&str, Vec<(u64, &str, OriginLinkKind)>>::new();

        for row in relation_table.rows() {
            let from = row[from_column].as_str();
            let to = row[to_column].as_str();
            self.origin_node_id_ordinal(
                TypedFactRelationName::OriginLink,
                TypedFactRelationColumnName::From,
                from,
            )?;
            let to_ordinal = self.origin_node_id_ordinal(
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

    fn reachable_origin_ids_from(
        &self,
        start: &'a str,
        outgoing: &BTreeMap<&'a str, Vec<(&'a str, OriginLinkKind)>>,
    ) -> Result<Vec<&'a str>, TypedFactRelationError> {
        let mut seen = BTreeSet::new();
        let mut queue = VecDeque::new();
        queue.push_back(start);

        while let Some(current) = queue.pop_front() {
            if let Some(targets) = outgoing.get(current) {
                for (target, _) in targets {
                    if seen.insert(*target) {
                        queue.push_back(*target);
                    }
                }
            }
        }

        let mut ids = seen
            .into_iter()
            .map(|id| {
                self.origin_node_id_ordinal(
                    TypedFactRelationName::OriginLink,
                    TypedFactRelationColumnName::To,
                    id,
                )
                .map(|ordinal| (ordinal, id))
            })
            .collect::<Result<Vec<_>, _>>()?;
        ids.sort_by_key(|(ordinal, _)| *ordinal);
        Ok(ids.into_iter().map(|(_, id)| id).collect())
    }

    fn origin_node_id_ordinal(
        &self,
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
}

fn shortest_origin_relation_path<'a>(
    from: &'a str,
    to: &'a str,
    keys_by_id: &BTreeMap<&'a str, OriginExportKey>,
    outgoing: &BTreeMap<&'a str, Vec<(&'a str, OriginLinkKind)>>,
) -> Option<(Vec<&'a str>, Vec<OriginLinkKind>)> {
    if !keys_by_id.contains_key(from) || !keys_by_id.contains_key(to) {
        return None;
    }
    if from == to {
        return Some((vec![from], Vec::new()));
    }

    let mut seen = BTreeSet::new();
    let mut predecessor = BTreeMap::new();
    let mut queue = VecDeque::new();
    seen.insert(from);
    queue.push_back(from);

    while let Some(current) = queue.pop_front() {
        for (target, kind) in outgoing.get(current).into_iter().flatten() {
            if !seen.insert(*target) {
                continue;
            }
            predecessor.insert(*target, (current, *kind));
            if *target == to {
                return reconstruct_origin_relation_path(from, to, predecessor);
            }
            queue.push_back(*target);
        }
    }

    None
}

fn reconstruct_origin_relation_path<'a>(
    from: &'a str,
    to: &'a str,
    predecessor: BTreeMap<&'a str, (&'a str, OriginLinkKind)>,
) -> Option<(Vec<&'a str>, Vec<OriginLinkKind>)> {
    let mut nodes = vec![to];
    let mut links = Vec::new();
    let mut current = to;

    while current != from {
        let (previous, kind) = predecessor.get(current).copied()?;
        links.push(kind);
        nodes.push(previous);
        current = previous;
    }

    nodes.reverse();
    links.reverse();
    Some((nodes, links))
}
