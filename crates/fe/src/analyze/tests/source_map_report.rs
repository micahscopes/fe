use super::*;

fn source_map_report_value() -> serde_json::Value {
    serde_json::json!({
        "scope": "test_bytecode",
        "label": "test_source_map",
        "test": "test_source_map",
        "object": "test_source_map",
        "section": "runtime",
        "total": 2,
        "source": 1,
        "debug_locations": 1,
        "debug_line_table_files": 1,
        "debug_line_table_rows": 1,
        "non_source": 1,
        "source_span_invalid": 0,
        "semantic_span_missing": 0,
        "runtime_stmt_missing": 0,
        "runtime_terminator_missing": 0,
        "runtime_synthetic": 0,
        "sonatina_synthetic": 0,
        "sonatina_unmapped": 0,
        "post_preopt_snapshot_gap": 0,
        "bytecode_unmapped": 1,
        "bytecode_origin_coverage": {
            "total": 2,
            "sonatina_post_opt": 1,
            "sonatina_backend_prepared": 0,
            "unmapped": 1
        },
        "post_opt_origin_coverage": {
            "total": 1,
            "same_inst_id": 1,
            "created_or_unmatched_after_preopt_snapshot": 0,
            "pre_opt_snapshot_losses": 0,
            "observed_pre_opt_total": 1
        },
        "entries": [{
            "object": "test_source_map",
            "section": "runtime",
            "pc_start": 0,
            "pc_end": 1,
            "kind": "source",
            "span_kind": "original",
            "file": "src/main.fe",
            "start_byte": 0,
            "end_byte": 4,
            "start_line": 0,
            "start_col": 0,
            "end_line": 0,
            "end_col": 4,
            "snippet": "main"
        }, {
            "object": "test_source_map",
            "section": "runtime",
            "pc_start": 1,
            "pc_end": 2,
            "kind": "bytecode_unmapped",
            "reason": "synthetic"
        }]
    })
}

#[test]
fn analyze_source_map_report_roundtrips_through_fail_closed_counts() {
    let value = source_map_report_value();
    let report = serde_json::from_value::<AnalyzeSourceMapReport>(value.clone())
        .expect("source-map report should decode");
    assert_eq!(report.total, 2);
    assert_eq!(report.non_source, 1);
    assert_eq!(report.entries.len(), 2);

    let json = serde_json::to_string(&report).expect("source-map report should serialize");
    serde_json::from_str::<AnalyzeSourceMapReport>(&json)
        .expect("source-map report should roundtrip");

    for (field, message) in [
        ("scope", "analyze source-map scope must not be empty"),
        ("label", "analyze source-map label must not be empty"),
        ("test", "analyze source-map test must not be empty"),
        ("object", "analyze source-map object must not be empty"),
        ("section", "analyze source-map section must not be empty"),
    ] {
        let mut empty_field = value.clone();
        empty_field[field] = serde_json::json!("");
        let err = serde_json::from_value::<AnalyzeSourceMapReport>(empty_field)
            .expect_err("source-map report should reject empty identity fields");
        assert!(err.to_string().contains(message), "{err}");
    }

    let mut no_entries = value.clone();
    no_entries
        .as_object_mut()
        .expect("report should be an object")
        .remove("entries");
    let decoded = serde_json::from_value::<AnalyzeSourceMapReport>(no_entries)
        .expect("entry rows are optional for compact reports");
    assert!(decoded.entries.is_empty());
    assert_eq!(decoded.total, 2);

    let mut bad_non_source = value.clone();
    bad_non_source["non_source"] = serde_json::json!(2);
    let err = serde_json::from_value::<AnalyzeSourceMapReport>(bad_non_source)
        .expect_err("source-map report should reject inconsistent non-source counts");
    assert!(
        err.to_string()
            .contains("non_source 2 does not match classified non-source count 1"),
        "{err}"
    );

    let mut bad_total = value.clone();
    bad_total["total"] = serde_json::json!(3);
    let err = serde_json::from_value::<AnalyzeSourceMapReport>(bad_total)
        .expect_err("source-map report should reject inconsistent totals");
    assert!(
        err.to_string()
            .contains("total 3 does not match source plus non_source count 2"),
        "{err}"
    );

    let mut bad_debug_locations = value.clone();
    bad_debug_locations["debug_locations"] = serde_json::json!(2);
    let err = serde_json::from_value::<AnalyzeSourceMapReport>(bad_debug_locations)
        .expect_err("source-map report should reject inconsistent debug-location counts");
    assert!(
        err.to_string()
            .contains("debug_locations 2 does not match source count 1"),
        "{err}"
    );

    let mut bad_debug_rows = value.clone();
    bad_debug_rows["debug_line_table_rows"] = serde_json::json!(2);
    let err = serde_json::from_value::<AnalyzeSourceMapReport>(bad_debug_rows)
        .expect_err("source-map report should reject inconsistent line-table rows");
    assert!(
        err.to_string()
            .contains("debug_line_table_rows 2 does not match source count 1"),
        "{err}"
    );

    let mut bad_debug_files = value.clone();
    bad_debug_files["debug_line_table_files"] = serde_json::json!(2);
    let err = serde_json::from_value::<AnalyzeSourceMapReport>(bad_debug_files)
        .expect_err("source-map report should reject impossible line-table file counts");
    assert!(
        err.to_string()
            .contains("debug_line_table_files 2 exceeds source count 1"),
        "{err}"
    );

    let mut bad_emitted_debug_files = value.clone();
    bad_emitted_debug_files["debug_line_table_files"] = serde_json::json!(0);
    let err = serde_json::from_value::<AnalyzeSourceMapReport>(bad_emitted_debug_files)
        .expect_err("source-map report should reject line-table file drift from entries");
    assert!(
        err.to_string()
            .contains("debug_line_table_files count 0 does not match emitted entry count 1"),
        "{err}"
    );

    let mut bad_coverage_total = value.clone();
    bad_coverage_total["bytecode_origin_coverage"] = serde_json::json!({
        "total": 3,
        "sonatina_post_opt": 2,
        "sonatina_backend_prepared": 0,
        "unmapped": 1
    });
    let err = serde_json::from_value::<AnalyzeSourceMapReport>(bad_coverage_total).expect_err(
        "source-map report should reject coverage totals that do not match report totals",
    );
    assert!(
        err.to_string()
            .contains("total 2 does not match bytecode origin coverage total 3"),
        "{err}"
    );

    let mut bad_entry_count = value.clone();
    bad_entry_count["entries"] = serde_json::json!([bad_entry_count["entries"][0].clone()]);
    let err = serde_json::from_value::<AnalyzeSourceMapReport>(bad_entry_count)
        .expect_err("source-map report should reject entry-count drift");
    assert!(
        err.to_string()
            .contains("total 2 does not match emitted entry count 1"),
        "{err}"
    );

    let mut bad_entry_object = value.clone();
    bad_entry_object["entries"][1]["object"] = serde_json::json!("other_object");
    let err = serde_json::from_value::<AnalyzeSourceMapReport>(bad_entry_object)
        .expect_err("source-map report should reject entry object drift");
    assert!(
        err.to_string()
            .contains("object `test_source_map` does not match entry object `other_object`"),
        "{err}"
    );

    let mut bad_entry_section = value.clone();
    bad_entry_section["entries"][1]["section"] = serde_json::json!("init");
    let err = serde_json::from_value::<AnalyzeSourceMapReport>(bad_entry_section)
        .expect_err("source-map report should reject entry section drift");
    assert!(
        err.to_string()
            .contains("section `runtime` does not match entry section `init`"),
        "{err}"
    );

    let mut aggregate_section = value.clone();
    aggregate_section["section"] = serde_json::json!(ANALYZE_SOURCE_MAP_ALL_SECTIONS);
    aggregate_section["entries"][1]["section"] = serde_json::json!("init");
    serde_json::from_value::<AnalyzeSourceMapReport>(aggregate_section)
        .expect("aggregate section reports can include multiple entry sections");

    let mut bad_entry_kind = value.clone();
    bad_entry_kind["entries"][1] = serde_json::json!({
        "object": "test_source_map",
        "section": "runtime",
        "pc_start": 1,
        "pc_end": 2,
        "kind": "semantic_span_missing"
    });
    let err = serde_json::from_value::<AnalyzeSourceMapReport>(bad_entry_kind)
        .expect_err("source-map report should reject entry classification drift");
    assert!(
        err.to_string()
            .contains("semantic_span_missing count 0 does not match emitted entry count 1"),
        "{err}"
    );
}

#[test]
fn analyze_source_map_report_builder_returns_error_for_entry_identity_drift() {
    let report = serde_json::from_value::<AnalyzeSourceMapReport>(source_map_report_value())
        .expect("source-map report should decode");
    let summary = bytecode_source_map_entries_summary(&report.entries, None)
        .expect("test entries should produce a summary");
    let err = AnalyzeSourceMapReport::try_from_summary(
        "test_bytecode",
        report.label.clone(),
        report.test.clone(),
        "other_object".to_string(),
        report.section.clone(),
        &summary,
        None,
        None,
        report.entries.clone(),
    )
    .expect_err("source-map report construction should fail instead of panicking");
    assert!(
        err.to_string()
            .contains("object `other_object` does not match entry object `test_source_map`"),
        "{err}"
    );
}
