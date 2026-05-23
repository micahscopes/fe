use std::{collections::HashSet, fmt::Write};

use camino::Utf8PathBuf;
use codegen::{
    OptLevel, SonatinaContractBytecode, SonatinaTestOptions, TestMetadata,
    debug::{
        BytecodeOriginCoverageExport, BytecodeSourceMapEntry, BytecodeSourceMapEntryKind,
        BytecodeSourceMapSummary, SonatinaPostOptOriginCoverageExport,
        bytecode_source_map_entries_summary,
    },
    emit_runtime_package_sonatina_bytecode_with_source_maps, emit_test_module_sonatina,
    origin::{BytecodeOriginCoverage, SonatinaPostOptOriginCoverage},
};
use common::{
    InputDb,
    config::{Config, WorkspaceConfig},
    facts::{
        OriginPathWitnessExport, OriginReachabilitySummary, OriginSourcePathWitnessExport,
        OwnedTypedFactSetExport, SourceSpanFileCount, TypedFactRelationCount,
        TypedFactRelationIndex, TypedFactRelationName, TypedFactRelationSet, TypedFactSet,
        shape_graph_facts,
    },
    file::IngotFileKind,
    origin::OriginExportKind,
    shape::{ShapeBuilder, ShapeDescribe, ShapeDimension, ShapeGraph, ShapeNodeId},
};
use driver::DriverDataBase;
use driver::cli_target::{CliTarget, resolve_cli_target};
use hir::{
    Ingot,
    hir_def::{HirIngot, ItemKind, TopLevelMod},
};
use mir::{
    RuntimeOriginFactOwnerKeys, RuntimeOriginFactTargetKey, RuntimeOriginSource,
    build_runtime_package, build_test_runtime_package, runtime_package_origin_facts,
    runtime_package_origins,
};
use salsa::Setter;
use serde::{Deserialize, Deserializer, Serialize, de};
use url::Url;

use crate::{
    AnalyzeFormat,
    dependency_diagnostics::DependencyIssues,
    workspace_ingot::{
        INGOT_REQUIRES_WORKSPACE_ROOT, WorkspaceMemberRef, select_workspace_member_paths,
    },
};

#[derive(Debug)]
pub(crate) struct AnalyzeOutcome {
    pub(crate) has_errors: bool,
    pub(crate) output: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct AnalyzeReport {
    schema_version: u32,
    profile: String,
    package_kind: String,
    targets: Vec<AnalyzeTargetReport>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct AnalyzeTargetReport {
    label: String,
    runtime_bodies: usize,
    runtime_statements: OriginCount,
    runtime_terminators: OriginCount,
    bodies: Vec<AnalyzeBodyReport>,
    source_maps: Vec<AnalyzeSourceMapReport>,
    origin_facts: Vec<AnalyzeOriginFactReport>,
    shapes: Vec<AnalyzeShapeReport>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct AnalyzeBodyReport {
    symbol: String,
    statements: OriginCount,
    terminators: OriginCount,
}

#[derive(Debug, Serialize)]
struct AnalyzeSourceMapReport {
    scope: String,
    label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    test: Option<String>,
    object: String,
    section: String,
    total: usize,
    source: usize,
    debug_locations: usize,
    debug_line_table_files: usize,
    debug_line_table_rows: usize,
    non_source: usize,
    source_span_invalid: usize,
    semantic_span_missing: usize,
    runtime_stmt_missing: usize,
    runtime_terminator_missing: usize,
    runtime_synthetic: usize,
    sonatina_synthetic: usize,
    sonatina_unmapped: usize,
    post_preopt_snapshot_gap: usize,
    bytecode_unmapped: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    bytecode_origin_coverage: Option<BytecodeOriginCoverageExport>,
    #[serde(skip_serializing_if = "Option::is_none")]
    post_opt_origin_coverage: Option<SonatinaPostOptOriginCoverageExport>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    entries: Vec<BytecodeSourceMapEntry>,
}

impl AnalyzeSourceMapReport {
    fn from_summary(
        scope: &'static str,
        label: String,
        test: Option<String>,
        object: String,
        section: String,
        summary: &BytecodeSourceMapSummary,
        bytecode_origin_coverage: Option<BytecodeOriginCoverage>,
        post_opt_origin_coverage: Option<SonatinaPostOptOriginCoverage>,
        entries: Vec<BytecodeSourceMapEntry>,
    ) -> Self {
        let report = Self {
            scope: scope.to_string(),
            label,
            test,
            object,
            section,
            total: summary.total(),
            source: summary.source(),
            debug_locations: summary.debug_locations(),
            debug_line_table_files: summary.debug_line_table_files(),
            debug_line_table_rows: summary.debug_line_table_rows(),
            non_source: summary.non_source(),
            source_span_invalid: summary.source_span_invalid(),
            semantic_span_missing: summary.semantic_span_missing(),
            runtime_stmt_missing: summary.runtime_stmt_missing(),
            runtime_terminator_missing: summary.runtime_terminator_missing(),
            runtime_synthetic: summary.runtime_synthetic(),
            sonatina_synthetic: summary.sonatina_synthetic(),
            sonatina_unmapped: summary.sonatina_unmapped(),
            post_preopt_snapshot_gap: summary.post_preopt_snapshot_gap(),
            bytecode_unmapped: summary.bytecode_unmapped(),
            bytecode_origin_coverage: bytecode_origin_coverage
                .map(BytecodeOriginCoverageExport::from),
            post_opt_origin_coverage: post_opt_origin_coverage
                .map(SonatinaPostOptOriginCoverageExport::from),
            entries,
        };
        report
            .validate()
            .unwrap_or_else(|err| panic!("invalid analyze source-map report: {err}"));
        report
    }

    fn validate(&self) -> Result<(), AnalyzeSourceMapReportError> {
        let classified_non_source = checked_sum(
            "analyze source-map non-source classifications",
            [
                self.source_span_invalid,
                self.semantic_span_missing,
                self.runtime_stmt_missing,
                self.runtime_terminator_missing,
                self.runtime_synthetic,
                self.sonatina_synthetic,
                self.sonatina_unmapped,
                self.post_preopt_snapshot_gap,
                self.bytecode_unmapped,
            ],
        )?;
        if self.non_source != classified_non_source {
            return Err(AnalyzeSourceMapReportError::NonSourceMismatch {
                declared: self.non_source,
                actual: classified_non_source,
            });
        }

        let classified_total = self.source.checked_add(self.non_source).ok_or(
            AnalyzeSourceMapReportError::CountOverflow {
                field: "analyze source-map total",
            },
        )?;
        if self.total != classified_total {
            return Err(AnalyzeSourceMapReportError::TotalMismatch {
                declared: self.total,
                actual: classified_total,
            });
        }
        if self.debug_locations != self.source {
            return Err(AnalyzeSourceMapReportError::DebugLocationsMismatch {
                declared: self.debug_locations,
                source: self.source,
            });
        }
        if self.debug_line_table_rows != self.source {
            return Err(AnalyzeSourceMapReportError::DebugLineTableRowsMismatch {
                declared: self.debug_line_table_rows,
                source: self.source,
            });
        }
        if self.debug_line_table_files > self.source {
            return Err(
                AnalyzeSourceMapReportError::DebugLineTableFilesExceedSource {
                    files: self.debug_line_table_files,
                    source: self.source,
                },
            );
        }
        if let Some(coverage) = &self.bytecode_origin_coverage {
            if coverage.total() != self.total {
                return Err(
                    AnalyzeSourceMapReportError::BytecodeOriginCoverageTotalMismatch {
                        report_total: self.total,
                        coverage_total: coverage.total(),
                    },
                );
            }
        }

        if !self.entries.is_empty() {
            if self.entries.len() != self.total {
                return Err(AnalyzeSourceMapReportError::EntryCountMismatch {
                    declared: self.total,
                    actual: self.entries.len(),
                });
            }
            let counts = AnalyzeSourceMapEntryCounts::from_entries(&self.entries);
            self.validate_entry_count("source", self.source, counts.source)?;
            self.validate_entry_count(
                "source_span_invalid",
                self.source_span_invalid,
                counts.source_span_invalid,
            )?;
            self.validate_entry_count(
                "semantic_span_missing",
                self.semantic_span_missing,
                counts.semantic_span_missing,
            )?;
            self.validate_entry_count(
                "runtime_stmt_missing",
                self.runtime_stmt_missing,
                counts.runtime_stmt_missing,
            )?;
            self.validate_entry_count(
                "runtime_terminator_missing",
                self.runtime_terminator_missing,
                counts.runtime_terminator_missing,
            )?;
            self.validate_entry_count(
                "runtime_synthetic",
                self.runtime_synthetic,
                counts.runtime_synthetic,
            )?;
            self.validate_entry_count(
                "sonatina_synthetic",
                self.sonatina_synthetic,
                counts.sonatina_synthetic,
            )?;
            self.validate_entry_count(
                "sonatina_unmapped",
                self.sonatina_unmapped,
                counts.sonatina_unmapped,
            )?;
            self.validate_entry_count(
                "post_preopt_snapshot_gap",
                self.post_preopt_snapshot_gap,
                counts.post_preopt_snapshot_gap,
            )?;
            self.validate_entry_count(
                "bytecode_unmapped",
                self.bytecode_unmapped,
                counts.bytecode_unmapped,
            )?;
        }

        Ok(())
    }

    fn validate_entry_count(
        &self,
        field: &'static str,
        declared: usize,
        actual: usize,
    ) -> Result<(), AnalyzeSourceMapReportError> {
        if declared == actual {
            Ok(())
        } else {
            Err(AnalyzeSourceMapReportError::EntryClassificationMismatch {
                field,
                declared,
                actual,
            })
        }
    }
}

impl<'de> Deserialize<'de> for AnalyzeSourceMapReport {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct RawReport {
            scope: String,
            label: String,
            test: Option<String>,
            object: String,
            section: String,
            total: usize,
            source: usize,
            debug_locations: usize,
            debug_line_table_files: usize,
            debug_line_table_rows: usize,
            non_source: usize,
            source_span_invalid: usize,
            semantic_span_missing: usize,
            runtime_stmt_missing: usize,
            runtime_terminator_missing: usize,
            runtime_synthetic: usize,
            sonatina_synthetic: usize,
            sonatina_unmapped: usize,
            post_preopt_snapshot_gap: usize,
            bytecode_unmapped: usize,
            bytecode_origin_coverage: Option<BytecodeOriginCoverageExport>,
            post_opt_origin_coverage: Option<SonatinaPostOptOriginCoverageExport>,
            #[serde(default)]
            entries: Vec<BytecodeSourceMapEntry>,
        }

        let raw = RawReport::deserialize(deserializer)?;
        let report = Self {
            scope: raw.scope,
            label: raw.label,
            test: raw.test,
            object: raw.object,
            section: raw.section,
            total: raw.total,
            source: raw.source,
            debug_locations: raw.debug_locations,
            debug_line_table_files: raw.debug_line_table_files,
            debug_line_table_rows: raw.debug_line_table_rows,
            non_source: raw.non_source,
            source_span_invalid: raw.source_span_invalid,
            semantic_span_missing: raw.semantic_span_missing,
            runtime_stmt_missing: raw.runtime_stmt_missing,
            runtime_terminator_missing: raw.runtime_terminator_missing,
            runtime_synthetic: raw.runtime_synthetic,
            sonatina_synthetic: raw.sonatina_synthetic,
            sonatina_unmapped: raw.sonatina_unmapped,
            post_preopt_snapshot_gap: raw.post_preopt_snapshot_gap,
            bytecode_unmapped: raw.bytecode_unmapped,
            bytecode_origin_coverage: raw.bytecode_origin_coverage,
            post_opt_origin_coverage: raw.post_opt_origin_coverage,
            entries: raw.entries,
        };
        report.validate().map_err(de::Error::custom)?;
        Ok(report)
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct AnalyzeSourceMapEntryCounts {
    source: usize,
    source_span_invalid: usize,
    semantic_span_missing: usize,
    runtime_stmt_missing: usize,
    runtime_terminator_missing: usize,
    runtime_synthetic: usize,
    sonatina_synthetic: usize,
    sonatina_unmapped: usize,
    post_preopt_snapshot_gap: usize,
    bytecode_unmapped: usize,
}

impl AnalyzeSourceMapEntryCounts {
    fn from_entries(entries: &[BytecodeSourceMapEntry]) -> Self {
        let mut counts = Self::default();
        for entry in entries {
            match entry.kind() {
                BytecodeSourceMapEntryKind::Source { .. } => counts.source += 1,
                BytecodeSourceMapEntryKind::SourceSpanInvalid { .. } => {
                    counts.source_span_invalid += 1;
                }
                BytecodeSourceMapEntryKind::SemanticSpanMissing => {
                    counts.semantic_span_missing += 1;
                }
                BytecodeSourceMapEntryKind::RuntimeStmtMissing => {
                    counts.runtime_stmt_missing += 1;
                }
                BytecodeSourceMapEntryKind::RuntimeTerminatorMissing => {
                    counts.runtime_terminator_missing += 1;
                }
                BytecodeSourceMapEntryKind::RuntimeSynthetic => {
                    counts.runtime_synthetic += 1;
                }
                BytecodeSourceMapEntryKind::SonatinaSynthetic { .. } => {
                    counts.sonatina_synthetic += 1;
                }
                BytecodeSourceMapEntryKind::SonatinaUnmapped { .. } => {
                    counts.sonatina_unmapped += 1;
                }
                BytecodeSourceMapEntryKind::PostPreOptSnapshotGap => {
                    counts.post_preopt_snapshot_gap += 1;
                }
                BytecodeSourceMapEntryKind::BytecodeUnmapped { .. } => {
                    counts.bytecode_unmapped += 1;
                }
            }
        }
        counts
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum AnalyzeSourceMapReportError {
    CountOverflow {
        field: &'static str,
    },
    NonSourceMismatch {
        declared: usize,
        actual: usize,
    },
    TotalMismatch {
        declared: usize,
        actual: usize,
    },
    DebugLocationsMismatch {
        declared: usize,
        source: usize,
    },
    DebugLineTableRowsMismatch {
        declared: usize,
        source: usize,
    },
    DebugLineTableFilesExceedSource {
        files: usize,
        source: usize,
    },
    BytecodeOriginCoverageTotalMismatch {
        report_total: usize,
        coverage_total: usize,
    },
    EntryCountMismatch {
        declared: usize,
        actual: usize,
    },
    EntryClassificationMismatch {
        field: &'static str,
        declared: usize,
        actual: usize,
    },
}

impl std::fmt::Display for AnalyzeSourceMapReportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CountOverflow { field } => write!(f, "{field} overflowed"),
            Self::NonSourceMismatch { declared, actual } => write!(
                f,
                "analyze source-map non_source {declared} does not match classified non-source count {actual}"
            ),
            Self::TotalMismatch { declared, actual } => write!(
                f,
                "analyze source-map total {declared} does not match source plus non_source count {actual}"
            ),
            Self::DebugLocationsMismatch { declared, source } => write!(
                f,
                "analyze source-map debug_locations {declared} does not match source count {source}"
            ),
            Self::DebugLineTableRowsMismatch { declared, source } => write!(
                f,
                "analyze source-map debug_line_table_rows {declared} does not match source count {source}"
            ),
            Self::DebugLineTableFilesExceedSource { files, source } => write!(
                f,
                "analyze source-map debug_line_table_files {files} exceeds source count {source}"
            ),
            Self::BytecodeOriginCoverageTotalMismatch {
                report_total,
                coverage_total,
            } => write!(
                f,
                "analyze source-map total {report_total} does not match bytecode origin coverage total {coverage_total}"
            ),
            Self::EntryCountMismatch { declared, actual } => write!(
                f,
                "analyze source-map total {declared} does not match emitted entry count {actual}"
            ),
            Self::EntryClassificationMismatch {
                field,
                declared,
                actual,
            } => write!(
                f,
                "analyze source-map {field} count {declared} does not match emitted entry count {actual}"
            ),
        }
    }
}

impl std::error::Error for AnalyzeSourceMapReportError {}

fn checked_sum(
    field: &'static str,
    values: impl IntoIterator<Item = usize>,
) -> Result<usize, AnalyzeSourceMapReportError> {
    values.into_iter().try_fold(0usize, |sum, value| {
        sum.checked_add(value)
            .ok_or(AnalyzeSourceMapReportError::CountOverflow { field })
    })
}

#[derive(Debug, Serialize)]
struct AnalyzeOriginFactReport {
    scope: String,
    label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    object: Option<String>,
    total: usize,
    origin_nodes: usize,
    origin_links: usize,
    source_spans: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    source_span_files: Vec<SourceSpanFileCount>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    relation_counts: Vec<TypedFactRelationCount>,
    #[serde(skip_serializing_if = "Option::is_none")]
    relation_tables: Option<TypedFactRelationSet>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reachability: Option<OriginReachabilitySummary>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    path_witnesses: Vec<OriginPathWitnessExport>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    source_path_witnesses: Vec<OriginSourcePathWitnessExport>,
    #[serde(skip_serializing_if = "Option::is_none")]
    query_error: Option<String>,
    facts: OwnedTypedFactSetExport,
}

impl AnalyzeOriginFactReport {
    fn validate(&self) -> Result<(), AnalyzeOriginFactReportError> {
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

        for count in &self.relation_counts {
            let expected = match count.relation() {
                TypedFactRelationName::OriginNode => self.origin_nodes,
                TypedFactRelationName::OriginLink => self.origin_links,
                TypedFactRelationName::SourceSpan => self.source_spans,
                relation => {
                    return Err(AnalyzeOriginFactReportError::UnexpectedRelationCount { relation });
                }
            };
            if count.rows() != expected {
                return Err(AnalyzeOriginFactReportError::RelationCountMismatch {
                    relation: count.relation(),
                    declared: count.rows(),
                    actual: expected,
                });
            }
        }

        if let Some(relations) = &self.relation_tables {
            self.validate_relation_table_count(
                relations,
                TypedFactRelationName::OriginNode,
                self.origin_nodes,
            )?;
            self.validate_relation_table_count(
                relations,
                TypedFactRelationName::OriginLink,
                self.origin_links,
            )?;
            self.validate_relation_table_count(
                relations,
                TypedFactRelationName::SourceSpan,
                self.source_spans,
            )?;
        }

        Ok(())
    }

    fn validate_relation_table_count(
        &self,
        relations: &TypedFactRelationSet,
        relation: TypedFactRelationName,
        expected: usize,
    ) -> Result<(), AnalyzeOriginFactReportError> {
        let actual = relations
            .relation(relation)
            .map(|relation| relation.row_count())
            .unwrap_or_default();
        if actual == expected {
            Ok(())
        } else {
            Err(AnalyzeOriginFactReportError::RelationTableCountMismatch {
                relation,
                declared: expected,
                actual,
            })
        }
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
enum AnalyzeOriginFactReportError {
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
    UnexpectedRelationCount {
        relation: TypedFactRelationName,
    },
    RelationCountMismatch {
        relation: TypedFactRelationName,
        declared: usize,
        actual: usize,
    },
    RelationTableCountMismatch {
        relation: TypedFactRelationName,
        declared: usize,
        actual: usize,
    },
}

impl std::fmt::Display for AnalyzeOriginFactReportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
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
            Self::RelationTableCountMismatch {
                relation,
                declared,
                actual,
            } => write!(
                f,
                "analyze origin-fact relation table {} has {actual} rows, expected {declared}",
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

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct AnalyzeShapeReport {
    scope: String,
    label: String,
    shape_nodes: usize,
    shape_fields: usize,
    shape_children: usize,
    shape_edges: usize,
    trace_events: usize,
    data_flows: usize,
    graph_hashes: Vec<AnalyzeShapeHashReport>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    relation_counts: Vec<TypedFactRelationCount>,
    #[serde(skip_serializing_if = "Option::is_none")]
    facts: Option<OwnedTypedFactSetExport>,
    #[serde(skip_serializing_if = "Option::is_none")]
    relation_tables: Option<TypedFactRelationSet>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct AnalyzeShapeHashReport {
    dimension: ShapeDimension,
    digest_hex: String,
}

const ORIGIN_PATH_WITNESS_LIMIT: usize = 12;
const ORIGIN_PATH_WITNESS_PRIORITY: &[(OriginExportKind, OriginExportKind)] = &[
    (OriginExportKind::Semantic, OriginExportKind::RuntimeStmt),
    (
        OriginExportKind::Semantic,
        OriginExportKind::RuntimeTerminator,
    ),
    (OriginExportKind::Semantic, OriginExportKind::BytecodePc),
    (
        OriginExportKind::RuntimeStmt,
        OriginExportKind::SonatinaInst,
    ),
    (
        OriginExportKind::RuntimeTerminator,
        OriginExportKind::SonatinaInst,
    ),
    (OriginExportKind::RuntimeStmt, OriginExportKind::BytecodePc),
    (
        OriginExportKind::RuntimeTerminator,
        OriginExportKind::BytecodePc,
    ),
    (OriginExportKind::SonatinaInst, OriginExportKind::BytecodePc),
    (
        OriginExportKind::BytecodeUnmapped,
        OriginExportKind::BytecodePc,
    ),
];

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
struct OriginCount {
    total: usize,
    semantic: usize,
    synthetic: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OriginCountError {
    TotalOverflow,
    TotalMismatch { declared: usize, actual: usize },
}

impl std::fmt::Display for OriginCountError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TotalOverflow => write!(f, "origin count total overflowed"),
            Self::TotalMismatch { declared, actual } => write!(
                f,
                "origin count total {declared} does not match semantic plus synthetic count {actual}"
            ),
        }
    }
}

impl std::error::Error for OriginCountError {}

impl OriginCount {
    fn try_new(total: usize, semantic: usize, synthetic: usize) -> Result<Self, OriginCountError> {
        let actual = semantic
            .checked_add(synthetic)
            .ok_or(OriginCountError::TotalOverflow)?;
        if total != actual {
            return Err(OriginCountError::TotalMismatch {
                declared: total,
                actual,
            });
        }
        Ok(Self {
            total,
            semantic,
            synthetic,
        })
    }

    fn push(&mut self, source: RuntimeOriginSource<'_>) {
        self.total += 1;
        match source {
            RuntimeOriginSource::Semantic(_) => self.semantic += 1,
            RuntimeOriginSource::Synthetic => self.synthetic += 1,
        }
    }

    fn extend(&mut self, other: Self) {
        self.total += other.total;
        self.semantic += other.semantic;
        self.synthetic += other.synthetic;
    }
}

impl<'de> Deserialize<'de> for OriginCount {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct RawCount {
            total: usize,
            semantic: usize,
            synthetic: usize,
        }

        let raw = RawCount::deserialize(deserializer)?;
        Self::try_new(raw.total, raw.semantic, raw.synthetic).map_err(de::Error::custom)
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct AnalyzeOptions<'a> {
    profile: &'a str,
    format: AnalyzeFormat,
    include_tests: bool,
    include_source_maps: bool,
    include_source_map_entries: bool,
    include_origin_facts: bool,
    include_fact_relation_tables: bool,
    include_shape_hashes: bool,
    include_shape_facts: bool,
    opt_level: OptLevel,
    recovery_mode: bool,
}

impl<'a> AnalyzeOptions<'a> {
    fn new(
        profile: &'a str,
        format: AnalyzeFormat,
        include_tests: bool,
        include_source_maps: bool,
        include_source_map_entries: bool,
        include_origin_facts: bool,
        include_fact_relation_tables: bool,
        include_shape_hashes: bool,
        include_shape_facts: bool,
        opt_level: OptLevel,
        recovery_mode: bool,
    ) -> Self {
        Self {
            profile,
            format,
            include_tests,
            include_source_maps,
            include_source_map_entries,
            include_origin_facts,
            include_fact_relation_tables,
            include_shape_hashes,
            include_shape_facts,
            opt_level,
            recovery_mode,
        }
    }

    fn validate(self) -> Result<Self, String> {
        if self.include_source_map_entries && !self.include_source_maps {
            return Err("`fe analyze --source-map-entries` requires `--source-maps`".to_string());
        }
        if self.include_fact_relation_tables && self.format != AnalyzeFormat::Json {
            return Err("`fe analyze --fact-relation-tables` requires `--format json`".to_string());
        }
        if self.include_fact_relation_tables
            && !(self.include_origin_facts || self.include_shape_facts)
        {
            return Err(
                "`fe analyze --fact-relation-tables` requires `--origin-facts` or `--shape-facts`"
                    .to_string(),
            );
        }
        Ok(self)
    }
}

pub fn analyze(
    path: &Utf8PathBuf,
    ingot: Option<&str>,
    force_standalone: bool,
    profile: &str,
    format: AnalyzeFormat,
    include_tests: bool,
    include_source_maps: bool,
    include_source_map_entries: bool,
    include_origin_facts: bool,
    include_fact_relation_tables: bool,
    include_shape_hashes: bool,
    include_shape_facts: bool,
    opt_level: OptLevel,
    recovery_mode: bool,
) -> Result<bool, String> {
    let options = AnalyzeOptions::new(
        profile,
        format,
        include_tests,
        include_source_maps,
        include_source_map_entries,
        include_origin_facts,
        include_fact_relation_tables,
        include_shape_hashes,
        include_shape_facts,
        opt_level,
        recovery_mode,
    );
    let outcome = analyze_to_string(path, ingot, force_standalone, options)?;
    if !outcome.output.is_empty() {
        print!("{}", outcome.output);
    }
    Ok(outcome.has_errors)
}

pub(crate) fn analyze_to_string(
    path: &Utf8PathBuf,
    ingot: Option<&str>,
    force_standalone: bool,
    options: AnalyzeOptions<'_>,
) -> Result<AnalyzeOutcome, String> {
    let options = options.validate()?;

    let mut db = DriverDataBase::default();
    db.compiler_options()
        .set_recovery_mode(&mut db)
        .to(options.recovery_mode);
    db.compilation_settings()
        .set_profile(&mut db)
        .to(options.profile.into());

    let mut report = AnalyzeReport {
        schema_version: 1,
        profile: options.profile.to_string(),
        package_kind: if options.include_tests {
            "tests"
        } else {
            "runtime"
        }
        .to_string(),
        targets: Vec::new(),
    };

    let target = resolve_cli_target(&mut db, path, force_standalone)?;
    let has_errors = match target {
        CliTarget::StandaloneFile(file_path) => {
            if ingot.is_some() {
                eprintln!("Error: {INGOT_REQUIRES_WORKSPACE_ROOT}");
                true
            } else {
                analyze_single_file(&mut db, &file_path, options, &mut report)
            }
        }
        CliTarget::Directory(dir_path) => {
            analyze_directory(&mut db, &dir_path, ingot, options, &mut report)
        }
    };

    report
        .targets
        .sort_by(|left, right| left.label.cmp(&right.label));
    let output = if has_errors {
        String::new()
    } else {
        render_report(&report, options.format)?
    };
    Ok(AnalyzeOutcome { has_errors, output })
}

fn analyze_single_file(
    db: &mut DriverDataBase,
    file_path: &Utf8PathBuf,
    options: AnalyzeOptions<'_>,
    report: &mut AnalyzeReport,
) -> bool {
    let canonical = match file_path.canonicalize_utf8() {
        Ok(path) => path,
        Err(err) => {
            eprintln!("Error: Cannot canonicalize {file_path}: {err}");
            return true;
        }
    };
    let file_url = match Url::from_file_path(&canonical) {
        Ok(url) => url,
        Err(_) => {
            eprintln!("Error: Invalid file path: {file_path}");
            return true;
        }
    };
    let content = match std::fs::read_to_string(file_path) {
        Ok(content) => content,
        Err(err) => {
            eprintln!("Error: Failed to read file {file_path}: {err}");
            return true;
        }
    };

    db.workspace().touch(db, file_url.clone(), Some(content));
    let Some(file) = db.workspace().get(db, &file_url) else {
        eprintln!("Error: Could not process file {file_path}");
        return true;
    };
    let top_mod = db.top_mod(file);
    analyze_top_mod(db, file_path.as_str(), top_mod, options, report)
}

fn analyze_directory(
    db: &mut DriverDataBase,
    dir_path: &Utf8PathBuf,
    ingot: Option<&str>,
    options: AnalyzeOptions<'_>,
    report: &mut AnalyzeReport,
) -> bool {
    let ingot_url = match dir_url(dir_path) {
        Ok(url) => url,
        Err(message) => {
            eprintln!("{message}");
            return true;
        }
    };

    let had_init_diagnostics = driver::init_ingot(db, &ingot_url);
    if had_init_diagnostics {
        return true;
    }

    let config = match config_from_db(db, &ingot_url) {
        Ok(Some(config)) => config,
        Ok(None) => {
            if ingot.is_some() {
                eprintln!("Error: {INGOT_REQUIRES_WORKSPACE_ROOT}");
                return true;
            }
            eprintln!("Error: No fe.toml file found in the root directory");
            return true;
        }
        Err(err) => {
            eprintln!("Error: {err}");
            return true;
        }
    };

    match config {
        Config::Workspace(workspace) => {
            analyze_workspace(db, dir_path, *workspace, ingot, options, report)
        }
        Config::Ingot(_) => {
            if ingot.is_some() {
                eprintln!("Error: {INGOT_REQUIRES_WORKSPACE_ROOT}");
                return true;
            }
            analyze_ingot_url(db, &ingot_url, options, report)
        }
    }
}

fn analyze_workspace(
    db: &mut DriverDataBase,
    dir_path: &Utf8PathBuf,
    workspace_config: WorkspaceConfig,
    ingot: Option<&str>,
    options: AnalyzeOptions<'_>,
    report: &mut AnalyzeReport,
) -> bool {
    let workspace_url = match dir_url(dir_path) {
        Ok(url) => url,
        Err(message) => {
            eprintln!("{message}");
            return true;
        }
    };

    let members = match driver::workspace_members(&workspace_config.workspace, &workspace_url) {
        Ok(members) => members,
        Err(err) => {
            eprintln!("Error: Failed to resolve workspace members: {err}");
            return true;
        }
    };

    let selected_member_paths = match select_workspace_member_paths(
        dir_path,
        dir_path,
        members
            .iter()
            .map(|member| WorkspaceMemberRef::new(member.path.as_path(), member.name.as_deref())),
        ingot,
    ) {
        Ok(paths) => paths.into_iter().collect::<HashSet<_>>(),
        Err(err) => {
            eprintln!("Error: {err}");
            return true;
        }
    };

    let mut seen = HashSet::new();
    let mut has_errors = false;
    for member in members {
        let member_path = dir_path.join(member.path.as_str());
        if !selected_member_paths.contains(&member_path) {
            continue;
        }
        has_errors |= analyze_ingot_and_dependencies(db, &member.url, options, report, &mut seen);
    }
    has_errors
}

fn analyze_ingot_url(
    db: &mut DriverDataBase,
    ingot_url: &Url,
    options: AnalyzeOptions<'_>,
    report: &mut AnalyzeReport,
) -> bool {
    let mut seen = HashSet::new();
    analyze_ingot_and_dependencies(db, ingot_url, options, report, &mut seen)
}

fn analyze_ingot_and_dependencies(
    db: &mut DriverDataBase,
    ingot_url: &Url,
    options: AnalyzeOptions<'_>,
    report: &mut AnalyzeReport,
    seen: &mut HashSet<Url>,
) -> bool {
    if !seen.insert(ingot_url.clone()) {
        return false;
    }

    let Some(ingot) = db.workspace().containing_ingot(db, ingot_url.clone()) else {
        eprintln!("Error: Could not resolve ingot {ingot_url}");
        return true;
    };

    if !ingot_has_source_files(db, ingot) {
        eprintln!("Error: Could not find source files for ingot {ingot_url}");
        return true;
    }

    let label = ingot_label(db, ingot, ingot_url);
    let mut has_errors = analyze_ingot_diagnostics(db, ingot, &label);

    let dependency_errors = DependencyIssues::collect(db, ingot_url, seen);
    if !dependency_errors.is_empty() {
        has_errors = true;
        eprint!("{}", dependency_errors.format(db));
    }

    if has_errors {
        return true;
    }

    if options.include_tests {
        analyze_ingot_test_modules(db, &label, ingot, options, report)
    } else {
        analyze_top_mod(db, &label, ingot.root_mod(db), options, report)
    }
}

fn analyze_top_mod(
    db: &DriverDataBase,
    label: &str,
    top_mod: TopLevelMod<'_>,
    options: AnalyzeOptions<'_>,
    report: &mut AnalyzeReport,
) -> bool {
    if analyze_top_mod_diagnostics(db, top_mod, label) {
        return true;
    }

    let package = if options.include_tests {
        match build_test_runtime_package(db, top_mod, None) {
            Ok(package) => package,
            Err(err) => {
                eprintln!("Error: failed to build test runtime package for {label}: {err}");
                return true;
            }
        }
    } else {
        match build_runtime_package(db, top_mod) {
            Ok(package) => package,
            Err(err) => {
                eprintln!("Error: failed to build runtime package for {label}: {err}");
                return true;
            }
        }
    };
    let origins = runtime_package_origins(db, package);
    let mut target = summarize_runtime_origins(label, &origins);
    if options.include_shape_hashes || options.include_shape_facts {
        target.shapes = summarize_const_region_shapes(
            db,
            package,
            options.include_shape_facts,
            options.include_fact_relation_tables,
        );
        target.shapes.extend(summarize_runtime_body_shapes(
            db,
            package,
            options.include_shape_facts,
            options.include_fact_relation_tables,
        ));
    }
    if options.include_origin_facts
        && let Some(facts) =
            runtime_origin_fact_report(label, &origins, options.include_fact_relation_tables)
    {
        target.origin_facts.push(facts);
    }
    if options.include_tests && (options.include_source_maps || options.include_origin_facts) {
        let codegen_reports = match summarize_test_codegen_reports(db, top_mod, options) {
            Ok(codegen_reports) => codegen_reports,
            Err(err) => {
                eprintln!("Error: failed to build Sonatina analysis reports for {label}: {err}");
                return true;
            }
        };
        target.source_maps = codegen_reports.source_maps;
        target.origin_facts.extend(codegen_reports.origin_facts);
    } else if options.include_source_maps {
        let codegen_reports = match summarize_runtime_codegen_reports(db, package, options) {
            Ok(codegen_reports) => codegen_reports,
            Err(err) => {
                eprintln!("Error: failed to build Sonatina analysis reports for {label}: {err}");
                return true;
            }
        };
        target.source_maps = codegen_reports.source_maps;
        target.origin_facts.extend(codegen_reports.origin_facts);
    }
    report.targets.push(target);
    false
}

fn analyze_ingot_test_modules(
    db: &DriverDataBase,
    label: &str,
    ingot: Ingot<'_>,
    options: AnalyzeOptions<'_>,
    report: &mut AnalyzeReport,
) -> bool {
    let mut top_mods = ingot.all_modules(db).to_vec();
    top_mods.sort_by(|left, right| left.name(db).cmp(&right.name(db)));

    let mut has_errors = false;
    for top_mod in top_mods {
        if !has_test_functions(db, top_mod) {
            continue;
        }
        let module_label = format!("{label}::{}", top_mod.name(db).data(db));
        has_errors |= analyze_top_mod(db, &module_label, top_mod, options, report);
    }

    has_errors
}

fn has_test_functions(db: &DriverDataBase, top_mod: TopLevelMod<'_>) -> bool {
    top_mod.all_funcs(db).iter().any(|func| {
        ItemKind::from(*func)
            .attrs(db)
            .is_some_and(|attrs| attrs.has_attr(db, "test"))
    })
}

struct AnalyzeTestCodegenReports {
    source_maps: Vec<AnalyzeSourceMapReport>,
    origin_facts: Vec<AnalyzeOriginFactReport>,
}

fn summarize_runtime_codegen_reports(
    db: &DriverDataBase,
    package: mir::RuntimePackage<'_>,
    options: AnalyzeOptions<'_>,
) -> Result<AnalyzeTestCodegenReports, codegen::LowerError> {
    let outputs = emit_runtime_package_sonatina_bytecode_with_source_maps(
        db,
        &package,
        codegen::EVM_LAYOUT,
        options.opt_level,
    )?;
    let source_maps = options
        .include_source_maps
        .then(|| {
            outputs
                .iter()
                .filter_map(|(contract, output)| {
                    source_map_report_for_runtime_contract(
                        contract,
                        output,
                        options.include_source_map_entries,
                    )
                })
                .collect()
        })
        .unwrap_or_default();
    let origin_facts = options
        .include_origin_facts
        .then(|| {
            outputs
                .iter()
                .flat_map(|(contract, output)| {
                    origin_fact_reports_for_runtime_contract(
                        contract,
                        output,
                        options.include_fact_relation_tables,
                    )
                })
                .collect()
        })
        .unwrap_or_default();

    Ok(AnalyzeTestCodegenReports {
        source_maps,
        origin_facts,
    })
}

fn summarize_test_codegen_reports(
    db: &DriverDataBase,
    top_mod: TopLevelMod<'_>,
    options: AnalyzeOptions<'_>,
) -> Result<AnalyzeTestCodegenReports, codegen::LowerError> {
    let output = emit_test_module_sonatina(
        db,
        top_mod,
        options.opt_level,
        SonatinaTestOptions {
            emit_observability: true,
        },
        None,
    )?;
    let source_maps = options
        .include_source_maps
        .then(|| {
            output
                .tests
                .iter()
                .filter_map(|test| {
                    source_map_report_for_test(test, options.include_source_map_entries)
                })
                .collect()
        })
        .unwrap_or_default();
    let origin_facts = options
        .include_origin_facts
        .then(|| {
            output
                .tests
                .iter()
                .flat_map(|test| {
                    origin_fact_reports_for_test(test, options.include_fact_relation_tables)
                })
                .collect()
        })
        .unwrap_or_default();

    Ok(AnalyzeTestCodegenReports {
        source_maps,
        origin_facts,
    })
}

fn source_map_report_for_test(
    test: &TestMetadata,
    include_source_map_entries: bool,
) -> Option<AnalyzeSourceMapReport> {
    let summary = test.sonatina_source_map_summary.as_ref()?;
    let entries = include_source_map_entries
        .then(|| test.sonatina_source_map_entries.clone())
        .unwrap_or_default();
    Some(source_map_report_from_summary(
        &test.display_name,
        &test.object_name,
        summary,
        test.sonatina_bytecode_origin_coverage,
        test.sonatina_post_opt_origin_coverage,
        entries,
    ))
}

fn source_map_report_for_runtime_contract(
    contract: &str,
    output: &SonatinaContractBytecode,
    include_source_map_entries: bool,
) -> Option<AnalyzeSourceMapReport> {
    source_map_report_from_entries(
        "runtime_bytecode",
        contract.to_string(),
        contract,
        &output.source_map_entries,
        output.bytecode_origin_coverage,
        output.post_opt_origin_coverage,
        include_source_map_entries,
    )
}

fn source_map_report_from_summary(
    test: &str,
    fallback_object: &str,
    summary: &BytecodeSourceMapSummary,
    bytecode_origin_coverage: Option<BytecodeOriginCoverage>,
    post_opt_origin_coverage: Option<SonatinaPostOptOriginCoverage>,
    entries: Vec<BytecodeSourceMapEntry>,
) -> AnalyzeSourceMapReport {
    AnalyzeSourceMapReport::from_summary(
        "test_bytecode",
        test.to_string(),
        Some(test.to_string()),
        summary.object().unwrap_or(fallback_object).to_string(),
        summary.section().unwrap_or("<all>").to_string(),
        summary,
        bytecode_origin_coverage,
        post_opt_origin_coverage,
        entries,
    )
}

fn source_map_report_from_entries(
    scope: &'static str,
    label: String,
    fallback_object: &str,
    all_entries: &[BytecodeSourceMapEntry],
    bytecode_origin_coverage: Option<BytecodeOriginCoverage>,
    post_opt_origin_coverage: Option<SonatinaPostOptOriginCoverage>,
    include_entries: bool,
) -> Option<AnalyzeSourceMapReport> {
    let summary = bytecode_source_map_entries_summary(all_entries, None)?;

    Some(AnalyzeSourceMapReport::from_summary(
        scope,
        label,
        None,
        all_entries
            .first()
            .map(|entry| entry.object().to_string())
            .unwrap_or_else(|| fallback_object.to_string()),
        "<all>".to_string(),
        &summary,
        bytecode_origin_coverage,
        post_opt_origin_coverage,
        include_entries
            .then(|| all_entries.to_vec())
            .unwrap_or_default(),
    ))
}

fn summarize_const_region_shapes<'db>(
    db: &'db DriverDataBase,
    package: mir::RuntimePackage<'db>,
    include_facts: bool,
    include_relation_tables: bool,
) -> Vec<AnalyzeShapeReport> {
    package
        .const_regions(db)
        .into_iter()
        .enumerate()
        .map(|(idx, region)| {
            let data = region.data(db);
            let graph = data.value.shape_graph();
            shape_report_for_graph(
                "const_region",
                format!("const_region:{idx}"),
                graph,
                include_facts,
                include_relation_tables,
            )
        })
        .collect()
}

fn summarize_runtime_body_shapes<'db>(
    db: &'db DriverDataBase,
    package: mir::RuntimePackage<'db>,
    include_facts: bool,
    include_relation_tables: bool,
) -> Vec<AnalyzeShapeReport> {
    package
        .functions(db)
        .into_iter()
        .map(|function| {
            let body = function.instance(db).body(db);
            let graph = RuntimeBodyShape { body: &body }.shape_graph();
            shape_report_for_graph(
                "runtime_body",
                function.symbol(db),
                graph,
                include_facts,
                include_relation_tables,
            )
        })
        .collect()
}

fn shape_report_for_graph(
    scope: &'static str,
    label: String,
    graph: ShapeGraph,
    include_facts: bool,
    include_relation_tables: bool,
) -> AnalyzeShapeReport {
    let graph_hashes = graph.hashes().graph();
    let shape_nodes = graph.nodes().len();
    let shape_fields = graph.nodes().iter().map(|node| node.fields().len()).sum();
    let shape_children = graph.nodes().iter().map(|node| node.children().len()).sum();
    let shape_edges = graph.edges().len();
    let trace_events = graph
        .nodes()
        .iter()
        .flat_map(|node| node.fields())
        .filter(|field| field.dimension() == ShapeDimension::TraceEvents)
        .count();
    let data_flows = graph.edges().len();
    let facts = include_facts.then(|| shape_graph_facts(&graph));
    let relation_export = facts.as_ref().map(TypedFactSet::relation_export);
    let relation_counts = relation_export
        .as_ref()
        .map(|export| {
            let relation_index = relation_index_for_export(export);
            relation_counts_from_relation_index(&relation_index)
        })
        .unwrap_or_default();
    let relation_tables = include_relation_tables.then_some(relation_export).flatten();
    let facts = facts.map(|facts| facts.to_owned_export());

    AnalyzeShapeReport {
        scope: scope.to_string(),
        label,
        shape_nodes,
        shape_fields,
        shape_children,
        shape_edges,
        trace_events,
        data_flows,
        graph_hashes: ShapeDimension::ALL
            .into_iter()
            .map(|dimension| AnalyzeShapeHashReport {
                dimension,
                digest_hex: graph_hashes.digest(dimension).to_hex(),
            })
            .collect(),
        relation_counts,
        facts,
        relation_tables,
    }
}

struct RuntimeBodyShape<'a, 'db> {
    body: &'a mir::RuntimeBody<'db>,
}

impl ShapeDescribe for RuntimeBodyShape<'_, '_> {
    fn describe_shape(&self, builder: &mut ShapeBuilder) -> ShapeNodeId {
        let node = builder.add_described_node("RuntimeBody", None);
        builder.add_field_value(
            node,
            ShapeDimension::Structure,
            "locals",
            &self.body.locals.len(),
        );
        builder.add_field_value(
            node,
            ShapeDimension::Structure,
            "blocks",
            &self.body.blocks.len(),
        );
        let statement_count = self
            .body
            .blocks
            .iter()
            .map(|block| block.stmts.len())
            .sum::<usize>();
        builder.add_field_value(
            node,
            ShapeDimension::Structure,
            "statements",
            &statement_count,
        );
        for (idx, block) in self.body.blocks.iter().enumerate() {
            builder.add_child_node(node, format!("block:{idx}"), block);
        }
        node
    }
}

fn origin_fact_reports_for_test(
    test: &TestMetadata,
    include_relation_tables: bool,
) -> Vec<AnalyzeOriginFactReport> {
    let mut reports = Vec::new();
    if let Some(facts) = test.sonatina_origin_facts.as_ref() {
        reports.push(origin_fact_report(
            "test_bytecode",
            test.display_name.clone(),
            Some(test.object_name.clone()),
            facts,
            include_relation_tables,
        ));
    }
    if let Some(facts) = test.sonatina_snapshot_origin_facts.as_ref() {
        reports.push(origin_fact_report(
            "test_sonatina_snapshot",
            test.display_name.clone(),
            Some(test.object_name.clone()),
            facts,
            include_relation_tables,
        ));
    }
    reports
}

fn origin_fact_reports_for_runtime_contract(
    contract: &str,
    output: &SonatinaContractBytecode,
    include_relation_tables: bool,
) -> Vec<AnalyzeOriginFactReport> {
    let mut reports = Vec::new();
    if let Some(facts) = output.origin_facts.as_ref() {
        reports.push(origin_fact_report(
            "runtime_bytecode",
            contract.to_string(),
            Some(contract.to_string()),
            facts,
            include_relation_tables,
        ));
    }
    if let Some(facts) = output.snapshot_origin_facts.as_ref() {
        reports.push(origin_fact_report(
            "runtime_sonatina_snapshot",
            contract.to_string(),
            Some(contract.to_string()),
            facts,
            include_relation_tables,
        ));
    }
    reports
}

fn runtime_origin_fact_report<'db>(
    label: &str,
    origins: &mir::RuntimePackageOrigins<'db>,
    include_relation_tables: bool,
) -> Option<AnalyzeOriginFactReport> {
    let target_key = RuntimeOriginFactTargetKey::new(label);
    let facts = runtime_package_origin_facts(origins, |body| {
        RuntimeOriginFactOwnerKeys::for_body(&target_key, body.symbol_key())
    });
    if facts.facts().is_empty() {
        return None;
    }
    Some(origin_fact_report(
        "runtime",
        label.to_string(),
        None,
        &facts,
        include_relation_tables,
    ))
}

fn origin_fact_report(
    scope: &'static str,
    label: String,
    object: Option<String>,
    facts: &TypedFactSet,
    include_relation_tables: bool,
) -> AnalyzeOriginFactReport {
    let relation_export = facts.relation_export();
    let relation_index = relation_index_for_export(&relation_export);
    let reachability = Some(
        relation_index
            .origin_reachability_summary()
            .expect("typed fact relation export should answer origin reachability"),
    );
    let relation_counts = relation_counts_from_relation_index(&relation_index);
    let source_span_files = source_span_file_counts_from_relation_index(&relation_index);

    let mut query_errors = Vec::new();
    let path_witnesses = match relation_index.representative_path_exports_with_priority(
        ORIGIN_PATH_WITNESS_PRIORITY.iter().copied(),
        ORIGIN_PATH_WITNESS_LIMIT,
    ) {
        Ok(path_witnesses) => path_witnesses,
        Err(err) => {
            query_errors.push(format!("{err:?}"));
            Vec::new()
        }
    };
    let source_path_witnesses = match relation_index
        .representative_source_path_exports_with_priority(
            ORIGIN_PATH_WITNESS_PRIORITY.iter().copied(),
            ORIGIN_PATH_WITNESS_LIMIT,
        ) {
        Ok(source_path_witnesses) => source_path_witnesses,
        Err(err) => {
            query_errors.push(format!("{err:?}"));
            Vec::new()
        }
    };

    AnalyzeOriginFactReport {
        scope: scope.to_string(),
        label,
        object,
        total: facts.facts().len(),
        origin_nodes: facts.origin_nodes().count(),
        origin_links: facts.origin_links().count(),
        source_spans: facts.source_spans().count(),
        source_span_files,
        relation_counts,
        relation_tables: include_relation_tables.then_some(relation_export),
        reachability,
        path_witnesses,
        source_path_witnesses,
        query_error: (!query_errors.is_empty()).then(|| query_errors.join("; ")),
        facts: facts.to_owned_export(),
    }
}

fn relation_index_for_export(export: &TypedFactRelationSet) -> TypedFactRelationIndex<'_> {
    TypedFactRelationIndex::new(export)
        .expect("typed fact relation export should build a query index")
}

fn relation_counts_from_relation_index(
    index: &TypedFactRelationIndex<'_>,
) -> Vec<TypedFactRelationCount> {
    index
        .relation_counts()
        .expect("typed fact relation export should contain declared relations")
}

fn source_span_file_counts_from_relation_index(
    index: &TypedFactRelationIndex<'_>,
) -> Vec<SourceSpanFileCount> {
    index
        .source_span_file_counts()
        .expect("typed fact relation export should contain source_span relation")
}

fn analyze_top_mod_diagnostics(db: &DriverDataBase, top_mod: TopLevelMod<'_>, label: &str) -> bool {
    let hir_diags = db.run_on_top_mod(top_mod);
    let mut has_errors = false;
    let hir_has_errors = hir_diags.has_errors(db);

    if !hir_diags.is_empty() {
        eprintln!("errors in {label}");
        eprintln!();
        hir_diags.emit(db);
        has_errors = true;
    }

    let mir_diags = if hir_has_errors {
        Vec::new()
    } else {
        db.mir_diagnostics_for_top_mod(top_mod)
    };
    if !mir_diags.is_empty() {
        if !has_errors {
            eprintln!("errors in {label}");
            eprintln!();
        }
        db.emit_complete_diagnostics(&mir_diags);
        has_errors = true;
    }

    has_errors
}

fn analyze_ingot_diagnostics(db: &DriverDataBase, ingot: Ingot<'_>, label: &str) -> bool {
    let hir_diags = db.run_on_ingot(ingot);
    let mut has_errors = false;
    let hir_has_errors = hir_diags.has_errors(db);

    if !hir_diags.is_empty() {
        eprintln!("errors in {label}");
        eprintln!();
        hir_diags.emit(db);
        has_errors = true;
    }

    let mir_diags = if hir_has_errors {
        Vec::new()
    } else {
        db.mir_diagnostics_for_ingot(ingot)
    };
    if !mir_diags.is_empty() {
        if !has_errors {
            eprintln!("errors in {label}");
            eprintln!();
        }
        db.emit_complete_diagnostics(&mir_diags);
        has_errors = true;
    }

    has_errors
}

fn summarize_runtime_origins(
    label: &str,
    origins: &mir::RuntimePackageOrigins<'_>,
) -> AnalyzeTargetReport {
    let mut runtime_statements = OriginCount::default();
    let mut runtime_terminators = OriginCount::default();
    let mut bodies = Vec::new();

    for body in origins.bodies() {
        let mut statements = OriginCount::default();
        for record in body.origins().stmt_origins() {
            statements.push(record.source());
        }
        let mut terminators = OriginCount::default();
        for record in body.origins().terminator_origins() {
            terminators.push(record.source());
        }
        runtime_statements.extend(statements);
        runtime_terminators.extend(terminators);
        bodies.push(AnalyzeBodyReport {
            symbol: body.symbol().to_string(),
            statements,
            terminators,
        });
    }

    bodies.sort_by(|left, right| left.symbol.cmp(&right.symbol));

    AnalyzeTargetReport {
        label: label.to_string(),
        runtime_bodies: bodies.len(),
        runtime_statements,
        runtime_terminators,
        bodies,
        source_maps: Vec::new(),
        origin_facts: Vec::new(),
        shapes: Vec::new(),
    }
}

fn render_report(report: &AnalyzeReport, format: AnalyzeFormat) -> Result<String, String> {
    match format {
        AnalyzeFormat::Text => Ok(render_text_report(report)),
        AnalyzeFormat::Json => serde_json::to_string_pretty(report)
            .map(|mut json| {
                json.push('\n');
                json
            })
            .map_err(|err| format!("failed to render analyze JSON: {err}")),
    }
}

fn render_text_report(report: &AnalyzeReport) -> String {
    let mut out = String::new();
    writeln!(out, "Fe Origin Analysis").unwrap();
    writeln!(out, "profile: {}", report.profile).unwrap();
    writeln!(out, "package_kind: {}", report.package_kind).unwrap();
    writeln!(out, "targets: {}", report.targets.len()).unwrap();
    writeln!(out).unwrap();

    for target in &report.targets {
        writeln!(out, "{}", target.label).unwrap();
        writeln!(out, "  runtime bodies: {}", target.runtime_bodies).unwrap();
        write_origin_count(&mut out, "  runtime statements", target.runtime_statements);
        write_origin_count(
            &mut out,
            "  runtime terminators",
            target.runtime_terminators,
        );
        if !target.bodies.is_empty() {
            writeln!(out, "  bodies:").unwrap();
            for body in &target.bodies {
                writeln!(out, "    {}", body.symbol).unwrap();
                write_origin_count(&mut out, "      statements", body.statements);
                write_origin_count(&mut out, "      terminators", body.terminators);
            }
        }
        if !target.source_maps.is_empty() {
            writeln!(out, "  source maps:").unwrap();
            for source_map in &target.source_maps {
                let bytecode_coverage = source_map
                    .bytecode_origin_coverage
                    .as_ref()
                    .map(|coverage| {
                        format!(
                            " bytecode_origins={} post_opt={} backend_prepared={} unmapped={}",
                            coverage.total(),
                            coverage.sonatina_post_opt(),
                            coverage.sonatina_backend_prepared(),
                            coverage.unmapped()
                        )
                    })
                    .unwrap_or_default();
                let post_opt_coverage = source_map
                    .post_opt_origin_coverage
                    .as_ref()
                    .map(|coverage| {
                        format!(
                            " post_opt_origins={} same_inst_id={} created_or_unmatched_after_preopt_snapshot={} pre_opt_snapshot_losses={}",
                            coverage.total(),
                            coverage.same_inst_id(),
                            coverage.created_or_unmatched_after_preopt_snapshot(),
                            coverage.pre_opt_snapshot_losses(),
                        )
                    })
                    .unwrap_or_default();
                writeln!(
                    out,
                    "    {} {} {}:{} total={} source={} debug_locations={} debug_line_table_files={} debug_line_table_rows={} non_source={}{}{}",
                    source_map.scope,
                    source_map.label,
                    source_map.object,
                    source_map.section,
                    source_map.total,
                    source_map.source,
                    source_map.debug_locations,
                    source_map.debug_line_table_files,
                    source_map.debug_line_table_rows,
                    source_map.non_source,
                    bytecode_coverage,
                    post_opt_coverage,
                )
                .unwrap();
                write_source_map_breakdown(&mut out, source_map);
                if !source_map.entries.is_empty() {
                    writeln!(out, "      entries:").unwrap();
                    for entry in &source_map.entries {
                        write_source_map_entry(&mut out, entry);
                    }
                }
            }
        }
        if !target.origin_facts.is_empty() {
            writeln!(out, "  origin facts:").unwrap();
            for origin_facts in &target.origin_facts {
                let reachable_pairs = origin_facts
                    .reachability
                    .as_ref()
                    .map(|summary| format!(" reachable_pairs={}", summary.reachable_pairs()))
                    .unwrap_or_default();
                let path_witnesses = (!origin_facts.path_witnesses.is_empty())
                    .then(|| format!(" path_witnesses={}", origin_facts.path_witnesses.len()))
                    .unwrap_or_default();
                let source_path_witnesses = (!origin_facts.source_path_witnesses.is_empty())
                    .then(|| {
                        format!(
                            " source_path_witnesses={}",
                            origin_facts.source_path_witnesses.len()
                        )
                    })
                    .unwrap_or_default();
                let query_error = origin_facts
                    .query_error
                    .as_ref()
                    .map(|err| format!(" query_error={err}"))
                    .unwrap_or_default();
                let relations = (!origin_facts.relation_counts.is_empty())
                    .then(|| format!(" relations={}", origin_facts.relation_counts.len()))
                    .unwrap_or_default();
                writeln!(
                    out,
                    "    {} {}{} total={} origin_nodes={} origin_links={} source_spans={}{}{}{}{}{}",
                    origin_facts.scope,
                    origin_facts.label,
                    origin_facts
                        .object
                        .as_ref()
                        .map(|object| format!(" object={object}"))
                        .unwrap_or_default(),
                    origin_facts.total,
                    origin_facts.origin_nodes,
                    origin_facts.origin_links,
                    origin_facts.source_spans,
                    reachable_pairs,
                    path_witnesses,
                    source_path_witnesses,
                    query_error,
                    relations,
                )
                .unwrap();
                if let Some(reachability) = origin_facts.reachability.as_ref()
                    && !reachability.reachable_pairs_by_kind().is_empty()
                {
                    write_origin_reachability_pairs(&mut out, reachability);
                }
                if !origin_facts.relation_counts.is_empty() {
                    write_relation_counts(&mut out, &origin_facts.relation_counts);
                }
                if !origin_facts.source_span_files.is_empty() {
                    write_source_span_file_counts(&mut out, &origin_facts.source_span_files);
                }
                if !origin_facts.path_witnesses.is_empty() {
                    writeln!(out, "      paths:").unwrap();
                    for witness in &origin_facts.path_witnesses {
                        write_origin_path_witness(&mut out, witness);
                    }
                }
                if !origin_facts.source_path_witnesses.is_empty() {
                    writeln!(out, "      source paths:").unwrap();
                    for witness in &origin_facts.source_path_witnesses {
                        write_origin_source_path_witness(&mut out, witness);
                    }
                }
            }
        }
        if !target.shapes.is_empty() {
            writeln!(out, "  shapes:").unwrap();
            for shape in &target.shapes {
                let relations = (!shape.relation_counts.is_empty())
                    .then(|| format!(" relations={}", shape.relation_counts.len()))
                    .unwrap_or_default();
                let graph_hashes = shape
                    .graph_hashes
                    .iter()
                    .map(|hash| format!("{}={}", hash.dimension.as_str(), hash.digest_hex))
                    .collect::<Vec<_>>()
                    .join(" ");
                writeln!(
                    out,
                    "    {} {} nodes={} fields={} children={} edges={} trace_events={} data_flows={}{} graph_hashes=[{}]",
                    shape.scope,
                    shape.label,
                    shape.shape_nodes,
                    shape.shape_fields,
                    shape.shape_children,
                    shape.shape_edges,
                    shape.trace_events,
                    shape.data_flows,
                    relations,
                    graph_hashes,
                )
                .unwrap();
                if !shape.relation_counts.is_empty() {
                    write_relation_counts(&mut out, &shape.relation_counts);
                }
            }
        }
        writeln!(out).unwrap();
    }

    out
}

fn write_source_map_breakdown(out: &mut String, source_map: &AnalyzeSourceMapReport) {
    writeln!(
        out,
        "      classification: source={} source_span_invalid={} semantic_span_missing={} runtime_stmt_missing={} runtime_terminator_missing={} runtime_synthetic={} sonatina_synthetic={} sonatina_unmapped={} post_preopt_snapshot_gap={} bytecode_unmapped={}",
        source_map.source,
        source_map.source_span_invalid,
        source_map.semantic_span_missing,
        source_map.runtime_stmt_missing,
        source_map.runtime_terminator_missing,
        source_map.runtime_synthetic,
        source_map.sonatina_synthetic,
        source_map.sonatina_unmapped,
        source_map.post_preopt_snapshot_gap,
        source_map.bytecode_unmapped,
    )
    .unwrap();
}

fn write_source_map_entry(out: &mut String, entry: &BytecodeSourceMapEntry) {
    write!(
        out,
        "        {}:{} {}..{} kind={}",
        entry.object(),
        entry.section(),
        entry.pc_start(),
        entry.pc_end(),
        entry.kind().kind_name(),
    )
    .unwrap();

    match entry.kind() {
        BytecodeSourceMapEntryKind::Source {
            span_kind,
            file,
            start_byte,
            end_byte,
            start_line,
            start_col,
            end_line,
            end_col,
            snippet,
        } => {
            write!(
                out,
                " span_kind={} file={file:?} bytes={}..{} lines={}:{}..{}:{} snippet={:?}",
                (*span_kind).as_str(),
                start_byte,
                end_byte,
                start_line,
                start_col,
                end_line,
                end_col,
                compact_source_snippet(snippet),
            )
            .unwrap();
        }
        kind => {
            if let Some(reason) = kind.reason() {
                write!(out, " reason={reason}").unwrap();
            }
        }
    }

    writeln!(out).unwrap();
}

fn compact_source_snippet(snippet: &str) -> String {
    const MAX_SNIPPET_CHARS: usize = 80;

    let mut compact = snippet.split_whitespace().collect::<Vec<_>>().join(" ");
    if compact.chars().count() > MAX_SNIPPET_CHARS {
        compact = compact.chars().take(MAX_SNIPPET_CHARS - 3).collect();
        compact.push_str("...");
    }
    compact
}

fn write_origin_reachability_pairs(out: &mut String, summary: &OriginReachabilitySummary) {
    let pairs = summary
        .reachable_pairs_by_kind()
        .iter()
        .map(|pair| {
            format!(
                "{}->{}={}",
                pair.from_kind().as_str(),
                pair.to_kind().as_str(),
                pair.reachable_pairs()
            )
        })
        .collect::<Vec<_>>()
        .join(" ");
    writeln!(out, "      reachable kind pairs: {pairs}").unwrap();
}

fn write_relation_counts(out: &mut String, counts: &[TypedFactRelationCount]) {
    let relation_counts = counts
        .iter()
        .map(|count| format!("{}={}", count.relation_name(), count.rows()))
        .collect::<Vec<_>>()
        .join(" ");
    writeln!(out, "      relation counts: {relation_counts}").unwrap();
}

fn write_source_span_file_counts(out: &mut String, counts: &[SourceSpanFileCount]) {
    let source_span_files = counts
        .iter()
        .map(|count| format!("{:?}={}", count.file(), count.spans()))
        .collect::<Vec<_>>()
        .join(" ");
    writeln!(out, "      source span files: {source_span_files}").unwrap();
}

fn write_origin_path_witness(out: &mut String, witness: &OriginPathWitnessExport) {
    writeln!(
        out,
        "        {} -> {}:",
        witness.from_kind().as_str(),
        witness.to_kind().as_str()
    )
    .unwrap();
    write_origin_path_nodes(out, witness);
}

fn write_origin_source_path_witness(out: &mut String, witness: &OriginSourcePathWitnessExport) {
    let span = witness.source_span();
    let path = witness.path();
    writeln!(
        out,
        "        {} -> {} source span_kind={} file={:?} bytes={}..{} lines={}:{}..{}:{}:",
        path.from_kind().as_str(),
        path.to_kind().as_str(),
        span.span_kind().as_str(),
        span.file(),
        span.start_byte(),
        span.end_byte(),
        span.start_line(),
        span.start_col(),
        span.end_line(),
        span.end_col(),
    )
    .unwrap();
    write_origin_path_nodes(out, path);
}

fn write_origin_path_nodes(out: &mut String, witness: &OriginPathWitnessExport) {
    let Some((first, rest)) = witness.nodes().split_first() else {
        writeln!(out, "          <empty>").unwrap();
        return;
    };

    write!(out, "          {}", first.display_label()).unwrap();
    for (idx, node) in rest.iter().enumerate() {
        let link = witness
            .links()
            .get(idx)
            .map(|kind| kind.as_str())
            .unwrap_or("<missing-link>");
        write!(out, " --{}--> {}", link, node.display_label()).unwrap();
    }
    writeln!(out).unwrap();
}

fn write_origin_count(out: &mut String, label: &str, count: OriginCount) {
    writeln!(
        out,
        "{label}: {} (semantic {}, synthetic {})",
        count.total, count.semantic, count.synthetic
    )
    .unwrap();
}

fn ingot_has_source_files(db: &DriverDataBase, ingot: Ingot<'_>) -> bool {
    ingot
        .files(db)
        .iter()
        .any(|(_, file)| matches!(file.kind(db), Some(IngotFileKind::Source)))
}

fn ingot_label(db: &DriverDataBase, ingot: Ingot<'_>, fallback: &Url) -> String {
    ingot
        .config(db)
        .and_then(|config| config.metadata.name)
        .map(|name| name.to_string())
        .unwrap_or_else(|| fallback.to_string())
}

fn config_from_db(db: &DriverDataBase, ingot_url: &Url) -> Result<Option<Config>, String> {
    let config_url = ingot_url
        .join("fe.toml")
        .map_err(|_| format!("Failed to locate fe.toml for {ingot_url}"))?;
    let Some(file) = db.workspace().get(db, &config_url) else {
        return Ok(None);
    };
    let config = Config::parse(file.text(db))
        .map_err(|err| format!("Failed to parse {config_url}: {err}"))?;
    Ok(Some(config))
}

fn dir_url(path: &Utf8PathBuf) -> Result<Url, String> {
    let canonical_path = match path.canonicalize_utf8() {
        Ok(path) => path,
        Err(_) => {
            let cwd = std::env::current_dir()
                .map_err(|err| format!("Failed to read current directory: {err}"))?;
            let cwd = Utf8PathBuf::from_path_buf(cwd)
                .map_err(|_| "Current directory is not valid UTF-8".to_string())?;
            cwd.join(path)
        }
    };
    Url::from_directory_path(canonical_path.as_str())
        .map_err(|_| format!("Error: invalid or non-existent directory path: {path}"))
}

#[cfg(test)]
mod tests {
    use std::fs;

    use camino::Utf8PathBuf;
    use codegen::{
        OptLevel,
        debug::{BytecodeSourceMapEntry, BytecodeSourceMapEntryKind},
    };
    use common::{
        facts::{
            OriginReachabilitySummary, OwnedTypedFactSetExport, TypedFactRelationCount,
            TypedFactRelationName, TypedFactRelationSet, TypedFactSet,
        },
        origin::{OriginExportKind, OriginLinkKind},
        shape::ShapeDimension,
    };
    use tempfile::tempdir;

    use super::{
        AnalyzeOptions, AnalyzeOriginFactReport, AnalyzeReport, AnalyzeShapeHashReport,
        AnalyzeShapeReport, AnalyzeSourceMapReport, OriginCount, OriginCountError,
        analyze_to_string,
    };
    use crate::AnalyzeFormat;

    fn json_options(
        include_tests: bool,
        include_source_maps: bool,
        include_source_map_entries: bool,
        include_origin_facts: bool,
        include_shape_hashes: bool,
        include_shape_facts: bool,
    ) -> AnalyzeOptions<'static> {
        AnalyzeOptions::new(
            "dev",
            AnalyzeFormat::Json,
            include_tests,
            include_source_maps,
            include_source_map_entries,
            include_origin_facts,
            false,
            include_shape_hashes,
            include_shape_facts,
            OptLevel::O0,
            false,
        )
    }

    fn analyze_report(output: &str) -> AnalyzeReport {
        let report = serde_json::from_str::<AnalyzeReport>(output)
            .expect("analyze report should match schema");
        assert_eq!(report.schema_version, 1);
        report
    }

    #[test]
    fn origin_count_roundtrips_through_fail_closed_schema() {
        let count = OriginCount::try_new(3, 2, 1).expect("origin count should validate");
        let json = serde_json::to_string(&count).expect("origin count should serialize");
        let decoded =
            serde_json::from_str::<OriginCount>(&json).expect("origin count should decode");
        assert_eq!(decoded, count);

        assert_eq!(
            OriginCount::try_new(4, 2, 1),
            Err(OriginCountError::TotalMismatch {
                declared: 4,
                actual: 3
            })
        );

        let mismatched_total = r#"{
            "total": 4,
            "semantic": 2,
            "synthetic": 1
        }"#;
        let err = serde_json::from_str::<OriginCount>(mismatched_total)
            .expect_err("origin count JSON should reject inconsistent totals");
        assert!(
            err.to_string()
                .contains("total 4 does not match semantic plus synthetic count 3"),
            "{err}"
        );

        let unknown_field = r#"{
            "total": 3,
            "semantic": 2,
            "synthetic": 1,
            "extra": 0
        }"#;
        let err = serde_json::from_str::<OriginCount>(unknown_field)
            .expect_err("origin count JSON should reject unknown fields");
        assert!(err.to_string().contains("unknown field"), "{err}");

        let mut report = serde_json::json!({
            "schema_version": 1,
            "profile": "dev",
            "package_kind": "single_file",
            "targets": [{
                "label": "target",
                "runtime_bodies": 1,
                "runtime_statements": {
                    "total": 2,
                    "semantic": 2,
                    "synthetic": 1
                },
                "runtime_terminators": {
                    "total": 0,
                    "semantic": 0,
                    "synthetic": 0
                },
                "bodies": [],
                "source_maps": [],
                "origin_facts": [],
                "shapes": []
            }]
        });
        let err = serde_json::from_value::<AnalyzeReport>(report.clone())
            .expect_err("analyze report should reject inconsistent nested origin counts");
        assert!(
            err.to_string()
                .contains("total 2 does not match semantic plus synthetic count 3"),
            "{err}"
        );

        report["targets"][0]["runtime_statements"]["total"] = serde_json::json!(3);
        serde_json::from_value::<AnalyzeReport>(report)
            .expect("analyze report should accept consistent nested origin counts");
    }

    fn source_map_report_value() -> serde_json::Value {
        serde_json::json!({
            "scope": "test_bytecode",
            "label": "test_source_map",
            "test": "test_source_map",
            "object": "test_source_map",
            "section": "runtime",
            "total": 2,
            "source": 1,
            "debug_locations": 1,
            "debug_line_table_files": 1,
            "debug_line_table_rows": 1,
            "non_source": 1,
            "source_span_invalid": 0,
            "semantic_span_missing": 0,
            "runtime_stmt_missing": 0,
            "runtime_terminator_missing": 0,
            "runtime_synthetic": 0,
            "sonatina_synthetic": 0,
            "sonatina_unmapped": 0,
            "post_preopt_snapshot_gap": 0,
            "bytecode_unmapped": 1,
            "bytecode_origin_coverage": {
                "total": 2,
                "sonatina_post_opt": 1,
                "sonatina_backend_prepared": 0,
                "unmapped": 1
            },
            "post_opt_origin_coverage": {
                "total": 1,
                "same_inst_id": 1,
                "created_or_unmatched_after_preopt_snapshot": 0,
                "pre_opt_snapshot_losses": 0,
                "observed_pre_opt_total": 1
            },
            "entries": [{
                "object": "test_source_map",
                "section": "runtime",
                "pc_start": 0,
                "pc_end": 1,
                "kind": "source",
                "span_kind": "original",
                "file": "src/main.fe",
                "start_byte": 0,
                "end_byte": 4,
                "start_line": 0,
                "start_col": 0,
                "end_line": 0,
                "end_col": 4,
                "snippet": "main"
            }, {
                "object": "test_source_map",
                "section": "runtime",
                "pc_start": 1,
                "pc_end": 2,
                "kind": "bytecode_unmapped",
                "reason": "synthetic"
            }]
        })
    }

    fn origin_fact_report_value() -> serde_json::Value {
        serde_json::json!({
            "scope": "runtime",
            "label": "Foo",
            "total": 1,
            "origin_nodes": 1,
            "origin_links": 0,
            "source_spans": 0,
            "facts": {
                "schema_version": 1,
                "facts": [{
                    "type": "origin_node",
                    "id": {"namespace": "origin_node", "ordinal": 0},
                    "key": {
                        "kind": "semantic",
                        "owner_key": "semantic:Foo",
                        "local_key": "expr:0"
                    }
                }]
            }
        })
    }

    fn origin_fact_report_with_source_span_value() -> serde_json::Value {
        serde_json::json!({
            "scope": "runtime_bytecode",
            "label": "Foo",
            "object": "Foo",
            "total": 2,
            "origin_nodes": 1,
            "origin_links": 0,
            "source_spans": 1,
            "source_span_files": [{
                "file": "file:///foo.fe",
                "spans": 1
            }],
            "facts": {
                "schema_version": 1,
                "facts": [{
                    "type": "origin_node",
                    "id": {"namespace": "origin_node", "ordinal": 0},
                    "key": {
                        "kind": "bytecode.pc",
                        "owner_key": "object:Foo:section:runtime",
                        "local_key": "pc:0..1"
                    }
                }, {
                    "type": "source_span",
                    "origin": {"namespace": "origin_node", "ordinal": 0},
                    "span_kind": "original",
                    "file": "file:///foo.fe",
                    "start_byte": 0,
                    "end_byte": 4,
                    "start_line": 0,
                    "start_col": 0,
                    "end_line": 0,
                    "end_col": 4
                }]
            }
        })
    }

    #[test]
    fn analyze_origin_fact_report_roundtrips_through_fail_closed_counts() {
        let value = origin_fact_report_value();
        let report = serde_json::from_value::<AnalyzeOriginFactReport>(value.clone())
            .expect("origin-fact report should decode");
        assert_eq!(report.total, 1);
        assert_eq!(report.origin_nodes, 1);
        assert_eq!(report.origin_links, 0);
        assert_eq!(report.source_spans, 0);

        let json = serde_json::to_string(&report).expect("origin-fact report should serialize");
        serde_json::from_str::<AnalyzeOriginFactReport>(&json)
            .expect("origin-fact report should roundtrip");

        let mut bad_total = value.clone();
        bad_total["total"] = serde_json::json!(2);
        let err = serde_json::from_value::<AnalyzeOriginFactReport>(bad_total)
            .expect_err("origin-fact report should reject inconsistent totals");
        assert!(
            err.to_string()
                .contains("total 2 does not match origin fact count 1"),
            "{err}"
        );

        let mut bad_origin_nodes = value.clone();
        bad_origin_nodes["origin_nodes"] = serde_json::json!(2);
        let err = serde_json::from_value::<AnalyzeOriginFactReport>(bad_origin_nodes)
            .expect_err("origin-fact report should reject inconsistent origin node counts");
        assert!(
            err.to_string()
                .contains("origin_nodes 2 does not match typed fact count 1"),
            "{err}"
        );

        let mut bad_relation_count = value.clone();
        bad_relation_count["relation_counts"] = serde_json::json!([{
            "relation": "origin_node",
            "rows": 2
        }]);
        let err = serde_json::from_value::<AnalyzeOriginFactReport>(bad_relation_count)
            .expect_err("origin-fact report should reject inconsistent relation counts");
        assert!(
            err.to_string()
                .contains("relation count origin_node=2 does not match report count 1"),
            "{err}"
        );

        let mut bad_unexpected_relation = value.clone();
        bad_unexpected_relation["relation_counts"] = serde_json::json!([{
            "relation": "shape_node",
            "rows": 1
        }]);
        let err = serde_json::from_value::<AnalyzeOriginFactReport>(bad_unexpected_relation)
            .expect_err("origin-fact report should reject shape relation counts");
        assert!(
            err.to_string()
                .contains("relation_counts contains non-origin relation shape_node"),
            "{err}"
        );

        let mut with_source_span = origin_fact_report_with_source_span_value();
        serde_json::from_value::<AnalyzeOriginFactReport>(with_source_span.clone())
            .expect("origin-fact report with source span should decode");
        with_source_span["source_span_files"][0]["spans"] = serde_json::json!(2);
        let err = serde_json::from_value::<AnalyzeOriginFactReport>(with_source_span)
            .expect_err("origin-fact report should reject source-span file summary drift");
        assert!(
            err.to_string()
                .contains("source_span_files total 2 does not match source_spans 1"),
            "{err}"
        );
    }

    #[test]
    fn analyze_source_map_report_roundtrips_through_fail_closed_counts() {
        let value = source_map_report_value();
        let report = serde_json::from_value::<AnalyzeSourceMapReport>(value.clone())
            .expect("source-map report should decode");
        assert_eq!(report.total, 2);
        assert_eq!(report.non_source, 1);
        assert_eq!(report.entries.len(), 2);

        let json = serde_json::to_string(&report).expect("source-map report should serialize");
        serde_json::from_str::<AnalyzeSourceMapReport>(&json)
            .expect("source-map report should roundtrip");

        let mut no_entries = value.clone();
        no_entries
            .as_object_mut()
            .expect("report should be an object")
            .remove("entries");
        let decoded = serde_json::from_value::<AnalyzeSourceMapReport>(no_entries)
            .expect("entry rows are optional for compact reports");
        assert!(decoded.entries.is_empty());
        assert_eq!(decoded.total, 2);

        let mut bad_non_source = value.clone();
        bad_non_source["non_source"] = serde_json::json!(2);
        let err = serde_json::from_value::<AnalyzeSourceMapReport>(bad_non_source)
            .expect_err("source-map report should reject inconsistent non-source counts");
        assert!(
            err.to_string()
                .contains("non_source 2 does not match classified non-source count 1"),
            "{err}"
        );

        let mut bad_total = value.clone();
        bad_total["total"] = serde_json::json!(3);
        let err = serde_json::from_value::<AnalyzeSourceMapReport>(bad_total)
            .expect_err("source-map report should reject inconsistent totals");
        assert!(
            err.to_string()
                .contains("total 3 does not match source plus non_source count 2"),
            "{err}"
        );

        let mut bad_debug_locations = value.clone();
        bad_debug_locations["debug_locations"] = serde_json::json!(2);
        let err = serde_json::from_value::<AnalyzeSourceMapReport>(bad_debug_locations)
            .expect_err("source-map report should reject inconsistent debug-location counts");
        assert!(
            err.to_string()
                .contains("debug_locations 2 does not match source count 1"),
            "{err}"
        );

        let mut bad_debug_rows = value.clone();
        bad_debug_rows["debug_line_table_rows"] = serde_json::json!(2);
        let err = serde_json::from_value::<AnalyzeSourceMapReport>(bad_debug_rows)
            .expect_err("source-map report should reject inconsistent line-table rows");
        assert!(
            err.to_string()
                .contains("debug_line_table_rows 2 does not match source count 1"),
            "{err}"
        );

        let mut bad_debug_files = value.clone();
        bad_debug_files["debug_line_table_files"] = serde_json::json!(2);
        let err = serde_json::from_value::<AnalyzeSourceMapReport>(bad_debug_files)
            .expect_err("source-map report should reject impossible line-table file counts");
        assert!(
            err.to_string()
                .contains("debug_line_table_files 2 exceeds source count 1"),
            "{err}"
        );

        let mut bad_coverage_total = value.clone();
        bad_coverage_total["bytecode_origin_coverage"] = serde_json::json!({
            "total": 3,
            "sonatina_post_opt": 2,
            "sonatina_backend_prepared": 0,
            "unmapped": 1
        });
        let err = serde_json::from_value::<AnalyzeSourceMapReport>(bad_coverage_total).expect_err(
            "source-map report should reject coverage totals that do not match report totals",
        );
        assert!(
            err.to_string()
                .contains("total 2 does not match bytecode origin coverage total 3"),
            "{err}"
        );

        let mut bad_entry_count = value.clone();
        bad_entry_count["entries"] = serde_json::json!([bad_entry_count["entries"][0].clone()]);
        let err = serde_json::from_value::<AnalyzeSourceMapReport>(bad_entry_count)
            .expect_err("source-map report should reject entry-count drift");
        assert!(
            err.to_string()
                .contains("total 2 does not match emitted entry count 1"),
            "{err}"
        );

        let mut bad_entry_kind = value.clone();
        bad_entry_kind["entries"][1] = serde_json::json!({
            "object": "test_source_map",
            "section": "runtime",
            "pc_start": 1,
            "pc_end": 2,
            "kind": "semantic_span_missing"
        });
        let err = serde_json::from_value::<AnalyzeSourceMapReport>(bad_entry_kind)
            .expect_err("source-map report should reject entry classification drift");
        assert!(
            err.to_string()
                .contains("semantic_span_missing count 0 does not match emitted entry count 1"),
            "{err}"
        );
    }

    fn typed_facts_from_export(export: &OwnedTypedFactSetExport) -> TypedFactSet {
        assert_eq!(
            export.schema_version(),
            OwnedTypedFactSetExport::SCHEMA_VERSION
        );
        TypedFactSet::new(export.facts().to_vec())
    }

    fn typed_origin_facts(report: &AnalyzeOriginFactReport) -> TypedFactSet {
        typed_facts_from_export(&report.facts)
    }

    fn shape_facts(report: &AnalyzeShapeReport) -> TypedFactSet {
        let facts = report
            .facts
            .as_ref()
            .expect("shape facts should be emitted");
        typed_facts_from_export(facts)
    }

    fn has_origin_node_kind(facts: &TypedFactSet, kind: OriginExportKind) -> bool {
        facts.origin_nodes().any(|fact| fact.key().kind() == kind)
    }

    fn has_origin_link_kind(facts: &TypedFactSet, kind: OriginLinkKind) -> bool {
        facts.origin_links().any(|fact| fact.kind() == kind)
    }

    fn has_source_span_fact(facts: &TypedFactSet) -> bool {
        facts.source_spans().next().is_some()
    }

    fn has_shape_node_fact(facts: &TypedFactSet) -> bool {
        facts.shape_nodes().next().is_some()
    }

    fn has_shape_hash_fact(facts: &TypedFactSet) -> bool {
        facts.shape_hashes().next().is_some()
    }

    fn origin_kind(raw: &str) -> OriginExportKind {
        OriginExportKind::from_str(raw).expect("test should use a known origin kind")
    }

    fn typed_reachability(report: &AnalyzeOriginFactReport) -> &OriginReachabilitySummary {
        report
            .reachability
            .as_ref()
            .expect("report reachability should match typed schema")
    }

    fn has_reachable_kind_pair(
        report: &AnalyzeOriginFactReport,
        from_kind: &str,
        to_kind: &str,
    ) -> bool {
        typed_reachability(report).pair_count(origin_kind(from_kind), origin_kind(to_kind)) > 0
    }

    fn has_path_witness(report: &AnalyzeOriginFactReport, from_kind: &str, to_kind: &str) -> bool {
        let from_kind = origin_kind(from_kind);
        let to_kind = origin_kind(to_kind);
        report.path_witnesses.iter().any(|witness| {
            witness.from_kind() == from_kind
                && witness.to_kind() == to_kind
                && !witness.links().is_empty()
                && witness.nodes().len() >= 2
                && witness
                    .nodes()
                    .first()
                    .is_some_and(|node| node.kind() == from_kind)
                && witness
                    .nodes()
                    .last()
                    .is_some_and(|node| node.kind() == to_kind)
        })
    }

    fn has_source_path_witness(
        report: &AnalyzeOriginFactReport,
        from_kind: &str,
        to_kind: &str,
    ) -> bool {
        let from_kind = origin_kind(from_kind);
        let to_kind = origin_kind(to_kind);
        report.source_path_witnesses.iter().any(|witness| {
            let path = witness.path();
            let span = witness.source_span();
            path.from_kind() == from_kind
                && path.to_kind() == to_kind
                && !path.links().is_empty()
                && path.nodes().len() >= 2
                && path
                    .nodes()
                    .first()
                    .is_some_and(|node| node.kind() == from_kind)
                && path
                    .nodes()
                    .last()
                    .is_some_and(|node| node.kind() == to_kind)
                && span.origin_key().kind() == to_kind
                && !span.file().is_empty()
                && span.start_byte() <= span.end_byte()
        })
    }

    fn has_relation_count(counts: &[TypedFactRelationCount], relation: &str) -> bool {
        counts
            .iter()
            .any(|count| count.relation_name() == relation && count.rows() > 0)
    }

    fn has_relation_table(
        relation_tables: &Option<TypedFactRelationSet>,
        relation: TypedFactRelationName,
    ) -> bool {
        let relation_tables = relation_tables
            .as_ref()
            .expect("report relation tables should match typed schema");
        assert_eq!(
            relation_tables.schema_version(),
            TypedFactRelationSet::SCHEMA_VERSION
        );
        relation_tables
            .relation(relation)
            .is_some_and(|table| !table.rows().is_empty())
    }

    #[test]
    fn analyze_shape_hash_report_uses_closed_dimensions() {
        let valid = r#"{"dimension":"structure","digest_hex":"0000000000000000"}"#;
        let decoded = serde_json::from_str::<AnalyzeShapeHashReport>(valid)
            .expect("known shape hash dimension should decode");
        assert_eq!(decoded.dimension, ShapeDimension::Structure);

        let unknown_dimension = r#"{"dimension":"unknown","digest_hex":"0000000000000000"}"#;
        serde_json::from_str::<AnalyzeShapeHashReport>(unknown_dimension)
            .expect_err("unknown shape hash dimensions should fail closed");

        let unknown_field =
            r#"{"dimension":"structure","digest_hex":"0000000000000000","extra":true}"#;
        let err = serde_json::from_str::<AnalyzeShapeHashReport>(unknown_field)
            .expect_err("shape hash reports should reject unknown fields");
        assert!(err.to_string().contains("unknown field"));
    }

    fn has_source_span_file_count(report: &AnalyzeOriginFactReport) -> bool {
        report
            .source_span_files
            .iter()
            .any(|file| !file.file().is_empty() && file.spans() > 0)
    }

    fn has_typed_source_entry(entries: &[BytecodeSourceMapEntry]) -> bool {
        entries.iter().any(|entry| {
            entry.pc_start() < entry.pc_end()
                && matches!(
                    entry.kind(),
                    BytecodeSourceMapEntryKind::Source { snippet, .. }
                        if !snippet.trim().is_empty()
                )
        })
    }

    fn has_partitioned_bytecode_origin_coverage(report: &AnalyzeSourceMapReport) -> bool {
        let coverage = report
            .bytecode_origin_coverage
            .as_ref()
            .expect("bytecode origin coverage should match typed schema");
        coverage.total() > 0 && coverage.classified_total() == coverage.total()
    }

    fn has_partitioned_post_opt_origin_coverage(report: &AnalyzeSourceMapReport) -> bool {
        let coverage = report
            .post_opt_origin_coverage
            .as_ref()
            .expect("post-opt origin coverage should match typed schema");
        coverage.total() > 0
            && coverage.post_opt_classified_total() == coverage.total()
            && coverage.computed_observed_pre_opt_total() == coverage.observed_pre_opt_total()
    }

    #[test]
    fn analyze_fact_relation_tables_validate_option_dependencies() {
        let missing_facts = AnalyzeOptions::new(
            "dev",
            AnalyzeFormat::Json,
            false,
            false,
            false,
            false,
            true,
            false,
            false,
            OptLevel::O0,
            false,
        )
        .validate()
        .expect_err("relation tables require emitted typed facts");
        assert!(
            missing_facts.contains("requires `--origin-facts` or `--shape-facts`"),
            "{missing_facts}"
        );

        let text_format = AnalyzeOptions::new(
            "dev",
            AnalyzeFormat::Text,
            false,
            false,
            false,
            true,
            true,
            false,
            false,
            OptLevel::O0,
            false,
        )
        .validate()
        .expect_err("relation tables are JSON-only");
        assert!(
            text_format.contains("requires `--format json`"),
            "{text_format}"
        );
    }

    #[test]
    fn analyze_standalone_file_reports_runtime_origin_summary_json() {
        let temp = tempdir().expect("tempdir");
        let file_path = Utf8PathBuf::from_path_buf(temp.path().join("sample.fe")).unwrap();
        fs::write(
            file_path.as_std_path(),
            r#"
fn sample() -> u256 {
    1
}
"#,
        )
        .expect("write fixture");

        let outcome = analyze_to_string(
            &file_path,
            None,
            true,
            json_options(false, false, false, false, false, false),
        )
        .expect("analyze succeeds");

        assert!(!outcome.has_errors);
        let report = analyze_report(&outcome.output);
        assert_eq!(report.targets[0].label, file_path.as_str());
        assert!(
            report.targets[0].runtime_statements.total > 0,
            "expected runtime statement origins in {report:#?}"
        );
    }

    #[test]
    fn analyze_origin_facts_reports_runtime_origin_facts_without_tests() {
        let temp = tempdir().expect("tempdir");
        let file_path = Utf8PathBuf::from_path_buf(temp.path().join("sample.fe")).unwrap();
        fs::write(
            file_path.as_std_path(),
            r#"
fn sample() -> u256 {
    let x: u256 = 1
    x
}
"#,
        )
        .expect("write fixture");

        let outcome = analyze_to_string(
            &file_path,
            None,
            true,
            json_options(false, false, false, true, false, false),
        )
        .expect("analyze succeeds");

        assert!(!outcome.has_errors);
        let report = analyze_report(&outcome.output);
        let origin_facts = &report.targets[0].origin_facts;
        assert!(
            origin_facts.iter().any(|report| {
                if report.scope != "runtime" {
                    return false;
                }
                let facts = typed_origin_facts(report);
                report.origin_nodes > 0
                    && report.origin_links > 0
                    && typed_reachability(report).reachable_pairs() > 0
                    && (has_reachable_kind_pair(report, "semantic", "runtime.stmt")
                        || has_reachable_kind_pair(report, "semantic", "runtime.terminator"))
                    && (has_path_witness(report, "semantic", "runtime.stmt")
                        || has_path_witness(report, "semantic", "runtime.terminator"))
                    && has_relation_count(&report.relation_counts, "origin_node")
                    && has_relation_count(&report.relation_counts, "origin_link")
                    && (has_origin_node_kind(&facts, OriginExportKind::RuntimeStmt)
                        || has_origin_node_kind(&facts, OriginExportKind::RuntimeTerminator))
                    && has_origin_link_kind(&facts, OriginLinkKind::Lowered)
            }),
            "expected typed runtime origin facts in {report:#?}"
        );
    }

    #[test]
    fn analyze_origin_facts_text_renders_path_witnesses() {
        let temp = tempdir().expect("tempdir");
        let file_path = Utf8PathBuf::from_path_buf(temp.path().join("sample.fe")).unwrap();
        fs::write(
            file_path.as_std_path(),
            r#"
fn sample() -> u256 {
    let x: u256 = 1
    x
}
"#,
        )
        .expect("write fixture");

        let outcome = analyze_to_string(
            &file_path,
            None,
            true,
            AnalyzeOptions::new(
                "dev",
                AnalyzeFormat::Text,
                false,
                false,
                false,
                true,
                false,
                false,
                false,
                OptLevel::O0,
                false,
            ),
        )
        .expect("analyze succeeds");

        assert!(!outcome.has_errors);
        assert!(
            outcome.output.contains("      paths:\n"),
            "{}",
            outcome.output
        );
        assert!(
            outcome.output.contains("semantic -> runtime."),
            "{}",
            outcome.output
        );
        assert!(
            outcome.output.contains("--lowered-->"),
            "{}",
            outcome.output
        );
        assert!(
            outcome.output.contains("      relation counts:"),
            "{}",
            outcome.output
        );
        assert!(
            outcome.output.contains("origin_node="),
            "{}",
            outcome.output
        );
        assert!(
            outcome.output.contains("origin_link="),
            "{}",
            outcome.output
        );
        assert!(
            outcome.output.contains("      reachable kind pairs:"),
            "{}",
            outcome.output
        );
        assert!(
            outcome.output.contains("semantic->runtime."),
            "{}",
            outcome.output
        );
    }

    #[test]
    fn analyze_origin_fact_relation_tables_reports_engine_agnostic_rows() {
        let temp = tempdir().expect("tempdir");
        let file_path = Utf8PathBuf::from_path_buf(temp.path().join("sample.fe")).unwrap();
        fs::write(
            file_path.as_std_path(),
            r#"
fn sample() -> u256 {
    let x: u256 = 1
    x
}
"#,
        )
        .expect("write fixture");

        let outcome = analyze_to_string(
            &file_path,
            None,
            true,
            AnalyzeOptions::new(
                "dev",
                AnalyzeFormat::Json,
                false,
                false,
                false,
                true,
                true,
                false,
                false,
                OptLevel::O0,
                false,
            ),
        )
        .expect("analyze succeeds");

        assert!(!outcome.has_errors);
        let report = analyze_report(&outcome.output);
        let origin_facts = &report.targets[0].origin_facts;
        assert!(
            origin_facts.iter().any(|report| {
                report.scope == "runtime"
                    && has_relation_table(
                        &report.relation_tables,
                        TypedFactRelationName::OriginNode,
                    )
                    && has_relation_table(
                        &report.relation_tables,
                        TypedFactRelationName::OriginLink,
                    )
            }),
            "expected runtime origin relation tables in {report:#?}"
        );
    }

    #[test]
    fn analyze_shape_hashes_reports_const_region_shapes() {
        let temp = tempdir().expect("tempdir");
        let file_path = Utf8PathBuf::from_path_buf(temp.path().join("sample.fe")).unwrap();
        fs::write(
            file_path.as_std_path(),
            r#"
const C: [u256; 3] = [10, 20, 30]

fn sample() -> u256 {
    C[1]
}
"#,
        )
        .expect("write fixture");

        let outcome = analyze_to_string(
            &file_path,
            None,
            true,
            json_options(false, false, false, false, true, true),
        )
        .expect("analyze succeeds");

        assert!(!outcome.has_errors);
        let report = analyze_report(&outcome.output);
        let shapes = &report.targets[0].shapes;
        assert!(
            shapes.iter().any(|shape| {
                if shape.scope != "const_region" {
                    return false;
                }
                let facts = shape_facts(shape);
                shape.shape_nodes > 0
                    && shape
                        .graph_hashes
                        .iter()
                        .any(|hash| hash.dimension == ShapeDimension::Constants)
                    && shape
                        .graph_hashes
                        .iter()
                        .any(|hash| hash.dimension == ShapeDimension::Types)
                    && has_relation_count(&shape.relation_counts, "shape_node")
                    && has_relation_count(&shape.relation_counts, "shape_hash")
                    && has_shape_node_fact(&facts)
                    && has_shape_hash_fact(&facts)
            }),
            "expected const-region shape hashes and facts in {report:#?}"
        );
        assert!(
            shapes.iter().any(|shape| {
                if shape.scope != "runtime_body" {
                    return false;
                }
                let facts = shape_facts(shape);
                shape.shape_nodes > 0
                    && shape
                        .graph_hashes
                        .iter()
                        .any(|hash| hash.dimension == ShapeDimension::Structure)
                    && shape
                        .graph_hashes
                        .iter()
                        .any(|hash| hash.dimension == ShapeDimension::Constants)
                    && has_relation_count(&shape.relation_counts, "shape_node")
                    && has_relation_count(&shape.relation_counts, "shape_hash")
                    && has_shape_node_fact(&facts)
                    && has_shape_hash_fact(&facts)
            }),
            "expected runtime body shape hashes and facts in {report:#?}"
        );
    }

    #[test]
    fn analyze_shape_hashes_text_renders_all_graph_dimensions() {
        let temp = tempdir().expect("tempdir");
        let file_path = Utf8PathBuf::from_path_buf(temp.path().join("sample.fe")).unwrap();
        fs::write(
            file_path.as_std_path(),
            r#"
const C: [u256; 3] = [10, 20, 30]

fn sample() -> u256 {
    C[1]
}
"#,
        )
        .expect("write fixture");

        let outcome = analyze_to_string(
            &file_path,
            None,
            true,
            AnalyzeOptions::new(
                "dev",
                AnalyzeFormat::Text,
                false,
                false,
                false,
                false,
                false,
                true,
                true,
                OptLevel::O0,
                false,
            ),
        )
        .expect("analyze succeeds");

        assert!(!outcome.has_errors);
        for dimension in ["structure", "names", "constants", "types", "trace_events"] {
            assert!(
                outcome.output.contains(&format!("{dimension}=")),
                "{}",
                outcome.output
            );
        }
        assert!(
            outcome.output.contains("      relation counts:"),
            "{}",
            outcome.output
        );
        assert!(outcome.output.contains("shape_node="), "{}", outcome.output);
        assert!(outcome.output.contains("shape_hash="), "{}", outcome.output);
    }

    #[test]
    fn analyze_shape_fact_relation_tables_reports_engine_agnostic_rows() {
        let temp = tempdir().expect("tempdir");
        let file_path = Utf8PathBuf::from_path_buf(temp.path().join("sample.fe")).unwrap();
        fs::write(
            file_path.as_std_path(),
            r#"
const C: [u256; 3] = [10, 20, 30]

fn sample() -> u256 {
    C[1]
}
"#,
        )
        .expect("write fixture");

        let outcome = analyze_to_string(
            &file_path,
            None,
            true,
            AnalyzeOptions::new(
                "dev",
                AnalyzeFormat::Json,
                false,
                false,
                false,
                false,
                true,
                true,
                true,
                OptLevel::O0,
                false,
            ),
        )
        .expect("analyze succeeds");

        assert!(!outcome.has_errors);
        let report = analyze_report(&outcome.output);
        let shapes = &report.targets[0].shapes;
        assert!(
            shapes.iter().any(|shape| {
                shape.scope == "const_region"
                    && has_relation_count(&shape.relation_counts, "shape_node")
                    && has_relation_count(&shape.relation_counts, "shape_hash")
                    && has_relation_table(&shape.relation_tables, TypedFactRelationName::ShapeNode)
                    && has_relation_table(&shape.relation_tables, TypedFactRelationName::ShapeHash)
            }),
            "expected shape relation tables in {report:#?}"
        );
    }

    #[test]
    fn analyze_file_inside_ingot_uses_ingot_context_by_default() {
        let temp = tempdir().expect("tempdir");
        let root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
        let src = root.join("src");
        fs::create_dir_all(src.as_std_path()).expect("create src");
        fs::write(
            root.join("fe.toml").as_std_path(),
            "[ingot]\nname = \"analyze_app\"\nversion = \"0.1.0\"\n",
        )
        .expect("write config");
        let file_path = src.join("lib.fe");
        fs::write(
            file_path.as_std_path(),
            r#"
fn sample() -> u256 {
    1
}
"#,
        )
        .expect("write source");

        let outcome = analyze_to_string(
            &file_path,
            None,
            false,
            json_options(false, false, false, false, false, false),
        )
        .expect("analyze succeeds");

        assert!(!outcome.has_errors);
        let report = analyze_report(&outcome.output);
        assert_eq!(report.targets[0].label, "analyze_app");
    }

    #[test]
    fn analyze_tests_mode_reports_test_runtime_origin_summary() {
        let temp = tempdir().expect("tempdir");
        let root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
        let src = root.join("src");
        fs::create_dir_all(src.as_std_path()).expect("create src");
        fs::write(
            root.join("fe.toml").as_std_path(),
            "[ingot]\nname = \"analyze_tests_app\"\nversion = \"0.1.0\"\n",
        )
        .expect("write config");
        let file_path = src.join("lib.fe");
        fs::write(
            file_path.as_std_path(),
            r#"
#[test]
fn test_sample() {
    let x: u256 = 1
}
"#,
        )
        .expect("write source");

        let outcome = analyze_to_string(
            &file_path,
            None,
            false,
            json_options(true, false, false, false, false, false),
        )
        .expect("analyze succeeds");

        assert!(!outcome.has_errors);
        let report = analyze_report(&outcome.output);
        assert_eq!(report.package_kind, "tests");
        assert!(
            report.targets[0].runtime_statements.total > 0,
            "expected test runtime statement origins in {report:#?}"
        );
    }

    #[test]
    fn analyze_source_maps_reports_typed_test_bytecode_summary() {
        let temp = tempdir().expect("tempdir");
        let root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
        let src = root.join("src");
        fs::create_dir_all(src.as_std_path()).expect("create src");
        fs::write(
            root.join("fe.toml").as_std_path(),
            "[ingot]\nname = \"analyze_source_maps_app\"\nversion = \"0.1.0\"\n",
        )
        .expect("write config");
        let file_path = src.join("lib.fe");
        fs::write(
            file_path.as_std_path(),
            r#"
#[test]
fn test_source_map() {
    let x: u256 = 1
    let y: u256 = x + 2
}
"#,
        )
        .expect("write source");

        let outcome = analyze_to_string(
            &file_path,
            None,
            false,
            json_options(true, true, true, false, false, false),
        )
        .expect("analyze succeeds");

        assert!(!outcome.has_errors);
        let report = analyze_report(&outcome.output);
        let source_maps = &report.targets[0].source_maps;
        assert!(
            source_maps.iter().any(|source_map| {
                if source_map.scope != "test_bytecode"
                    || source_map.label != "test_source_map"
                    || source_map.test.as_deref() != Some("test_source_map")
                {
                    return false;
                }
                source_map.total > 0
                    && source_map.source > 0
                    && source_map.debug_locations > 0
                    && source_map.debug_line_table_files > 0
                    && source_map.debug_line_table_rows > 0
                    && has_partitioned_bytecode_origin_coverage(source_map)
                    && has_partitioned_post_opt_origin_coverage(source_map)
                    && has_typed_source_entry(&source_map.entries)
            }),
            "expected source-map summary with full source entries in {report:#?}"
        );
    }

    #[test]
    fn analyze_source_maps_reports_runtime_bytecode_summary_without_tests() {
        let temp = tempdir().expect("tempdir");
        let file_path = Utf8PathBuf::from_path_buf(temp.path().join("sample.fe")).unwrap();
        fs::write(
            file_path.as_std_path(),
            r#"
msg FooMsg {
    #[selector = 0x12345678]
    Ping -> u256,
}

pub contract Foo {
    recv FooMsg {
        Ping -> u256 {
            let x: u256 = 1
            return x + 2
        }
    }
}
"#,
        )
        .expect("write fixture");

        let outcome = analyze_to_string(
            &file_path,
            None,
            true,
            json_options(false, true, true, true, false, false),
        )
        .expect("analyze succeeds");

        assert!(!outcome.has_errors);
        let report = analyze_report(&outcome.output);
        let source_maps = &report.targets[0].source_maps;
        assert!(
            source_maps.iter().any(|source_map| {
                if source_map.scope != "runtime_bytecode"
                    || source_map.label != "Foo"
                    || source_map.object != "Foo"
                {
                    return false;
                }
                source_map.total > 0
                    && source_map.debug_locations > 0
                    && source_map.debug_line_table_files > 0
                    && source_map.debug_line_table_rows > 0
                    && has_partitioned_bytecode_origin_coverage(source_map)
                    && has_partitioned_post_opt_origin_coverage(source_map)
                    && has_typed_source_entry(&source_map.entries)
            }),
            "expected runtime bytecode source-map report in {report:#?}"
        );

        let origin_facts = &report.targets[0].origin_facts;
        assert!(
            origin_facts.iter().any(|report| {
                if report.scope != "runtime_bytecode"
                    || report.label != "Foo"
                    || report.object.as_deref() != Some("Foo")
                {
                    return false;
                }
                let facts = typed_origin_facts(report);
                has_path_witness(report, "semantic", "bytecode.pc")
                    && has_source_path_witness(report, "semantic", "bytecode.pc")
                    && (has_reachable_kind_pair(report, "runtime.stmt", "bytecode.pc")
                        || has_reachable_kind_pair(report, "runtime.terminator", "bytecode.pc"))
                    && (has_path_witness(report, "runtime.stmt", "bytecode.pc")
                        || has_path_witness(report, "runtime.terminator", "bytecode.pc"))
                    && (has_source_path_witness(report, "runtime.stmt", "bytecode.pc")
                        || has_source_path_witness(report, "runtime.terminator", "bytecode.pc"))
                    && has_source_span_file_count(report)
                    && has_origin_node_kind(&facts, OriginExportKind::BytecodePc)
                    && has_source_span_fact(&facts)
            }),
            "expected runtime bytecode origin facts in {report:#?}"
        );
        assert!(
            origin_facts.iter().any(|report| {
                if report.scope != "runtime_sonatina_snapshot"
                    || report.label != "Foo"
                    || report.object.as_deref() != Some("Foo")
                {
                    return false;
                }
                let facts = typed_origin_facts(report);
                has_origin_node_kind(&facts, OriginExportKind::SonatinaInst)
                    || has_origin_node_kind(&facts, OriginExportKind::SonatinaSynthetic)
            }),
            "expected runtime Sonatina snapshot origin facts in {report:#?}"
        );
    }

    #[test]
    fn analyze_source_maps_text_renders_classification_and_entries() {
        let temp = tempdir().expect("tempdir");
        let file_path = Utf8PathBuf::from_path_buf(temp.path().join("sample.fe")).unwrap();
        fs::write(
            file_path.as_std_path(),
            r#"
msg FooMsg {
    #[selector = 0x12345678]
    Ping -> u256,
}

pub contract Foo {
    recv FooMsg {
        Ping -> u256 {
            let x: u256 = 1
            return x + 2
        }
    }
}
"#,
        )
        .expect("write fixture");

        let outcome = analyze_to_string(
            &file_path,
            None,
            true,
            AnalyzeOptions::new(
                "dev",
                AnalyzeFormat::Text,
                false,
                true,
                true,
                true,
                false,
                false,
                false,
                OptLevel::O0,
                false,
            ),
        )
        .expect("analyze succeeds");

        assert!(!outcome.has_errors);
        for expected in [
            "  source maps:\n",
            "classification: source=",
            "debug_locations=",
            "debug_line_table_files=",
            "debug_line_table_rows=",
            "post_opt_origins=",
            "source_span_invalid=",
            "semantic_span_missing=",
            "runtime_stmt_missing=",
            "runtime_terminator_missing=",
            "sonatina_unmapped=",
            "      entries:\n",
            "kind=source",
            "snippet=",
            "      source span files:",
            "      source paths:\n",
            "source span_kind=original",
        ] {
            assert!(outcome.output.contains(expected), "{}", outcome.output);
        }
    }

    #[test]
    fn analyze_origin_facts_reports_typed_test_bytecode_facts() {
        let temp = tempdir().expect("tempdir");
        let root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
        let src = root.join("src");
        fs::create_dir_all(src.as_std_path()).expect("create src");
        fs::write(
            root.join("fe.toml").as_std_path(),
            "[ingot]\nname = \"analyze_origin_facts_app\"\nversion = \"0.1.0\"\n",
        )
        .expect("write config");
        let file_path = src.join("lib.fe");
        fs::write(
            file_path.as_std_path(),
            r#"
#[test]
fn test_origin_facts() {
    let x: u256 = 1
    let y: u256 = x + 2
}
"#,
        )
        .expect("write source");

        let outcome = analyze_to_string(
            &file_path,
            None,
            false,
            json_options(true, false, false, true, false, false),
        )
        .expect("analyze succeeds");

        assert!(!outcome.has_errors);
        let report = analyze_report(&outcome.output);
        let origin_facts = &report.targets[0].origin_facts;
        assert!(
            origin_facts
                .iter()
                .any(|report| { report.scope == "runtime" && report.origin_links > 0 }),
            "expected runtime origin facts alongside test bytecode facts in {report:#?}"
        );
        assert!(
            origin_facts.iter().any(|report| {
                if report.scope != "test_bytecode" || report.label != "test_origin_facts" {
                    return false;
                }
                let facts = typed_origin_facts(report);
                report.origin_nodes > 0
                    && report.origin_links > 0
                    && report.source_spans > 0
                    && has_source_span_file_count(report)
                    && has_relation_count(&report.relation_counts, "origin_node")
                    && has_relation_count(&report.relation_counts, "origin_link")
                    && has_relation_count(&report.relation_counts, "source_span")
                    && (has_reachable_kind_pair(report, "runtime.stmt", "bytecode.pc")
                        || has_reachable_kind_pair(report, "runtime.terminator", "bytecode.pc"))
                    && (has_path_witness(report, "runtime.stmt", "bytecode.pc")
                        || has_path_witness(report, "runtime.terminator", "bytecode.pc"))
                    && (has_source_path_witness(report, "runtime.stmt", "bytecode.pc")
                        || has_source_path_witness(report, "runtime.terminator", "bytecode.pc"))
                    && has_origin_node_kind(&facts, OriginExportKind::BytecodePc)
                    && (has_origin_link_kind(&facts, OriginLinkKind::Lowered)
                        || has_origin_link_kind(&facts, OriginLinkKind::Transformed))
                    && has_source_span_fact(&facts)
            }),
            "expected typed origin facts for test bytecode in {report:#?}"
        );
        assert!(
            origin_facts.iter().any(|report| {
                if report.scope != "test_sonatina_snapshot"
                    || report.label != "test_origin_facts"
                    || report.object.as_deref() != Some("test_origin_facts")
                {
                    return false;
                }
                let facts = typed_origin_facts(report);
                report.origin_nodes > 0
                    && report.origin_links > 0
                    && has_origin_node_kind(&facts, OriginExportKind::SonatinaInst)
                    && has_origin_link_kind(&facts, OriginLinkKind::Alias)
            }),
            "expected typed Sonatina snapshot origin facts for test bytecode in {report:#?}"
        );
    }
}
