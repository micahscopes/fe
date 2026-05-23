use super::*;

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
