use codegen::debug::{
    BytecodeOriginCoverageExport, BytecodeSourceMapEntry, BytecodeSourceMapSummary,
    SonatinaPostOptOriginCoverageExport, bytecode_source_map_entries_summary,
};
use codegen::origin::{BytecodeOriginCoverage, SonatinaPostOptOriginCoverage};
use serde::{Deserialize, Deserializer, Serialize, de};

use super::validation::EmptyAnalyzeReportField;
use super::{ANALYZE_SOURCE_MAP_ALL_SECTIONS, validation::validate_non_empty_report_field};

#[derive(Debug, Serialize)]
pub(in crate::analyze) struct AnalyzeSourceMapReport {
    pub(in crate::analyze) scope: String,
    pub(in crate::analyze) label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(in crate::analyze) test: Option<String>,
    pub(in crate::analyze) object: String,
    pub(in crate::analyze) section: String,
    pub(in crate::analyze) total: usize,
    pub(in crate::analyze) source: usize,
    pub(in crate::analyze) debug_locations: usize,
    pub(in crate::analyze) debug_line_table_files: usize,
    pub(in crate::analyze) debug_line_table_rows: usize,
    pub(in crate::analyze) non_source: usize,
    pub(in crate::analyze) source_span_invalid: usize,
    pub(in crate::analyze) semantic_span_missing: usize,
    pub(in crate::analyze) runtime_stmt_missing: usize,
    pub(in crate::analyze) runtime_terminator_missing: usize,
    pub(in crate::analyze) runtime_synthetic: usize,
    pub(in crate::analyze) sonatina_synthetic: usize,
    pub(in crate::analyze) sonatina_unmapped: usize,
    pub(in crate::analyze) post_preopt_snapshot_gap: usize,
    pub(in crate::analyze) bytecode_unmapped: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(in crate::analyze) bytecode_origin_coverage: Option<BytecodeOriginCoverageExport>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(in crate::analyze) post_opt_origin_coverage: Option<SonatinaPostOptOriginCoverageExport>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(in crate::analyze) entries: Vec<BytecodeSourceMapEntry>,
}

impl AnalyzeSourceMapReport {
    pub(in crate::analyze) fn try_from_summary(
        scope: &'static str,
        label: String,
        test: Option<String>,
        object: String,
        section: String,
        summary: &BytecodeSourceMapSummary,
        bytecode_origin_coverage: Option<BytecodeOriginCoverage>,
        post_opt_origin_coverage: Option<SonatinaPostOptOriginCoverage>,
        entries: Vec<BytecodeSourceMapEntry>,
    ) -> Result<Self, AnalyzeSourceMapReportError> {
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
        report.validate()?;
        Ok(report)
    }

    pub(in crate::analyze) fn validate(&self) -> Result<(), AnalyzeSourceMapReportError> {
        validate_non_empty_report_field("analyze source-map", "scope", &self.scope)
            .map_err(AnalyzeSourceMapReportError::EmptyField)?;
        validate_non_empty_report_field("analyze source-map", "label", &self.label)
            .map_err(AnalyzeSourceMapReportError::EmptyField)?;
        if let Some(test) = &self.test {
            validate_non_empty_report_field("analyze source-map", "test", test)
                .map_err(AnalyzeSourceMapReportError::EmptyField)?;
        }
        validate_non_empty_report_field("analyze source-map", "object", &self.object)
            .map_err(AnalyzeSourceMapReportError::EmptyField)?;
        validate_non_empty_report_field("analyze source-map", "section", &self.section)
            .map_err(AnalyzeSourceMapReportError::EmptyField)?;

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
            self.validate_entry_identity()?;
            let summary = bytecode_source_map_entries_summary(&self.entries, None)
                .expect("non-empty source-map entries should produce a summary");
            self.validate_entry_count("source", self.source, summary.source())?;
            self.validate_entry_count(
                "debug_line_table_files",
                self.debug_line_table_files,
                summary.debug_line_table_files(),
            )?;
            self.validate_entry_count(
                "debug_line_table_rows",
                self.debug_line_table_rows,
                summary.debug_line_table_rows(),
            )?;
            self.validate_entry_count(
                "source_span_invalid",
                self.source_span_invalid,
                summary.source_span_invalid(),
            )?;
            self.validate_entry_count(
                "semantic_span_missing",
                self.semantic_span_missing,
                summary.semantic_span_missing(),
            )?;
            self.validate_entry_count(
                "runtime_stmt_missing",
                self.runtime_stmt_missing,
                summary.runtime_stmt_missing(),
            )?;
            self.validate_entry_count(
                "runtime_terminator_missing",
                self.runtime_terminator_missing,
                summary.runtime_terminator_missing(),
            )?;
            self.validate_entry_count(
                "runtime_synthetic",
                self.runtime_synthetic,
                summary.runtime_synthetic(),
            )?;
            self.validate_entry_count(
                "sonatina_synthetic",
                self.sonatina_synthetic,
                summary.sonatina_synthetic(),
            )?;
            self.validate_entry_count(
                "sonatina_unmapped",
                self.sonatina_unmapped,
                summary.sonatina_unmapped(),
            )?;
            self.validate_entry_count(
                "post_preopt_snapshot_gap",
                self.post_preopt_snapshot_gap,
                summary.post_preopt_snapshot_gap(),
            )?;
            self.validate_entry_count(
                "bytecode_unmapped",
                self.bytecode_unmapped,
                summary.bytecode_unmapped(),
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

    fn validate_entry_identity(&self) -> Result<(), AnalyzeSourceMapReportError> {
        for entry in &self.entries {
            if entry.object() != self.object {
                return Err(AnalyzeSourceMapReportError::EntryObjectMismatch {
                    report_object: self.object.clone(),
                    entry_object: entry.object().to_string(),
                });
            }
            if self.section != ANALYZE_SOURCE_MAP_ALL_SECTIONS && entry.section() != self.section {
                return Err(AnalyzeSourceMapReportError::EntrySectionMismatch {
                    report_section: self.section.clone(),
                    entry_section: entry.section().to_string(),
                });
            }
        }
        Ok(())
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::analyze) enum AnalyzeSourceMapReportError {
    EmptyField(EmptyAnalyzeReportField),
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
    EntryObjectMismatch {
        report_object: String,
        entry_object: String,
    },
    EntrySectionMismatch {
        report_section: String,
        entry_section: String,
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
            Self::EmptyField(err) => err.fmt(f),
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
            Self::EntryObjectMismatch {
                report_object,
                entry_object,
            } => write!(
                f,
                "analyze source-map object `{report_object}` does not match entry object `{entry_object}`"
            ),
            Self::EntrySectionMismatch {
                report_section,
                entry_section,
            } => write!(
                f,
                "analyze source-map section `{report_section}` does not match entry section `{entry_section}`"
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
