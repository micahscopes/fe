use super::*;

#[test]
fn analyze_origin_facts_reports_runtime_origin_facts_without_tests() {
    let temp = tempdir().expect("tempdir");
    let file_path = Utf8PathBuf::from_path_buf(temp.path().join("sample.fe")).unwrap();
    fs::write(
        file_path.as_std_path(),
        r#"
fn sample() -> u256 {
let x: u256 = 1
x
}
"#,
    )
    .expect("write fixture");

    let outcome = analyze_to_string(
        &file_path,
        None,
        true,
        json_options(false, false, false, true, false, false),
    )
    .expect("analyze succeeds");

    assert!(!outcome.has_errors);
    let report = analyze_report(&outcome.output);
    let origin_facts = &report.targets[0].origin_facts;
    assert!(
        origin_facts.iter().any(|report| {
            if report.scope != "runtime" {
                return false;
            }
            let facts = typed_origin_facts(report);
            report.origin_nodes > 0
                && report.origin_links > 0
                && typed_reachability(report).reachable_pairs() > 0
                && (has_reachable_kind_pair(report, "semantic", "runtime.stmt")
                    || has_reachable_kind_pair(report, "semantic", "runtime.terminator"))
                && (has_path_witness(report, "semantic", "runtime.stmt")
                    || has_path_witness(report, "semantic", "runtime.terminator"))
                && has_relation_count(&report.relation_counts, "origin_node")
                && has_relation_count(&report.relation_counts, "origin_link")
                && (has_origin_node_kind(&facts, OriginExportKind::RuntimeStmt)
                    || has_origin_node_kind(&facts, OriginExportKind::RuntimeTerminator))
                && has_origin_link_kind(&facts, OriginLinkKind::Lowered)
        }),
        "expected typed runtime origin facts in {report:#?}"
    );
}

#[test]
fn analyze_origin_facts_text_renders_path_witnesses() {
    let temp = tempdir().expect("tempdir");
    let file_path = Utf8PathBuf::from_path_buf(temp.path().join("sample.fe")).unwrap();
    fs::write(
        file_path.as_std_path(),
        r#"
fn sample() -> u256 {
let x: u256 = 1
x
}
"#,
    )
    .expect("write fixture");

    let outcome = analyze_to_string(
        &file_path,
        None,
        true,
        AnalyzeOptions::new(
            "dev",
            AnalyzeFormat::Text,
            false,
            false,
            false,
            true,
            false,
            false,
            false,
            OptLevel::O0,
            false,
        ),
    )
    .expect("analyze succeeds");

    assert!(!outcome.has_errors);
    assert!(
        outcome.output.contains("      paths:\n"),
        "{}",
        outcome.output
    );
    assert!(
        outcome.output.contains("semantic -> runtime."),
        "{}",
        outcome.output
    );
    assert!(
        outcome.output.contains("--lowered-->"),
        "{}",
        outcome.output
    );
    assert!(
        outcome.output.contains("      relation counts:"),
        "{}",
        outcome.output
    );
    assert!(
        outcome.output.contains("origin_node="),
        "{}",
        outcome.output
    );
    assert!(
        outcome.output.contains("origin_link="),
        "{}",
        outcome.output
    );
    assert!(
        outcome.output.contains("      reachable kind pairs:"),
        "{}",
        outcome.output
    );
    assert!(
        outcome.output.contains("semantic->runtime."),
        "{}",
        outcome.output
    );
}

#[test]
fn analyze_origin_fact_relation_tables_reports_engine_agnostic_rows() {
    let temp = tempdir().expect("tempdir");
    let file_path = Utf8PathBuf::from_path_buf(temp.path().join("sample.fe")).unwrap();
    fs::write(
        file_path.as_std_path(),
        r#"
fn sample() -> u256 {
let x: u256 = 1
x
}
"#,
    )
    .expect("write fixture");

    let outcome = analyze_to_string(
        &file_path,
        None,
        true,
        AnalyzeOptions::new(
            "dev",
            AnalyzeFormat::Json,
            false,
            false,
            false,
            true,
            true,
            false,
            false,
            OptLevel::O0,
            false,
        ),
    )
    .expect("analyze succeeds");

    assert!(!outcome.has_errors);
    let report = analyze_report(&outcome.output);
    let origin_facts = &report.targets[0].origin_facts;
    assert!(
        origin_facts.iter().any(|report| {
            report.scope == "runtime"
                && has_relation_table(&report.relation_tables, TypedFactRelationName::OriginNode)
                && has_relation_table(&report.relation_tables, TypedFactRelationName::OriginLink)
        }),
        "expected runtime origin relation tables in {report:#?}"
    );
}

#[test]
fn analyze_origin_facts_reports_typed_test_bytecode_facts() {
    let temp = tempdir().expect("tempdir");
    let root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let src = root.join("src");
    fs::create_dir_all(src.as_std_path()).expect("create src");
    fs::write(
        root.join("fe.toml").as_std_path(),
        "[ingot]\nname = \"analyze_origin_facts_app\"\nversion = \"0.1.0\"\n",
    )
    .expect("write config");
    let file_path = src.join("lib.fe");
    fs::write(
        file_path.as_std_path(),
        r#"
#[test]
fn test_origin_facts() {
let x: u256 = 1
let y: u256 = x + 2
}
"#,
    )
    .expect("write source");

    let outcome = analyze_to_string(
        &file_path,
        None,
        false,
        json_options(true, false, false, true, false, false),
    )
    .expect("analyze succeeds");

    assert!(!outcome.has_errors);
    let report = analyze_report(&outcome.output);
    let origin_facts = &report.targets[0].origin_facts;
    assert!(
        origin_facts
            .iter()
            .any(|report| { report.scope == "runtime" && report.origin_links > 0 }),
        "expected runtime origin facts alongside test bytecode facts in {report:#?}"
    );
    assert!(
        origin_facts.iter().any(|report| {
            if report.scope != "test_bytecode" || report.label != "test_origin_facts" {
                return false;
            }
            let facts = typed_origin_facts(report);
            report.origin_nodes > 0
                && report.origin_links > 0
                && report.source_spans > 0
                && has_source_span_file_count(report)
                && has_relation_count(&report.relation_counts, "origin_node")
                && has_relation_count(&report.relation_counts, "origin_link")
                && has_relation_count(&report.relation_counts, "source_span")
                && (has_reachable_kind_pair(report, "runtime.stmt", "bytecode.pc")
                    || has_reachable_kind_pair(report, "runtime.terminator", "bytecode.pc"))
                && (has_path_witness(report, "runtime.stmt", "bytecode.pc")
                    || has_path_witness(report, "runtime.terminator", "bytecode.pc"))
                && (has_source_path_witness(report, "runtime.stmt", "bytecode.pc")
                    || has_source_path_witness(report, "runtime.terminator", "bytecode.pc"))
                && has_origin_node_kind(&facts, OriginExportKind::BytecodePc)
                && (has_origin_link_kind(&facts, OriginLinkKind::Lowered)
                    || has_origin_link_kind(&facts, OriginLinkKind::Transformed))
                && has_source_span_fact(&facts)
        }),
        "expected typed origin facts for test bytecode in {report:#?}"
    );
    assert!(
        origin_facts.iter().any(|report| {
            if report.scope != "test_sonatina_snapshot"
                || report.label != "test_origin_facts"
                || report.object.as_deref() != Some("test_origin_facts")
            {
                return false;
            }
            let facts = typed_origin_facts(report);
            report.origin_nodes > 0
                && report.origin_links > 0
                && has_origin_node_kind(&facts, OriginExportKind::SonatinaInst)
                && has_origin_link_kind(&facts, OriginLinkKind::Alias)
        }),
        "expected typed Sonatina snapshot origin facts for test bytecode in {report:#?}"
    );
}
