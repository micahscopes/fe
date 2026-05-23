use std::collections::BTreeSet;

use super::{
    BytecodeSourceMapEntry, BytecodeSourceMapEntryKind, BytecodeSourceMapExportMetadata,
    export_metadata_parts,
};

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
