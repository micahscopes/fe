use std::fmt;

use common::{
    InputDb,
    diagnostics::{Span, SpanKind},
    facts::{SourceSpanExport, SourceSpanKind},
};
use serde::{Deserialize, Deserializer, Serialize, de};

use crate::origin::{
    BytecodeObjectKey, BytecodeOriginCoverage, BytecodePcOrigin, BytecodePcRange,
    BytecodeSectionKey, BytecodeSourceResolution, BytecodeSourceResolutionResult,
    BytecodeUnmappedReason, SonatinaSyntheticOrigin, SonatinaUnmappedReason,
    bytecode_pc_export_key,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BytecodeSourceMapFilter {
    section: BytecodeSectionKey,
}

impl BytecodeSourceMapFilter {
    pub fn new(section: BytecodeSectionKey) -> Self {
        Self { section }
    }

    pub fn metadata(&self) -> BytecodeSourceMapExportMetadata<'_> {
        BytecodeSourceMapExportMetadata::section(&self.section)
    }

    pub fn object(&self) -> &str {
        self.section.object().as_str()
    }

    pub fn section(&self) -> &str {
        self.section.section()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BytecodeSourceMapExportMetadata<'a> {
    Object(&'a BytecodeObjectKey),
    Section(&'a BytecodeSectionKey),
}

impl<'a> BytecodeSourceMapExportMetadata<'a> {
    pub const fn object(object: &'a BytecodeObjectKey) -> Self {
        Self::Object(object)
    }

    pub const fn section(section: &'a BytecodeSectionKey) -> Self {
        Self::Section(section)
    }

    pub fn object_name(self) -> &'a str {
        match self {
            Self::Object(object) => object.as_str(),
            Self::Section(section) => section.object().as_str(),
        }
    }

    pub fn section_name(self) -> Option<&'a str> {
        match self {
            Self::Object(_) => None,
            Self::Section(section) => Some(section.section()),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BytecodeSourceMapExportOptions<'a> {
    metadata: Option<BytecodeSourceMapExportMetadata<'a>>,
    bytecode_origin_coverage: Option<BytecodeOriginCoverage>,
}

impl<'a> BytecodeSourceMapExportOptions<'a> {
    pub const fn new() -> Self {
        Self {
            metadata: None,
            bytecode_origin_coverage: None,
        }
    }

    pub fn from_filter(filter: Option<&'a BytecodeSourceMapFilter>) -> Self {
        Self::new().with_optional_metadata(filter.map(BytecodeSourceMapFilter::metadata))
    }

    pub fn with_metadata(mut self, metadata: BytecodeSourceMapExportMetadata<'a>) -> Self {
        self.metadata = Some(metadata);
        self
    }

    pub fn with_optional_metadata(
        mut self,
        metadata: Option<BytecodeSourceMapExportMetadata<'a>>,
    ) -> Self {
        self.metadata = metadata;
        self
    }

    pub fn with_object_key(self, object: &'a BytecodeObjectKey) -> Self {
        self.with_metadata(BytecodeSourceMapExportMetadata::object(object))
    }

    pub fn with_section_key(self, section: &'a BytecodeSectionKey) -> Self {
        self.with_metadata(BytecodeSourceMapExportMetadata::section(section))
    }

    pub fn with_bytecode_origin_coverage(
        mut self,
        bytecode_origin_coverage: Option<BytecodeOriginCoverage>,
    ) -> Self {
        self.bytecode_origin_coverage = bytecode_origin_coverage;
        self
    }
}

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
    entries: Vec<BytecodeSourceMapEntry>,
}

impl OwnedBytecodeSourceMapExport {
    pub const SCHEMA_VERSION: u32 = 2;

    pub fn try_new(
        metadata: Option<BytecodeSourceMapExportMetadata<'_>>,
        entries: Vec<BytecodeSourceMapEntry>,
    ) -> Result<Self, BytecodeSourceMapExportEntryError> {
        Self::from_serialized_parts(
            metadata.map(|metadata| metadata.object_name().to_owned()),
            metadata.and_then(|metadata| metadata.section_name().map(str::to_owned)),
            None,
            entries,
        )
    }

    fn from_serialized_parts(
        object: Option<String>,
        section: Option<String>,
        bytecode_origin_coverage: Option<BytecodeOriginCoverageExport>,
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
        Self::from_serialized_parts(
            options
                .metadata
                .map(|metadata| metadata.object_name().to_owned()),
            options
                .metadata
                .and_then(|metadata| metadata.section_name().map(str::to_owned)),
            bytecode_origin_coverage,
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

    pub fn entries(&self) -> &[BytecodeSourceMapEntry] {
        &self.entries
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub struct BytecodeOriginCoverageExport {
    total: usize,
    sonatina_post_opt: usize,
    sonatina_backend_prepared: usize,
    unmapped: usize,
}

impl From<BytecodeOriginCoverage> for BytecodeOriginCoverageExport {
    fn from(coverage: BytecodeOriginCoverage) -> Self {
        Self {
            total: coverage.total(),
            sonatina_post_opt: coverage.sonatina_post_opt(),
            sonatina_backend_prepared: coverage.sonatina_backend_prepared(),
            unmapped: coverage.unmapped(),
        }
    }
}

impl BytecodeOriginCoverageExport {
    pub const fn total(&self) -> usize {
        self.total
    }

    pub const fn sonatina_post_opt(&self) -> usize {
        self.sonatina_post_opt
    }

    pub const fn sonatina_backend_prepared(&self) -> usize {
        self.sonatina_backend_prepared
    }

    pub const fn unmapped(&self) -> usize {
        self.unmapped
    }

    pub const fn classified_total(&self) -> usize {
        self.sonatina_post_opt + self.sonatina_backend_prepared + self.unmapped
    }
}

impl<'de> Deserialize<'de> for BytecodeOriginCoverageExport {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct RawCoverage {
            total: usize,
            sonatina_post_opt: usize,
            sonatina_backend_prepared: usize,
            unmapped: usize,
        }

        let raw = RawCoverage::deserialize(deserializer)?;
        let classified_total = raw
            .sonatina_post_opt
            .checked_add(raw.sonatina_backend_prepared)
            .and_then(|total| total.checked_add(raw.unmapped))
            .ok_or_else(|| {
                de::Error::custom("bytecode_origin_coverage classified total overflows usize")
            })?;
        if raw.total != classified_total {
            return Err(de::Error::custom(format!(
                "bytecode_origin_coverage total {} does not match classified total {}",
                raw.total, classified_total
            )));
        }

        Ok(Self {
            total: raw.total,
            sonatina_post_opt: raw.sonatina_post_opt,
            sonatina_backend_prepared: raw.sonatina_backend_prepared,
            unmapped: raw.unmapped,
        })
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
            raw.entries,
        )
        .map_err(de::Error::custom)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BytecodeSourceMapExportEntryError {
    EmptyObject,
    EmptySection,
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
}

impl fmt::Display for BytecodeSourceMapExportEntryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyObject => write!(f, "bytecode source-map export object must not be empty"),
            Self::EmptySection => {
                write!(f, "bytecode source-map export section must not be empty")
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

fn validate_source_map_export_entries(
    object: Option<&str>,
    section: Option<&str>,
    entries: &[BytecodeSourceMapEntry],
) -> Result<(), BytecodeSourceMapExportEntryError> {
    if object.is_some_and(str::is_empty) {
        return Err(BytecodeSourceMapExportEntryError::EmptyObject);
    }
    if section.is_some_and(str::is_empty) {
        return Err(BytecodeSourceMapExportEntryError::EmptySection);
    }

    for entry in entries {
        validate_source_map_entry_kind(entry.kind())
            .map_err(BytecodeSourceMapExportEntryError::from)?;
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
    object: Option<&str>,
    section: Option<&str>,
) -> Option<BytecodeSourceMapSummary> {
    let mut summary = BytecodeSourceMapSummary {
        object: object.map(str::to_owned),
        section: section.map(str::to_owned),
        ..BytecodeSourceMapSummary::default()
    };

    for entry in entries.iter().filter(|entry| {
        object.is_none_or(|object| entry.object() == object)
            && section.is_none_or(|section| entry.section() == section)
    }) {
        summary.total += 1;
        match entry.kind() {
            BytecodeSourceMapEntryKind::Source { .. } => summary.source += 1,
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

fn matches_filter(origin: &BytecodePcOrigin, filter: &BytecodeSourceMapFilter) -> bool {
    origin.section().object().as_str() == filter.object()
        && origin.section().section() == filter.section()
}

fn source_map_entry_for_resolution(
    db: &dyn InputDb,
    resolution: &BytecodeSourceResolution<'_>,
) -> BytecodeSourceMapEntry {
    let origin = resolution.origin();
    BytecodeSourceMapEntry::from_origin(origin, source_map_entry_kind(db, resolution))
}

fn bytecode_source_span_exports_for_resolutions(
    db: &dyn InputDb,
    resolutions: &[BytecodeSourceResolution<'_>],
    mut include_origin: impl FnMut(&BytecodePcOrigin) -> bool,
) -> Vec<SourceSpanExport> {
    let mut spans = resolutions
        .iter()
        .filter(|resolution| include_origin(resolution.origin()))
        .filter_map(|resolution| source_span_export_for_resolution(db, resolution))
        .collect::<Vec<_>>();
    spans.sort();
    spans
}

fn source_span_export_for_resolution(
    db: &dyn InputDb,
    resolution: &BytecodeSourceResolution<'_>,
) -> Option<SourceSpanExport> {
    let BytecodeSourceResolutionResult::SourceSpan { span, .. } = resolution.result() else {
        return None;
    };
    let details = source_span_details(db, span).ok()?;

    Some(SourceSpanExport::new(
        bytecode_pc_export_key(resolution.origin().clone()),
        details.span_kind,
        details.file,
        details.start_byte,
        details.end_byte,
        details.start_line,
        details.start_col,
        details.end_line,
        details.end_col,
    ))
}

fn source_map_entry_kind(
    db: &dyn InputDb,
    resolution: &BytecodeSourceResolution<'_>,
) -> BytecodeSourceMapEntryKind {
    match resolution.result() {
        BytecodeSourceResolutionResult::SourceSpan { span, .. } => {
            let details = match source_span_details(db, span) {
                Ok(details) => details,
                Err(reason) => return BytecodeSourceMapEntryKind::SourceSpanInvalid { reason },
            };
            BytecodeSourceMapEntryKind::Source {
                span_kind: details.span_kind,
                file: details.file,
                start_byte: details.start_byte,
                end_byte: details.end_byte,
                start_line: details.start_line,
                start_col: details.start_col,
                end_line: details.end_line,
                end_col: details.end_col,
                snippet: details.snippet,
            }
        }
        BytecodeSourceResolutionResult::SemanticSpanMissing(_) => {
            BytecodeSourceMapEntryKind::SemanticSpanMissing
        }
        BytecodeSourceResolutionResult::RuntimeStmtMissing(_) => {
            BytecodeSourceMapEntryKind::RuntimeStmtMissing
        }
        BytecodeSourceResolutionResult::RuntimeTerminatorMissing(_) => {
            BytecodeSourceMapEntryKind::RuntimeTerminatorMissing
        }
        BytecodeSourceResolutionResult::RuntimeSynthetic => {
            BytecodeSourceMapEntryKind::RuntimeSynthetic
        }
        BytecodeSourceResolutionResult::SonatinaSynthetic(origin) => {
            BytecodeSourceMapEntryKind::SonatinaSynthetic { reason: *origin }
        }
        BytecodeSourceResolutionResult::SonatinaUnmapped(reason) => {
            BytecodeSourceMapEntryKind::SonatinaUnmapped { reason: *reason }
        }
        BytecodeSourceResolutionResult::PostPreOptSnapshotGap => {
            BytecodeSourceMapEntryKind::PostPreOptSnapshotGap
        }
        BytecodeSourceResolutionResult::BytecodeUnmapped(reason) => {
            BytecodeSourceMapEntryKind::BytecodeUnmapped { reason: *reason }
        }
    }
}

struct SourceSpanDetails {
    span_kind: SourceSpanKind,
    file: String,
    start_byte: usize,
    end_byte: usize,
    start_line: usize,
    start_col: usize,
    end_line: usize,
    end_col: usize,
    snippet: String,
}

fn source_span_details(
    db: &dyn InputDb,
    span: &Span,
) -> Result<SourceSpanDetails, SourceSpanInvalidReason> {
    let text = span.file.text(db);
    let start_byte = usize::from(span.range.start());
    let end_byte = usize::from(span.range.end());
    let snippet = span_snippet(text, start_byte, end_byte)?;
    let line_index = LineIndex::new(text);
    let start = line_index.position(start_byte);
    let end = line_index.position(end_byte);

    Ok(SourceSpanDetails {
        span_kind: source_span_kind(span.kind),
        file: span_file_name(db, span.file),
        start_byte,
        end_byte,
        start_line: start.line,
        start_col: start.col,
        end_line: end.line,
        end_col: end.col,
        snippet,
    })
}

fn span_file_name(db: &dyn InputDb, file: common::file::File) -> String {
    file.path(db)
        .as_ref()
        .map(|path| path.to_string())
        .or_else(|| file.url(db).map(|url| url.to_string()))
        .unwrap_or_else(|| "<unknown>".to_string())
}

fn span_snippet(text: &str, start: usize, end: usize) -> Result<String, SourceSpanInvalidReason> {
    if start > end {
        return Err(SourceSpanInvalidReason::InvalidByteRange);
    }

    let Some(snippet) = text.get(start..end) else {
        return Err(SourceSpanInvalidReason::InvalidSnippetRange);
    };
    if snippet.is_empty() {
        return Err(SourceSpanInvalidReason::EmptySnippet);
    }

    Ok(snippet.to_string())
}

fn source_span_kind(kind: SpanKind) -> SourceSpanKind {
    match kind {
        SpanKind::Original => SourceSpanKind::Original,
        SpanKind::Expanded => SourceSpanKind::Expanded,
        SpanKind::NotFound => SourceSpanKind::NotFound,
    }
}

struct LineIndex {
    line_starts: Vec<usize>,
}

#[derive(Clone, Copy)]
struct LinePosition {
    line: usize,
    col: usize,
}

impl LineIndex {
    fn new(text: &str) -> Self {
        let mut line_starts = vec![0];
        line_starts.extend(
            text.char_indices()
                .filter_map(|(idx, ch)| (ch == '\n').then_some(idx + 1)),
        );
        Self { line_starts }
    }

    fn position(&self, byte_offset: usize) -> LinePosition {
        let line = self
            .line_starts
            .partition_point(|line_start| *line_start <= byte_offset)
            .saturating_sub(1);
        let line_start = self.line_starts.get(line).copied().unwrap_or(0);
        LinePosition {
            line,
            col: byte_offset.saturating_sub(line_start),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::origin::{
        BytecodeObjectKey, BytecodeOriginCoverage, BytecodePcOrigin, BytecodePcRange,
        BytecodeSectionKey, BytecodeSectionNameKey, BytecodeUnmappedReason,
    };

    use super::{
        BytecodeSourceMapEntry, BytecodeSourceMapEntryError, BytecodeSourceMapEntryKind,
        BytecodeSourceMapExportMetadata, BytecodeSourceMapExportOptions,
        OwnedBytecodeSourceMapExport, SourceSpanInvalidReason, SourceSpanKind,
        bytecode_source_map_entries_export, bytecode_source_map_entries_json,
        bytecode_source_map_entries_summary, span_snippet,
    };

    fn source_map_origin(
        object: &str,
        section: &str,
        pc_start: u32,
        pc_end: u32,
    ) -> BytecodePcOrigin {
        let range = BytecodePcRange::new(pc_start, pc_end).expect("valid source-map PC range");
        let section = BytecodeSectionKey::new(
            BytecodeObjectKey::new(object),
            BytecodeSectionNameKey::new(section),
        );
        BytecodePcOrigin::new(section, range)
    }

    fn source_map_entry(
        object: &str,
        section: &str,
        pc_start: u32,
        pc_end: u32,
        kind: BytecodeSourceMapEntryKind,
    ) -> BytecodeSourceMapEntry {
        let origin = source_map_origin(object, section, pc_start, pc_end);
        BytecodeSourceMapEntry::from_origin(&origin, kind)
    }

    #[test]
    fn bytecode_source_map_json_roundtrips_owned_export_schema() {
        let entries = vec![
            source_map_entry(
                "Foo",
                "runtime",
                4,
                8,
                BytecodeSourceMapEntryKind::Source {
                    span_kind: SourceSpanKind::Original,
                    file: "src/main.fe".to_string(),
                    start_byte: 10,
                    end_byte: 14,
                    start_line: 1,
                    start_col: 2,
                    end_line: 1,
                    end_col: 6,
                    snippet: "main".to_string(),
                },
            ),
            source_map_entry(
                "Foo",
                "runtime",
                8,
                12,
                BytecodeSourceMapEntryKind::BytecodeUnmapped {
                    reason: BytecodeUnmappedReason::NoIrInst,
                },
            ),
        ];

        let section_key = BytecodeSectionKey::new(
            BytecodeObjectKey::new("Foo"),
            BytecodeSectionNameKey::new("runtime"),
        );
        let options = BytecodeSourceMapExportOptions::new().with_section_key(&section_key);
        let export = bytecode_source_map_entries_export(&entries, options)
            .expect("source-map export should validate")
            .expect("non-empty entries should export");
        assert_eq!(
            export.schema_version(),
            OwnedBytecodeSourceMapExport::SCHEMA_VERSION
        );
        assert_eq!(export.object(), Some("Foo"));
        assert_eq!(export.section(), Some("runtime"));
        assert_eq!(export.entries(), entries.as_slice());

        let bytecode_origin_coverage = BytecodeOriginCoverage::new(1, 1, 0);
        let json = bytecode_source_map_entries_json(
            &entries,
            options.with_bytecode_origin_coverage(Some(bytecode_origin_coverage)),
        )
        .expect("source map should serialize")
        .expect("non-empty entries should render JSON");
        let decoded: OwnedBytecodeSourceMapExport = serde_json::from_str(&json).unwrap();

        assert_eq!(decoded.schema_version(), export.schema_version());
        assert_eq!(decoded.object(), export.object());
        assert_eq!(decoded.section(), export.section());
        assert_eq!(decoded.entries()[0].kind().kind_name(), "source");
        match decoded.entries()[0].kind() {
            BytecodeSourceMapEntryKind::Source { snippet, .. } => assert_eq!(snippet, "main"),
            other => panic!("expected source entry, got {other:?}"),
        }
        assert_eq!(decoded.entries()[1].kind().reason(), Some("no_ir_inst"));
        let decoded_coverage = decoded
            .bytecode_origin_coverage()
            .expect("coverage should roundtrip through source-map JSON");
        assert_eq!(decoded_coverage.total(), 2);
        assert_eq!(decoded_coverage.sonatina_post_opt(), 1);
        assert_eq!(decoded_coverage.sonatina_backend_prepared(), 1);
        assert_eq!(decoded_coverage.unmapped(), 0);
    }

    #[test]
    fn bytecode_source_map_json_rejects_unknown_schema_version() {
        let json = r#"{"schema_version":999,"entries":[]}"#;
        let err = serde_json::from_str::<OwnedBytecodeSourceMapExport>(json)
            .expect_err("unknown schema versions must fail closed");

        assert!(
            err.to_string()
                .contains("unsupported bytecode source-map schema_version 999"),
            "{err}"
        );
    }

    #[test]
    fn bytecode_source_map_json_rejects_unknown_export_fields() {
        let json = r#"{"schema_version":2,"entries":[],"extra":true}"#;
        let err = serde_json::from_str::<OwnedBytecodeSourceMapExport>(json)
            .expect_err("unknown source-map export fields must fail closed");

        assert!(err.to_string().contains("unknown field"), "{err}");
    }

    #[test]
    fn bytecode_source_map_json_rejects_empty_export_objects() {
        let json = r#"{"schema_version":2,"object":"","entries":[]}"#;
        let err = serde_json::from_str::<OwnedBytecodeSourceMapExport>(json)
            .expect_err("source-map export object metadata must be non-empty");

        assert!(
            err.to_string()
                .contains("bytecode source-map export object must not be empty"),
            "{err}"
        );
    }

    #[test]
    fn bytecode_source_map_json_rejects_empty_export_sections() {
        let json = r#"{"schema_version":2,"section":"","entries":[]}"#;
        let err = serde_json::from_str::<OwnedBytecodeSourceMapExport>(json)
            .expect_err("source-map export section metadata must be non-empty");

        assert!(
            err.to_string()
                .contains("bytecode source-map export section must not be empty"),
            "{err}"
        );
    }

    #[test]
    fn bytecode_source_map_json_rejects_unknown_entry_fields() {
        let json = r#"{
            "schema_version": 2,
            "entries": [{
                "object": "Foo",
                "section": "runtime",
                "pc_start": 0,
                "pc_end": 4,
                "kind": "semantic_span_missing",
                "extra": true
            }]
        }"#;
        let err = serde_json::from_str::<OwnedBytecodeSourceMapExport>(json)
            .expect_err("unknown source-map entry fields must fail closed");

        assert!(err.to_string().contains("unknown field"), "{err}");
    }

    #[test]
    fn bytecode_source_map_json_rejects_invalid_pc_ranges() {
        let json = r#"{
            "schema_version": 2,
            "entries": [{
                "object": "Foo",
                "section": "runtime",
                "pc_start": 4,
                "pc_end": 4,
                "kind": "semantic_span_missing"
            }]
        }"#;
        let err = serde_json::from_str::<OwnedBytecodeSourceMapExport>(json)
            .expect_err("source-map PC ranges must be non-empty");

        assert!(
            err.to_string()
                .contains("invalid bytecode source-map PC range 4..4"),
            "{err}"
        );
    }

    #[test]
    fn bytecode_source_map_json_rejects_empty_objects() {
        let json = r#"{
            "schema_version": 2,
            "entries": [{
                "object": "",
                "section": "runtime",
                "pc_start": 0,
                "pc_end": 4,
                "kind": "semantic_span_missing"
            }]
        }"#;
        let err = serde_json::from_str::<OwnedBytecodeSourceMapExport>(json)
            .expect_err("source-map object keys must be non-empty");

        assert!(
            err.to_string()
                .contains("bytecode source-map object must not be empty"),
            "{err}"
        );
    }

    #[test]
    fn bytecode_source_map_json_rejects_empty_sections() {
        let json = r#"{
            "schema_version": 2,
            "entries": [{
                "object": "Foo",
                "section": "",
                "pc_start": 0,
                "pc_end": 4,
                "kind": "semantic_span_missing"
            }]
        }"#;
        let err = serde_json::from_str::<OwnedBytecodeSourceMapExport>(json)
            .expect_err("source-map section keys must be non-empty");

        assert!(
            err.to_string()
                .contains("bytecode source-map section must not be empty"),
            "{err}"
        );
    }

    #[test]
    fn bytecode_source_map_json_rejects_unknown_coverage_fields() {
        let json = r#"{
            "schema_version": 2,
            "bytecode_origin_coverage": {
                "total": 1,
                "sonatina_post_opt": 1,
                "sonatina_backend_prepared": 0,
                "unmapped": 0,
                "extra": true
            },
            "entries": [{
                "object": "Foo",
                "section": "runtime",
                "pc_start": 0,
                "pc_end": 4,
                "kind": "semantic_span_missing"
            }]
        }"#;
        let err = serde_json::from_str::<OwnedBytecodeSourceMapExport>(json)
            .expect_err("unknown source-map coverage fields must fail closed");

        assert!(err.to_string().contains("unknown field"), "{err}");
    }

    #[test]
    fn bytecode_source_map_json_rejects_inconsistent_coverage_partition() {
        let json = r#"{
            "schema_version": 2,
            "bytecode_origin_coverage": {
                "total": 2,
                "sonatina_post_opt": 1,
                "sonatina_backend_prepared": 0,
                "unmapped": 0
            },
            "entries": [{
                "object": "Foo",
                "section": "runtime",
                "pc_start": 0,
                "pc_end": 4,
                "kind": "semantic_span_missing"
            }]
        }"#;
        let err = serde_json::from_str::<OwnedBytecodeSourceMapExport>(json)
            .expect_err("source-map coverage partitions must match their total");

        assert!(
            err.to_string()
                .contains("bytecode_origin_coverage total 2 does not match classified total 1"),
            "{err}"
        );
    }

    #[test]
    fn bytecode_source_map_json_rejects_coverage_entry_count_mismatch() {
        let json = r#"{
            "schema_version": 2,
            "bytecode_origin_coverage": {
                "total": 2,
                "sonatina_post_opt": 1,
                "sonatina_backend_prepared": 1,
                "unmapped": 0
            },
            "entries": [{
                "object": "Foo",
                "section": "runtime",
                "pc_start": 0,
                "pc_end": 4,
                "kind": "semantic_span_missing"
            }]
        }"#;
        let err = serde_json::from_str::<OwnedBytecodeSourceMapExport>(json)
            .expect_err("source-map coverage totals must match exported entries");

        assert!(
            err.to_string()
                .contains("bytecode_origin_coverage total 2 does not match 1 source-map entries"),
            "{err}"
        );
    }

    #[test]
    fn bytecode_source_map_json_rejects_unknown_source_fields() {
        let json = r#"{
            "schema_version": 2,
            "entries": [{
                "object": "Foo",
                "section": "runtime",
                "pc_start": 0,
                "pc_end": 4,
                "kind": "source",
                "span_kind": "original",
                "file": "src/main.fe",
                "start_byte": 10,
                "end_byte": 14,
                "start_line": 1,
                "start_col": 2,
                "end_line": 1,
                "end_col": 6,
                "snippet": "main",
                "extra": true
            }]
        }"#;
        let err = serde_json::from_str::<OwnedBytecodeSourceMapExport>(json)
            .expect_err("unknown source-map source fields must fail closed");

        assert!(err.to_string().contains("unknown field"), "{err}");
    }

    #[test]
    fn bytecode_source_map_json_rejects_missing_source_snippet() {
        let json = r#"{
            "schema_version": 2,
            "entries": [{
                "object": "Foo",
                "section": "runtime",
                "pc_start": 0,
                "pc_end": 4,
                "kind": "source",
                "span_kind": "original",
                "file": "src/main.fe",
                "start_byte": 10,
                "end_byte": 14,
                "start_line": 1,
                "start_col": 2,
                "end_line": 1,
                "end_col": 6
            }]
        }"#;
        let err = serde_json::from_str::<OwnedBytecodeSourceMapExport>(json)
            .expect_err("source-map source entries must carry a snippet");

        assert!(err.to_string().contains("missing field `snippet`"), "{err}");
    }

    #[test]
    fn bytecode_source_map_json_rejects_empty_source_files() {
        let json = r#"{
            "schema_version": 2,
            "entries": [{
                "object": "Foo",
                "section": "runtime",
                "pc_start": 0,
                "pc_end": 4,
                "kind": "source",
                "span_kind": "original",
                "file": "",
                "start_byte": 10,
                "end_byte": 14,
                "start_line": 1,
                "start_col": 2,
                "end_line": 1,
                "end_col": 6,
                "snippet": "main"
            }]
        }"#;
        let err = serde_json::from_str::<OwnedBytecodeSourceMapExport>(json)
            .expect_err("source-map source file labels must be non-empty");

        assert!(
            err.to_string()
                .contains("bytecode source-map source file must not be empty"),
            "{err}"
        );
    }

    #[test]
    fn bytecode_source_map_json_rejects_empty_source_snippets() {
        let json = r#"{
            "schema_version": 2,
            "entries": [{
                "object": "Foo",
                "section": "runtime",
                "pc_start": 0,
                "pc_end": 4,
                "kind": "source",
                "span_kind": "original",
                "file": "src/main.fe",
                "start_byte": 10,
                "end_byte": 14,
                "start_line": 1,
                "start_col": 2,
                "end_line": 1,
                "end_col": 6,
                "snippet": ""
            }]
        }"#;
        let err = serde_json::from_str::<OwnedBytecodeSourceMapExport>(json)
            .expect_err("source-map source snippets must be non-empty");

        assert!(
            err.to_string()
                .contains("bytecode source-map source snippet must not be empty"),
            "{err}"
        );
    }

    #[test]
    fn bytecode_source_map_json_rejects_unknown_source_span_kind() {
        let json = r#"{
            "schema_version": 2,
            "entries": [{
                "object": "Foo",
                "section": "runtime",
                "pc_start": 0,
                "pc_end": 4,
                "kind": "source",
                "span_kind": "mystery",
                "file": "src/main.fe",
                "start_byte": 10,
                "end_byte": 14,
                "start_line": 1,
                "start_col": 2,
                "end_line": 1,
                "end_col": 6,
                "snippet": "main"
            }]
        }"#;
        let err = serde_json::from_str::<OwnedBytecodeSourceMapExport>(json)
            .expect_err("source-map source span kinds must be closed");

        assert!(
            err.to_string()
                .contains("unknown bytecode source-map source span kind `mystery`"),
            "{err}"
        );
    }

    #[test]
    fn bytecode_source_map_json_roundtrips_source_span_invalid_reason() {
        let json = r#"{
            "schema_version": 2,
            "entries": [{
                "object": "Foo",
                "section": "runtime",
                "pc_start": 0,
                "pc_end": 4,
                "kind": "source_span_invalid",
                "reason": "invalid_snippet_range"
            }]
        }"#;
        let decoded: OwnedBytecodeSourceMapExport =
            serde_json::from_str(json).expect("closed invalid source-span reasons should decode");

        assert_eq!(
            decoded.entries()[0].kind().kind_name(),
            "source_span_invalid"
        );
        assert_eq!(
            decoded.entries()[0].kind().reason(),
            Some("invalid_snippet_range")
        );
    }

    #[test]
    fn bytecode_source_map_json_rejects_unknown_source_span_invalid_reason() {
        let json = r#"{
            "schema_version": 2,
            "entries": [{
                "object": "Foo",
                "section": "runtime",
                "pc_start": 0,
                "pc_end": 4,
                "kind": "source_span_invalid",
                "reason": "mystery"
            }]
        }"#;
        let err = serde_json::from_str::<OwnedBytecodeSourceMapExport>(json)
            .expect_err("source-map invalid source-span reasons must be closed");

        assert!(
            err.to_string()
                .contains("unknown bytecode source-map source_span_invalid reason `mystery`"),
            "{err}"
        );
    }

    #[test]
    fn bytecode_source_map_json_rejects_source_fields_for_source_span_invalid() {
        let json = r#"{
            "schema_version": 2,
            "entries": [{
                "object": "Foo",
                "section": "runtime",
                "pc_start": 0,
                "pc_end": 4,
                "kind": "source_span_invalid",
                "reason": "invalid_snippet_range",
                "file": "src/main.fe"
            }]
        }"#;
        let err = serde_json::from_str::<OwnedBytecodeSourceMapExport>(json)
            .expect_err("invalid source-span entries must not carry source payload fields");

        assert!(
            err.to_string()
                .contains("unexpected field `file` for source_span_invalid source-map entry"),
            "{err}"
        );
    }

    #[test]
    fn bytecode_source_map_json_rejects_unknown_sonatina_synthetic_reason() {
        let json = r#"{
            "schema_version": 2,
            "entries": [{
                "object": "Foo",
                "section": "runtime",
                "pc_start": 0,
                "pc_end": 4,
                "kind": "sonatina_synthetic",
                "reason": "mystery"
            }]
        }"#;
        let err = serde_json::from_str::<OwnedBytecodeSourceMapExport>(json)
            .expect_err("source-map sonatina synthetic reasons must be closed");

        assert!(
            err.to_string()
                .contains("unknown bytecode source-map sonatina_synthetic reason `mystery`"),
            "{err}"
        );
    }

    #[test]
    fn bytecode_source_map_json_rejects_unknown_sonatina_unmapped_reason() {
        let json = r#"{
            "schema_version": 2,
            "entries": [{
                "object": "Foo",
                "section": "runtime",
                "pc_start": 0,
                "pc_end": 4,
                "kind": "sonatina_unmapped",
                "reason": "mystery"
            }]
        }"#;
        let err = serde_json::from_str::<OwnedBytecodeSourceMapExport>(json)
            .expect_err("source-map sonatina unmapped reasons must be closed");

        assert!(
            err.to_string()
                .contains("unknown bytecode source-map sonatina_unmapped reason `mystery`"),
            "{err}"
        );
    }

    #[test]
    fn bytecode_source_map_json_rejects_unknown_bytecode_unmapped_reason() {
        let json = r#"{
            "schema_version": 2,
            "entries": [{
                "object": "Foo",
                "section": "runtime",
                "pc_start": 0,
                "pc_end": 4,
                "kind": "bytecode_unmapped",
                "reason": "mystery"
            }]
        }"#;
        let err = serde_json::from_str::<OwnedBytecodeSourceMapExport>(json)
            .expect_err("source-map bytecode unmapped reasons must be closed");

        assert!(
            err.to_string()
                .contains("unknown bytecode source-map bytecode_unmapped reason `mystery`"),
            "{err}"
        );
    }

    #[test]
    fn bytecode_source_map_json_rejects_inverted_source_byte_ranges() {
        let json = r#"{
            "schema_version": 2,
            "entries": [{
                "object": "Foo",
                "section": "runtime",
                "pc_start": 0,
                "pc_end": 4,
                "kind": "source",
                "span_kind": "original",
                "file": "src/main.fe",
                "start_byte": 14,
                "end_byte": 10,
                "start_line": 1,
                "start_col": 2,
                "end_line": 1,
                "end_col": 6,
                "snippet": "main"
            }]
        }"#;
        let err = serde_json::from_str::<OwnedBytecodeSourceMapExport>(json)
            .expect_err("source-map source byte ranges must be ordered");

        assert!(
            err.to_string()
                .contains("bytecode source-map source entry has invalid byte range 14..10"),
            "{err}"
        );
    }

    #[test]
    fn bytecode_source_map_json_rejects_inverted_source_positions() {
        let json = r#"{
            "schema_version": 2,
            "entries": [{
                "object": "Foo",
                "section": "runtime",
                "pc_start": 0,
                "pc_end": 4,
                "kind": "source",
                "span_kind": "original",
                "file": "src/main.fe",
                "start_byte": 10,
                "end_byte": 14,
                "start_line": 1,
                "start_col": 6,
                "end_line": 1,
                "end_col": 2,
                "snippet": "main"
            }]
        }"#;
        let err = serde_json::from_str::<OwnedBytecodeSourceMapExport>(json)
            .expect_err("source-map source line/column ranges must be ordered");

        assert!(
            err.to_string().contains(
                "bytecode source-map source entry has invalid line/column range 1:6..1:2"
            ),
            "{err}"
        );
    }

    #[test]
    fn bytecode_source_map_json_rejects_export_object_mismatch() {
        let json = r#"{
            "schema_version": 2,
            "object": "Foo",
            "entries": [{
                "object": "Bar",
                "section": "runtime",
                "pc_start": 0,
                "pc_end": 4,
                "kind": "semantic_span_missing"
            }]
        }"#;
        let err = serde_json::from_str::<OwnedBytecodeSourceMapExport>(json)
            .expect_err("source-map export object must match entry objects");

        assert!(
            err.to_string().contains(
                "bytecode source-map export object `Foo` does not match entry object `Bar`"
            ),
            "{err}"
        );
    }

    #[test]
    fn bytecode_source_map_json_rejects_export_section_mismatch() {
        let json = r#"{
            "schema_version": 2,
            "section": "runtime",
            "entries": [{
                "object": "Foo",
                "section": "init",
                "pc_start": 0,
                "pc_end": 4,
                "kind": "semantic_span_missing"
            }]
        }"#;
        let err = serde_json::from_str::<OwnedBytecodeSourceMapExport>(json)
            .expect_err("source-map export section must match entry sections");

        assert!(
            err.to_string().contains(
                "bytecode source-map export section `runtime` does not match entry section `init`"
            ),
            "{err}"
        );
    }

    #[test]
    fn bytecode_source_map_json_rejects_overlapping_pc_ranges() {
        let json = r#"{
            "schema_version": 2,
            "entries": [{
                "object": "Foo",
                "section": "runtime",
                "pc_start": 0,
                "pc_end": 8,
                "kind": "semantic_span_missing"
            }, {
                "object": "Foo",
                "section": "runtime",
                "pc_start": 4,
                "pc_end": 12,
                "kind": "runtime_synthetic"
            }]
        }"#;
        let err = serde_json::from_str::<OwnedBytecodeSourceMapExport>(json)
            .expect_err("source-map PC ranges must not overlap within one object section");

        assert!(
            err.to_string().contains(
                "bytecode source-map PC ranges overlap in object `Foo` section `runtime`: 0..8 overlaps 4..12"
            ),
            "{err}"
        );
    }

    #[test]
    fn bytecode_source_map_export_rejects_empty_object_metadata_before_serializing() {
        let err = OwnedBytecodeSourceMapExport::from_serialized_parts(
            Some(String::new()),
            None,
            None,
            Vec::new(),
        )
        .expect_err("source-map export object metadata must be non-empty");

        assert!(
            err.to_string()
                .contains("bytecode source-map export object must not be empty"),
            "{err}"
        );
    }

    #[test]
    fn bytecode_source_map_export_rejects_empty_section_metadata_before_serializing() {
        let err = OwnedBytecodeSourceMapExport::from_serialized_parts(
            None,
            Some(String::new()),
            None,
            Vec::new(),
        )
        .expect_err("source-map export section metadata must be non-empty");

        assert!(
            err.to_string()
                .contains("bytecode source-map export section must not be empty"),
            "{err}"
        );
    }

    #[test]
    fn bytecode_source_map_export_rejects_object_mismatch_before_serializing() {
        let entries = vec![source_map_entry(
            "Foo",
            "runtime",
            0,
            4,
            BytecodeSourceMapEntryKind::SemanticSpanMissing,
        )];
        let object = BytecodeObjectKey::new("Bar");
        let err = bytecode_source_map_entries_export(
            &entries,
            BytecodeSourceMapExportOptions::new()
                .with_metadata(BytecodeSourceMapExportMetadata::object(&object)),
        )
        .expect_err("source-map writer must validate object metadata before serializing");

        assert!(
            err.to_string().contains(
                "bytecode source-map export object `Bar` does not match entry object `Foo`"
            ),
            "{err}"
        );
    }

    #[test]
    fn bytecode_source_map_export_try_new_uses_typed_metadata() {
        let entries = vec![source_map_entry(
            "Foo",
            "runtime",
            0,
            4,
            BytecodeSourceMapEntryKind::SemanticSpanMissing,
        )];
        let object = BytecodeObjectKey::new("Foo");
        let export = OwnedBytecodeSourceMapExport::try_new(
            Some(BytecodeSourceMapExportMetadata::object(&object)),
            entries,
        )
        .expect("typed source-map export metadata should validate");

        assert_eq!(export.object(), Some("Foo"));
        assert_eq!(export.section(), None);
    }

    #[test]
    #[should_panic(expected = "bytecode source-map source file must not be empty")]
    fn bytecode_source_map_entry_rejects_empty_source_files_at_construction() {
        source_map_entry(
            "Foo",
            "runtime",
            0,
            4,
            BytecodeSourceMapEntryKind::Source {
                span_kind: SourceSpanKind::Original,
                file: String::new(),
                start_byte: 10,
                end_byte: 14,
                start_line: 1,
                start_col: 2,
                end_line: 1,
                end_col: 6,
                snippet: "main".to_string(),
            },
        );
    }

    #[test]
    fn bytecode_source_map_entry_try_from_origin_reports_source_semantic_errors() {
        let origin = source_map_origin("Foo", "runtime", 0, 4);
        let err = BytecodeSourceMapEntry::try_from_origin(
            &origin,
            BytecodeSourceMapEntryKind::Source {
                span_kind: SourceSpanKind::Original,
                file: "src/main.fe".to_string(),
                start_byte: 14,
                end_byte: 10,
                start_line: 1,
                start_col: 2,
                end_line: 1,
                end_col: 6,
                snippet: "main".to_string(),
            },
        )
        .expect_err("fallible source-map row construction should expose source range errors");

        assert_eq!(
            err,
            BytecodeSourceMapEntryError::InvalidSourceByteRange {
                start_byte: 14,
                end_byte: 10,
            }
        );
    }

    #[test]
    #[should_panic(expected = "bytecode source-map source snippet must not be empty")]
    fn bytecode_source_map_entry_rejects_empty_source_snippets_at_construction() {
        source_map_entry(
            "Foo",
            "runtime",
            0,
            4,
            BytecodeSourceMapEntryKind::Source {
                span_kind: SourceSpanKind::Original,
                file: "src/main.fe".to_string(),
                start_byte: 10,
                end_byte: 14,
                start_line: 1,
                start_col: 2,
                end_line: 1,
                end_col: 6,
                snippet: String::new(),
            },
        );
    }

    #[test]
    #[should_panic(expected = "bytecode source-map source entry has invalid byte range 14..10")]
    fn bytecode_source_map_entry_rejects_inverted_source_byte_ranges_at_construction() {
        source_map_entry(
            "Foo",
            "runtime",
            0,
            4,
            BytecodeSourceMapEntryKind::Source {
                span_kind: SourceSpanKind::Original,
                file: "src/main.fe".to_string(),
                start_byte: 14,
                end_byte: 10,
                start_line: 1,
                start_col: 2,
                end_line: 1,
                end_col: 6,
                snippet: "main".to_string(),
            },
        );
    }

    #[test]
    #[should_panic(
        expected = "bytecode source-map source entry has invalid line/column range 1:6..1:2"
    )]
    fn bytecode_source_map_entry_rejects_inverted_source_positions_at_construction() {
        source_map_entry(
            "Foo",
            "runtime",
            0,
            4,
            BytecodeSourceMapEntryKind::Source {
                span_kind: SourceSpanKind::Original,
                file: "src/main.fe".to_string(),
                start_byte: 10,
                end_byte: 14,
                start_line: 1,
                start_col: 6,
                end_line: 1,
                end_col: 2,
                snippet: "main".to_string(),
            },
        );
    }

    #[test]
    fn bytecode_source_map_export_rejects_overlapping_pc_ranges_before_serializing() {
        let entries = vec![
            source_map_entry(
                "Foo",
                "runtime",
                0,
                8,
                BytecodeSourceMapEntryKind::SemanticSpanMissing,
            ),
            source_map_entry(
                "Foo",
                "runtime",
                4,
                12,
                BytecodeSourceMapEntryKind::RuntimeSynthetic,
            ),
        ];
        let err =
            bytecode_source_map_entries_export(&entries, BytecodeSourceMapExportOptions::new())
                .expect_err("source-map writer must validate PC range overlap before serializing");

        assert!(
            err.to_string().contains(
                "bytecode source-map PC ranges overlap in object `Foo` section `runtime`: 0..8 overlaps 4..12"
            ),
            "{err}"
        );
    }

    #[test]
    fn bytecode_source_map_json_rejects_coverage_count_mismatch_before_serializing() {
        let entries = vec![
            source_map_entry(
                "Foo",
                "runtime",
                0,
                4,
                BytecodeSourceMapEntryKind::SemanticSpanMissing,
            ),
            source_map_entry(
                "Foo",
                "runtime",
                4,
                8,
                BytecodeSourceMapEntryKind::RuntimeSynthetic,
            ),
        ];
        let err = bytecode_source_map_entries_json(
            &entries,
            BytecodeSourceMapExportOptions::new()
                .with_bytecode_origin_coverage(Some(BytecodeOriginCoverage::new(1, 0, 0))),
        )
        .expect_err("source-map writer must validate coverage totals before serializing");

        assert!(
            err.to_string()
                .contains("bytecode_origin_coverage total 1 does not match 2 source-map entries"),
            "{err}"
        );
    }

    #[test]
    fn bytecode_source_map_export_skips_empty_entries() {
        assert_eq!(
            bytecode_source_map_entries_export(&[], BytecodeSourceMapExportOptions::new())
                .expect("empty export should validate"),
            None
        );
        assert_eq!(
            bytecode_source_map_entries_json(&[], BytecodeSourceMapExportOptions::new())
                .expect("empty export should not fail"),
            None
        );
    }

    #[test]
    fn span_snippet_classifies_invalid_source_ranges_without_panicking() {
        assert_eq!(
            span_snippet("abcd", 1, 3).expect("valid range should produce a snippet"),
            "bc"
        );
        assert_eq!(
            span_snippet("abcd", 3, 1),
            Err(SourceSpanInvalidReason::InvalidByteRange)
        );
        assert_eq!(
            span_snippet("é", 1, 2),
            Err(SourceSpanInvalidReason::InvalidSnippetRange)
        );
        assert_eq!(
            span_snippet("abcd", 2, 2),
            Err(SourceSpanInvalidReason::EmptySnippet)
        );
    }

    #[test]
    fn bytecode_source_map_entries_summary_counts_typed_entry_kinds() {
        let entries = vec![
            source_map_entry(
                "Foo",
                "runtime",
                0,
                4,
                BytecodeSourceMapEntryKind::Source {
                    span_kind: SourceSpanKind::Original,
                    file: "src/main.fe".to_string(),
                    start_byte: 0,
                    end_byte: 4,
                    start_line: 1,
                    start_col: 1,
                    end_line: 1,
                    end_col: 5,
                    snippet: "main".to_string(),
                },
            ),
            source_map_entry(
                "Foo",
                "runtime",
                4,
                8,
                BytecodeSourceMapEntryKind::SourceSpanInvalid {
                    reason: SourceSpanInvalidReason::InvalidSnippetRange,
                },
            ),
            source_map_entry(
                "Foo",
                "runtime",
                8,
                12,
                BytecodeSourceMapEntryKind::RuntimeStmtMissing,
            ),
            source_map_entry(
                "Foo",
                "deploy",
                12,
                16,
                BytecodeSourceMapEntryKind::BytecodeUnmapped {
                    reason: BytecodeUnmappedReason::NoIrInst,
                },
            ),
        ];

        let summary = bytecode_source_map_entries_summary(&entries, Some("Foo"), Some("runtime"))
            .expect("runtime entries should summarize");

        assert_eq!(summary.object(), Some("Foo"));
        assert_eq!(summary.section(), Some("runtime"));
        assert_eq!(summary.total(), 3);
        assert_eq!(summary.source(), 1);
        assert_eq!(summary.source_span_invalid(), 1);
        assert_eq!(summary.runtime_stmt_missing(), 1);
        assert_eq!(summary.bytecode_unmapped(), 0);
        assert_eq!(summary.non_source(), 2);
    }
}
