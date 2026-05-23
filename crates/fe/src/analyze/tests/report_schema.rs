use super::*;

fn minimal_analyze_report_value() -> serde_json::Value {
    serde_json::json!({
        "schema_version": ANALYZE_REPORT_SCHEMA_VERSION,
        "profile": "dev",
        "package_kind": "runtime",
        "targets": [{
            "label": "target",
            "runtime_bodies": 0,
            "runtime_statements": {
                "total": 0,
                "semantic": 0,
                "synthetic": 0
            },
            "runtime_terminators": {
                "total": 0,
                "semantic": 0,
                "synthetic": 0
            },
            "bodies": [],
            "source_maps": [],
            "origin_facts": [],
            "shapes": []
        }]
    })
}

fn analyze_body_report_value() -> serde_json::Value {
    serde_json::json!({
        "symbol": "target::main",
        "statements": {
            "total": 2,
            "semantic": 1,
            "synthetic": 1
        },
        "terminators": {
            "total": 1,
            "semantic": 1,
            "synthetic": 0
        }
    })
}

fn analyze_report_with_body_value() -> serde_json::Value {
    let body = analyze_body_report_value();
    serde_json::json!({
        "schema_version": ANALYZE_REPORT_SCHEMA_VERSION,
        "profile": "dev",
        "package_kind": "runtime",
        "targets": [{
            "label": "target",
            "runtime_bodies": 1,
            "runtime_statements": {
                "total": 2,
                "semantic": 1,
                "synthetic": 1
            },
            "runtime_terminators": {
                "total": 1,
                "semantic": 1,
                "synthetic": 0
            },
            "bodies": [body],
            "source_maps": [],
            "origin_facts": [],
            "shapes": []
        }]
    })
}

#[test]
fn analyze_body_report_roundtrips_through_fail_closed_schema() {
    let report = serde_json::from_value::<AnalyzeBodyReport>(analyze_body_report_value())
        .expect("body report should decode");
    assert_eq!(report.symbol, "target::main");

    let json = serde_json::to_string(&report).expect("body report should serialize");
    serde_json::from_str::<AnalyzeBodyReport>(&json).expect("body report should roundtrip");

    let mut empty_symbol = analyze_body_report_value();
    empty_symbol["symbol"] = serde_json::json!("");
    let err = serde_json::from_value::<AnalyzeBodyReport>(empty_symbol)
        .expect_err("body report should reject empty symbols");
    assert!(
        err.to_string().contains("body symbol must not be empty"),
        "{err}"
    );

    let mut unknown_field = analyze_body_report_value();
    unknown_field["extra"] = serde_json::json!(true);
    let err = serde_json::from_value::<AnalyzeBodyReport>(unknown_field)
        .expect_err("body report should reject unknown fields");
    assert!(err.to_string().contains("unknown field"), "{err}");
}

#[test]
fn analyze_report_roundtrips_through_fail_closed_schema() {
    let report = serde_json::from_value::<AnalyzeReport>(minimal_analyze_report_value())
        .expect("minimal analyze report should decode");
    assert_eq!(report.schema_version, ANALYZE_REPORT_SCHEMA_VERSION);
    assert_eq!(report.package_kind, AnalyzePackageKind::Runtime);

    let json = serde_json::to_string(&report).expect("analyze report should serialize");
    serde_json::from_str::<AnalyzeReport>(&json).expect("analyze report should roundtrip");

    let mut bad_schema = minimal_analyze_report_value();
    bad_schema["schema_version"] = serde_json::json!(ANALYZE_REPORT_SCHEMA_VERSION + 1);
    let err = serde_json::from_value::<AnalyzeReport>(bad_schema)
        .expect_err("analyze report should reject unsupported schema versions");
    assert!(
        err.to_string()
            .contains("unsupported analyze report schema_version 2; expected 1"),
        "{err}"
    );

    let mut empty_profile = minimal_analyze_report_value();
    empty_profile["profile"] = serde_json::json!("");
    let err = serde_json::from_value::<AnalyzeReport>(empty_profile)
        .expect_err("analyze report should reject empty profiles");
    assert!(
        err.to_string().contains("profile must not be empty"),
        "{err}"
    );

    let mut unknown_package_kind = minimal_analyze_report_value();
    unknown_package_kind["package_kind"] = serde_json::json!("single_file");
    let err = serde_json::from_value::<AnalyzeReport>(unknown_package_kind)
        .expect_err("analyze report should reject unknown package kinds");
    assert!(err.to_string().contains("unknown variant"), "{err}");

    let mut bad_body_count = minimal_analyze_report_value();
    bad_body_count["targets"][0]["runtime_bodies"] = serde_json::json!(1);
    let err = serde_json::from_value::<AnalyzeReport>(bad_body_count)
        .expect_err("analyze report should reject target body-count drift");
    assert!(
        err.to_string()
            .contains("runtime_bodies 1 does not match body count 0"),
        "{err}"
    );

    let mut empty_target_label = minimal_analyze_report_value();
    empty_target_label["targets"][0]["label"] = serde_json::json!("");
    let err = serde_json::from_value::<AnalyzeReport>(empty_target_label)
        .expect_err("analyze report should reject empty target labels");
    assert!(
        err.to_string().contains("target label must not be empty"),
        "{err}"
    );

    let mut duplicate_target_label = minimal_analyze_report_value();
    let duplicate_target = duplicate_target_label["targets"][0].clone();
    duplicate_target_label["targets"]
        .as_array_mut()
        .expect("test target rows should be an array")
        .push(duplicate_target);
    let err = serde_json::from_value::<AnalyzeReport>(duplicate_target_label)
        .expect_err("analyze report should reject duplicate target labels");
    assert!(
        err.to_string().contains("duplicate target label `target`"),
        "{err}"
    );

    let report_with_body =
        serde_json::from_value::<AnalyzeReport>(analyze_report_with_body_value())
            .expect("analyze report with body rows should decode");
    assert_eq!(report_with_body.targets[0].runtime_bodies, 1);

    let mut empty_body_symbol = analyze_report_with_body_value();
    empty_body_symbol["targets"][0]["bodies"][0]["symbol"] = serde_json::json!("");
    let err = serde_json::from_value::<AnalyzeReport>(empty_body_symbol)
        .expect_err("analyze report should reject empty body symbols");
    assert!(
        err.to_string().contains("body symbol must not be empty"),
        "{err}"
    );

    let mut duplicate_body_symbol = analyze_report_with_body_value();
    duplicate_body_symbol["targets"][0]["runtime_bodies"] = serde_json::json!(2);
    let duplicate_body = duplicate_body_symbol["targets"][0]["bodies"][0].clone();
    duplicate_body_symbol["targets"][0]["bodies"]
        .as_array_mut()
        .expect("test body rows should be an array")
        .push(duplicate_body);
    let err = serde_json::from_value::<AnalyzeReport>(duplicate_body_symbol)
        .expect_err("analyze report should reject duplicate body symbols");
    assert!(
        err.to_string()
            .contains("duplicate body symbol `target::main`"),
        "{err}"
    );

    let mut bad_statement_count = analyze_report_with_body_value();
    bad_statement_count["targets"][0]["runtime_statements"] = serde_json::json!({
        "total": 3,
        "semantic": 2,
        "synthetic": 1
    });
    let err = serde_json::from_value::<AnalyzeReport>(bad_statement_count)
        .expect_err("analyze report should reject statement aggregate drift");
    assert!(
        err.to_string().contains(
            "runtime_statements total=3 semantic=2 synthetic=1 does not match body sum total=2 semantic=1 synthetic=1"
        ),
        "{err}"
    );

    let mut bad_terminator_count = analyze_report_with_body_value();
    bad_terminator_count["targets"][0]["runtime_terminators"] = serde_json::json!({
        "total": 2,
        "semantic": 1,
        "synthetic": 1
    });
    let err = serde_json::from_value::<AnalyzeReport>(bad_terminator_count)
        .expect_err("analyze report should reject terminator aggregate drift");
    assert!(
        err.to_string().contains(
            "runtime_terminators total=2 semantic=1 synthetic=1 does not match body sum total=1 semantic=1 synthetic=0"
        ),
        "{err}"
    );
}

#[test]
fn origin_count_roundtrips_through_fail_closed_schema() {
    let count = OriginCount::try_new(3, 2, 1).expect("origin count should validate");
    let json = serde_json::to_string(&count).expect("origin count should serialize");
    let decoded = serde_json::from_str::<OriginCount>(&json).expect("origin count should decode");
    assert_eq!(decoded, count);

    assert_eq!(
        OriginCount::try_new(4, 2, 1),
        Err(OriginCountError::TotalMismatch {
            declared: 4,
            actual: 3
        })
    );

    let mismatched_total = r#"{
        "total": 4,
        "semantic": 2,
        "synthetic": 1
    }"#;
    let err = serde_json::from_str::<OriginCount>(mismatched_total)
        .expect_err("origin count JSON should reject inconsistent totals");
    assert!(
        err.to_string()
            .contains("total 4 does not match semantic plus synthetic count 3"),
        "{err}"
    );

    let unknown_field = r#"{
        "total": 3,
        "semantic": 2,
        "synthetic": 1,
        "extra": 0
    }"#;
    let err = serde_json::from_str::<OriginCount>(unknown_field)
        .expect_err("origin count JSON should reject unknown fields");
    assert!(err.to_string().contains("unknown field"), "{err}");

    let mut report = minimal_analyze_report_value();
    report["targets"][0]["runtime_statements"] = serde_json::json!({
        "total": 2,
        "semantic": 2,
        "synthetic": 1
    });
    let err = serde_json::from_value::<AnalyzeReport>(report.clone())
        .expect_err("analyze report should reject inconsistent nested origin counts");
    assert!(
        err.to_string()
            .contains("total 2 does not match semantic plus synthetic count 3"),
        "{err}"
    );

    report["targets"][0]["runtime_statements"] = serde_json::json!({
        "total": 0,
        "semantic": 0,
        "synthetic": 0
    });
    serde_json::from_value::<AnalyzeReport>(report)
        .expect("analyze report should accept consistent nested origin counts");
}
