use std::{collections::BTreeSet, fmt};

use common::{
    InputDb,
    diagnostics::{Span, SpanKind},
    facts::{SourceSpanExport, SourceSpanKind},
};
use serde::{Deserialize, Deserializer, Serialize, de};

use crate::origin::{
    BytecodeObjectKey, BytecodeOriginCoverage, BytecodePcOrigin, BytecodePcRange,
    BytecodeSectionKey, BytecodeSourceResolution, BytecodeSourceResolutionResult,
    BytecodeUnmappedReason, SonatinaPostOptOriginCoverage, SonatinaSyntheticOrigin,
    SonatinaUnmappedReason, bytecode_pc_export_key,
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
    post_opt_origin_coverage: Option<SonatinaPostOptOriginCoverage>,
}

impl<'a> BytecodeSourceMapExportOptions<'a> {
    pub const fn new() -> Self {
        Self {
            metadata: None,
            bytecode_origin_coverage: None,
            post_opt_origin_coverage: None,
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

    pub fn with_post_opt_origin_coverage(
        mut self,
        post_opt_origin_coverage: Option<SonatinaPostOptOriginCoverage>,
    ) -> Self {
        self.post_opt_origin_coverage = post_opt_origin_coverage;
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

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct BytecodeDebugLocationEntry {
    object: String,
    section: String,
    pc_start: u32,
    pc_end: u32,
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

impl BytecodeDebugLocationEntry {
    fn from_serialized_parts(
        object: String,
        section: String,
        pc_start: u32,
        pc_end: u32,
        span_kind: SourceSpanKind,
        file: String,
        start_byte: usize,
        end_byte: usize,
        start_line: usize,
        start_col: usize,
        end_line: usize,
        end_col: usize,
        snippet: String,
    ) -> Result<Self, BytecodeSourceMapExportEntryError> {
        validate_debug_location_entry_parts(
            &object, &section, pc_start, pc_end, &file, start_byte, end_byte, start_line,
            start_col, end_line, end_col, &snippet,
        )?;
        Ok(Self {
            object,
            section,
            pc_start,
            pc_end,
            span_kind,
            file,
            start_byte,
            end_byte,
            start_line,
            start_col,
            end_line,
            end_col,
            snippet,
        })
    }

    fn from_source_map_entry(entry: &BytecodeSourceMapEntry) -> Option<Self> {
        let BytecodeSourceMapEntryKind::Source {
            span_kind,
            file,
            start_byte,
            end_byte,
            start_line,
            start_col,
            end_line,
            end_col,
            snippet,
        } = entry.kind()
        else {
            return None;
        };

        Some(Self {
            object: entry.object().to_string(),
            section: entry.section().to_string(),
            pc_start: entry.pc_start(),
            pc_end: entry.pc_end(),
            span_kind: *span_kind,
            file: file.clone(),
            start_byte: *start_byte,
            end_byte: *end_byte,
            start_line: *start_line,
            start_col: *start_col,
            end_line: *end_line,
            end_col: *end_col,
            snippet: snippet.clone(),
        })
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

    pub const fn span_kind(&self) -> SourceSpanKind {
        self.span_kind
    }

    pub fn file(&self) -> &str {
        &self.file
    }

    pub const fn start_byte(&self) -> usize {
        self.start_byte
    }

    pub const fn end_byte(&self) -> usize {
        self.end_byte
    }

    pub const fn start_line(&self) -> usize {
        self.start_line
    }

    pub const fn start_col(&self) -> usize {
        self.start_col
    }

    pub const fn end_line(&self) -> usize {
        self.end_line
    }

    pub const fn end_col(&self) -> usize {
        self.end_col
    }

    pub fn snippet(&self) -> &str {
        &self.snippet
    }
}

impl<'de> Deserialize<'de> for BytecodeDebugLocationEntry {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct RawLocation {
            object: String,
            section: String,
            pc_start: u32,
            pc_end: u32,
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

        let raw = RawLocation::deserialize(deserializer)?;
        Self::from_serialized_parts(
            raw.object,
            raw.section,
            raw.pc_start,
            raw.pc_end,
            raw.span_kind,
            raw.file,
            raw.start_byte,
            raw.end_byte,
            raw.start_line,
            raw.start_col,
            raw.end_line,
            raw.end_col,
            raw.snippet,
        )
        .map_err(de::Error::custom)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct OwnedBytecodeDebugLocationExport {
    schema_version: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    object: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    section: Option<String>,
    locations: Vec<BytecodeDebugLocationEntry>,
}

impl OwnedBytecodeDebugLocationExport {
    pub const SCHEMA_VERSION: u32 = 1;

    fn from_serialized_parts(
        object: Option<String>,
        section: Option<String>,
        locations: Vec<BytecodeDebugLocationEntry>,
    ) -> Result<Self, BytecodeSourceMapExportEntryError> {
        if locations.is_empty() {
            return Err(BytecodeSourceMapExportEntryError::EmptyDebugLocations);
        }
        validate_debug_location_export_entries(object.as_deref(), section.as_deref(), &locations)?;

        Ok(Self {
            schema_version: Self::SCHEMA_VERSION,
            object,
            section,
            locations,
        })
    }

    fn from_options(
        options: BytecodeSourceMapExportOptions<'_>,
        source_map_entries: &[BytecodeSourceMapEntry],
    ) -> Result<Option<Self>, BytecodeSourceMapExportEntryError> {
        let object = options
            .metadata
            .map(|metadata| metadata.object_name().to_owned());
        let section = options
            .metadata
            .and_then(|metadata| metadata.section_name().map(str::to_owned));
        validate_source_map_export_entries(
            object.as_deref(),
            section.as_deref(),
            source_map_entries,
        )?;

        let locations = source_map_entries
            .iter()
            .filter_map(BytecodeDebugLocationEntry::from_source_map_entry)
            .collect::<Vec<_>>();
        if locations.is_empty() {
            return Ok(None);
        }

        Self::from_serialized_parts(object, section, locations).map(Some)
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

    pub fn locations(&self) -> &[BytecodeDebugLocationEntry] {
        &self.locations
    }
}

impl<'de> Deserialize<'de> for OwnedBytecodeDebugLocationExport {
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
            locations: Vec<BytecodeDebugLocationEntry>,
        }

        let raw = RawExport::deserialize(deserializer)?;
        if raw.schema_version != Self::SCHEMA_VERSION {
            return Err(de::Error::custom(format!(
                "unsupported bytecode debug-location schema_version {}; expected {}",
                raw.schema_version,
                Self::SCHEMA_VERSION
            )));
        }
        Self::from_serialized_parts(raw.object, raw.section, raw.locations)
            .map_err(de::Error::custom)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct BytecodeDebugSourceFile {
    path: String,
}

impl BytecodeDebugSourceFile {
    fn from_serialized_path(path: String) -> Result<Self, BytecodeSourceMapExportEntryError> {
        if path.is_empty() {
            return Err(BytecodeSourceMapExportEntryError::EmptySourceFile);
        }
        Ok(Self { path })
    }

    pub fn path(&self) -> &str {
        &self.path
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct BytecodeDebugLineRow {
    object: String,
    section: String,
    pc_start: u32,
    pc_end: u32,
    file_index: usize,
    span_kind: SourceSpanKind,
    start_byte: usize,
    end_byte: usize,
    start_line: usize,
    start_col: usize,
    end_line: usize,
    end_col: usize,
    snippet: String,
}

impl BytecodeDebugLineRow {
    #[allow(clippy::too_many_arguments)]
    fn from_serialized_parts(
        object: String,
        section: String,
        pc_start: u32,
        pc_end: u32,
        file_index: usize,
        span_kind: SourceSpanKind,
        start_byte: usize,
        end_byte: usize,
        start_line: usize,
        start_col: usize,
        end_line: usize,
        end_col: usize,
        snippet: String,
    ) -> Self {
        Self {
            object,
            section,
            pc_start,
            pc_end,
            file_index,
            span_kind,
            start_byte,
            end_byte,
            start_line,
            start_col,
            end_line,
            end_col,
            snippet,
        }
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

    pub const fn file_index(&self) -> usize {
        self.file_index
    }

    pub const fn span_kind(&self) -> SourceSpanKind {
        self.span_kind
    }

    pub const fn start_byte(&self) -> usize {
        self.start_byte
    }

    pub const fn end_byte(&self) -> usize {
        self.end_byte
    }

    pub const fn start_line(&self) -> usize {
        self.start_line
    }

    pub const fn start_col(&self) -> usize {
        self.start_col
    }

    pub const fn end_line(&self) -> usize {
        self.end_line
    }

    pub const fn end_col(&self) -> usize {
        self.end_col
    }

    pub fn snippet(&self) -> &str {
        &self.snippet
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BytecodeDebugLineRecord<'a> {
    file: &'a BytecodeDebugSourceFile,
    row: &'a BytecodeDebugLineRow,
}

impl<'a> BytecodeDebugLineRecord<'a> {
    pub const fn source_file(&self) -> &'a BytecodeDebugSourceFile {
        self.file
    }

    pub const fn row(&self) -> &'a BytecodeDebugLineRow {
        self.row
    }

    pub fn object(&self) -> &str {
        self.row.object()
    }

    pub fn section(&self) -> &str {
        self.row.section()
    }

    pub const fn pc_start(&self) -> u32 {
        self.row.pc_start()
    }

    pub const fn pc_end(&self) -> u32 {
        self.row.pc_end()
    }

    pub fn file(&self) -> &str {
        self.file.path()
    }

    pub const fn span_kind(&self) -> SourceSpanKind {
        self.row.span_kind()
    }

    pub const fn start_byte(&self) -> usize {
        self.row.start_byte()
    }

    pub const fn end_byte(&self) -> usize {
        self.row.end_byte()
    }

    pub const fn start_line(&self) -> usize {
        self.row.start_line()
    }

    pub const fn start_col(&self) -> usize {
        self.row.start_col()
    }

    pub const fn end_line(&self) -> usize {
        self.row.end_line()
    }

    pub const fn end_col(&self) -> usize {
        self.row.end_col()
    }

    pub fn snippet(&self) -> &str {
        self.row.snippet()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BytecodeDebugLineTable {
    object: Option<String>,
    section: Option<String>,
    files: Vec<BytecodeDebugSourceFile>,
    rows: Vec<BytecodeDebugLineRow>,
}

impl BytecodeDebugLineTable {
    pub fn from_debug_locations(export: &OwnedBytecodeDebugLocationExport) -> Self {
        let mut files = Vec::<BytecodeDebugSourceFile>::new();
        let mut rows = Vec::with_capacity(export.locations().len());

        for location in export.locations() {
            let file_index = match files.iter().position(|file| file.path() == location.file()) {
                Some(index) => index,
                None => {
                    let index = files.len();
                    files.push(BytecodeDebugSourceFile {
                        path: location.file().to_string(),
                    });
                    index
                }
            };
            rows.push(BytecodeDebugLineRow {
                object: location.object().to_string(),
                section: location.section().to_string(),
                pc_start: location.pc_start(),
                pc_end: location.pc_end(),
                file_index,
                span_kind: location.span_kind(),
                start_byte: location.start_byte(),
                end_byte: location.end_byte(),
                start_line: location.start_line(),
                start_col: location.start_col(),
                end_line: location.end_line(),
                end_col: location.end_col(),
                snippet: location.snippet().to_string(),
            });
        }

        Self {
            object: export.object().map(str::to_owned),
            section: export.section().map(str::to_owned),
            files,
            rows,
        }
    }

    pub fn object(&self) -> Option<&str> {
        self.object.as_deref()
    }

    pub fn section(&self) -> Option<&str> {
        self.section.as_deref()
    }

    pub fn files(&self) -> &[BytecodeDebugSourceFile] {
        &self.files
    }

    pub fn rows(&self) -> &[BytecodeDebugLineRow] {
        &self.rows
    }

    pub fn line_records(&self) -> impl Iterator<Item = BytecodeDebugLineRecord<'_>> {
        debug_line_records(&self.files, &self.rows)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct OwnedBytecodeDebugLineTableExport {
    schema_version: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    object: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    section: Option<String>,
    files: Vec<BytecodeDebugSourceFile>,
    rows: Vec<BytecodeDebugLineRow>,
}

impl OwnedBytecodeDebugLineTableExport {
    pub const SCHEMA_VERSION: u32 = 1;

    fn from_serialized_parts(
        object: Option<String>,
        section: Option<String>,
        files: Vec<BytecodeDebugSourceFile>,
        rows: Vec<BytecodeDebugLineRow>,
    ) -> Result<Self, BytecodeSourceMapExportEntryError> {
        validate_debug_line_table_entries(object.as_deref(), section.as_deref(), &files, &rows)?;

        Ok(Self {
            schema_version: Self::SCHEMA_VERSION,
            object,
            section,
            files,
            rows,
        })
    }

    pub fn from_debug_locations(
        export: &OwnedBytecodeDebugLocationExport,
    ) -> Result<Self, BytecodeSourceMapExportEntryError> {
        let table = BytecodeDebugLineTable::from_debug_locations(export);
        Self::from_serialized_parts(table.object, table.section, table.files, table.rows)
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

    pub fn files(&self) -> &[BytecodeDebugSourceFile] {
        &self.files
    }

    pub fn rows(&self) -> &[BytecodeDebugLineRow] {
        &self.rows
    }

    pub fn line_records(&self) -> impl Iterator<Item = BytecodeDebugLineRecord<'_>> {
        debug_line_records(&self.files, &self.rows)
    }
}

fn debug_line_records<'a>(
    files: &'a [BytecodeDebugSourceFile],
    rows: &'a [BytecodeDebugLineRow],
) -> impl Iterator<Item = BytecodeDebugLineRecord<'a>> {
    rows.iter().map(move |row| {
        let file = files
            .get(row.file_index())
            .expect("validated bytecode debug line tables contain valid file indices");
        BytecodeDebugLineRecord { file, row }
    })
}

impl<'de> Deserialize<'de> for OwnedBytecodeDebugLineTableExport {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct RawFile {
            path: String,
        }

        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct RawRow {
            object: String,
            section: String,
            pc_start: u32,
            pc_end: u32,
            file_index: usize,
            span_kind: SourceSpanKind,
            start_byte: usize,
            end_byte: usize,
            start_line: usize,
            start_col: usize,
            end_line: usize,
            end_col: usize,
            snippet: String,
        }

        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct RawExport {
            schema_version: u32,
            object: Option<String>,
            section: Option<String>,
            files: Vec<RawFile>,
            rows: Vec<RawRow>,
        }

        let raw = RawExport::deserialize(deserializer)?;
        if raw.schema_version != Self::SCHEMA_VERSION {
            return Err(de::Error::custom(format!(
                "unsupported bytecode debug-line-table schema_version {}; expected {}",
                raw.schema_version,
                Self::SCHEMA_VERSION
            )));
        }
        let files = raw
            .files
            .into_iter()
            .map(|file| BytecodeDebugSourceFile::from_serialized_path(file.path))
            .collect::<Result<Vec<_>, _>>()
            .map_err(de::Error::custom)?;
        let rows = raw
            .rows
            .into_iter()
            .map(|row| {
                BytecodeDebugLineRow::from_serialized_parts(
                    row.object,
                    row.section,
                    row.pc_start,
                    row.pc_end,
                    row.file_index,
                    row.span_kind,
                    row.start_byte,
                    row.end_byte,
                    row.start_line,
                    row.start_col,
                    row.end_line,
                    row.end_col,
                    row.snippet,
                )
            })
            .collect::<Vec<_>>();
        Self::from_serialized_parts(raw.object, raw.section, files, rows).map_err(de::Error::custom)
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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub struct SonatinaPostOptOriginCoverageExport {
    total: usize,
    same_inst_id: usize,
    created_or_unmatched_after_preopt_snapshot: usize,
    pre_opt_snapshot_losses: usize,
    observed_pre_opt_total: usize,
}

impl From<SonatinaPostOptOriginCoverage> for SonatinaPostOptOriginCoverageExport {
    fn from(coverage: SonatinaPostOptOriginCoverage) -> Self {
        Self {
            total: coverage.total(),
            same_inst_id: coverage.same_inst_id(),
            created_or_unmatched_after_preopt_snapshot: coverage
                .created_or_unmatched_after_preopt_snapshot(),
            pre_opt_snapshot_losses: coverage.pre_opt_snapshot_losses(),
            observed_pre_opt_total: coverage.observed_pre_opt_total(),
        }
    }
}

impl SonatinaPostOptOriginCoverageExport {
    pub const fn total(&self) -> usize {
        self.total
    }

    pub const fn same_inst_id(&self) -> usize {
        self.same_inst_id
    }

    pub const fn created_or_unmatched_after_preopt_snapshot(&self) -> usize {
        self.created_or_unmatched_after_preopt_snapshot
    }

    pub const fn pre_opt_snapshot_losses(&self) -> usize {
        self.pre_opt_snapshot_losses
    }

    pub const fn observed_pre_opt_total(&self) -> usize {
        self.observed_pre_opt_total
    }

    pub const fn post_opt_classified_total(&self) -> usize {
        self.same_inst_id + self.created_or_unmatched_after_preopt_snapshot
    }

    pub const fn computed_observed_pre_opt_total(&self) -> usize {
        self.same_inst_id + self.pre_opt_snapshot_losses
    }
}

impl<'de> Deserialize<'de> for SonatinaPostOptOriginCoverageExport {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct RawCoverage {
            total: usize,
            same_inst_id: usize,
            created_or_unmatched_after_preopt_snapshot: usize,
            pre_opt_snapshot_losses: usize,
            observed_pre_opt_total: usize,
        }

        let raw = RawCoverage::deserialize(deserializer)?;
        let classified_total = raw
            .same_inst_id
            .checked_add(raw.created_or_unmatched_after_preopt_snapshot)
            .ok_or_else(|| {
                de::Error::custom("post_opt_origin_coverage classified total overflows usize")
            })?;
        if raw.total != classified_total {
            return Err(de::Error::custom(format!(
                "post_opt_origin_coverage total {} does not match classified total {}",
                raw.total, classified_total
            )));
        }
        let observed_pre_opt_total = raw
            .same_inst_id
            .checked_add(raw.pre_opt_snapshot_losses)
            .ok_or_else(|| {
                de::Error::custom("post_opt_origin_coverage observed pre-opt total overflows usize")
            })?;
        if raw.observed_pre_opt_total != observed_pre_opt_total {
            return Err(de::Error::custom(format!(
                "post_opt_origin_coverage observed_pre_opt_total {} does not match same_inst_id plus pre_opt_snapshot_losses {}",
                raw.observed_pre_opt_total, observed_pre_opt_total
            )));
        }

        Ok(Self {
            total: raw.total,
            same_inst_id: raw.same_inst_id,
            created_or_unmatched_after_preopt_snapshot: raw
                .created_or_unmatched_after_preopt_snapshot,
            pre_opt_snapshot_losses: raw.pre_opt_snapshot_losses,
            observed_pre_opt_total: raw.observed_pre_opt_total,
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BytecodeDebugArtifactMetadataMismatch {
    source_map_object: Option<String>,
    source_map_section: Option<String>,
    debug_locations_object: Option<String>,
    debug_locations_section: Option<String>,
}

impl BytecodeDebugArtifactMetadataMismatch {
    fn new(
        source_map_object: Option<String>,
        source_map_section: Option<String>,
        debug_locations_object: Option<String>,
        debug_locations_section: Option<String>,
    ) -> Self {
        Self {
            source_map_object,
            source_map_section,
            debug_locations_object,
            debug_locations_section,
        }
    }

    pub fn source_map_object(&self) -> Option<&str> {
        self.source_map_object.as_deref()
    }

    pub fn source_map_section(&self) -> Option<&str> {
        self.source_map_section.as_deref()
    }

    pub fn debug_locations_object(&self) -> Option<&str> {
        self.debug_locations_object.as_deref()
    }

    pub fn debug_locations_section(&self) -> Option<&str> {
        self.debug_locations_section.as_deref()
    }
}

impl fmt::Display for BytecodeDebugArtifactMetadataMismatch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "debug artifact metadata mismatch: source-map object={:?} section={:?}; debug-location object={:?} section={:?}",
            self.source_map_object(),
            self.source_map_section(),
            self.debug_locations_object(),
            self.debug_locations_section()
        )
    }
}

#[derive(Debug)]
pub enum BytecodeDebugArtifactsExportError {
    MetadataMismatch(BytecodeDebugArtifactMetadataMismatch),
    SourceMap(BytecodeSourceMapExportEntryError),
    DebugLocations(BytecodeSourceMapExportEntryError),
    DebugLineTable(BytecodeSourceMapExportEntryError),
}

impl fmt::Display for BytecodeDebugArtifactsExportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MetadataMismatch(err) => err.fmt(f),
            Self::SourceMap(err) => write!(f, "source-map artifact: {err}"),
            Self::DebugLocations(err) => write!(f, "debug-location artifact: {err}"),
            Self::DebugLineTable(err) => write!(f, "debug-line-table artifact: {err}"),
        }
    }
}

impl std::error::Error for BytecodeDebugArtifactsExportError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::MetadataMismatch { .. } => None,
            Self::SourceMap(err) | Self::DebugLocations(err) | Self::DebugLineTable(err) => {
                Some(err)
            }
        }
    }
}

impl From<BytecodeDebugArtifactsExportError> for BytecodeDebugArtifactsJsonError {
    fn from(err: BytecodeDebugArtifactsExportError) -> Self {
        match err {
            BytecodeDebugArtifactsExportError::MetadataMismatch(err) => Self::MetadataMismatch(err),
            BytecodeDebugArtifactsExportError::SourceMap(err) => Self::SourceMap(err.into()),
            BytecodeDebugArtifactsExportError::DebugLocations(err) => {
                Self::DebugLocations(err.into())
            }
            BytecodeDebugArtifactsExportError::DebugLineTable(err) => {
                Self::DebugLineTable(err.into())
            }
        }
    }
}

#[derive(Debug)]
pub enum BytecodeDebugArtifactsJsonError {
    MetadataMismatch(BytecodeDebugArtifactMetadataMismatch),
    SourceMap(BytecodeSourceMapExportError),
    DebugLocations(BytecodeSourceMapExportError),
    DebugLineTable(BytecodeSourceMapExportError),
}

impl fmt::Display for BytecodeDebugArtifactsJsonError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MetadataMismatch(err) => err.fmt(f),
            Self::SourceMap(err) => write!(f, "source-map artifact: {err}"),
            Self::DebugLocations(err) => write!(f, "debug-location artifact: {err}"),
            Self::DebugLineTable(err) => write!(f, "debug-line-table artifact: {err}"),
        }
    }
}

impl std::error::Error for BytecodeDebugArtifactsJsonError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::MetadataMismatch { .. } => None,
            Self::SourceMap(err) | Self::DebugLocations(err) | Self::DebugLineTable(err) => {
                Some(err)
            }
        }
    }
}

fn validate_source_map_export_entries(
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

fn validate_debug_location_entry_parts(
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

fn validate_debug_location_export_entries(
    object: Option<&str>,
    section: Option<&str>,
    locations: &[BytecodeDebugLocationEntry],
) -> Result<(), BytecodeSourceMapExportEntryError> {
    for location in locations {
        validate_debug_location_entry_parts(
            location.object(),
            location.section(),
            location.pc_start(),
            location.pc_end(),
            location.file(),
            location.start_byte(),
            location.end_byte(),
            location.start_line(),
            location.start_col(),
            location.end_line(),
            location.end_col(),
            location.snippet(),
        )?;
    }

    validate_export_metadata_and_pc_ranges(object, section, locations)
}

fn validate_debug_line_table_entries(
    object: Option<&str>,
    section: Option<&str>,
    files: &[BytecodeDebugSourceFile],
    rows: &[BytecodeDebugLineRow],
) -> Result<(), BytecodeSourceMapExportEntryError> {
    if files.is_empty() {
        return Err(BytecodeSourceMapExportEntryError::EmptyDebugLineTableFiles);
    }
    if rows.is_empty() {
        return Err(BytecodeSourceMapExportEntryError::EmptyDebugLineTableRows);
    }
    for file in files {
        if file.path().is_empty() {
            return Err(BytecodeSourceMapExportEntryError::EmptySourceFile);
        }
    }
    for row in rows {
        let Some(file) = files.get(row.file_index()) else {
            return Err(
                BytecodeSourceMapExportEntryError::InvalidDebugLineTableFileIndex {
                    file_index: row.file_index(),
                    file_count: files.len(),
                },
            );
        };
        validate_debug_location_entry_parts(
            row.object(),
            row.section(),
            row.pc_start(),
            row.pc_end(),
            file.path(),
            row.start_byte(),
            row.end_byte(),
            row.start_line(),
            row.start_col(),
            row.end_line(),
            row.end_col(),
            row.snippet(),
        )?;
    }

    validate_export_metadata_and_pc_ranges(object, section, rows)
}

trait BytecodePcExportEntry {
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

impl BytecodePcExportEntry for BytecodeDebugLocationEntry {
    fn object(&self) -> &str {
        BytecodeDebugLocationEntry::object(self)
    }

    fn section(&self) -> &str {
        BytecodeDebugLocationEntry::section(self)
    }

    fn pc_start(&self) -> u32 {
        BytecodeDebugLocationEntry::pc_start(self)
    }

    fn pc_end(&self) -> u32 {
        BytecodeDebugLocationEntry::pc_end(self)
    }
}

impl BytecodePcExportEntry for BytecodeDebugLineRow {
    fn object(&self) -> &str {
        BytecodeDebugLineRow::object(self)
    }

    fn section(&self) -> &str {
        BytecodeDebugLineRow::section(self)
    }

    fn pc_start(&self) -> u32 {
        BytecodeDebugLineRow::pc_start(self)
    }

    fn pc_end(&self) -> u32 {
        BytecodeDebugLineRow::pc_end(self)
    }
}

fn validate_export_metadata_and_pc_ranges<T: BytecodePcExportEntry>(
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

#[derive(Debug, Default, PartialEq, Eq)]
pub struct BytecodeDebugArtifactsExport {
    source_map: Option<OwnedBytecodeSourceMapExport>,
    debug_locations: Option<OwnedBytecodeDebugLocationExport>,
    debug_line_table: Option<OwnedBytecodeDebugLineTableExport>,
}

impl BytecodeDebugArtifactsExport {
    pub fn source_map(&self) -> Option<&OwnedBytecodeSourceMapExport> {
        self.source_map.as_ref()
    }

    pub fn debug_locations(&self) -> Option<&OwnedBytecodeDebugLocationExport> {
        self.debug_locations.as_ref()
    }

    pub fn debug_line_table(&self) -> Option<&OwnedBytecodeDebugLineTableExport> {
        self.debug_line_table.as_ref()
    }
}

pub fn bytecode_debug_artifacts_export(
    source_map_entries: &[BytecodeSourceMapEntry],
    source_map_options: BytecodeSourceMapExportOptions<'_>,
    debug_location_options: BytecodeSourceMapExportOptions<'_>,
) -> Result<BytecodeDebugArtifactsExport, BytecodeDebugArtifactsExportError> {
    validate_debug_artifact_metadata(source_map_options.metadata, debug_location_options.metadata)?;
    let source_map = bytecode_source_map_entries_export(source_map_entries, source_map_options)
        .map_err(BytecodeDebugArtifactsExportError::SourceMap)?;
    let debug_locations =
        bytecode_debug_location_entries_export(source_map_entries, debug_location_options)
            .map_err(BytecodeDebugArtifactsExportError::DebugLocations)?;
    let debug_line_table = debug_locations
        .as_ref()
        .map(OwnedBytecodeDebugLineTableExport::from_debug_locations)
        .transpose()
        .map_err(BytecodeDebugArtifactsExportError::DebugLineTable)?;

    Ok(BytecodeDebugArtifactsExport {
        source_map,
        debug_locations,
        debug_line_table,
    })
}

fn export_metadata_parts(
    metadata: Option<BytecodeSourceMapExportMetadata<'_>>,
) -> (Option<&str>, Option<&str>) {
    match metadata {
        Some(metadata) => (Some(metadata.object_name()), metadata.section_name()),
        None => (None, None),
    }
}

fn validate_debug_artifact_metadata(
    source_map_metadata: Option<BytecodeSourceMapExportMetadata<'_>>,
    debug_location_metadata: Option<BytecodeSourceMapExportMetadata<'_>>,
) -> Result<(), BytecodeDebugArtifactsExportError> {
    let source_map_parts = export_metadata_parts(source_map_metadata);
    let debug_location_parts = export_metadata_parts(debug_location_metadata);
    if source_map_parts == debug_location_parts {
        return Ok(());
    }

    Err(BytecodeDebugArtifactsExportError::MetadataMismatch(
        BytecodeDebugArtifactMetadataMismatch::new(
            source_map_parts.0.map(str::to_owned),
            source_map_parts.1.map(str::to_owned),
            debug_location_parts.0.map(str::to_owned),
            debug_location_parts.1.map(str::to_owned),
        ),
    ))
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct BytecodeDebugArtifactsJson {
    source_map: Option<String>,
    debug_locations: Option<String>,
    debug_line_table: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BytecodeDebugArtifactKind {
    SourceMap,
    DebugLocations,
    DebugLineTable,
}

impl BytecodeDebugArtifactKind {
    pub const fn file_name(self) -> &'static str {
        match self {
            Self::SourceMap => "source_map.json",
            Self::DebugLocations => "debug_locations.json",
            Self::DebugLineTable => "debug_line_table.json",
        }
    }

    pub fn file_name_with_base(self, base: &str) -> String {
        format!("{base}.{}", self.file_name())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BytecodeDebugArtifactJson<'a> {
    kind: BytecodeDebugArtifactKind,
    json: &'a str,
}

impl<'a> BytecodeDebugArtifactJson<'a> {
    pub const fn kind(self) -> BytecodeDebugArtifactKind {
        self.kind
    }

    pub const fn json(self) -> &'a str {
        self.json
    }

    pub const fn file_name(self) -> &'static str {
        self.kind.file_name()
    }

    pub fn file_name_with_base(self, base: &str) -> String {
        self.kind.file_name_with_base(base)
    }
}

impl BytecodeDebugArtifactsJson {
    pub fn source_map(&self) -> Option<&str> {
        self.source_map.as_deref()
    }

    pub fn debug_locations(&self) -> Option<&str> {
        self.debug_locations.as_deref()
    }

    pub fn debug_line_table(&self) -> Option<&str> {
        self.debug_line_table.as_deref()
    }

    pub fn artifacts(&self) -> impl Iterator<Item = BytecodeDebugArtifactJson<'_>> {
        [
            (BytecodeDebugArtifactKind::SourceMap, self.source_map()),
            (
                BytecodeDebugArtifactKind::DebugLocations,
                self.debug_locations(),
            ),
            (
                BytecodeDebugArtifactKind::DebugLineTable,
                self.debug_line_table(),
            ),
        ]
        .into_iter()
        .filter_map(|(kind, json)| json.map(|json| BytecodeDebugArtifactJson { kind, json }))
    }
}

pub fn bytecode_debug_artifacts_json(
    source_map_entries: &[BytecodeSourceMapEntry],
    source_map_options: BytecodeSourceMapExportOptions<'_>,
    debug_location_options: BytecodeSourceMapExportOptions<'_>,
) -> Result<BytecodeDebugArtifactsJson, BytecodeDebugArtifactsJsonError> {
    let exports = bytecode_debug_artifacts_export(
        source_map_entries,
        source_map_options,
        debug_location_options,
    )
    .map_err(BytecodeDebugArtifactsJsonError::from)?;
    let source_map = exports
        .source_map()
        .map(serde_json::to_string)
        .transpose()
        .map_err(BytecodeSourceMapExportError::from)
        .map_err(BytecodeDebugArtifactsJsonError::SourceMap)?;
    let debug_locations = exports
        .debug_locations()
        .map(serde_json::to_string)
        .transpose()
        .map_err(BytecodeSourceMapExportError::from)
        .map_err(BytecodeDebugArtifactsJsonError::DebugLocations)?;
    let debug_line_table = exports
        .debug_line_table()
        .map(serde_json::to_string)
        .transpose()
        .map_err(BytecodeSourceMapExportError::from)
        .map_err(BytecodeDebugArtifactsJsonError::DebugLineTable)?;

    Ok(BytecodeDebugArtifactsJson {
        source_map,
        debug_locations,
        debug_line_table,
    })
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
        SonatinaPostOptOriginCoverage,
    };

    use super::{
        BytecodeDebugArtifactKind, BytecodeDebugArtifactsExportError, BytecodeSourceMapEntry,
        BytecodeSourceMapEntryError, BytecodeSourceMapEntryKind, BytecodeSourceMapExportMetadata,
        BytecodeSourceMapExportOptions, OwnedBytecodeDebugLineTableExport,
        OwnedBytecodeDebugLocationExport, OwnedBytecodeSourceMapExport, SourceSpanInvalidReason,
        SourceSpanKind, bytecode_debug_artifacts_export, bytecode_debug_artifacts_json,
        bytecode_debug_location_entries_export, bytecode_debug_location_entries_json,
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

    fn debug_location_value(
        object: &str,
        section: &str,
        pc_start: u32,
        pc_end: u32,
    ) -> serde_json::Value {
        serde_json::json!({
            "object": object,
            "section": section,
            "pc_start": pc_start,
            "pc_end": pc_end,
            "span_kind": "original",
            "file": "src/main.fe",
            "start_byte": 10,
            "end_byte": 14,
            "start_line": 1,
            "start_col": 2,
            "end_line": 1,
            "end_col": 6,
            "snippet": "main"
        })
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
        let post_opt_origin_coverage = SonatinaPostOptOriginCoverage::new(1, 1, 1);
        let json = bytecode_source_map_entries_json(
            &entries,
            options
                .with_bytecode_origin_coverage(Some(bytecode_origin_coverage))
                .with_post_opt_origin_coverage(Some(post_opt_origin_coverage)),
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
        let decoded_post_opt_coverage = decoded
            .post_opt_origin_coverage()
            .expect("post-opt coverage should roundtrip through source-map JSON");
        assert_eq!(decoded_post_opt_coverage.total(), 2);
        assert_eq!(decoded_post_opt_coverage.same_inst_id(), 1);
        assert_eq!(
            decoded_post_opt_coverage.created_or_unmatched_after_preopt_snapshot(),
            1
        );
        assert_eq!(decoded_post_opt_coverage.pre_opt_snapshot_losses(), 1);
        assert_eq!(decoded_post_opt_coverage.observed_pre_opt_total(), 2);
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
        let json = r#"{"schema_version":3,"entries":[],"extra":true}"#;
        let err = serde_json::from_str::<OwnedBytecodeSourceMapExport>(json)
            .expect_err("unknown source-map export fields must fail closed");

        assert!(err.to_string().contains("unknown field"), "{err}");
    }

    #[test]
    fn bytecode_source_map_json_rejects_empty_export_objects() {
        let json = r#"{"schema_version":3,"object":"","entries":[]}"#;
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
        let json = r#"{"schema_version":3,"section":"","entries":[]}"#;
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
            "schema_version": 3,
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
            "schema_version": 3,
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
            "schema_version": 3,
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
            "schema_version": 3,
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
            "schema_version": 3,
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
            "schema_version": 3,
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
            "schema_version": 3,
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
    fn bytecode_source_map_json_rejects_unknown_post_opt_coverage_fields() {
        let json = r#"{
            "schema_version": 3,
            "post_opt_origin_coverage": {
                "total": 1,
                "same_inst_id": 1,
                "created_or_unmatched_after_preopt_snapshot": 0,
                "pre_opt_snapshot_losses": 0,
                "observed_pre_opt_total": 1,
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
            .expect_err("unknown source-map post-opt coverage fields must fail closed");

        assert!(err.to_string().contains("unknown field"), "{err}");
    }

    #[test]
    fn bytecode_source_map_json_rejects_inconsistent_post_opt_coverage_partition() {
        let json = r#"{
            "schema_version": 3,
            "post_opt_origin_coverage": {
                "total": 2,
                "same_inst_id": 1,
                "created_or_unmatched_after_preopt_snapshot": 0,
                "pre_opt_snapshot_losses": 0,
                "observed_pre_opt_total": 1
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
            .expect_err("source-map post-opt coverage partitions must match their total");

        assert!(
            err.to_string()
                .contains("post_opt_origin_coverage total 2 does not match classified total 1"),
            "{err}"
        );
    }

    #[test]
    fn bytecode_source_map_json_rejects_inconsistent_post_opt_observed_total() {
        let json = r#"{
            "schema_version": 3,
            "post_opt_origin_coverage": {
                "total": 1,
                "same_inst_id": 1,
                "created_or_unmatched_after_preopt_snapshot": 0,
                "pre_opt_snapshot_losses": 1,
                "observed_pre_opt_total": 1
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
            .expect_err("source-map post-opt observed pre-opt totals must match");

        assert!(
            err.to_string().contains(
                "post_opt_origin_coverage observed_pre_opt_total 1 does not match same_inst_id plus pre_opt_snapshot_losses 2"
            ),
            "{err}"
        );
    }

    #[test]
    fn bytecode_source_map_json_rejects_unknown_source_fields() {
        let json = r#"{
            "schema_version": 3,
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
            "schema_version": 3,
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
            "schema_version": 3,
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
            "schema_version": 3,
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
            "schema_version": 3,
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
            "schema_version": 3,
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
            "schema_version": 3,
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
            "schema_version": 3,
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
            "schema_version": 3,
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
            "schema_version": 3,
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
            "schema_version": 3,
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
            "schema_version": 3,
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
            "schema_version": 3,
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
            "schema_version": 3,
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
            "schema_version": 3,
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
            "schema_version": 3,
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
    fn bytecode_debug_location_export_uses_only_valid_source_entries() {
        let entries = vec![
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
                    snippet: "main".to_string(),
                },
            ),
            source_map_entry(
                "Foo",
                "runtime",
                4,
                8,
                BytecodeSourceMapEntryKind::BytecodeUnmapped {
                    reason: BytecodeUnmappedReason::Synthetic,
                },
            ),
        ];
        let object = BytecodeObjectKey::new("Foo");
        let options = BytecodeSourceMapExportOptions::new()
            .with_metadata(BytecodeSourceMapExportMetadata::object(&object));
        let export = bytecode_debug_location_entries_export(&entries, options)
            .expect("debug location export should validate")
            .expect("source entries should produce debug locations");

        assert_eq!(export.schema_version(), 1);
        assert_eq!(export.object(), Some("Foo"));
        assert_eq!(export.section(), None);
        assert_eq!(export.locations().len(), 1);
        assert_eq!(export.locations()[0].object(), "Foo");
        assert_eq!(export.locations()[0].section(), "runtime");
        assert_eq!(export.locations()[0].pc_start(), 0);
        assert_eq!(export.locations()[0].pc_end(), 4);
        assert_eq!(export.locations()[0].span_kind(), SourceSpanKind::Original);
        assert_eq!(export.locations()[0].file(), "src/main.fe");
        assert_eq!(export.locations()[0].start_byte(), 10);
        assert_eq!(export.locations()[0].end_byte(), 14);
        assert_eq!(export.locations()[0].start_line(), 1);
        assert_eq!(export.locations()[0].start_col(), 2);
        assert_eq!(export.locations()[0].end_line(), 1);
        assert_eq!(export.locations()[0].end_col(), 6);
        assert_eq!(export.locations()[0].snippet(), "main");

        let json = bytecode_debug_location_entries_json(&entries, options)
            .expect("debug location JSON should serialize")
            .expect("source entries should produce debug location JSON");
        let decoded: OwnedBytecodeDebugLocationExport =
            serde_json::from_str(&json).expect("debug location JSON should roundtrip");

        assert_eq!(
            decoded.schema_version(),
            OwnedBytecodeDebugLocationExport::SCHEMA_VERSION
        );
        assert_eq!(decoded.object(), Some("Foo"));
        assert_eq!(decoded.section(), None);
        assert_eq!(decoded.locations(), export.locations());
    }

    #[test]
    fn bytecode_debug_artifacts_json_renders_source_map_and_debug_locations_together() {
        let entries = vec![
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
                    snippet: "main".to_string(),
                },
            ),
            source_map_entry(
                "Foo",
                "runtime",
                4,
                8,
                BytecodeSourceMapEntryKind::BytecodeUnmapped {
                    reason: BytecodeUnmappedReason::Synthetic,
                },
            ),
        ];
        let object = BytecodeObjectKey::new("Foo");
        let source_map_options = BytecodeSourceMapExportOptions::new()
            .with_object_key(&object)
            .with_bytecode_origin_coverage(Some(BytecodeOriginCoverage::new(1, 0, 1)))
            .with_post_opt_origin_coverage(Some(SonatinaPostOptOriginCoverage::new(1, 0, 1)));
        let debug_location_options = BytecodeSourceMapExportOptions::new().with_object_key(&object);

        let artifacts =
            bytecode_debug_artifacts_json(&entries, source_map_options, debug_location_options)
                .expect("debug artifacts should render together");
        let source_map: OwnedBytecodeSourceMapExport = serde_json::from_str(
            artifacts
                .source_map()
                .expect("source-map JSON should render"),
        )
        .expect("source-map JSON should decode");
        let debug_locations: OwnedBytecodeDebugLocationExport = serde_json::from_str(
            artifacts
                .debug_locations()
                .expect("debug-location JSON should render"),
        )
        .expect("debug-location JSON should decode");
        let debug_line_table: OwnedBytecodeDebugLineTableExport = serde_json::from_str(
            artifacts
                .debug_line_table()
                .expect("debug-line-table JSON should render"),
        )
        .expect("debug-line-table JSON should decode");

        assert_eq!(source_map.object(), Some("Foo"));
        assert_eq!(source_map.entries().len(), 2);
        assert_eq!(
            source_map
                .bytecode_origin_coverage()
                .expect("coverage should render")
                .total(),
            2
        );
        assert_eq!(
            source_map
                .post_opt_origin_coverage()
                .expect("post-opt coverage should render")
                .observed_pre_opt_total(),
            2
        );
        assert_eq!(debug_locations.object(), Some("Foo"));
        assert_eq!(debug_locations.locations().len(), 1);
        assert_eq!(debug_locations.locations()[0].snippet(), "main");
        assert_eq!(debug_line_table.object(), Some("Foo"));
        assert_eq!(debug_line_table.files().len(), 1);
        assert_eq!(debug_line_table.rows().len(), 1);
        assert_eq!(debug_line_table.rows()[0].snippet(), "main");
    }

    #[test]
    fn bytecode_debug_artifacts_json_iterates_stable_artifact_filenames() {
        let entries = vec![
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
                    snippet: "main".to_string(),
                },
            ),
            source_map_entry(
                "Foo",
                "runtime",
                4,
                8,
                BytecodeSourceMapEntryKind::BytecodeUnmapped {
                    reason: BytecodeUnmappedReason::Synthetic,
                },
            ),
        ];
        let object = BytecodeObjectKey::new("Foo");
        let source_map_options = BytecodeSourceMapExportOptions::new().with_object_key(&object);
        let debug_location_options = BytecodeSourceMapExportOptions::new().with_object_key(&object);
        let artifacts =
            bytecode_debug_artifacts_json(&entries, source_map_options, debug_location_options)
                .expect("debug artifacts should render together");
        let filenames = artifacts
            .artifacts()
            .map(|artifact| {
                (
                    artifact.kind(),
                    artifact.file_name(),
                    artifact.file_name_with_base("Foo"),
                    artifact.json().is_empty(),
                )
            })
            .collect::<Vec<_>>();

        assert_eq!(
            filenames,
            vec![
                (
                    BytecodeDebugArtifactKind::SourceMap,
                    "source_map.json",
                    "Foo.source_map.json".to_string(),
                    false
                ),
                (
                    BytecodeDebugArtifactKind::DebugLocations,
                    "debug_locations.json",
                    "Foo.debug_locations.json".to_string(),
                    false
                ),
                (
                    BytecodeDebugArtifactKind::DebugLineTable,
                    "debug_line_table.json",
                    "Foo.debug_line_table.json".to_string(),
                    false
                ),
            ]
        );
    }

    #[test]
    fn bytecode_debug_artifacts_export_returns_typed_source_map_and_locations() {
        let entries = vec![
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
                    snippet: "main".to_string(),
                },
            ),
            source_map_entry(
                "Foo",
                "runtime",
                4,
                8,
                BytecodeSourceMapEntryKind::BytecodeUnmapped {
                    reason: BytecodeUnmappedReason::Synthetic,
                },
            ),
        ];
        let object = BytecodeObjectKey::new("Foo");
        let source_map_options = BytecodeSourceMapExportOptions::new()
            .with_object_key(&object)
            .with_bytecode_origin_coverage(Some(BytecodeOriginCoverage::new(1, 0, 1)))
            .with_post_opt_origin_coverage(Some(SonatinaPostOptOriginCoverage::new(1, 0, 1)));
        let debug_location_options = BytecodeSourceMapExportOptions::new().with_object_key(&object);

        let artifacts =
            bytecode_debug_artifacts_export(&entries, source_map_options, debug_location_options)
                .expect("typed debug artifacts should be exported together");
        let source_map = artifacts
            .source_map()
            .expect("source-map export should be present");
        let debug_locations = artifacts
            .debug_locations()
            .expect("debug-location export should be present");
        let debug_line_table = artifacts
            .debug_line_table()
            .expect("debug-line-table export should be present");

        assert_eq!(source_map.entries().len(), 2);
        assert_eq!(source_map.bytecode_origin_coverage().unwrap().total(), 2);
        assert_eq!(
            source_map
                .post_opt_origin_coverage()
                .unwrap()
                .pre_opt_snapshot_losses(),
            1
        );
        assert_eq!(debug_locations.locations().len(), 1);
        assert_eq!(debug_locations.locations()[0].snippet(), "main");
        assert_eq!(
            debug_line_table.schema_version(),
            OwnedBytecodeDebugLineTableExport::SCHEMA_VERSION
        );
        assert_eq!(debug_line_table.files().len(), 1);
        assert_eq!(debug_line_table.rows().len(), 1);
    }

    #[test]
    fn bytecode_debug_artifacts_export_rejects_metadata_mismatch() {
        let entries = vec![source_map_entry(
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
                snippet: "main".to_string(),
            },
        )];
        let object = BytecodeObjectKey::new("Foo");
        let section =
            BytecodeSectionKey::new(object.clone(), BytecodeSectionNameKey::new("runtime"));
        let source_map_options = BytecodeSourceMapExportOptions::new().with_object_key(&object);
        let debug_location_options =
            BytecodeSourceMapExportOptions::new().with_section_key(&section);

        let err =
            bytecode_debug_artifacts_export(&entries, source_map_options, debug_location_options)
                .expect_err("debug artifact bundle scopes should match");

        match err {
            BytecodeDebugArtifactsExportError::MetadataMismatch(mismatch) => {
                assert_eq!(mismatch.source_map_object(), Some("Foo"));
                assert_eq!(mismatch.source_map_section(), None);
                assert_eq!(mismatch.debug_locations_object(), Some("Foo"));
                assert_eq!(mismatch.debug_locations_section(), Some("runtime"));
            }
            other => panic!("expected metadata mismatch, got {other}"),
        }
    }

    #[test]
    fn bytecode_debug_line_table_interns_files_and_preserves_location_rows() {
        let entries = vec![
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
                    snippet: "main".to_string(),
                },
            ),
            source_map_entry(
                "Foo",
                "runtime",
                4,
                8,
                BytecodeSourceMapEntryKind::Source {
                    span_kind: SourceSpanKind::Original,
                    file: "src/main.fe".to_string(),
                    start_byte: 20,
                    end_byte: 24,
                    start_line: 2,
                    start_col: 3,
                    end_line: 2,
                    end_col: 7,
                    snippet: "next".to_string(),
                },
            ),
            source_map_entry(
                "Foo",
                "runtime",
                8,
                12,
                BytecodeSourceMapEntryKind::Source {
                    span_kind: SourceSpanKind::Expanded,
                    file: "src/helper.fe".to_string(),
                    start_byte: 30,
                    end_byte: 36,
                    start_line: 4,
                    start_col: 1,
                    end_line: 4,
                    end_col: 7,
                    snippet: "helper".to_string(),
                },
            ),
        ];
        let object = BytecodeObjectKey::new("Foo");
        let source_map_options = BytecodeSourceMapExportOptions::new().with_object_key(&object);
        let debug_location_options = BytecodeSourceMapExportOptions::new().with_object_key(&object);

        let artifacts =
            bytecode_debug_artifacts_export(&entries, source_map_options, debug_location_options)
                .expect("typed debug artifacts should be exported together");
        let table = artifacts
            .debug_line_table()
            .expect("source locations should produce a debug line table");

        assert_eq!(
            table.schema_version(),
            OwnedBytecodeDebugLineTableExport::SCHEMA_VERSION
        );
        assert_eq!(table.object(), Some("Foo"));
        assert_eq!(table.section(), None);
        assert_eq!(table.files().len(), 2);
        assert_eq!(table.files()[0].path(), "src/main.fe");
        assert_eq!(table.files()[1].path(), "src/helper.fe");
        assert_eq!(table.rows().len(), 3);
        assert_eq!(table.rows()[0].file_index(), 0);
        assert_eq!(table.rows()[1].file_index(), 0);
        assert_eq!(table.rows()[2].file_index(), 1);
        assert_eq!(table.rows()[0].pc_start(), 0);
        assert_eq!(table.rows()[2].pc_end(), 12);
        assert_eq!(table.rows()[2].span_kind(), SourceSpanKind::Expanded);
        assert_eq!(table.rows()[2].snippet(), "helper");

        let line_records = table
            .line_records()
            .map(|record| {
                (
                    record.pc_start(),
                    record.pc_end(),
                    record.file().to_string(),
                    record.span_kind(),
                    record.snippet().to_string(),
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(
            line_records,
            vec![
                (
                    0,
                    4,
                    "src/main.fe".to_string(),
                    SourceSpanKind::Original,
                    "main".to_string()
                ),
                (
                    4,
                    8,
                    "src/main.fe".to_string(),
                    SourceSpanKind::Original,
                    "next".to_string()
                ),
                (
                    8,
                    12,
                    "src/helper.fe".to_string(),
                    SourceSpanKind::Expanded,
                    "helper".to_string()
                ),
            ]
        );
    }

    #[test]
    fn bytecode_debug_line_table_json_rejects_invalid_file_indices() {
        let json = serde_json::json!({
            "schema_version": 1,
            "object": "Foo",
            "files": [{"path": "src/main.fe"}],
            "rows": [{
                "object": "Foo",
                "section": "runtime",
                "pc_start": 0,
                "pc_end": 4,
                "file_index": 1,
                "span_kind": "original",
                "start_byte": 10,
                "end_byte": 14,
                "start_line": 1,
                "start_col": 2,
                "end_line": 1,
                "end_col": 6,
                "snippet": "main"
            }]
        })
        .to_string();
        let err = serde_json::from_str::<OwnedBytecodeDebugLineTableExport>(&json)
            .expect_err("debug-line-table rows must reference interned files");

        assert!(
            err.to_string().contains(
                "bytecode debug-line-table row references file_index 1, but only 1 source files exist"
            ),
            "{err}"
        );
    }

    #[test]
    fn bytecode_debug_line_table_json_line_records_resolve_file_indices() {
        let json = serde_json::json!({
            "schema_version": 1,
            "object": "Foo",
            "section": "runtime",
            "files": [
                {"path": "src/main.fe"},
                {"path": "src/helper.fe"}
            ],
            "rows": [
                {
                    "object": "Foo",
                    "section": "runtime",
                    "pc_start": 0,
                    "pc_end": 4,
                    "file_index": 0,
                    "span_kind": "original",
                    "start_byte": 10,
                    "end_byte": 14,
                    "start_line": 1,
                    "start_col": 2,
                    "end_line": 1,
                    "end_col": 6,
                    "snippet": "main"
                },
                {
                    "object": "Foo",
                    "section": "runtime",
                    "pc_start": 4,
                    "pc_end": 8,
                    "file_index": 1,
                    "span_kind": "expanded",
                    "start_byte": 30,
                    "end_byte": 36,
                    "start_line": 4,
                    "start_col": 1,
                    "end_line": 4,
                    "end_col": 7,
                    "snippet": "helper"
                }
            ]
        })
        .to_string();
        let table: OwnedBytecodeDebugLineTableExport =
            serde_json::from_str(&json).expect("debug-line-table JSON should decode");
        let line_records = table
            .line_records()
            .map(|record| {
                (
                    record.object().to_string(),
                    record.section().to_string(),
                    record.pc_start(),
                    record.pc_end(),
                    record.file().to_string(),
                    record.start_byte(),
                    record.end_byte(),
                    record.start_line(),
                    record.start_col(),
                    record.end_line(),
                    record.end_col(),
                    record.snippet().to_string(),
                )
            })
            .collect::<Vec<_>>();

        assert_eq!(
            line_records,
            vec![
                (
                    "Foo".to_string(),
                    "runtime".to_string(),
                    0,
                    4,
                    "src/main.fe".to_string(),
                    10,
                    14,
                    1,
                    2,
                    1,
                    6,
                    "main".to_string(),
                ),
                (
                    "Foo".to_string(),
                    "runtime".to_string(),
                    4,
                    8,
                    "src/helper.fe".to_string(),
                    30,
                    36,
                    4,
                    1,
                    4,
                    7,
                    "helper".to_string(),
                ),
            ]
        );
        let first = table
            .line_records()
            .next()
            .expect("decoded line table should contain a first record");
        assert_eq!(first.source_file().path(), "src/main.fe");
        assert_eq!(first.row().file_index(), 0);
    }

    #[test]
    fn bytecode_debug_line_table_json_rejects_unknown_schema_version() {
        let json = serde_json::json!({
            "schema_version": 999,
            "files": [{"path": "src/main.fe"}],
            "rows": [{
                "object": "Foo",
                "section": "runtime",
                "pc_start": 0,
                "pc_end": 4,
                "file_index": 0,
                "span_kind": "original",
                "start_byte": 10,
                "end_byte": 14,
                "start_line": 1,
                "start_col": 2,
                "end_line": 1,
                "end_col": 6,
                "snippet": "main"
            }]
        })
        .to_string();
        let err = serde_json::from_str::<OwnedBytecodeDebugLineTableExport>(&json)
            .expect_err("unknown debug-line-table schema versions must fail closed");

        assert!(
            err.to_string()
                .contains("unsupported bytecode debug-line-table schema_version 999"),
            "{err}"
        );
    }

    #[test]
    fn bytecode_debug_location_json_skips_non_source_entries() {
        let entries = vec![source_map_entry(
            "Foo",
            "runtime",
            0,
            4,
            BytecodeSourceMapEntryKind::BytecodeUnmapped {
                reason: BytecodeUnmappedReason::Synthetic,
            },
        )];

        assert!(
            bytecode_debug_location_entries_json(&entries, BytecodeSourceMapExportOptions::new())
                .expect("non-source debug location export should validate")
                .is_none()
        );
    }

    #[test]
    fn bytecode_debug_location_json_rejects_unknown_schema_version() {
        let json = serde_json::json!({
            "schema_version": 999,
            "locations": [debug_location_value("Foo", "runtime", 0, 4)]
        })
        .to_string();
        let err = serde_json::from_str::<OwnedBytecodeDebugLocationExport>(&json)
            .expect_err("unknown debug-location schema versions must fail closed");

        assert!(
            err.to_string()
                .contains("unsupported bytecode debug-location schema_version 999"),
            "{err}"
        );
    }

    #[test]
    fn bytecode_debug_location_json_rejects_unknown_export_fields() {
        let json = serde_json::json!({
            "schema_version": 1,
            "locations": [debug_location_value("Foo", "runtime", 0, 4)],
            "extra": true
        })
        .to_string();
        let err = serde_json::from_str::<OwnedBytecodeDebugLocationExport>(&json)
            .expect_err("unknown debug-location export fields must fail closed");

        assert!(err.to_string().contains("unknown field"), "{err}");
    }

    #[test]
    fn bytecode_debug_location_json_rejects_unknown_location_fields() {
        let mut location = debug_location_value("Foo", "runtime", 0, 4);
        location["extra"] = serde_json::json!(true);
        let json = serde_json::json!({
            "schema_version": 1,
            "locations": [location]
        })
        .to_string();
        let err = serde_json::from_str::<OwnedBytecodeDebugLocationExport>(&json)
            .expect_err("unknown debug-location entry fields must fail closed");

        assert!(err.to_string().contains("unknown field"), "{err}");
    }

    #[test]
    fn bytecode_debug_location_json_rejects_empty_locations() {
        let json = r#"{"schema_version":1,"locations":[]}"#;
        let err = serde_json::from_str::<OwnedBytecodeDebugLocationExport>(json)
            .expect_err("empty debug-location exports must fail closed");

        assert!(
            err.to_string()
                .contains("bytecode debug-location export must contain at least one location"),
            "{err}"
        );
    }

    #[test]
    fn bytecode_debug_location_json_rejects_empty_export_objects() {
        let json = serde_json::json!({
            "schema_version": 1,
            "object": "",
            "locations": [debug_location_value("Foo", "runtime", 0, 4)]
        })
        .to_string();
        let err = serde_json::from_str::<OwnedBytecodeDebugLocationExport>(&json)
            .expect_err("debug-location export object metadata must be non-empty");

        assert!(
            err.to_string()
                .contains("bytecode source-map export object must not be empty"),
            "{err}"
        );
    }

    #[test]
    fn bytecode_debug_location_json_rejects_invalid_pc_ranges() {
        let json = serde_json::json!({
            "schema_version": 1,
            "locations": [debug_location_value("Foo", "runtime", 4, 4)]
        })
        .to_string();
        let err = serde_json::from_str::<OwnedBytecodeDebugLocationExport>(&json)
            .expect_err("debug-location PC ranges must be non-empty");

        assert!(
            err.to_string()
                .contains("invalid bytecode source-map export PC range 4..4"),
            "{err}"
        );
    }

    #[test]
    fn bytecode_debug_location_json_rejects_object_mismatch() {
        let json = serde_json::json!({
            "schema_version": 1,
            "object": "Foo",
            "locations": [debug_location_value("Bar", "runtime", 0, 4)]
        })
        .to_string();
        let err = serde_json::from_str::<OwnedBytecodeDebugLocationExport>(&json)
            .expect_err("debug-location export object must match location objects");

        assert!(
            err.to_string().contains(
                "bytecode source-map export object `Foo` does not match entry object `Bar`"
            ),
            "{err}"
        );
    }

    #[test]
    fn bytecode_debug_location_json_rejects_overlapping_pc_ranges() {
        let json = serde_json::json!({
            "schema_version": 1,
            "locations": [
                debug_location_value("Foo", "runtime", 0, 8),
                debug_location_value("Foo", "runtime", 4, 12)
            ]
        })
        .to_string();
        let err = serde_json::from_str::<OwnedBytecodeDebugLocationExport>(&json)
            .expect_err("debug-location PC ranges must not overlap within one object section");

        assert!(
            err.to_string().contains(
                "bytecode source-map PC ranges overlap in object `Foo` section `runtime`: 0..8 overlaps 4..12"
            ),
            "{err}"
        );
    }

    #[test]
    fn bytecode_debug_location_json_rejects_inverted_source_byte_ranges() {
        let mut location = debug_location_value("Foo", "runtime", 0, 4);
        location["start_byte"] = serde_json::json!(14);
        location["end_byte"] = serde_json::json!(10);
        let json = serde_json::json!({
            "schema_version": 1,
            "locations": [location]
        })
        .to_string();
        let err = serde_json::from_str::<OwnedBytecodeDebugLocationExport>(&json)
            .expect_err("debug-location source byte ranges must be ordered");

        assert!(
            err.to_string()
                .contains("bytecode source-map source entry has invalid byte range 14..10"),
            "{err}"
        );
    }

    #[test]
    fn bytecode_debug_location_json_rejects_inverted_source_positions() {
        let mut location = debug_location_value("Foo", "runtime", 0, 4);
        location["start_col"] = serde_json::json!(6);
        location["end_col"] = serde_json::json!(2);
        let json = serde_json::json!({
            "schema_version": 1,
            "locations": [location]
        })
        .to_string();
        let err = serde_json::from_str::<OwnedBytecodeDebugLocationExport>(&json)
            .expect_err("debug-location source line/column ranges must be ordered");

        assert!(
            err.to_string().contains(
                "bytecode source-map source entry has invalid line/column range 1:6..1:2"
            ),
            "{err}"
        );
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

        let object = BytecodeObjectKey::new("Foo");
        let section = BytecodeSectionKey::new(object, BytecodeSectionNameKey::new("runtime"));
        let summary = bytecode_source_map_entries_summary(
            &entries,
            Some(BytecodeSourceMapExportMetadata::section(&section)),
        )
        .expect("runtime entries should summarize");

        assert_eq!(summary.object(), Some("Foo"));
        assert_eq!(summary.section(), Some("runtime"));
        assert_eq!(summary.total(), 3);
        assert_eq!(summary.source(), 1);
        assert_eq!(summary.debug_locations(), 1);
        assert_eq!(summary.debug_line_table_files(), 1);
        assert_eq!(summary.debug_line_table_rows(), 1);
        assert_eq!(summary.source_span_invalid(), 1);
        assert_eq!(summary.runtime_stmt_missing(), 1);
        assert_eq!(summary.bytecode_unmapped(), 0);
        assert_eq!(summary.non_source(), 2);
    }

    #[test]
    fn bytecode_source_map_entries_summary_counts_debug_line_table_files() {
        let entries = vec![
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
                    snippet: "main".to_string(),
                },
            ),
            source_map_entry(
                "Foo",
                "runtime",
                4,
                8,
                BytecodeSourceMapEntryKind::Source {
                    span_kind: SourceSpanKind::Original,
                    file: "src/main.fe".to_string(),
                    start_byte: 20,
                    end_byte: 24,
                    start_line: 2,
                    start_col: 2,
                    end_line: 2,
                    end_col: 6,
                    snippet: "next".to_string(),
                },
            ),
            source_map_entry(
                "Foo",
                "runtime",
                8,
                12,
                BytecodeSourceMapEntryKind::Source {
                    span_kind: SourceSpanKind::Expanded,
                    file: "src/helper.fe".to_string(),
                    start_byte: 30,
                    end_byte: 36,
                    start_line: 4,
                    start_col: 1,
                    end_line: 4,
                    end_col: 7,
                    snippet: "helper".to_string(),
                },
            ),
        ];

        let object = BytecodeObjectKey::new("Foo");
        let section = BytecodeSectionKey::new(object, BytecodeSectionNameKey::new("runtime"));
        let summary = bytecode_source_map_entries_summary(
            &entries,
            Some(BytecodeSourceMapExportMetadata::section(&section)),
        )
        .expect("runtime entries should summarize");

        assert_eq!(summary.debug_locations(), 3);
        assert_eq!(summary.debug_line_table_rows(), 3);
        assert_eq!(summary.debug_line_table_files(), 2);
    }
}
