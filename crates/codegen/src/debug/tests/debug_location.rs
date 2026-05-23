use super::*;

#[test]
fn bytecode_debug_location_export_uses_only_valid_source_entries() {
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
    let options = BytecodeSourceMapExportOptions::new()
        .with_metadata(BytecodeSourceMapExportMetadata::object(&object));
    let export = bytecode_debug_location_entries_export(&entries, options)
        .expect("debug location export should validate")
        .expect("source entries should produce debug locations");

    assert_eq!(export.schema_version(), 1);
    assert_eq!(export.object(), Some("Foo"));
    assert_eq!(export.section(), None);
    assert_eq!(export.locations().len(), 1);
    assert_eq!(export.locations()[0].object(), "Foo");
    assert_eq!(export.locations()[0].section(), "runtime");
    assert_eq!(export.locations()[0].pc_start(), 0);
    assert_eq!(export.locations()[0].pc_end(), 4);
    assert_eq!(export.locations()[0].span_kind(), SourceSpanKind::Original);
    assert_eq!(export.locations()[0].file(), "src/main.fe");
    assert_eq!(export.locations()[0].start_byte(), 10);
    assert_eq!(export.locations()[0].end_byte(), 14);
    assert_eq!(export.locations()[0].start_line(), 1);
    assert_eq!(export.locations()[0].start_col(), 2);
    assert_eq!(export.locations()[0].end_line(), 1);
    assert_eq!(export.locations()[0].end_col(), 6);
    assert_eq!(export.locations()[0].snippet(), "main");

    let json = bytecode_debug_location_entries_json(&entries, options)
        .expect("debug location JSON should serialize")
        .expect("source entries should produce debug location JSON");
    let decoded: OwnedBytecodeDebugLocationExport =
        serde_json::from_str(&json).expect("debug location JSON should roundtrip");

    assert_eq!(
        decoded.schema_version(),
        OwnedBytecodeDebugLocationExport::SCHEMA_VERSION
    );
    assert_eq!(decoded.object(), Some("Foo"));
    assert_eq!(decoded.section(), None);
    assert_eq!(decoded.locations(), export.locations());
}

#[test]
fn bytecode_debug_location_json_skips_non_source_entries() {
    let entries = vec![source_map_entry(
        "Foo",
        "runtime",
        0,
        4,
        BytecodeSourceMapEntryKind::BytecodeUnmapped {
            reason: BytecodeUnmappedReason::Synthetic,
        },
    )];

    assert!(
        bytecode_debug_location_entries_json(&entries, BytecodeSourceMapExportOptions::new())
            .expect("non-source debug location export should validate")
            .is_none()
    );
}

#[test]
fn bytecode_debug_location_json_rejects_unknown_schema_version() {
    let json = serde_json::json!({
        "schema_version": 999,
        "locations": [debug_location_value("Foo", "runtime", 0, 4)]
    })
    .to_string();
    let err = serde_json::from_str::<OwnedBytecodeDebugLocationExport>(&json)
        .expect_err("unknown debug-location schema versions must fail closed");

    assert!(
        err.to_string()
            .contains("unsupported bytecode debug-location schema_version 999"),
        "{err}"
    );
}

#[test]
fn bytecode_debug_location_json_rejects_unknown_export_fields() {
    let json = serde_json::json!({
        "schema_version": 1,
        "locations": [debug_location_value("Foo", "runtime", 0, 4)],
        "extra": true
    })
    .to_string();
    let err = serde_json::from_str::<OwnedBytecodeDebugLocationExport>(&json)
        .expect_err("unknown debug-location export fields must fail closed");

    assert!(err.to_string().contains("unknown field"), "{err}");
}

#[test]
fn bytecode_debug_location_json_rejects_unknown_location_fields() {
    let mut location = debug_location_value("Foo", "runtime", 0, 4);
    location["extra"] = serde_json::json!(true);
    let json = serde_json::json!({
        "schema_version": 1,
        "locations": [location]
    })
    .to_string();
    let err = serde_json::from_str::<OwnedBytecodeDebugLocationExport>(&json)
        .expect_err("unknown debug-location entry fields must fail closed");

    assert!(err.to_string().contains("unknown field"), "{err}");
}

#[test]
fn bytecode_debug_location_json_rejects_empty_locations() {
    let json = r#"{"schema_version":1,"locations":[]}"#;
    let err = serde_json::from_str::<OwnedBytecodeDebugLocationExport>(json)
        .expect_err("empty debug-location exports must fail closed");

    assert!(
        err.to_string()
            .contains("bytecode debug-location export must contain at least one location"),
        "{err}"
    );
}

#[test]
fn bytecode_debug_location_json_rejects_empty_export_objects() {
    let json = serde_json::json!({
        "schema_version": 1,
        "object": "",
        "locations": [debug_location_value("Foo", "runtime", 0, 4)]
    })
    .to_string();
    let err = serde_json::from_str::<OwnedBytecodeDebugLocationExport>(&json)
        .expect_err("debug-location export object metadata must be non-empty");

    assert!(
        err.to_string()
            .contains("bytecode source-map export object must not be empty"),
        "{err}"
    );
}

#[test]
fn bytecode_debug_location_json_rejects_invalid_pc_ranges() {
    let json = serde_json::json!({
        "schema_version": 1,
        "locations": [debug_location_value("Foo", "runtime", 4, 4)]
    })
    .to_string();
    let err = serde_json::from_str::<OwnedBytecodeDebugLocationExport>(&json)
        .expect_err("debug-location PC ranges must be non-empty");

    assert!(
        err.to_string()
            .contains("invalid bytecode source-map export PC range 4..4"),
        "{err}"
    );
}

#[test]
fn bytecode_debug_location_json_rejects_object_mismatch() {
    let json = serde_json::json!({
        "schema_version": 1,
        "object": "Foo",
        "locations": [debug_location_value("Bar", "runtime", 0, 4)]
    })
    .to_string();
    let err = serde_json::from_str::<OwnedBytecodeDebugLocationExport>(&json)
        .expect_err("debug-location export object must match location objects");

    assert!(
        err.to_string()
            .contains("bytecode source-map export object `Foo` does not match entry object `Bar`"),
        "{err}"
    );
}

#[test]
fn bytecode_debug_location_json_rejects_overlapping_pc_ranges() {
    let json = serde_json::json!({
        "schema_version": 1,
        "locations": [
            debug_location_value("Foo", "runtime", 0, 8),
            debug_location_value("Foo", "runtime", 4, 12)
        ]
    })
    .to_string();
    let err = serde_json::from_str::<OwnedBytecodeDebugLocationExport>(&json)
        .expect_err("debug-location PC ranges must not overlap within one object section");

    assert!(
            err.to_string().contains(
                "bytecode source-map PC ranges overlap in object `Foo` section `runtime`: 0..8 overlaps 4..12"
            ),
            "{err}"
        );
}

#[test]
fn bytecode_debug_location_json_rejects_inverted_source_byte_ranges() {
    let mut location = debug_location_value("Foo", "runtime", 0, 4);
    location["start_byte"] = serde_json::json!(14);
    location["end_byte"] = serde_json::json!(10);
    let json = serde_json::json!({
        "schema_version": 1,
        "locations": [location]
    })
    .to_string();
    let err = serde_json::from_str::<OwnedBytecodeDebugLocationExport>(&json)
        .expect_err("debug-location source byte ranges must be ordered");

    assert!(
        err.to_string()
            .contains("bytecode source-map source entry has invalid byte range 14..10"),
        "{err}"
    );
}

#[test]
fn bytecode_debug_location_json_rejects_inverted_source_positions() {
    let mut location = debug_location_value("Foo", "runtime", 0, 4);
    location["start_col"] = serde_json::json!(6);
    location["end_col"] = serde_json::json!(2);
    let json = serde_json::json!({
        "schema_version": 1,
        "locations": [location]
    })
    .to_string();
    let err = serde_json::from_str::<OwnedBytecodeDebugLocationExport>(&json)
        .expect_err("debug-location source line/column ranges must be ordered");

    assert!(
        err.to_string()
            .contains("bytecode source-map source entry has invalid line/column range 1:6..1:2"),
        "{err}"
    );
}
