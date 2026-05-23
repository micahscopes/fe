use common::{InputDb, facts::SourceSpanExport};

use crate::origin::{BytecodeObjectKey, BytecodePcOrigin, BytecodeSourceResolution};

mod artifacts;
mod coverage;
mod line_table;
mod location;
mod source_map_entry;
mod source_map_export;
mod source_map_options;
mod source_map_summary;
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
pub use source_map_entry::{
    BytecodeSourceMapEntry, BytecodeSourceMapEntryError, BytecodeSourceMapEntryKind,
    SourceSpanInvalidReason,
};
pub use source_map_export::{
    BytecodeSourceMapExportEntryError, BytecodeSourceMapExportError, OwnedBytecodeSourceMapExport,
    bytecode_source_map_entries_export, bytecode_source_map_entries_json,
};
pub use source_map_options::{
    BytecodeSourceMapExportMetadata, BytecodeSourceMapExportOptions, BytecodeSourceMapFilter,
};
pub use source_map_summary::{BytecodeSourceMapSummary, bytecode_source_map_entries_summary};

use source_map_export::{
    BytecodePcExportEntry, export_metadata_parts, validate_debug_location_entry_parts,
    validate_export_metadata_and_pc_ranges, validate_source_map_export_entries,
};
use source_spans::{bytecode_source_span_exports_for_resolutions, source_map_entry_for_resolution};

#[cfg(test)]
use common::facts::SourceSpanKind;

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

fn matches_filter(origin: &BytecodePcOrigin, filter: &BytecodeSourceMapFilter) -> bool {
    origin.section().object().as_str() == filter.object()
        && origin.section().section() == filter.section()
}

#[cfg(test)]
mod tests;
