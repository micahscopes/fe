use super::*;

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
