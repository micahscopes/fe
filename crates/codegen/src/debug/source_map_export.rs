use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, de};

use crate::origin::BytecodePcRange;

use super::{
    coverage::{BytecodeOriginCoverageExport, SonatinaPostOptOriginCoverageExport},
    source_map_entry::{
        BytecodeSourceMapEntry, BytecodeSourceMapEntrySemanticError, validate_source_map_entry_kind,
    },
    source_map_options::{BytecodeSourceMapExportMetadata, BytecodeSourceMapExportOptions},
};

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

    pub(super) fn from_serialized_parts(
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

#[allow(clippy::too_many_arguments)]
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

pub(super) fn export_metadata_parts(
    metadata: Option<BytecodeSourceMapExportMetadata<'_>>,
) -> (Option<&str>, Option<&str>) {
    match metadata {
        Some(metadata) => (Some(metadata.object_name()), metadata.section_name()),
        None => (None, None),
    }
}
