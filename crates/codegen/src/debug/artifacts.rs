use std::fmt;

use super::{
    BytecodeSourceMapEntry, BytecodeSourceMapExportEntryError, BytecodeSourceMapExportError,
    BytecodeSourceMapExportMetadata, BytecodeSourceMapExportOptions,
    OwnedBytecodeDebugLineTableExport, OwnedBytecodeDebugLocationExport,
    OwnedBytecodeSourceMapExport, bytecode_debug_location_entries_export,
    bytecode_source_map_entries_export, export_metadata_parts,
};

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
