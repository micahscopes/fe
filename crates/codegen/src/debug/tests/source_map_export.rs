use super::*;

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
        err.to_string()
            .contains("bytecode source-map export object `Bar` does not match entry object `Foo`"),
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
    let err = bytecode_source_map_entries_export(&entries, BytecodeSourceMapExportOptions::new())
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
