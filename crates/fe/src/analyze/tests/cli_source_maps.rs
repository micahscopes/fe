use super::*;

#[test]
fn analyze_source_maps_reports_typed_test_bytecode_summary() {
    let temp = tempdir().expect("tempdir");
    let root = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
    let src = root.join("src");
    fs::create_dir_all(src.as_std_path()).expect("create src");
    fs::write(
        root.join("fe.toml").as_std_path(),
        "[ingot]\nname = \"analyze_source_maps_app\"\nversion = \"0.1.0\"\n",
    )
    .expect("write config");
    let file_path = src.join("lib.fe");
    fs::write(
        file_path.as_std_path(),
        r#"
#[test]
fn test_source_map() {
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
        json_options(true, true, true, false, false, false),
    )
    .expect("analyze succeeds");

    assert!(!outcome.has_errors);
    let report = analyze_report(&outcome.output);
    let source_maps = &report.targets[0].source_maps;
    assert!(
        source_maps.iter().any(|source_map| {
            if source_map.scope != "test_bytecode"
                || source_map.label != "test_source_map"
                || source_map.test.as_deref() != Some("test_source_map")
            {
                return false;
            }
            source_map.total > 0
                && source_map.source > 0
                && source_map.debug_locations > 0
                && source_map.debug_line_table_files > 0
                && source_map.debug_line_table_rows > 0
                && has_partitioned_bytecode_origin_coverage(source_map)
                && has_partitioned_post_opt_origin_coverage(source_map)
                && has_typed_source_entry(&source_map.entries)
        }),
        "expected source-map summary with full source entries in {report:#?}"
    );
}

#[test]
fn analyze_source_maps_reports_runtime_bytecode_summary_without_tests() {
    let temp = tempdir().expect("tempdir");
    let file_path = Utf8PathBuf::from_path_buf(temp.path().join("sample.fe")).unwrap();
    fs::write(
        file_path.as_std_path(),
        r#"
msg FooMsg {
#[selector = 0x12345678]
Ping -> u256,
}

pub contract Foo {
recv FooMsg {
    Ping -> u256 {
        let x: u256 = 1
        return x + 2
    }
}
}
"#,
    )
    .expect("write fixture");

    let outcome = analyze_to_string(
        &file_path,
        None,
        true,
        json_options(false, true, true, true, false, false),
    )
    .expect("analyze succeeds");

    assert!(!outcome.has_errors);
    let report = analyze_report(&outcome.output);
    let source_maps = &report.targets[0].source_maps;
    assert!(
        source_maps.iter().any(|source_map| {
            if source_map.scope != "runtime_bytecode"
                || source_map.label != "Foo"
                || source_map.object != "Foo"
            {
                return false;
            }
            source_map.total > 0
                && source_map.debug_locations > 0
                && source_map.debug_line_table_files > 0
                && source_map.debug_line_table_rows > 0
                && has_partitioned_bytecode_origin_coverage(source_map)
                && has_partitioned_post_opt_origin_coverage(source_map)
                && has_typed_source_entry(&source_map.entries)
        }),
        "expected runtime bytecode source-map report in {report:#?}"
    );

    let origin_facts = &report.targets[0].origin_facts;
    assert!(
        origin_facts.iter().any(|report| {
            if report.scope != "runtime_bytecode"
                || report.label != "Foo"
                || report.object.as_deref() != Some("Foo")
            {
                return false;
            }
            let facts = typed_origin_facts(report);
            has_path_witness(report, "semantic", "bytecode.pc")
                && has_source_path_witness(report, "semantic", "bytecode.pc")
                && (has_reachable_kind_pair(report, "runtime.stmt", "bytecode.pc")
                    || has_reachable_kind_pair(report, "runtime.terminator", "bytecode.pc"))
                && (has_path_witness(report, "runtime.stmt", "bytecode.pc")
                    || has_path_witness(report, "runtime.terminator", "bytecode.pc"))
                && (has_source_path_witness(report, "runtime.stmt", "bytecode.pc")
                    || has_source_path_witness(report, "runtime.terminator", "bytecode.pc"))
                && has_source_span_file_count(report)
                && has_origin_node_kind(&facts, OriginExportKind::BytecodePc)
                && has_source_span_fact(&facts)
        }),
        "expected runtime bytecode origin facts in {report:#?}"
    );
    assert!(
        origin_facts.iter().any(|report| {
            if report.scope != "runtime_sonatina_snapshot"
                || report.label != "Foo"
                || report.object.as_deref() != Some("Foo")
            {
                return false;
            }
            let facts = typed_origin_facts(report);
            has_origin_node_kind(&facts, OriginExportKind::SonatinaInst)
                || has_origin_node_kind(&facts, OriginExportKind::SonatinaSynthetic)
        }),
        "expected runtime Sonatina snapshot origin facts in {report:#?}"
    );
}

#[test]
fn analyze_source_maps_text_renders_classification_and_entries() {
    let temp = tempdir().expect("tempdir");
    let file_path = Utf8PathBuf::from_path_buf(temp.path().join("sample.fe")).unwrap();
    fs::write(
        file_path.as_std_path(),
        r#"
msg FooMsg {
#[selector = 0x12345678]
Ping -> u256,
}

pub contract Foo {
recv FooMsg {
    Ping -> u256 {
        let x: u256 = 1
        return x + 2
    }
}
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
            true,
            true,
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
    for expected in [
        "  source maps:\n",
        "classification: source=",
        "debug_locations=",
        "debug_line_table_files=",
        "debug_line_table_rows=",
        "post_opt_origins=",
        "source_span_invalid=",
        "semantic_span_missing=",
        "runtime_stmt_missing=",
        "runtime_terminator_missing=",
        "sonatina_unmapped=",
        "      entries:\n",
        "kind=source",
        "snippet=",
        "      source span files:",
        "      source paths:\n",
        "source span_kind=original",
    ] {
        assert!(outcome.output.contains(expected), "{}", outcome.output);
    }
}
