use common::{
    InputDb,
    diagnostics::{Span, SpanKind},
    facts::{SourceSpanExport, SourceSpanKind},
};

use crate::origin::{
    BytecodePcOrigin, BytecodeSourceResolution, BytecodeSourceResolutionResult,
    bytecode_pc_export_key,
};

use super::{BytecodeSourceMapEntry, BytecodeSourceMapEntryKind, SourceSpanInvalidReason};

pub(super) fn source_map_entry_for_resolution(
    db: &dyn InputDb,
    resolution: &BytecodeSourceResolution<'_>,
) -> BytecodeSourceMapEntry {
    let origin = resolution.origin();
    BytecodeSourceMapEntry::from_origin(origin, source_map_entry_kind(db, resolution))
}

pub(super) fn bytecode_source_span_exports_for_resolutions(
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

pub(super) fn span_snippet(
    text: &str,
    start: usize,
    end: usize,
) -> Result<String, SourceSpanInvalidReason> {
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
