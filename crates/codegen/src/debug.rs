use std::{collections::BTreeSet, fmt};

use common::{
    InputDb,
    facts::{SourceSpanExport, SourceSpanKind},
};
use serde::{Deserialize, Deserializer, Serialize, de};

use crate::origin::{
    BytecodeObjectKey, BytecodePcOrigin, BytecodePcRange, BytecodeSourceResolution,
    BytecodeUnmappedReason, SonatinaSyntheticOrigin, SonatinaUnmappedReason,
};

mod artifacts;
mod coverage;
mod line_table;
mod location;
mod source_map_options;
mod source_spans;

pub use artifacts::{
    BytecodeDebugArtifactJson, BytecodeDebugArtifactKind, BytecodeDebugArtifactMetadataMismatch,
    BytecodeDebugArtifactsExport, BytecodeDebugArtifactsExportError, BytecodeDebugArtifactsJson,
    BytecodeDebugArtifactsJsonError, bytecode_debug_artifacts_export,
    bytecode_debug_artifacts_json,
};
pub use coverage::{BytecodeOriginCoverageExport, SonatinaPostOptOriginCoverageExport};
pub use line_table::{
    BytecodeDebugLineRecord, BytecodeDebugLineRow, BytecodeDebugLineTable, BytecodeDebugSourceFile,
    OwnedBytecodeDebugLineTableExport,
};
pub use location::{BytecodeDebugLocationEntry, OwnedBytecodeDebugLocationExport};
pub use source_map_options::{
    BytecodeSourceMapExportMetadata, BytecodeSourceMapExportOptions, BytecodeSourceMapFilter,
};

use source_spans::{bytecode_source_span_exports_for_resolutions, source_map_entry_for_resolution};

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct BytecodeSourceMapEntry {
    object: String,
    section: String,
    pc_start: u32,
    pc_end: u32,
    #[serde(flatten)]
    kind: BytecodeSourceMapEntryKind,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BytecodeSourceMapEntryError {
    EmptyObject,
    EmptySection,
    InvalidPcRange {
        pc_start: u32,
        pc_end: u32,
    },
    EmptySourceFile,
    EmptySourceSnippet,
    InvalidSourceByteRange {
        start_byte: usize,
        end_byte: usize,
    },
    InvalidSourcePositionRange {
        start_line: usize,
        start_col: usize,
        end_line: usize,
        end_col: usize,
    },
}

impl fmt::Display for BytecodeSourceMapEntryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyObject => write!(f, "bytecode source-map object must not be empty"),
            Self::EmptySection => write!(f, "bytecode source-map section must not be empty"),
            Self::InvalidPcRange { pc_start, pc_end } => {
                write!(
                    f,
                    "invalid bytecode source-map PC range {pc_start}..{pc_end}"
                )
            }
            Self::EmptySourceFile => write!(f, "bytecode source-map source file must not be empty"),
            Self::EmptySourceSnippet => {
                write!(f, "bytecode source-map source snippet must not be empty")
            }
            Self::InvalidSourceByteRange {
                start_byte,
                end_byte,
            } => write!(
                f,
                "bytecode source-map source entry has invalid byte range {start_byte}..{end_byte}"
            ),
            Self::InvalidSourcePositionRange {
                start_line,
                start_col,
                end_line,
                end_col,
            } => write!(
                f,
                "bytecode source-map source entry has invalid line/column range {start_line}:{start_col}..{end_line}:{end_col}"
            ),
        }
    }
}

impl std::error::Error for BytecodeSourceMapEntryError {}

impl From<BytecodeSourceMapEntrySemanticError> for BytecodeSourceMapEntryError {
    fn from(err: BytecodeSourceMapEntrySemanticError) -> Self {
        match err {
            BytecodeSourceMapEntrySemanticError::UnknownSourceSpanKind(_) => {
                unreachable!("typed source-map entries store closed source span kinds")
            }
            BytecodeSourceMapEntrySemanticError::UnknownReason { .. } => {
                unreachable!("typed source-map entries store closed reason enums")
            }
            BytecodeSourceMapEntrySemanticError::EmptySourceFile => Self::EmptySourceFile,
            BytecodeSourceMapEntrySemanticError::EmptySourceSnippet => Self::EmptySourceSnippet,
            BytecodeSourceMapEntrySemanticError::InvalidSourceByteRange {
                start_byte,
                end_byte,
            } => Self::InvalidSourceByteRange {
                start_byte,
                end_byte,
            },
            BytecodeSourceMapEntrySemanticError::InvalidSourcePositionRange {
                start_line,
                start_col,
                end_line,
                end_col,
            } => Self::InvalidSourcePositionRange {
                start_line,
                start_col,
                end_line,
                end_col,
            },
        }
    }
}

impl BytecodeSourceMapEntry {
    fn from_serialized_parts(
        object: impl Into<String>,
        section: impl Into<String>,
        pc_start: u32,
        pc_end: u32,
        kind: BytecodeSourceMapEntryKind,
    ) -> Result<Self, BytecodeSourceMapEntryError> {
        let object = object.into();
        let section = section.into();
        if object.is_empty() {
            return Err(BytecodeSourceMapEntryError::EmptyObject);
        }
        if section.is_empty() {
            return Err(BytecodeSourceMapEntryError::EmptySection);
        }
        if BytecodePcRange::new(pc_start, pc_end).is_none() {
            return Err(BytecodeSourceMapEntryError::InvalidPcRange { pc_start, pc_end });
        }
        validate_source_map_entry_kind(&kind).map_err(BytecodeSourceMapEntryError::from)?;
        Ok(Self {
            object,
            section,
            pc_start,
            pc_end,
            kind,
        })
    }

    pub fn try_from_origin(
        origin: &BytecodePcOrigin,
        kind: BytecodeSourceMapEntryKind,
    ) -> Result<Self, BytecodeSourceMapEntryError> {
        let range = origin.range();
        Self::from_serialized_parts(
            origin.section().object().as_str(),
            origin.section().section(),
            range.start(),
            range.end(),
            kind,
        )
    }

    pub fn from_origin(origin: &BytecodePcOrigin, kind: BytecodeSourceMapEntryKind) -> Self {
        Self::try_from_origin(origin, kind).unwrap_or_else(|err| panic!("{err}"))
    }

    pub fn object(&self) -> &str {
        &self.object
    }

    pub fn section(&self) -> &str {
        &self.section
    }

    pub const fn pc_start(&self) -> u32 {
        self.pc_start
    }

    pub const fn pc_end(&self) -> u32 {
        self.pc_end
    }

    pub const fn kind(&self) -> &BytecodeSourceMapEntryKind {
        &self.kind
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum BytecodeSourceMapEntrySemanticError {
    UnknownSourceSpanKind(String),
    UnknownReason {
        kind: &'static str,
        reason: String,
    },
    EmptySourceFile,
    EmptySourceSnippet,
    InvalidSourceByteRange {
        start_byte: usize,
        end_byte: usize,
    },
    InvalidSourcePositionRange {
        start_line: usize,
        start_col: usize,
        end_line: usize,
        end_col: usize,
    },
}

impl fmt::Display for BytecodeSourceMapEntrySemanticError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownSourceSpanKind(kind) => {
                write!(f, "unknown bytecode source-map source span kind `{kind}`")
            }
            Self::UnknownReason { kind, reason } => {
                write!(f, "unknown bytecode source-map {kind} reason `{reason}`")
            }
            Self::EmptySourceFile => write!(f, "bytecode source-map source file must not be empty"),
            Self::EmptySourceSnippet => {
                write!(f, "bytecode source-map source snippet must not be empty")
            }
            Self::InvalidSourceByteRange {
                start_byte,
                end_byte,
            } => write!(
                f,
                "bytecode source-map source entry has invalid byte range {start_byte}..{end_byte}"
            ),
            Self::InvalidSourcePositionRange {
                start_line,
                start_col,
                end_line,
                end_col,
            } => write!(
                f,
                "bytecode source-map source entry has invalid line/column range {start_line}:{start_col}..{end_line}:{end_col}"
            ),
        }
    }
}

common::define_closed_string_enum! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    pub enum SourceSpanInvalidReason {
        InvalidByteRange => "invalid_byte_range",
        InvalidSnippetRange => "invalid_snippet_range",
        EmptySnippet => "empty_snippet",
    }
}

fn validate_source_map_entry_kind(
    kind: &BytecodeSourceMapEntryKind,
) -> Result<(), BytecodeSourceMapEntrySemanticError> {
    match kind {
        BytecodeSourceMapEntryKind::Source {
            span_kind: _,
            file,
            start_byte,
            end_byte,
            start_line,
            start_col,
            end_line,
            end_col,
            snippet,
        } => {
            if file.is_empty() {
                return Err(BytecodeSourceMapEntrySemanticError::EmptySourceFile);
            }
            if snippet.is_empty() {
                return Err(BytecodeSourceMapEntrySemanticError::EmptySourceSnippet);
            }
            if start_byte > end_byte {
                return Err(
                    BytecodeSourceMapEntrySemanticError::InvalidSourceByteRange {
                        start_byte: *start_byte,
                        end_byte: *end_byte,
                    },
                );
            }
            if start_line > end_line || (start_line == end_line && start_col > end_col) {
                return Err(
                    BytecodeSourceMapEntrySemanticError::InvalidSourcePositionRange {
                        start_line: *start_line,
                        start_col: *start_col,
                        end_line: *end_line,
                        end_col: *end_col,
                    },
                );
            }
        }
        BytecodeSourceMapEntryKind::SourceSpanInvalid { .. }
        | BytecodeSourceMapEntryKind::SonatinaSynthetic { .. }
        | BytecodeSourceMapEntryKind::SonatinaUnmapped { .. }
        | BytecodeSourceMapEntryKind::BytecodeUnmapped { .. }
        | BytecodeSourceMapEntryKind::SemanticSpanMissing
        | BytecodeSourceMapEntryKind::RuntimeStmtMissing
        | BytecodeSourceMapEntryKind::RuntimeTerminatorMissing
        | BytecodeSourceMapEntryKind::RuntimeSynthetic
        | BytecodeSourceMapEntryKind::PostPreOptSnapshotGap => {}
    }

    Ok(())
}

impl<'de> Deserialize<'de> for BytecodeSourceMapEntry {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct SourceFields {
            span_kind: Option<String>,
            file: Option<String>,
            start_byte: Option<usize>,
            end_byte: Option<usize>,
            start_line: Option<usize>,
            start_col: Option<usize>,
            end_line: Option<usize>,
            end_col: Option<usize>,
            snippet: Option<String>,
        }

        impl SourceFields {
            fn into_source<E>(self) -> Result<BytecodeSourceMapEntryKind, E>
            where
                E: de::Error,
            {
                let span_kind = required(self.span_kind, "span_kind")?;
                let span_kind = SourceSpanKind::from_str(&span_kind).ok_or_else(|| {
                    de::Error::custom(BytecodeSourceMapEntrySemanticError::UnknownSourceSpanKind(
                        span_kind,
                    ))
                })?;

                Ok(BytecodeSourceMapEntryKind::Source {
                    span_kind,
                    file: required(self.file, "file")?,
                    start_byte: required(self.start_byte, "start_byte")?,
                    end_byte: required(self.end_byte, "end_byte")?,
                    start_line: required(self.start_line, "start_line")?,
                    start_col: required(self.start_col, "start_col")?,
                    end_line: required(self.end_line, "end_line")?,
                    end_col: required(self.end_col, "end_col")?,
                    snippet: required(self.snippet, "snippet")?,
                })
            }

            fn reject_for<E>(self, kind: &'static str) -> Result<(), E>
            where
                E: de::Error,
            {
                reject_present(self.span_kind, "span_kind", kind)?;
                reject_present(self.file, "file", kind)?;
                reject_present(self.start_byte, "start_byte", kind)?;
                reject_present(self.end_byte, "end_byte", kind)?;
                reject_present(self.start_line, "start_line", kind)?;
                reject_present(self.start_col, "start_col", kind)?;
                reject_present(self.end_line, "end_line", kind)?;
                reject_present(self.end_col, "end_col", kind)?;
                reject_present(self.snippet, "snippet", kind)
            }
        }

        fn required<T, E>(field: Option<T>, name: &'static str) -> Result<T, E>
        where
            E: de::Error,
        {
            field.ok_or_else(|| de::Error::missing_field(name))
        }

        fn reject_present<T, E>(
            field: Option<T>,
            name: &'static str,
            kind: &'static str,
        ) -> Result<(), E>
        where
            E: de::Error,
        {
            if field.is_some() {
                Err(de::Error::custom(format!(
                    "unexpected field `{name}` for {kind} source-map entry"
                )))
            } else {
                Ok(())
            }
        }

        #[derive(Deserialize)]
        #[serde(rename_all = "snake_case")]
        enum RawEntryKind {
            Source,
            SourceSpanInvalid,
            SemanticSpanMissing,
            RuntimeStmtMissing,
            RuntimeTerminatorMissing,
            RuntimeSynthetic,
            SonatinaSynthetic,
            SonatinaUnmapped,
            PostPreOptSnapshotGap,
            BytecodeUnmapped,
        }

        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct RawEntry {
            object: String,
            section: String,
            pc_start: u32,
            pc_end: u32,
            kind: RawEntryKind,
            span_kind: Option<String>,
            file: Option<String>,
            start_byte: Option<usize>,
            end_byte: Option<usize>,
            start_line: Option<usize>,
            start_col: Option<usize>,
            end_line: Option<usize>,
            end_col: Option<usize>,
            snippet: Option<String>,
            reason: Option<String>,
        }

        let RawEntry {
            object,
            section,
            pc_start,
            pc_end,
            kind,
            span_kind,
            file,
            start_byte,
            end_byte,
            start_line,
            start_col,
            end_line,
            end_col,
            snippet,
            reason,
        } = RawEntry::deserialize(deserializer)?;

        let source_fields = SourceFields {
            span_kind,
            file,
            start_byte,
            end_byte,
            start_line,
            start_col,
            end_line,
            end_col,
            snippet,
        };

        let kind = match kind {
            RawEntryKind::Source => {
                reject_present(reason, "reason", "source")?;
                source_fields.into_source()?
            }
            RawEntryKind::SourceSpanInvalid => {
                source_fields.reject_for("source_span_invalid")?;
                let reason = required(reason, "reason")?;
                let reason = SourceSpanInvalidReason::from_str(&reason).ok_or_else(|| {
                    de::Error::custom(BytecodeSourceMapEntrySemanticError::UnknownReason {
                        kind: "source_span_invalid",
                        reason,
                    })
                })?;
                BytecodeSourceMapEntryKind::SourceSpanInvalid { reason }
            }
            RawEntryKind::SemanticSpanMissing => {
                source_fields.reject_for("semantic_span_missing")?;
                reject_present(reason, "reason", "semantic_span_missing")?;
                BytecodeSourceMapEntryKind::SemanticSpanMissing
            }
            RawEntryKind::RuntimeStmtMissing => {
                source_fields.reject_for("runtime_stmt_missing")?;
                reject_present(reason, "reason", "runtime_stmt_missing")?;
                BytecodeSourceMapEntryKind::RuntimeStmtMissing
            }
            RawEntryKind::RuntimeTerminatorMissing => {
                source_fields.reject_for("runtime_terminator_missing")?;
                reject_present(reason, "reason", "runtime_terminator_missing")?;
                BytecodeSourceMapEntryKind::RuntimeTerminatorMissing
            }
            RawEntryKind::RuntimeSynthetic => {
                source_fields.reject_for("runtime_synthetic")?;
                reject_present(reason, "reason", "runtime_synthetic")?;
                BytecodeSourceMapEntryKind::RuntimeSynthetic
            }
            RawEntryKind::SonatinaSynthetic => {
                source_fields.reject_for("sonatina_synthetic")?;
                let reason = required(reason, "reason")?;
                let reason = SonatinaSyntheticOrigin::from_str(&reason).ok_or_else(|| {
                    de::Error::custom(BytecodeSourceMapEntrySemanticError::UnknownReason {
                        kind: "sonatina_synthetic",
                        reason,
                    })
                })?;
                BytecodeSourceMapEntryKind::SonatinaSynthetic { reason }
            }
            RawEntryKind::SonatinaUnmapped => {
                source_fields.reject_for("sonatina_unmapped")?;
                let reason = required(reason, "reason")?;
                let reason = SonatinaUnmappedReason::from_str(&reason).ok_or_else(|| {
                    de::Error::custom(BytecodeSourceMapEntrySemanticError::UnknownReason {
                        kind: "sonatina_unmapped",
                        reason,
                    })
                })?;
                BytecodeSourceMapEntryKind::SonatinaUnmapped { reason }
            }
            RawEntryKind::PostPreOptSnapshotGap => {
                source_fields.reject_for("post_preopt_snapshot_gap")?;
                reject_present(reason, "reason", "post_preopt_snapshot_gap")?;
                BytecodeSourceMapEntryKind::PostPreOptSnapshotGap
            }
            RawEntryKind::BytecodeUnmapped => {
                source_fields.reject_for("bytecode_unmapped")?;
                let reason = required(reason, "reason")?;
                let reason = BytecodeUnmappedReason::from_str(&reason).ok_or_else(|| {
                    de::Error::custom(BytecodeSourceMapEntrySemanticError::UnknownReason {
                        kind: "bytecode_unmapped",
                        reason,
                    })
                })?;
                BytecodeSourceMapEntryKind::BytecodeUnmapped { reason }
            }
        };

        Self::from_serialized_parts(object, section, pc_start, pc_end, kind)
            .map_err(de::Error::custom)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum BytecodeSourceMapEntryKind {
    Source {
        span_kind: SourceSpanKind,
        file: String,
        start_byte: usize,
        end_byte: usize,
        start_line: usize,
        start_col: usize,
        end_line: usize,
        end_col: usize,
        snippet: String,
    },
    SourceSpanInvalid {
        reason: SourceSpanInvalidReason,
    },
    SemanticSpanMissing,
    RuntimeStmtMissing,
    RuntimeTerminatorMissing,
    RuntimeSynthetic,
    SonatinaSynthetic {
        reason: SonatinaSyntheticOrigin,
    },
    SonatinaUnmapped {
        reason: SonatinaUnmappedReason,
    },
    PostPreOptSnapshotGap,
    BytecodeUnmapped {
        reason: BytecodeUnmappedReason,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct OwnedBytecodeSourceMapExport {
    schema_version: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    object: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    section: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    bytecode_origin_coverage: Option<BytecodeOriginCoverageExport>,
    #[serde(skip_serializing_if = "Option::is_none")]
    post_opt_origin_coverage: Option<SonatinaPostOptOriginCoverageExport>,
    entries: Vec<BytecodeSourceMapEntry>,
}

impl OwnedBytecodeSourceMapExport {
    pub const SCHEMA_VERSION: u32 = 3;

    pub fn try_new(
        metadata: Option<BytecodeSourceMapExportMetadata<'_>>,
        entries: Vec<BytecodeSourceMapEntry>,
    ) -> Result<Self, BytecodeSourceMapExportEntryError> {
        Self::from_serialized_parts(
            metadata.map(|metadata| metadata.object_name().to_owned()),
            metadata.and_then(|metadata| metadata.section_name().map(str::to_owned)),
            None,
            None,
            entries,
        )
    }

    fn from_serialized_parts(
        object: Option<String>,
        section: Option<String>,
        bytecode_origin_coverage: Option<BytecodeOriginCoverageExport>,
        post_opt_origin_coverage: Option<SonatinaPostOptOriginCoverageExport>,
        entries: Vec<BytecodeSourceMapEntry>,
    ) -> Result<Self, BytecodeSourceMapExportEntryError> {
        validate_source_map_export_entries(object.as_deref(), section.as_deref(), &entries)?;
        if let Some(coverage) = bytecode_origin_coverage.as_ref()
            && coverage.total() != entries.len()
        {
            return Err(
                BytecodeSourceMapExportEntryError::CoverageEntryCountMismatch {
                    coverage_total: coverage.total(),
                    entry_count: entries.len(),
                },
            );
        }
        Ok(Self {
            schema_version: Self::SCHEMA_VERSION,
            object,
            section,
            bytecode_origin_coverage,
            post_opt_origin_coverage,
            entries,
        })
    }

    pub fn from_options(
        options: BytecodeSourceMapExportOptions<'_>,
        entries: Vec<BytecodeSourceMapEntry>,
    ) -> Result<Self, BytecodeSourceMapExportEntryError> {
        let bytecode_origin_coverage = options
            .bytecode_origin_coverage
            .map(BytecodeOriginCoverageExport::from);
        let post_opt_origin_coverage = options
            .post_opt_origin_coverage
            .map(SonatinaPostOptOriginCoverageExport::from);
        Self::from_serialized_parts(
            options
                .metadata
                .map(|metadata| metadata.object_name().to_owned()),
            options
                .metadata
                .and_then(|metadata| metadata.section_name().map(str::to_owned)),
            bytecode_origin_coverage,
            post_opt_origin_coverage,
            entries,
        )
    }

    pub const fn schema_version(&self) -> u32 {
        self.schema_version
    }

    pub fn object(&self) -> Option<&str> {
        self.object.as_deref()
    }

    pub fn section(&self) -> Option<&str> {
        self.section.as_deref()
    }

    pub const fn bytecode_origin_coverage(&self) -> Option<&BytecodeOriginCoverageExport> {
        self.bytecode_origin_coverage.as_ref()
    }

    pub const fn post_opt_origin_coverage(&self) -> Option<&SonatinaPostOptOriginCoverageExport> {
        self.post_opt_origin_coverage.as_ref()
    }

    pub fn entries(&self) -> &[BytecodeSourceMapEntry] {
        &self.entries
    }
}

impl<'de> Deserialize<'de> for OwnedBytecodeSourceMapExport {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct RawExport {
            schema_version: u32,
            object: Option<String>,
            section: Option<String>,
            bytecode_origin_coverage: Option<BytecodeOriginCoverageExport>,
            post_opt_origin_coverage: Option<SonatinaPostOptOriginCoverageExport>,
            entries: Vec<BytecodeSourceMapEntry>,
        }

        let raw = RawExport::deserialize(deserializer)?;
        if raw.schema_version != Self::SCHEMA_VERSION {
            return Err(de::Error::custom(format!(
                "unsupported bytecode source-map schema_version {}; expected {}",
                raw.schema_version,
                Self::SCHEMA_VERSION
            )));
        }

        Self::from_serialized_parts(
            raw.object,
            raw.section,
            raw.bytecode_origin_coverage,
            raw.post_opt_origin_coverage,
            raw.entries,
        )
        .map_err(de::Error::custom)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BytecodeSourceMapExportEntryError {
    EmptyObject,
    EmptySection,
    InvalidPcRange {
        pc_start: u32,
        pc_end: u32,
    },
    EmptySourceFile,
    EmptySourceSnippet,
    InvalidSourceByteRange {
        start_byte: usize,
        end_byte: usize,
    },
    InvalidSourcePositionRange {
        start_line: usize,
        start_col: usize,
        end_line: usize,
        end_col: usize,
    },
    ObjectMismatch {
        expected: String,
        actual: String,
    },
    SectionMismatch {
        expected: String,
        actual: String,
    },
    OverlappingPcRanges {
        object: String,
        section: String,
        previous_start: u32,
        previous_end: u32,
        current_start: u32,
        current_end: u32,
    },
    CoverageEntryCountMismatch {
        coverage_total: usize,
        entry_count: usize,
    },
    EmptyDebugLocations,
    EmptyDebugLineTableFiles,
    EmptyDebugLineTableRows,
    InvalidDebugLineTableFileIndex {
        file_index: usize,
        file_count: usize,
    },
}

impl fmt::Display for BytecodeSourceMapExportEntryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyObject => write!(f, "bytecode source-map export object must not be empty"),
            Self::EmptySection => {
                write!(f, "bytecode source-map export section must not be empty")
            }
            Self::InvalidPcRange { pc_start, pc_end } => {
                write!(
                    f,
                    "invalid bytecode source-map export PC range {pc_start}..{pc_end}"
                )
            }
            Self::EmptySourceFile => write!(f, "bytecode source-map source file must not be empty"),
            Self::EmptySourceSnippet => {
                write!(f, "bytecode source-map source snippet must not be empty")
            }
            Self::InvalidSourceByteRange {
                start_byte,
                end_byte,
            } => write!(
                f,
                "bytecode source-map source entry has invalid byte range {start_byte}..{end_byte}"
            ),
            Self::InvalidSourcePositionRange {
                start_line,
                start_col,
                end_line,
                end_col,
            } => write!(
                f,
                "bytecode source-map source entry has invalid line/column range {start_line}:{start_col}..{end_line}:{end_col}"
            ),
            Self::ObjectMismatch { expected, actual } => write!(
                f,
                "bytecode source-map export object `{expected}` does not match entry object `{actual}`"
            ),
            Self::SectionMismatch { expected, actual } => write!(
                f,
                "bytecode source-map export section `{expected}` does not match entry section `{actual}`"
            ),
            Self::OverlappingPcRanges {
                object,
                section,
                previous_start,
                previous_end,
                current_start,
                current_end,
            } => write!(
                f,
                "bytecode source-map PC ranges overlap in object `{object}` section `{section}`: {previous_start}..{previous_end} overlaps {current_start}..{current_end}"
            ),
            Self::CoverageEntryCountMismatch {
                coverage_total,
                entry_count,
            } => write!(
                f,
                "bytecode_origin_coverage total {coverage_total} does not match {entry_count} source-map entries"
            ),
            Self::EmptyDebugLocations => {
                write!(
                    f,
                    "bytecode debug-location export must contain at least one location"
                )
            }
            Self::EmptyDebugLineTableFiles => {
                write!(
                    f,
                    "bytecode debug-line-table export must contain at least one source file"
                )
            }
            Self::EmptyDebugLineTableRows => {
                write!(
                    f,
                    "bytecode debug-line-table export must contain at least one row"
                )
            }
            Self::InvalidDebugLineTableFileIndex {
                file_index,
                file_count,
            } => write!(
                f,
                "bytecode debug-line-table row references file_index {file_index}, but only {file_count} source files exist"
            ),
        }
    }
}

impl std::error::Error for BytecodeSourceMapExportEntryError {}

impl From<BytecodeSourceMapEntrySemanticError> for BytecodeSourceMapExportEntryError {
    fn from(err: BytecodeSourceMapEntrySemanticError) -> Self {
        match err {
            BytecodeSourceMapEntrySemanticError::UnknownSourceSpanKind(_) => {
                unreachable!("typed source-map entries store closed source span kinds")
            }
            BytecodeSourceMapEntrySemanticError::UnknownReason { .. } => {
                unreachable!("typed source-map entries store closed reason enums")
            }
            BytecodeSourceMapEntrySemanticError::EmptySourceFile => Self::EmptySourceFile,
            BytecodeSourceMapEntrySemanticError::EmptySourceSnippet => Self::EmptySourceSnippet,
            BytecodeSourceMapEntrySemanticError::InvalidSourceByteRange {
                start_byte,
                end_byte,
            } => Self::InvalidSourceByteRange {
                start_byte,
                end_byte,
            },
            BytecodeSourceMapEntrySemanticError::InvalidSourcePositionRange {
                start_line,
                start_col,
                end_line,
                end_col,
            } => Self::InvalidSourcePositionRange {
                start_line,
                start_col,
                end_line,
                end_col,
            },
        }
    }
}

#[derive(Debug)]
pub enum BytecodeSourceMapExportError {
    InvalidEntries(BytecodeSourceMapExportEntryError),
    Serialize(serde_json::Error),
}

impl fmt::Display for BytecodeSourceMapExportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidEntries(err) => err.fmt(f),
            Self::Serialize(err) => err.fmt(f),
        }
    }
}

impl std::error::Error for BytecodeSourceMapExportError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::InvalidEntries(err) => Some(err),
            Self::Serialize(err) => Some(err),
        }
    }
}

impl From<BytecodeSourceMapExportEntryError> for BytecodeSourceMapExportError {
    fn from(err: BytecodeSourceMapExportEntryError) -> Self {
        Self::InvalidEntries(err)
    }
}

impl From<serde_json::Error> for BytecodeSourceMapExportError {
    fn from(err: serde_json::Error) -> Self {
        Self::Serialize(err)
    }
}

pub(super) fn validate_source_map_export_entries(
    object: Option<&str>,
    section: Option<&str>,
    entries: &[BytecodeSourceMapEntry],
) -> Result<(), BytecodeSourceMapExportEntryError> {
    for entry in entries {
        validate_source_map_entry_kind(entry.kind())
            .map_err(BytecodeSourceMapExportEntryError::from)?;
    }

    validate_export_metadata_and_pc_ranges(object, section, entries)
}

pub(super) fn validate_debug_location_entry_parts(
    object: &str,
    section: &str,
    pc_start: u32,
    pc_end: u32,
    file: &str,
    start_byte: usize,
    end_byte: usize,
    start_line: usize,
    start_col: usize,
    end_line: usize,
    end_col: usize,
    snippet: &str,
) -> Result<(), BytecodeSourceMapExportEntryError> {
    if object.is_empty() {
        return Err(BytecodeSourceMapExportEntryError::EmptyObject);
    }
    if section.is_empty() {
        return Err(BytecodeSourceMapExportEntryError::EmptySection);
    }
    if BytecodePcRange::new(pc_start, pc_end).is_none() {
        return Err(BytecodeSourceMapExportEntryError::InvalidPcRange { pc_start, pc_end });
    }
    if file.is_empty() {
        return Err(BytecodeSourceMapExportEntryError::EmptySourceFile);
    }
    if snippet.is_empty() {
        return Err(BytecodeSourceMapExportEntryError::EmptySourceSnippet);
    }
    if start_byte > end_byte {
        return Err(BytecodeSourceMapExportEntryError::InvalidSourceByteRange {
            start_byte,
            end_byte,
        });
    }
    if start_line > end_line || (start_line == end_line && start_col > end_col) {
        return Err(
            BytecodeSourceMapExportEntryError::InvalidSourcePositionRange {
                start_line,
                start_col,
                end_line,
                end_col,
            },
        );
    }

    Ok(())
}

pub(super) trait BytecodePcExportEntry {
    fn object(&self) -> &str;
    fn section(&self) -> &str;
    fn pc_start(&self) -> u32;
    fn pc_end(&self) -> u32;
}

impl BytecodePcExportEntry for BytecodeSourceMapEntry {
    fn object(&self) -> &str {
        BytecodeSourceMapEntry::object(self)
    }

    fn section(&self) -> &str {
        BytecodeSourceMapEntry::section(self)
    }

    fn pc_start(&self) -> u32 {
        BytecodeSourceMapEntry::pc_start(self)
    }

    fn pc_end(&self) -> u32 {
        BytecodeSourceMapEntry::pc_end(self)
    }
}

pub(super) fn validate_export_metadata_and_pc_ranges<T: BytecodePcExportEntry>(
    object: Option<&str>,
    section: Option<&str>,
    entries: &[T],
) -> Result<(), BytecodeSourceMapExportEntryError> {
    if object.is_some_and(str::is_empty) {
        return Err(BytecodeSourceMapExportEntryError::EmptyObject);
    }
    if section.is_some_and(str::is_empty) {
        return Err(BytecodeSourceMapExportEntryError::EmptySection);
    }

    for entry in entries {
        if BytecodePcRange::new(entry.pc_start(), entry.pc_end()).is_none() {
            return Err(BytecodeSourceMapExportEntryError::InvalidPcRange {
                pc_start: entry.pc_start(),
                pc_end: entry.pc_end(),
            });
        }
        if let Some(object) = object
            && entry.object() != object
        {
            return Err(BytecodeSourceMapExportEntryError::ObjectMismatch {
                expected: object.to_string(),
                actual: entry.object().to_string(),
            });
        }
        if let Some(section) = section
            && entry.section() != section
        {
            return Err(BytecodeSourceMapExportEntryError::SectionMismatch {
                expected: section.to_string(),
                actual: entry.section().to_string(),
            });
        }
    }

    let mut sorted = entries.iter().collect::<Vec<_>>();
    sorted.sort_by(|left, right| {
        left.object()
            .cmp(right.object())
            .then_with(|| left.section().cmp(right.section()))
            .then_with(|| left.pc_start().cmp(&right.pc_start()))
            .then_with(|| left.pc_end().cmp(&right.pc_end()))
    });
    for pair in sorted.windows(2) {
        let previous = pair[0];
        let current = pair[1];
        if previous.object() != current.object() || previous.section() != current.section() {
            continue;
        }
        if previous.pc_end() > current.pc_start() {
            return Err(BytecodeSourceMapExportEntryError::OverlappingPcRanges {
                object: previous.object().to_string(),
                section: previous.section().to_string(),
                previous_start: previous.pc_start(),
                previous_end: previous.pc_end(),
                current_start: current.pc_start(),
                current_end: current.pc_end(),
            });
        }
    }

    Ok(())
}

impl BytecodeSourceMapEntryKind {
    pub const fn kind_name(&self) -> &'static str {
        match self {
            Self::Source { .. } => "source",
            Self::SourceSpanInvalid { .. } => "source_span_invalid",
            Self::SemanticSpanMissing => "semantic_span_missing",
            Self::RuntimeStmtMissing => "runtime_stmt_missing",
            Self::RuntimeTerminatorMissing => "runtime_terminator_missing",
            Self::RuntimeSynthetic => "runtime_synthetic",
            Self::SonatinaSynthetic { .. } => "sonatina_synthetic",
            Self::SonatinaUnmapped { .. } => "sonatina_unmapped",
            Self::PostPreOptSnapshotGap => "post_preopt_snapshot_gap",
            Self::BytecodeUnmapped { .. } => "bytecode_unmapped",
        }
    }

    pub fn reason(&self) -> Option<&str> {
        match self {
            Self::SourceSpanInvalid { reason } => Some(reason.as_str()),
            Self::SonatinaSynthetic { reason } => Some(reason.as_str()),
            Self::SonatinaUnmapped { reason } => Some(reason.as_str()),
            Self::BytecodeUnmapped { reason } => Some(reason.as_str()),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BytecodeSourceMapSummary {
    object: Option<String>,
    section: Option<String>,
    total: usize,
    source: usize,
    debug_line_table_files: usize,
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

impl BytecodeSourceMapSummary {
    pub fn object(&self) -> Option<&str> {
        self.object.as_deref()
    }

    pub fn section(&self) -> Option<&str> {
        self.section.as_deref()
    }

    pub const fn total(&self) -> usize {
        self.total
    }

    pub const fn source(&self) -> usize {
        self.source
    }

    pub const fn debug_locations(&self) -> usize {
        self.source
    }

    pub const fn debug_line_table_files(&self) -> usize {
        self.debug_line_table_files
    }

    pub const fn debug_line_table_rows(&self) -> usize {
        self.source
    }

    pub const fn source_span_invalid(&self) -> usize {
        self.source_span_invalid
    }

    pub const fn semantic_span_missing(&self) -> usize {
        self.semantic_span_missing
    }

    pub const fn runtime_stmt_missing(&self) -> usize {
        self.runtime_stmt_missing
    }

    pub const fn runtime_terminator_missing(&self) -> usize {
        self.runtime_terminator_missing
    }

    pub const fn runtime_synthetic(&self) -> usize {
        self.runtime_synthetic
    }

    pub const fn sonatina_synthetic(&self) -> usize {
        self.sonatina_synthetic
    }

    pub const fn sonatina_unmapped(&self) -> usize {
        self.sonatina_unmapped
    }

    pub const fn post_preopt_snapshot_gap(&self) -> usize {
        self.post_preopt_snapshot_gap
    }

    pub const fn bytecode_unmapped(&self) -> usize {
        self.bytecode_unmapped
    }

    pub const fn non_source(&self) -> usize {
        self.total.saturating_sub(self.source)
    }
}

pub fn bytecode_source_map_entries_summary(
    entries: &[BytecodeSourceMapEntry],
    metadata: Option<BytecodeSourceMapExportMetadata<'_>>,
) -> Option<BytecodeSourceMapSummary> {
    let (object, section) = export_metadata_parts(metadata);
    let mut summary = BytecodeSourceMapSummary {
        object: object.map(str::to_owned),
        section: section.map(str::to_owned),
        ..BytecodeSourceMapSummary::default()
    };
    let mut debug_line_table_files = BTreeSet::new();

    for entry in entries.iter().filter(|entry| {
        object.is_none_or(|object| entry.object() == object)
            && section.is_none_or(|section| entry.section() == section)
    }) {
        summary.total += 1;
        match entry.kind() {
            BytecodeSourceMapEntryKind::Source { file, .. } => {
                summary.source += 1;
                debug_line_table_files.insert(file);
            }
            BytecodeSourceMapEntryKind::SourceSpanInvalid { .. } => {
                summary.source_span_invalid += 1;
            }
            BytecodeSourceMapEntryKind::SemanticSpanMissing => {
                summary.semantic_span_missing += 1;
            }
            BytecodeSourceMapEntryKind::RuntimeStmtMissing => {
                summary.runtime_stmt_missing += 1;
            }
            BytecodeSourceMapEntryKind::RuntimeTerminatorMissing => {
                summary.runtime_terminator_missing += 1;
            }
            BytecodeSourceMapEntryKind::RuntimeSynthetic => summary.runtime_synthetic += 1,
            BytecodeSourceMapEntryKind::SonatinaSynthetic { .. } => {
                summary.sonatina_synthetic += 1;
            }
            BytecodeSourceMapEntryKind::SonatinaUnmapped { .. } => {
                summary.sonatina_unmapped += 1;
            }
            BytecodeSourceMapEntryKind::PostPreOptSnapshotGap => {
                summary.post_preopt_snapshot_gap += 1;
            }
            BytecodeSourceMapEntryKind::BytecodeUnmapped { .. } => {
                summary.bytecode_unmapped += 1;
            }
        }
    }
    summary.debug_line_table_files = debug_line_table_files.len();

    (summary.total > 0).then_some(summary)
}

pub fn bytecode_source_map_entries(
    db: &dyn InputDb,
    resolutions: &[BytecodeSourceResolution<'_>],
    filter: Option<&BytecodeSourceMapFilter>,
) -> Vec<BytecodeSourceMapEntry> {
    let mut entries = resolutions
        .iter()
        .filter(|resolution| {
            filter
                .map(|filter| matches_filter(resolution.origin(), filter))
                .unwrap_or(true)
        })
        .map(|resolution| source_map_entry_for_resolution(db, resolution))
        .collect::<Vec<_>>();
    entries.sort_by(|left, right| {
        left.object()
            .cmp(right.object())
            .then_with(|| left.section().cmp(right.section()))
            .then_with(|| left.pc_start().cmp(&right.pc_start()))
            .then_with(|| left.pc_end().cmp(&right.pc_end()))
    });
    entries
}

pub fn bytecode_source_span_exports_for_object(
    db: &dyn InputDb,
    resolutions: &[BytecodeSourceResolution<'_>],
    object: &BytecodeObjectKey,
) -> Vec<SourceSpanExport> {
    bytecode_source_span_exports_for_resolutions(db, resolutions, |origin| {
        origin.section().object() == object
    })
}

pub fn bytecode_source_span_exports(
    db: &dyn InputDb,
    resolutions: &[BytecodeSourceResolution<'_>],
    filter: Option<&BytecodeSourceMapFilter>,
) -> Vec<SourceSpanExport> {
    bytecode_source_span_exports_for_resolutions(db, resolutions, |origin| {
        filter
            .map(|filter| matches_filter(origin, filter))
            .unwrap_or(true)
    })
}

pub fn bytecode_source_map_json(
    db: &dyn InputDb,
    resolutions: &[BytecodeSourceResolution<'_>],
    filter: Option<&BytecodeSourceMapFilter>,
) -> Result<Option<String>, BytecodeSourceMapExportError> {
    let entries = bytecode_source_map_entries(db, resolutions, filter);
    bytecode_source_map_entries_json(
        &entries,
        BytecodeSourceMapExportOptions::from_filter(filter),
    )
}

pub fn bytecode_source_map_entries_json(
    entries: &[BytecodeSourceMapEntry],
    options: BytecodeSourceMapExportOptions<'_>,
) -> Result<Option<String>, BytecodeSourceMapExportError> {
    let Some(export) = bytecode_source_map_entries_export(entries, options)? else {
        return Ok(None);
    };
    serde_json::to_string(&export)
        .map(Some)
        .map_err(BytecodeSourceMapExportError::from)
}

pub fn bytecode_source_map_entries_export(
    entries: &[BytecodeSourceMapEntry],
    options: BytecodeSourceMapExportOptions<'_>,
) -> Result<Option<OwnedBytecodeSourceMapExport>, BytecodeSourceMapExportEntryError> {
    (!entries.is_empty())
        .then(|| OwnedBytecodeSourceMapExport::from_options(options, entries.to_vec()))
        .transpose()
}

pub fn bytecode_debug_location_entries_json(
    source_map_entries: &[BytecodeSourceMapEntry],
    options: BytecodeSourceMapExportOptions<'_>,
) -> Result<Option<String>, BytecodeSourceMapExportError> {
    let Some(export) = bytecode_debug_location_entries_export(source_map_entries, options)? else {
        return Ok(None);
    };
    serde_json::to_string(&export)
        .map(Some)
        .map_err(BytecodeSourceMapExportError::from)
}

pub fn bytecode_debug_location_entries_export(
    source_map_entries: &[BytecodeSourceMapEntry],
    options: BytecodeSourceMapExportOptions<'_>,
) -> Result<Option<OwnedBytecodeDebugLocationExport>, BytecodeSourceMapExportEntryError> {
    OwnedBytecodeDebugLocationExport::from_options(options, source_map_entries)
}

pub(super) fn export_metadata_parts(
    metadata: Option<BytecodeSourceMapExportMetadata<'_>>,
) -> (Option<&str>, Option<&str>) {
    match metadata {
        Some(metadata) => (Some(metadata.object_name()), metadata.section_name()),
        None => (None, None),
    }
}

fn matches_filter(origin: &BytecodePcOrigin, filter: &BytecodeSourceMapFilter) -> bool {
    origin.section().object().as_str() == filter.object()
        && origin.section().section() == filter.section()
}

#[cfg(test)]
mod tests;
