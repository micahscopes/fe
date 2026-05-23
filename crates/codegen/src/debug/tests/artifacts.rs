use super::*;

#[test]
fn bytecode_debug_artifacts_json_renders_source_map_and_debug_locations_together() {
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
            BytecodeSourceMapEntryKind::BytecodeUnmapped {
                reason: BytecodeUnmappedReason::Synthetic,
            },
        ),
    ];
    let object = BytecodeObjectKey::new("Foo");
    let source_map_options = BytecodeSourceMapExportOptions::new()
        .with_object_key(&object)
        .with_bytecode_origin_coverage(Some(BytecodeOriginCoverage::new(1, 0, 1)))
        .with_post_opt_origin_coverage(Some(SonatinaPostOptOriginCoverage::new(1, 0, 1)));
    let debug_location_options = BytecodeSourceMapExportOptions::new().with_object_key(&object);

    let artifacts =
        bytecode_debug_artifacts_json(&entries, source_map_options, debug_location_options)
            .expect("debug artifacts should render together");
    let source_map: OwnedBytecodeSourceMapExport = serde_json::from_str(
        artifacts
            .source_map()
            .expect("source-map JSON should render"),
    )
    .expect("source-map JSON should decode");
    let debug_locations: OwnedBytecodeDebugLocationExport = serde_json::from_str(
        artifacts
            .debug_locations()
            .expect("debug-location JSON should render"),
    )
    .expect("debug-location JSON should decode");
    let debug_line_table: OwnedBytecodeDebugLineTableExport = serde_json::from_str(
        artifacts
            .debug_line_table()
            .expect("debug-line-table JSON should render"),
    )
    .expect("debug-line-table JSON should decode");

    assert_eq!(source_map.object(), Some("Foo"));
    assert_eq!(source_map.entries().len(), 2);
    assert_eq!(
        source_map
            .bytecode_origin_coverage()
            .expect("coverage should render")
            .total(),
        2
    );
    assert_eq!(
        source_map
            .post_opt_origin_coverage()
            .expect("post-opt coverage should render")
            .observed_pre_opt_total(),
        2
    );
    assert_eq!(debug_locations.object(), Some("Foo"));
    assert_eq!(debug_locations.locations().len(), 1);
    assert_eq!(debug_locations.locations()[0].snippet(), "main");
    assert_eq!(debug_line_table.object(), Some("Foo"));
    assert_eq!(debug_line_table.files().len(), 1);
    assert_eq!(debug_line_table.rows().len(), 1);
    assert_eq!(debug_line_table.rows()[0].snippet(), "main");
}

#[test]
fn bytecode_debug_artifacts_json_iterates_stable_artifact_filenames() {
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
            BytecodeSourceMapEntryKind::BytecodeUnmapped {
                reason: BytecodeUnmappedReason::Synthetic,
            },
        ),
    ];
    let object = BytecodeObjectKey::new("Foo");
    let source_map_options = BytecodeSourceMapExportOptions::new().with_object_key(&object);
    let debug_location_options = BytecodeSourceMapExportOptions::new().with_object_key(&object);
    let artifacts =
        bytecode_debug_artifacts_json(&entries, source_map_options, debug_location_options)
            .expect("debug artifacts should render together");
    let filenames = artifacts
        .artifacts()
        .map(|artifact| {
            (
                artifact.kind(),
                artifact.file_name(),
                artifact.file_name_with_base("Foo"),
                artifact.json().is_empty(),
            )
        })
        .collect::<Vec<_>>();

    assert_eq!(
        filenames,
        vec![
            (
                BytecodeDebugArtifactKind::SourceMap,
                "source_map.json",
                "Foo.source_map.json".to_string(),
                false
            ),
            (
                BytecodeDebugArtifactKind::DebugLocations,
                "debug_locations.json",
                "Foo.debug_locations.json".to_string(),
                false
            ),
            (
                BytecodeDebugArtifactKind::DebugLineTable,
                "debug_line_table.json",
                "Foo.debug_line_table.json".to_string(),
                false
            ),
        ]
    );
}

#[test]
fn bytecode_debug_artifacts_export_returns_typed_source_map_and_locations() {
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
            BytecodeSourceMapEntryKind::BytecodeUnmapped {
                reason: BytecodeUnmappedReason::Synthetic,
            },
        ),
    ];
    let object = BytecodeObjectKey::new("Foo");
    let source_map_options = BytecodeSourceMapExportOptions::new()
        .with_object_key(&object)
        .with_bytecode_origin_coverage(Some(BytecodeOriginCoverage::new(1, 0, 1)))
        .with_post_opt_origin_coverage(Some(SonatinaPostOptOriginCoverage::new(1, 0, 1)));
    let debug_location_options = BytecodeSourceMapExportOptions::new().with_object_key(&object);

    let artifacts =
        bytecode_debug_artifacts_export(&entries, source_map_options, debug_location_options)
            .expect("typed debug artifacts should be exported together");
    let source_map = artifacts
        .source_map()
        .expect("source-map export should be present");
    let debug_locations = artifacts
        .debug_locations()
        .expect("debug-location export should be present");
    let debug_line_table = artifacts
        .debug_line_table()
        .expect("debug-line-table export should be present");

    assert_eq!(source_map.entries().len(), 2);
    assert_eq!(source_map.bytecode_origin_coverage().unwrap().total(), 2);
    assert_eq!(
        source_map
            .post_opt_origin_coverage()
            .unwrap()
            .pre_opt_snapshot_losses(),
        1
    );
    assert_eq!(debug_locations.locations().len(), 1);
    assert_eq!(debug_locations.locations()[0].snippet(), "main");
    assert_eq!(
        debug_line_table.schema_version(),
        OwnedBytecodeDebugLineTableExport::SCHEMA_VERSION
    );
    assert_eq!(debug_line_table.files().len(), 1);
    assert_eq!(debug_line_table.rows().len(), 1);
}

#[test]
fn bytecode_debug_artifacts_export_rejects_metadata_mismatch() {
    let entries = vec![source_map_entry(
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
    )];
    let object = BytecodeObjectKey::new("Foo");
    let section = BytecodeSectionKey::new(object.clone(), BytecodeSectionNameKey::new("runtime"));
    let source_map_options = BytecodeSourceMapExportOptions::new().with_object_key(&object);
    let debug_location_options = BytecodeSourceMapExportOptions::new().with_section_key(&section);

    let err = bytecode_debug_artifacts_export(&entries, source_map_options, debug_location_options)
        .expect_err("debug artifact bundle scopes should match");

    match err {
        BytecodeDebugArtifactsExportError::MetadataMismatch(mismatch) => {
            assert_eq!(mismatch.source_map_object(), Some("Foo"));
            assert_eq!(mismatch.source_map_section(), None);
            assert_eq!(mismatch.debug_locations_object(), Some("Foo"));
            assert_eq!(mismatch.debug_locations_section(), Some("runtime"));
        }
        other => panic!("expected metadata mismatch, got {other}"),
    }
}
