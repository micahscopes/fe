use crate::origin::{
    BytecodeObjectKey, BytecodeOriginCoverage, BytecodePcOrigin, BytecodePcRange,
    BytecodeSectionKey, BytecodeSectionNameKey, BytecodeUnmappedReason,
    SonatinaPostOptOriginCoverage,
};

use super::source_spans::span_snippet;
use super::{
    BytecodeDebugArtifactKind, BytecodeDebugArtifactsExportError, BytecodeSourceMapEntry,
    BytecodeSourceMapEntryError, BytecodeSourceMapEntryKind, BytecodeSourceMapExportMetadata,
    BytecodeSourceMapExportOptions, OwnedBytecodeDebugLineTableExport,
    OwnedBytecodeDebugLocationExport, OwnedBytecodeSourceMapExport, SourceSpanInvalidReason,
    SourceSpanKind, bytecode_debug_artifacts_export, bytecode_debug_artifacts_json,
    bytecode_debug_location_entries_export, bytecode_debug_location_entries_json,
    bytecode_source_map_entries_export, bytecode_source_map_entries_json,
    bytecode_source_map_entries_summary,
};

fn source_map_origin(object: &str, section: &str, pc_start: u32, pc_end: u32) -> BytecodePcOrigin {
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

mod artifacts;
mod debug_location;
mod line_table;
mod source_map_export;
mod source_map_json;
mod source_map_summary;
