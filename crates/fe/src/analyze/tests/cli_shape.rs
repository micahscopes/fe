use super::*;

#[test]
fn analyze_shape_hashes_reports_const_region_shapes() {
    let temp = tempdir().expect("tempdir");
    let file_path = Utf8PathBuf::from_path_buf(temp.path().join("sample.fe")).unwrap();
    fs::write(
        file_path.as_std_path(),
        r#"
const C: [u256; 3] = [10, 20, 30]

fn sample() -> u256 {
C[1]
}
"#,
    )
    .expect("write fixture");

    let outcome = analyze_to_string(
        &file_path,
        None,
        true,
        json_options(false, false, false, false, true, true),
    )
    .expect("analyze succeeds");

    assert!(!outcome.has_errors);
    let report = analyze_report(&outcome.output);
    let shapes = &report.targets[0].shapes;
    assert!(
        shapes.iter().any(|shape| {
            if shape.scope != "const_region" {
                return false;
            }
            let facts = shape_facts(shape);
            shape.shape_nodes > 0
                && shape
                    .graph_hashes
                    .iter()
                    .any(|hash| hash.dimension == ShapeDimension::Constants)
                && shape
                    .graph_hashes
                    .iter()
                    .any(|hash| hash.dimension == ShapeDimension::Types)
                && has_relation_count(&shape.relation_counts, "shape_node")
                && has_relation_count(&shape.relation_counts, "shape_hash")
                && has_shape_node_fact(&facts)
                && has_shape_hash_fact(&facts)
        }),
        "expected const-region shape hashes and facts in {report:#?}"
    );
    assert!(
        shapes.iter().any(|shape| {
            if shape.scope != "runtime_body" {
                return false;
            }
            let facts = shape_facts(shape);
            shape.shape_nodes > 0
                && shape
                    .graph_hashes
                    .iter()
                    .any(|hash| hash.dimension == ShapeDimension::Structure)
                && shape
                    .graph_hashes
                    .iter()
                    .any(|hash| hash.dimension == ShapeDimension::Constants)
                && has_relation_count(&shape.relation_counts, "shape_node")
                && has_relation_count(&shape.relation_counts, "shape_hash")
                && has_shape_node_fact(&facts)
                && has_shape_hash_fact(&facts)
        }),
        "expected runtime body shape hashes and facts in {report:#?}"
    );
}

#[test]
fn analyze_shape_hashes_text_renders_all_graph_dimensions() {
    let temp = tempdir().expect("tempdir");
    let file_path = Utf8PathBuf::from_path_buf(temp.path().join("sample.fe")).unwrap();
    fs::write(
        file_path.as_std_path(),
        r#"
const C: [u256; 3] = [10, 20, 30]

fn sample() -> u256 {
C[1]
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
            false,
            false,
            true,
            true,
            OptLevel::O0,
            false,
        ),
    )
    .expect("analyze succeeds");

    assert!(!outcome.has_errors);
    for dimension in ["structure", "names", "constants", "types", "trace_events"] {
        assert!(
            outcome.output.contains(&format!("{dimension}=")),
            "{}",
            outcome.output
        );
    }
    assert!(
        outcome.output.contains("      relation counts:"),
        "{}",
        outcome.output
    );
    assert!(outcome.output.contains("shape_node="), "{}", outcome.output);
    assert!(outcome.output.contains("shape_hash="), "{}", outcome.output);
}

#[test]
fn analyze_shape_fact_relation_tables_reports_engine_agnostic_rows() {
    let temp = tempdir().expect("tempdir");
    let file_path = Utf8PathBuf::from_path_buf(temp.path().join("sample.fe")).unwrap();
    fs::write(
        file_path.as_std_path(),
        r#"
const C: [u256; 3] = [10, 20, 30]

fn sample() -> u256 {
C[1]
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
            false,
            true,
            true,
            true,
            OptLevel::O0,
            false,
        ),
    )
    .expect("analyze succeeds");

    assert!(!outcome.has_errors);
    let report = analyze_report(&outcome.output);
    let shapes = &report.targets[0].shapes;
    assert!(
        shapes.iter().any(|shape| {
            shape.scope == "const_region"
                && has_relation_count(&shape.relation_counts, "shape_node")
                && has_relation_count(&shape.relation_counts, "shape_hash")
                && has_relation_table(&shape.relation_tables, TypedFactRelationName::ShapeNode)
                && has_relation_table(&shape.relation_tables, TypedFactRelationName::ShapeHash)
        }),
        "expected shape relation tables in {report:#?}"
    );
}
