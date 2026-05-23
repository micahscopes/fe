use super::*;

#[test]
fn analyze_fact_relation_tables_validate_option_dependencies() {
    let missing_facts = AnalyzeOptions::new(
        "dev",
        AnalyzeFormat::Json,
        false,
        false,
        false,
        false,
        true,
        false,
        false,
        OptLevel::O0,
        false,
    )
    .validate()
    .expect_err("relation tables require emitted typed facts");
    assert!(
        missing_facts.contains("requires `--origin-facts` or `--shape-facts`"),
        "{missing_facts}"
    );

    let text_format = AnalyzeOptions::new(
        "dev",
        AnalyzeFormat::Text,
        false,
        false,
        false,
        true,
        true,
        false,
        false,
        OptLevel::O0,
        false,
    )
    .validate()
    .expect_err("relation tables are JSON-only");
    assert!(
        text_format.contains("requires `--format json`"),
        "{text_format}"
    );
}

#[test]
fn analyze_standalone_file_reports_runtime_origin_summary_json() {
    let temp = tempdir().expect("tempdir");
    let file_path = Utf8PathBuf::from_path_buf(temp.path().join("sample.fe")).unwrap();
    fs::write(
        file_path.as_std_path(),
        r#"
fn sample() -> u256 {
1
}
"#,
    )
    .expect("write fixture");

    let outcome = analyze_to_string(
        &file_path,
        None,
        true,
        json_options(false, false, false, false, false, false),
    )
    .expect("analyze succeeds");

    assert!(!outcome.has_errors);
    let report = analyze_report(&outcome.output);
    assert_eq!(report.targets[0].label, file_path.as_str());
    assert!(
        report.targets[0].runtime_statements.total > 0,
        "expected runtime statement origins in {report:#?}"
    );
}

#[test]
fn analyze_file_inside_ingot_uses_ingot_context_by_default() {
    let temp = tempdir().expect("tempdir");
    let root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let src = root.join("src");
    fs::create_dir_all(src.as_std_path()).expect("create src");
    fs::write(
        root.join("fe.toml").as_std_path(),
        "[ingot]\nname = \"analyze_app\"\nversion = \"0.1.0\"\n",
    )
    .expect("write config");
    let file_path = src.join("lib.fe");
    fs::write(
        file_path.as_std_path(),
        r#"
fn sample() -> u256 {
1
}
"#,
    )
    .expect("write source");

    let outcome = analyze_to_string(
        &file_path,
        None,
        false,
        json_options(false, false, false, false, false, false),
    )
    .expect("analyze succeeds");

    assert!(!outcome.has_errors);
    let report = analyze_report(&outcome.output);
    assert_eq!(report.targets[0].label, "analyze_app");
}

#[test]
fn analyze_tests_mode_reports_test_runtime_origin_summary() {
    let temp = tempdir().expect("tempdir");
    let root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let src = root.join("src");
    fs::create_dir_all(src.as_std_path()).expect("create src");
    fs::write(
        root.join("fe.toml").as_std_path(),
        "[ingot]\nname = \"analyze_tests_app\"\nversion = \"0.1.0\"\n",
    )
    .expect("write config");
    let file_path = src.join("lib.fe");
    fs::write(
        file_path.as_std_path(),
        r#"
#[test]
fn test_sample() {
let x: u256 = 1
}
"#,
    )
    .expect("write source");

    let outcome = analyze_to_string(
        &file_path,
        None,
        false,
        json_options(true, false, false, false, false, false),
    )
    .expect("analyze succeeds");

    assert!(!outcome.has_errors);
    let report = analyze_report(&outcome.output);
    assert_eq!(report.package_kind, AnalyzePackageKind::Tests);
    assert!(
        report.targets[0].runtime_statements.total > 0,
        "expected test runtime statement origins in {report:#?}"
    );
}
