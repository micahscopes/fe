use common::facts::SourceSpanKind;
use serde::{Deserialize, Deserializer, Serialize, de};

use super::{
    BytecodePcExportEntry, BytecodeSourceMapEntry, BytecodeSourceMapEntryKind,
    BytecodeSourceMapExportEntryError, BytecodeSourceMapExportOptions,
    validate_debug_location_entry_parts, validate_export_metadata_and_pc_ranges,
    validate_source_map_export_entries,
};

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
    #[allow(clippy::too_many_arguments)]
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

    pub(super) fn from_options(
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
