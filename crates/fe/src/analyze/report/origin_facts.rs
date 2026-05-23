use std::collections::HashSet;

use common::facts::{
    OriginPathWitnessExport, OriginReachabilitySummary, OriginSourcePathWitnessExport,
    OwnedTypedFactSetExport, SourceSpanFileCount, TypedFactRelationCount, TypedFactRelationName,
    TypedFactRelationSet, TypedFactSet,
};
use serde::{Deserialize, Deserializer, Serialize, de};

use super::validation::{
    EmptyAnalyzeReportField, validate_non_empty_report_field,
    validate_relation_count_completeness_for_facts, validate_relation_counts,
    validate_relation_table_rows_match_facts, validate_relation_tables,
};

#[derive(Debug, Serialize)]
pub(in crate::analyze) struct AnalyzeOriginFactReport {
    pub(in crate::analyze) scope: String,
    pub(in crate::analyze) label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(in crate::analyze) object: Option<String>,
    pub(in crate::analyze) total: usize,
    pub(in crate::analyze) origin_nodes: usize,
    pub(in crate::analyze) origin_links: usize,
    pub(in crate::analyze) source_spans: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(in crate::analyze) source_span_files: Vec<SourceSpanFileCount>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(in crate::analyze) relation_counts: Vec<TypedFactRelationCount>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(in crate::analyze) relation_tables: Option<TypedFactRelationSet>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(in crate::analyze) reachability: Option<OriginReachabilitySummary>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(in crate::analyze) path_witnesses: Vec<OriginPathWitnessExport>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(in crate::analyze) source_path_witnesses: Vec<OriginSourcePathWitnessExport>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(in crate::analyze) query_error: Option<String>,
    pub(in crate::analyze) facts: OwnedTypedFactSetExport,
}

impl AnalyzeOriginFactReport {
    pub(in crate::analyze) fn validate(&self) -> Result<(), AnalyzeOriginFactReportError> {
        validate_non_empty_report_field("analyze origin-fact", "scope", &self.scope)
            .map_err(AnalyzeOriginFactReportError::EmptyField)?;
        validate_non_empty_report_field("analyze origin-fact", "label", &self.label)
            .map_err(AnalyzeOriginFactReportError::EmptyField)?;
        if let Some(object) = &self.object {
            validate_non_empty_report_field("analyze origin-fact", "object", object)
                .map_err(AnalyzeOriginFactReportError::EmptyField)?;
        }
        if let Some(query_error) = &self.query_error {
            validate_non_empty_report_field("analyze origin-fact", "query_error", query_error)
                .map_err(AnalyzeOriginFactReportError::EmptyField)?;
        }

        let facts = TypedFactSet::new(self.facts.facts().to_vec());
        let origin_nodes = facts.origin_nodes().count();
        let origin_links = facts.origin_links().count();
        let source_spans = facts.source_spans().count();
        let origin_total = checked_origin_fact_total(origin_nodes, origin_links, source_spans)?;

        if self.origin_nodes != origin_nodes {
            return Err(AnalyzeOriginFactReportError::OriginNodeCountMismatch {
                declared: self.origin_nodes,
                actual: origin_nodes,
            });
        }
        if self.origin_links != origin_links {
            return Err(AnalyzeOriginFactReportError::OriginLinkCountMismatch {
                declared: self.origin_links,
                actual: origin_links,
            });
        }
        if self.source_spans != source_spans {
            return Err(AnalyzeOriginFactReportError::SourceSpanCountMismatch {
                declared: self.source_spans,
                actual: source_spans,
            });
        }
        if self.total != origin_total {
            return Err(AnalyzeOriginFactReportError::TotalMismatch {
                declared: self.total,
                actual: origin_total,
            });
        }
        if self.total != facts.facts().len() {
            return Err(AnalyzeOriginFactReportError::NonOriginFactRows {
                total: self.total,
                fact_rows: facts.facts().len(),
            });
        }

        if let Some(file) = duplicate_source_span_file_count(&self.source_span_files) {
            return Err(AnalyzeOriginFactReportError::DuplicateSourceSpanFileCount { file });
        }

        if !self.source_span_files.is_empty() {
            let source_span_file_total =
                self.source_span_files
                    .iter()
                    .try_fold(0usize, |sum, file| {
                        sum.checked_add(file.spans())
                            .ok_or(AnalyzeOriginFactReportError::SourceSpanFileCountOverflow)
                    })?;
            if source_span_file_total != self.source_spans {
                return Err(AnalyzeOriginFactReportError::SourceSpanFileCountMismatch {
                    declared: self.source_spans,
                    actual: source_span_file_total,
                });
            }
        }

        validate_relation_counts(
            &self.relation_counts,
            |relation| self.expected_origin_relation_count(relation),
            |relation| AnalyzeOriginFactReportError::DuplicateRelationCount { relation },
            |relation| AnalyzeOriginFactReportError::UnexpectedRelationCount { relation },
            |relation, declared, actual| AnalyzeOriginFactReportError::RelationCountMismatch {
                relation,
                declared,
                actual,
            },
        )?;
        validate_relation_count_completeness_for_facts(
            &self.relation_counts,
            &facts,
            |relation, rows| AnalyzeOriginFactReportError::MissingRelationCount { relation, rows },
        )?;

        if let Some(relations) = &self.relation_tables {
            validate_relation_tables(
                relations,
                |relation| self.expected_origin_relation_count(relation),
                |relation, rows| AnalyzeOriginFactReportError::UnexpectedRelationTable {
                    relation,
                    rows,
                },
                |relation, declared, actual| {
                    AnalyzeOriginFactReportError::RelationTableCountMismatch {
                        relation,
                        declared,
                        actual,
                    }
                },
            )?;
            validate_relation_table_rows_match_facts(relations, &facts, |relation| {
                AnalyzeOriginFactReportError::RelationTableRowsMismatch { relation }
            })?;
        }

        Ok(())
    }

    fn expected_origin_relation_count(&self, relation: TypedFactRelationName) -> Option<usize> {
        let expected = match relation {
            TypedFactRelationName::OriginNode => self.origin_nodes,
            TypedFactRelationName::OriginLink => self.origin_links,
            TypedFactRelationName::SourceSpan => self.source_spans,
            _ => return None,
        };
        Some(expected)
    }
}

impl<'de> Deserialize<'de> for AnalyzeOriginFactReport {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct RawReport {
            scope: String,
            label: String,
            object: Option<String>,
            total: usize,
            origin_nodes: usize,
            origin_links: usize,
            source_spans: usize,
            #[serde(default)]
            source_span_files: Vec<SourceSpanFileCount>,
            #[serde(default)]
            relation_counts: Vec<TypedFactRelationCount>,
            relation_tables: Option<TypedFactRelationSet>,
            reachability: Option<OriginReachabilitySummary>,
            #[serde(default)]
            path_witnesses: Vec<OriginPathWitnessExport>,
            #[serde(default)]
            source_path_witnesses: Vec<OriginSourcePathWitnessExport>,
            query_error: Option<String>,
            facts: OwnedTypedFactSetExport,
        }

        let raw = RawReport::deserialize(deserializer)?;
        let report = Self {
            scope: raw.scope,
            label: raw.label,
            object: raw.object,
            total: raw.total,
            origin_nodes: raw.origin_nodes,
            origin_links: raw.origin_links,
            source_spans: raw.source_spans,
            source_span_files: raw.source_span_files,
            relation_counts: raw.relation_counts,
            relation_tables: raw.relation_tables,
            reachability: raw.reachability,
            path_witnesses: raw.path_witnesses,
            source_path_witnesses: raw.source_path_witnesses,
            query_error: raw.query_error,
            facts: raw.facts,
        };
        report.validate().map_err(de::Error::custom)?;
        Ok(report)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::analyze) enum AnalyzeOriginFactReportError {
    EmptyField(EmptyAnalyzeReportField),
    CountOverflow,
    TotalMismatch {
        declared: usize,
        actual: usize,
    },
    NonOriginFactRows {
        total: usize,
        fact_rows: usize,
    },
    OriginNodeCountMismatch {
        declared: usize,
        actual: usize,
    },
    OriginLinkCountMismatch {
        declared: usize,
        actual: usize,
    },
    SourceSpanCountMismatch {
        declared: usize,
        actual: usize,
    },
    SourceSpanFileCountOverflow,
    SourceSpanFileCountMismatch {
        declared: usize,
        actual: usize,
    },
    DuplicateSourceSpanFileCount {
        file: String,
    },
    DuplicateRelationCount {
        relation: TypedFactRelationName,
    },
    UnexpectedRelationCount {
        relation: TypedFactRelationName,
    },
    RelationCountMismatch {
        relation: TypedFactRelationName,
        declared: usize,
        actual: usize,
    },
    MissingRelationCount {
        relation: TypedFactRelationName,
        rows: usize,
    },
    RelationTableCountMismatch {
        relation: TypedFactRelationName,
        declared: usize,
        actual: usize,
    },
    RelationTableRowsMismatch {
        relation: TypedFactRelationName,
    },
    UnexpectedRelationTable {
        relation: TypedFactRelationName,
        rows: usize,
    },
}

impl std::fmt::Display for AnalyzeOriginFactReportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyField(err) => err.fmt(f),
            Self::CountOverflow => write!(f, "analyze origin-fact total overflowed"),
            Self::TotalMismatch { declared, actual } => write!(
                f,
                "analyze origin-fact total {declared} does not match origin fact count {actual}"
            ),
            Self::NonOriginFactRows { total, fact_rows } => write!(
                f,
                "analyze origin-fact total {total} does not cover all typed fact rows {fact_rows}"
            ),
            Self::OriginNodeCountMismatch { declared, actual } => write!(
                f,
                "analyze origin-fact origin_nodes {declared} does not match typed fact count {actual}"
            ),
            Self::OriginLinkCountMismatch { declared, actual } => write!(
                f,
                "analyze origin-fact origin_links {declared} does not match typed fact count {actual}"
            ),
            Self::SourceSpanCountMismatch { declared, actual } => write!(
                f,
                "analyze origin-fact source_spans {declared} does not match typed fact count {actual}"
            ),
            Self::SourceSpanFileCountOverflow => {
                write!(f, "analyze origin-fact source-span file count overflowed")
            }
            Self::SourceSpanFileCountMismatch { declared, actual } => write!(
                f,
                "analyze origin-fact source_span_files total {actual} does not match source_spans {declared}"
            ),
            Self::DuplicateSourceSpanFileCount { file } => write!(
                f,
                "analyze origin-fact source_span_files contains duplicate file `{file}`"
            ),
            Self::DuplicateRelationCount { relation } => write!(
                f,
                "analyze origin-fact relation_counts contains duplicate relation {}",
                relation.as_str()
            ),
            Self::UnexpectedRelationCount { relation } => write!(
                f,
                "analyze origin-fact relation_counts contains non-origin relation {}",
                relation.as_str()
            ),
            Self::RelationCountMismatch {
                relation,
                declared,
                actual,
            } => write!(
                f,
                "analyze origin-fact relation count {}={declared} does not match report count {actual}",
                relation.as_str()
            ),
            Self::MissingRelationCount { relation, rows } => write!(
                f,
                "analyze origin-fact relation_counts is missing {}={rows}",
                relation.as_str()
            ),
            Self::RelationTableCountMismatch {
                relation,
                declared,
                actual,
            } => write!(
                f,
                "analyze origin-fact relation table {} has {actual} rows, expected {declared}",
                relation.as_str()
            ),
            Self::RelationTableRowsMismatch { relation } => write!(
                f,
                "analyze origin-fact relation table {} rows do not match typed facts",
                relation.as_str()
            ),
            Self::UnexpectedRelationTable { relation, rows } => write!(
                f,
                "analyze origin-fact relation table {} has non-origin rows {rows}",
                relation.as_str()
            ),
        }
    }
}

impl std::error::Error for AnalyzeOriginFactReportError {}

fn checked_origin_fact_total(
    origin_nodes: usize,
    origin_links: usize,
    source_spans: usize,
) -> Result<usize, AnalyzeOriginFactReportError> {
    origin_nodes
        .checked_add(origin_links)
        .and_then(|sum| sum.checked_add(source_spans))
        .ok_or(AnalyzeOriginFactReportError::CountOverflow)
}

fn duplicate_source_span_file_count(counts: &[SourceSpanFileCount]) -> Option<String> {
    let mut seen = HashSet::new();
    for count in counts {
        if !seen.insert(count.file()) {
            return Some(count.file().to_string());
        }
    }
    None
}
