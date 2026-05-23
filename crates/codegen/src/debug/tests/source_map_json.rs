use super::*;

#[test]
fn bytecode_source_map_json_roundtrips_owned_export_schema() {
    let entries = vec![
        source_map_entry(
            "Foo",
            "runtime",
            4,
            8,
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
            8,
            12,
            BytecodeSourceMapEntryKind::BytecodeUnmapped {
                reason: BytecodeUnmappedReason::NoIrInst,
            },
        ),
    ];

    let section_key = BytecodeSectionKey::new(
        BytecodeObjectKey::new("Foo"),
        BytecodeSectionNameKey::new("runtime"),
    );
    let options = BytecodeSourceMapExportOptions::new().with_section_key(&section_key);
    let export = bytecode_source_map_entries_export(&entries, options)
        .expect("source-map export should validate")
        .expect("non-empty entries should export");
    assert_eq!(
        export.schema_version(),
        OwnedBytecodeSourceMapExport::SCHEMA_VERSION
    );
    assert_eq!(export.object(), Some("Foo"));
    assert_eq!(export.section(), Some("runtime"));
    assert_eq!(export.entries(), entries.as_slice());

    let bytecode_origin_coverage = BytecodeOriginCoverage::new(1, 1, 0);
    let post_opt_origin_coverage = SonatinaPostOptOriginCoverage::new(1, 1, 1);
    let json = bytecode_source_map_entries_json(
        &entries,
        options
            .with_bytecode_origin_coverage(Some(bytecode_origin_coverage))
            .with_post_opt_origin_coverage(Some(post_opt_origin_coverage)),
    )
    .expect("source map should serialize")
    .expect("non-empty entries should render JSON");
    let decoded: OwnedBytecodeSourceMapExport = serde_json::from_str(&json).unwrap();

    assert_eq!(decoded.schema_version(), export.schema_version());
    assert_eq!(decoded.object(), export.object());
    assert_eq!(decoded.section(), export.section());
    assert_eq!(decoded.entries()[0].kind().kind_name(), "source");
    match decoded.entries()[0].kind() {
        BytecodeSourceMapEntryKind::Source { snippet, .. } => assert_eq!(snippet, "main"),
        other => panic!("expected source entry, got {other:?}"),
    }
    assert_eq!(decoded.entries()[1].kind().reason(), Some("no_ir_inst"));
    let decoded_coverage = decoded
        .bytecode_origin_coverage()
        .expect("coverage should roundtrip through source-map JSON");
    assert_eq!(decoded_coverage.total(), 2);
    assert_eq!(decoded_coverage.sonatina_post_opt(), 1);
    assert_eq!(decoded_coverage.sonatina_backend_prepared(), 1);
    assert_eq!(decoded_coverage.unmapped(), 0);
    let decoded_post_opt_coverage = decoded
        .post_opt_origin_coverage()
        .expect("post-opt coverage should roundtrip through source-map JSON");
    assert_eq!(decoded_post_opt_coverage.total(), 2);
    assert_eq!(decoded_post_opt_coverage.same_inst_id(), 1);
    assert_eq!(
        decoded_post_opt_coverage.created_or_unmatched_after_preopt_snapshot(),
        1
    );
    assert_eq!(decoded_post_opt_coverage.pre_opt_snapshot_losses(), 1);
    assert_eq!(decoded_post_opt_coverage.observed_pre_opt_total(), 2);
}

#[test]
fn bytecode_source_map_json_rejects_unknown_schema_version() {
    let json = r#"{"schema_version":999,"entries":[]}"#;
    let err = serde_json::from_str::<OwnedBytecodeSourceMapExport>(json)
        .expect_err("unknown schema versions must fail closed");

    assert!(
        err.to_string()
            .contains("unsupported bytecode source-map schema_version 999"),
        "{err}"
    );
}

#[test]
fn bytecode_source_map_json_rejects_unknown_export_fields() {
    let json = r#"{"schema_version":3,"entries":[],"extra":true}"#;
    let err = serde_json::from_str::<OwnedBytecodeSourceMapExport>(json)
        .expect_err("unknown source-map export fields must fail closed");

    assert!(err.to_string().contains("unknown field"), "{err}");
}

#[test]
fn bytecode_source_map_json_rejects_empty_export_objects() {
    let json = r#"{"schema_version":3,"object":"","entries":[]}"#;
    let err = serde_json::from_str::<OwnedBytecodeSourceMapExport>(json)
        .expect_err("source-map export object metadata must be non-empty");

    assert!(
        err.to_string()
            .contains("bytecode source-map export object must not be empty"),
        "{err}"
    );
}

#[test]
fn bytecode_source_map_json_rejects_empty_export_sections() {
    let json = r#"{"schema_version":3,"section":"","entries":[]}"#;
    let err = serde_json::from_str::<OwnedBytecodeSourceMapExport>(json)
        .expect_err("source-map export section metadata must be non-empty");

    assert!(
        err.to_string()
            .contains("bytecode source-map export section must not be empty"),
        "{err}"
    );
}

#[test]
fn bytecode_source_map_json_rejects_unknown_entry_fields() {
    let json = r#"{
            "schema_version": 3,
            "entries": [{
                "object": "Foo",
                "section": "runtime",
                "pc_start": 0,
                "pc_end": 4,
                "kind": "semantic_span_missing",
                "extra": true
            }]
        }"#;
    let err = serde_json::from_str::<OwnedBytecodeSourceMapExport>(json)
        .expect_err("unknown source-map entry fields must fail closed");

    assert!(err.to_string().contains("unknown field"), "{err}");
}

#[test]
fn bytecode_source_map_json_rejects_invalid_pc_ranges() {
    let json = r#"{
            "schema_version": 3,
            "entries": [{
                "object": "Foo",
                "section": "runtime",
                "pc_start": 4,
                "pc_end": 4,
                "kind": "semantic_span_missing"
            }]
        }"#;
    let err = serde_json::from_str::<OwnedBytecodeSourceMapExport>(json)
        .expect_err("source-map PC ranges must be non-empty");

    assert!(
        err.to_string()
            .contains("invalid bytecode source-map PC range 4..4"),
        "{err}"
    );
}

#[test]
fn bytecode_source_map_json_rejects_empty_objects() {
    let json = r#"{
            "schema_version": 3,
            "entries": [{
                "object": "",
                "section": "runtime",
                "pc_start": 0,
                "pc_end": 4,
                "kind": "semantic_span_missing"
            }]
        }"#;
    let err = serde_json::from_str::<OwnedBytecodeSourceMapExport>(json)
        .expect_err("source-map object keys must be non-empty");

    assert!(
        err.to_string()
            .contains("bytecode source-map object must not be empty"),
        "{err}"
    );
}

#[test]
fn bytecode_source_map_json_rejects_empty_sections() {
    let json = r#"{
            "schema_version": 3,
            "entries": [{
                "object": "Foo",
                "section": "",
                "pc_start": 0,
                "pc_end": 4,
                "kind": "semantic_span_missing"
            }]
        }"#;
    let err = serde_json::from_str::<OwnedBytecodeSourceMapExport>(json)
        .expect_err("source-map section keys must be non-empty");

    assert!(
        err.to_string()
            .contains("bytecode source-map section must not be empty"),
        "{err}"
    );
}

#[test]
fn bytecode_source_map_json_rejects_unknown_coverage_fields() {
    let json = r#"{
            "schema_version": 3,
            "bytecode_origin_coverage": {
                "total": 1,
                "sonatina_post_opt": 1,
                "sonatina_backend_prepared": 0,
                "unmapped": 0,
                "extra": true
            },
            "entries": [{
                "object": "Foo",
                "section": "runtime",
                "pc_start": 0,
                "pc_end": 4,
                "kind": "semantic_span_missing"
            }]
        }"#;
    let err = serde_json::from_str::<OwnedBytecodeSourceMapExport>(json)
        .expect_err("unknown source-map coverage fields must fail closed");

    assert!(err.to_string().contains("unknown field"), "{err}");
}

#[test]
fn bytecode_source_map_json_rejects_inconsistent_coverage_partition() {
    let json = r#"{
            "schema_version": 3,
            "bytecode_origin_coverage": {
                "total": 2,
                "sonatina_post_opt": 1,
                "sonatina_backend_prepared": 0,
                "unmapped": 0
            },
            "entries": [{
                "object": "Foo",
                "section": "runtime",
                "pc_start": 0,
                "pc_end": 4,
                "kind": "semantic_span_missing"
            }]
        }"#;
    let err = serde_json::from_str::<OwnedBytecodeSourceMapExport>(json)
        .expect_err("source-map coverage partitions must match their total");

    assert!(
        err.to_string()
            .contains("bytecode_origin_coverage total 2 does not match classified total 1"),
        "{err}"
    );
}

#[test]
fn bytecode_source_map_json_rejects_coverage_entry_count_mismatch() {
    let json = r#"{
            "schema_version": 3,
            "bytecode_origin_coverage": {
                "total": 2,
                "sonatina_post_opt": 1,
                "sonatina_backend_prepared": 1,
                "unmapped": 0
            },
            "entries": [{
                "object": "Foo",
                "section": "runtime",
                "pc_start": 0,
                "pc_end": 4,
                "kind": "semantic_span_missing"
            }]
        }"#;
    let err = serde_json::from_str::<OwnedBytecodeSourceMapExport>(json)
        .expect_err("source-map coverage totals must match exported entries");

    assert!(
        err.to_string()
            .contains("bytecode_origin_coverage total 2 does not match 1 source-map entries"),
        "{err}"
    );
}

#[test]
fn bytecode_source_map_json_rejects_unknown_post_opt_coverage_fields() {
    let json = r#"{
            "schema_version": 3,
            "post_opt_origin_coverage": {
                "total": 1,
                "same_inst_id": 1,
                "created_or_unmatched_after_preopt_snapshot": 0,
                "pre_opt_snapshot_losses": 0,
                "observed_pre_opt_total": 1,
                "extra": true
            },
            "entries": [{
                "object": "Foo",
                "section": "runtime",
                "pc_start": 0,
                "pc_end": 4,
                "kind": "semantic_span_missing"
            }]
        }"#;
    let err = serde_json::from_str::<OwnedBytecodeSourceMapExport>(json)
        .expect_err("unknown source-map post-opt coverage fields must fail closed");

    assert!(err.to_string().contains("unknown field"), "{err}");
}

#[test]
fn bytecode_source_map_json_rejects_inconsistent_post_opt_coverage_partition() {
    let json = r#"{
            "schema_version": 3,
            "post_opt_origin_coverage": {
                "total": 2,
                "same_inst_id": 1,
                "created_or_unmatched_after_preopt_snapshot": 0,
                "pre_opt_snapshot_losses": 0,
                "observed_pre_opt_total": 1
            },
            "entries": [{
                "object": "Foo",
                "section": "runtime",
                "pc_start": 0,
                "pc_end": 4,
                "kind": "semantic_span_missing"
            }]
        }"#;
    let err = serde_json::from_str::<OwnedBytecodeSourceMapExport>(json)
        .expect_err("source-map post-opt coverage partitions must match their total");

    assert!(
        err.to_string()
            .contains("post_opt_origin_coverage total 2 does not match classified total 1"),
        "{err}"
    );
}

#[test]
fn bytecode_source_map_json_rejects_inconsistent_post_opt_observed_total() {
    let json = r#"{
            "schema_version": 3,
            "post_opt_origin_coverage": {
                "total": 1,
                "same_inst_id": 1,
                "created_or_unmatched_after_preopt_snapshot": 0,
                "pre_opt_snapshot_losses": 1,
                "observed_pre_opt_total": 1
            },
            "entries": [{
                "object": "Foo",
                "section": "runtime",
                "pc_start": 0,
                "pc_end": 4,
                "kind": "semantic_span_missing"
            }]
        }"#;
    let err = serde_json::from_str::<OwnedBytecodeSourceMapExport>(json)
        .expect_err("source-map post-opt observed pre-opt totals must match");

    assert!(
            err.to_string().contains(
                "post_opt_origin_coverage observed_pre_opt_total 1 does not match same_inst_id plus pre_opt_snapshot_losses 2"
            ),
            "{err}"
        );
}

#[test]
fn bytecode_source_map_json_rejects_unknown_source_fields() {
    let json = r#"{
            "schema_version": 3,
            "entries": [{
                "object": "Foo",
                "section": "runtime",
                "pc_start": 0,
                "pc_end": 4,
                "kind": "source",
                "span_kind": "original",
                "file": "src/main.fe",
                "start_byte": 10,
                "end_byte": 14,
                "start_line": 1,
                "start_col": 2,
                "end_line": 1,
                "end_col": 6,
                "snippet": "main",
                "extra": true
            }]
        }"#;
    let err = serde_json::from_str::<OwnedBytecodeSourceMapExport>(json)
        .expect_err("unknown source-map source fields must fail closed");

    assert!(err.to_string().contains("unknown field"), "{err}");
}

#[test]
fn bytecode_source_map_json_rejects_missing_source_snippet() {
    let json = r#"{
            "schema_version": 3,
            "entries": [{
                "object": "Foo",
                "section": "runtime",
                "pc_start": 0,
                "pc_end": 4,
                "kind": "source",
                "span_kind": "original",
                "file": "src/main.fe",
                "start_byte": 10,
                "end_byte": 14,
                "start_line": 1,
                "start_col": 2,
                "end_line": 1,
                "end_col": 6
            }]
        }"#;
    let err = serde_json::from_str::<OwnedBytecodeSourceMapExport>(json)
        .expect_err("source-map source entries must carry a snippet");

    assert!(err.to_string().contains("missing field `snippet`"), "{err}");
}

#[test]
fn bytecode_source_map_json_rejects_empty_source_files() {
    let json = r#"{
            "schema_version": 3,
            "entries": [{
                "object": "Foo",
                "section": "runtime",
                "pc_start": 0,
                "pc_end": 4,
                "kind": "source",
                "span_kind": "original",
                "file": "",
                "start_byte": 10,
                "end_byte": 14,
                "start_line": 1,
                "start_col": 2,
                "end_line": 1,
                "end_col": 6,
                "snippet": "main"
            }]
        }"#;
    let err = serde_json::from_str::<OwnedBytecodeSourceMapExport>(json)
        .expect_err("source-map source file labels must be non-empty");

    assert!(
        err.to_string()
            .contains("bytecode source-map source file must not be empty"),
        "{err}"
    );
}

#[test]
fn bytecode_source_map_json_rejects_empty_source_snippets() {
    let json = r#"{
            "schema_version": 3,
            "entries": [{
                "object": "Foo",
                "section": "runtime",
                "pc_start": 0,
                "pc_end": 4,
                "kind": "source",
                "span_kind": "original",
                "file": "src/main.fe",
                "start_byte": 10,
                "end_byte": 14,
                "start_line": 1,
                "start_col": 2,
                "end_line": 1,
                "end_col": 6,
                "snippet": ""
            }]
        }"#;
    let err = serde_json::from_str::<OwnedBytecodeSourceMapExport>(json)
        .expect_err("source-map source snippets must be non-empty");

    assert!(
        err.to_string()
            .contains("bytecode source-map source snippet must not be empty"),
        "{err}"
    );
}

#[test]
fn bytecode_source_map_json_rejects_unknown_source_span_kind() {
    let json = r#"{
            "schema_version": 3,
            "entries": [{
                "object": "Foo",
                "section": "runtime",
                "pc_start": 0,
                "pc_end": 4,
                "kind": "source",
                "span_kind": "mystery",
                "file": "src/main.fe",
                "start_byte": 10,
                "end_byte": 14,
                "start_line": 1,
                "start_col": 2,
                "end_line": 1,
                "end_col": 6,
                "snippet": "main"
            }]
        }"#;
    let err = serde_json::from_str::<OwnedBytecodeSourceMapExport>(json)
        .expect_err("source-map source span kinds must be closed");

    assert!(
        err.to_string()
            .contains("unknown bytecode source-map source span kind `mystery`"),
        "{err}"
    );
}

#[test]
fn bytecode_source_map_json_roundtrips_source_span_invalid_reason() {
    let json = r#"{
            "schema_version": 3,
            "entries": [{
                "object": "Foo",
                "section": "runtime",
                "pc_start": 0,
                "pc_end": 4,
                "kind": "source_span_invalid",
                "reason": "invalid_snippet_range"
            }]
        }"#;
    let decoded: OwnedBytecodeSourceMapExport =
        serde_json::from_str(json).expect("closed invalid source-span reasons should decode");

    assert_eq!(
        decoded.entries()[0].kind().kind_name(),
        "source_span_invalid"
    );
    assert_eq!(
        decoded.entries()[0].kind().reason(),
        Some("invalid_snippet_range")
    );
}

#[test]
fn bytecode_source_map_json_rejects_unknown_source_span_invalid_reason() {
    let json = r#"{
            "schema_version": 3,
            "entries": [{
                "object": "Foo",
                "section": "runtime",
                "pc_start": 0,
                "pc_end": 4,
                "kind": "source_span_invalid",
                "reason": "mystery"
            }]
        }"#;
    let err = serde_json::from_str::<OwnedBytecodeSourceMapExport>(json)
        .expect_err("source-map invalid source-span reasons must be closed");

    assert!(
        err.to_string()
            .contains("unknown bytecode source-map source_span_invalid reason `mystery`"),
        "{err}"
    );
}

#[test]
fn bytecode_source_map_json_rejects_source_fields_for_source_span_invalid() {
    let json = r#"{
            "schema_version": 3,
            "entries": [{
                "object": "Foo",
                "section": "runtime",
                "pc_start": 0,
                "pc_end": 4,
                "kind": "source_span_invalid",
                "reason": "invalid_snippet_range",
                "file": "src/main.fe"
            }]
        }"#;
    let err = serde_json::from_str::<OwnedBytecodeSourceMapExport>(json)
        .expect_err("invalid source-span entries must not carry source payload fields");

    assert!(
        err.to_string()
            .contains("unexpected field `file` for source_span_invalid source-map entry"),
        "{err}"
    );
}

#[test]
fn bytecode_source_map_json_rejects_unknown_sonatina_synthetic_reason() {
    let json = r#"{
            "schema_version": 3,
            "entries": [{
                "object": "Foo",
                "section": "runtime",
                "pc_start": 0,
                "pc_end": 4,
                "kind": "sonatina_synthetic",
                "reason": "mystery"
            }]
        }"#;
    let err = serde_json::from_str::<OwnedBytecodeSourceMapExport>(json)
        .expect_err("source-map sonatina synthetic reasons must be closed");

    assert!(
        err.to_string()
            .contains("unknown bytecode source-map sonatina_synthetic reason `mystery`"),
        "{err}"
    );
}

#[test]
fn bytecode_source_map_json_rejects_unknown_sonatina_unmapped_reason() {
    let json = r#"{
            "schema_version": 3,
            "entries": [{
                "object": "Foo",
                "section": "runtime",
                "pc_start": 0,
                "pc_end": 4,
                "kind": "sonatina_unmapped",
                "reason": "mystery"
            }]
        }"#;
    let err = serde_json::from_str::<OwnedBytecodeSourceMapExport>(json)
        .expect_err("source-map sonatina unmapped reasons must be closed");

    assert!(
        err.to_string()
            .contains("unknown bytecode source-map sonatina_unmapped reason `mystery`"),
        "{err}"
    );
}

#[test]
fn bytecode_source_map_json_rejects_unknown_bytecode_unmapped_reason() {
    let json = r#"{
            "schema_version": 3,
            "entries": [{
                "object": "Foo",
                "section": "runtime",
                "pc_start": 0,
                "pc_end": 4,
                "kind": "bytecode_unmapped",
                "reason": "mystery"
            }]
        }"#;
    let err = serde_json::from_str::<OwnedBytecodeSourceMapExport>(json)
        .expect_err("source-map bytecode unmapped reasons must be closed");

    assert!(
        err.to_string()
            .contains("unknown bytecode source-map bytecode_unmapped reason `mystery`"),
        "{err}"
    );
}

#[test]
fn bytecode_source_map_json_rejects_inverted_source_byte_ranges() {
    let json = r#"{
            "schema_version": 3,
            "entries": [{
                "object": "Foo",
                "section": "runtime",
                "pc_start": 0,
                "pc_end": 4,
                "kind": "source",
                "span_kind": "original",
                "file": "src/main.fe",
                "start_byte": 14,
                "end_byte": 10,
                "start_line": 1,
                "start_col": 2,
                "end_line": 1,
                "end_col": 6,
                "snippet": "main"
            }]
        }"#;
    let err = serde_json::from_str::<OwnedBytecodeSourceMapExport>(json)
        .expect_err("source-map source byte ranges must be ordered");

    assert!(
        err.to_string()
            .contains("bytecode source-map source entry has invalid byte range 14..10"),
        "{err}"
    );
}

#[test]
fn bytecode_source_map_json_rejects_inverted_source_positions() {
    let json = r#"{
            "schema_version": 3,
            "entries": [{
                "object": "Foo",
                "section": "runtime",
                "pc_start": 0,
                "pc_end": 4,
                "kind": "source",
                "span_kind": "original",
                "file": "src/main.fe",
                "start_byte": 10,
                "end_byte": 14,
                "start_line": 1,
                "start_col": 6,
                "end_line": 1,
                "end_col": 2,
                "snippet": "main"
            }]
        }"#;
    let err = serde_json::from_str::<OwnedBytecodeSourceMapExport>(json)
        .expect_err("source-map source line/column ranges must be ordered");

    assert!(
        err.to_string()
            .contains("bytecode source-map source entry has invalid line/column range 1:6..1:2"),
        "{err}"
    );
}

#[test]
fn bytecode_source_map_json_rejects_export_object_mismatch() {
    let json = r#"{
            "schema_version": 3,
            "object": "Foo",
            "entries": [{
                "object": "Bar",
                "section": "runtime",
                "pc_start": 0,
                "pc_end": 4,
                "kind": "semantic_span_missing"
            }]
        }"#;
    let err = serde_json::from_str::<OwnedBytecodeSourceMapExport>(json)
        .expect_err("source-map export object must match entry objects");

    assert!(
        err.to_string()
            .contains("bytecode source-map export object `Foo` does not match entry object `Bar`"),
        "{err}"
    );
}

#[test]
fn bytecode_source_map_json_rejects_export_section_mismatch() {
    let json = r#"{
            "schema_version": 3,
            "section": "runtime",
            "entries": [{
                "object": "Foo",
                "section": "init",
                "pc_start": 0,
                "pc_end": 4,
                "kind": "semantic_span_missing"
            }]
        }"#;
    let err = serde_json::from_str::<OwnedBytecodeSourceMapExport>(json)
        .expect_err("source-map export section must match entry sections");

    assert!(
        err.to_string().contains(
            "bytecode source-map export section `runtime` does not match entry section `init`"
        ),
        "{err}"
    );
}

#[test]
fn bytecode_source_map_json_rejects_overlapping_pc_ranges() {
    let json = r#"{
            "schema_version": 3,
            "entries": [{
                "object": "Foo",
                "section": "runtime",
                "pc_start": 0,
                "pc_end": 8,
                "kind": "semantic_span_missing"
            }, {
                "object": "Foo",
                "section": "runtime",
                "pc_start": 4,
                "pc_end": 12,
                "kind": "runtime_synthetic"
            }]
        }"#;
    let err = serde_json::from_str::<OwnedBytecodeSourceMapExport>(json)
        .expect_err("source-map PC ranges must not overlap within one object section");

    assert!(
            err.to_string().contains(
                "bytecode source-map PC ranges overlap in object `Foo` section `runtime`: 0..8 overlaps 4..12"
            ),
            "{err}"
        );
}
