use super::*;

#[test]
fn analyze_shape_hash_report_uses_closed_dimensions() {
    let valid = r#"{"dimension":"structure","digest_hex":"0000000000000000"}"#;
    let decoded = serde_json::from_str::<AnalyzeShapeHashReport>(valid)
        .expect("known shape hash dimension should decode");
    assert_eq!(decoded.dimension, ShapeDimension::Structure);

    let unknown_dimension = r#"{"dimension":"unknown","digest_hex":"0000000000000000"}"#;
    serde_json::from_str::<AnalyzeShapeHashReport>(unknown_dimension)
        .expect_err("unknown shape hash dimensions should fail closed");

    let unknown_field = r#"{"dimension":"structure","digest_hex":"0000000000000000","extra":true}"#;
    let err = serde_json::from_str::<AnalyzeShapeHashReport>(unknown_field)
        .expect_err("shape hash reports should reject unknown fields");
    assert!(err.to_string().contains("unknown field"));

    let invalid_digest = r#"{"dimension":"structure","digest_hex":"ABCDEF0000000000"}"#;
    let err = serde_json::from_str::<AnalyzeShapeHashReport>(invalid_digest)
        .expect_err("shape hash reports should reject non-canonical digests");
    assert!(
        err.to_string()
            .contains("canonical 16-character lowercase hex"),
        "{err}"
    );
}

fn minimal_shape_report_value() -> serde_json::Value {
    let dimensions = [
        ("structure", "0000000000000000"),
        ("names", "0000000000000001"),
        ("constants", "0000000000000002"),
        ("types", "0000000000000003"),
        ("trace_events", "0000000000000004"),
    ];
    let node = serde_json::json!({"namespace": "shape_node", "ordinal": 0});
    let mut facts = vec![serde_json::json!({
        "type": "shape_node",
        "id": node,
        "source_id": 0,
        "stable_key": "root",
        "kind": "block"
    })];

    for (dimension, digest_hex) in dimensions {
        facts.push(serde_json::json!({
            "type": "shape_hash",
            "node": null,
            "scope": "graph",
            "dimension": dimension,
            "digest_hex": digest_hex
        }));
    }
    for (idx, (dimension, _)) in dimensions.iter().enumerate() {
        facts.push(serde_json::json!({
            "type": "shape_hash",
            "node": {"namespace": "shape_node", "ordinal": 0},
            "scope": "local",
            "dimension": dimension,
            "digest_hex": format!("{:016x}", idx + 0x10)
        }));
        facts.push(serde_json::json!({
            "type": "shape_hash",
            "node": {"namespace": "shape_node", "ordinal": 0},
            "scope": "tree",
            "dimension": dimension,
            "digest_hex": format!("{:016x}", idx + 0x20)
        }));
    }

    serde_json::json!({
        "scope": "test_shape",
        "label": "minimal",
        "shape_nodes": 1,
        "shape_fields": 0,
        "shape_children": 0,
        "shape_edges": 0,
        "trace_events": 0,
        "data_flows": 0,
        "graph_hashes": dimensions
            .into_iter()
            .map(|(dimension, digest_hex)| {
                serde_json::json!({
                    "dimension": dimension,
                    "digest_hex": digest_hex
                })
            })
            .collect::<Vec<_>>(),
        "relation_counts": [
            {"relation": "shape_node", "rows": 1},
            {"relation": "shape_hash", "rows": 15}
        ],
        "facts": {
            "schema_version": 1,
            "facts": facts
        }
    })
}

#[test]
fn analyze_shape_report_roundtrips_through_fail_closed_counts() {
    let value = minimal_shape_report_value();
    serde_json::from_value::<AnalyzeShapeReport>(value.clone())
        .expect("valid shape report should decode");

    for (field, message) in [
        ("scope", "analyze shape scope must not be empty"),
        ("label", "analyze shape label must not be empty"),
    ] {
        let mut empty_field = value.clone();
        empty_field[field] = serde_json::json!("");
        let err = serde_json::from_value::<AnalyzeShapeReport>(empty_field)
            .expect_err("shape report should reject empty identity fields");
        assert!(err.to_string().contains(message), "{err}");
    }

    let mut duplicate_graph_hash = value.clone();
    duplicate_graph_hash["graph_hashes"][1]["dimension"] = serde_json::json!("structure");
    let err = serde_json::from_value::<AnalyzeShapeReport>(duplicate_graph_hash)
        .expect_err("shape report should reject duplicate graph hash dimensions");
    assert!(
        err.to_string().contains("duplicate dimension structure"),
        "{err}"
    );

    let mut bad_shape_node_count = value.clone();
    bad_shape_node_count["shape_nodes"] = serde_json::json!(2);
    let err = serde_json::from_value::<AnalyzeShapeReport>(bad_shape_node_count)
        .expect_err("shape report should reject fact count drift");
    assert!(
        err.to_string()
            .contains("shape_nodes 2 does not match typed fact count 1"),
        "{err}"
    );

    let mut bad_graph_digest = value.clone();
    bad_graph_digest["graph_hashes"][0]["digest_hex"] = serde_json::json!("1111111111111111");
    let err = serde_json::from_value::<AnalyzeShapeReport>(bad_graph_digest)
        .expect_err("shape report should reject graph hash digest drift");
    assert!(
        err.to_string().contains(
            "graph_hashes structure digest 1111111111111111 does not match typed fact digest 0000000000000000"
        ),
        "{err}"
    );

    let mut unexpected_relation = value.clone();
    unexpected_relation["relation_counts"] =
        serde_json::json!([{"relation": "origin_node", "rows": 1}]);
    let err = serde_json::from_value::<AnalyzeShapeReport>(unexpected_relation)
        .expect_err("shape report should reject origin relation counts");
    assert!(
        err.to_string()
            .contains("contains non-shape relation origin_node"),
        "{err}"
    );

    let mut duplicate_relation_count = value.clone();
    duplicate_relation_count["relation_counts"] = serde_json::json!([
        {"relation": "shape_node", "rows": 1},
        {"relation": "shape_node", "rows": 1}
    ]);
    let err = serde_json::from_value::<AnalyzeShapeReport>(duplicate_relation_count)
        .expect_err("shape report should reject duplicate relation counts");
    assert!(
        err.to_string()
            .contains("relation_counts contains duplicate relation shape_node"),
        "{err}"
    );

    let mut missing_relation_count = value.clone();
    missing_relation_count
        .as_object_mut()
        .expect("shape report should be an object")
        .remove("relation_counts");
    let err = serde_json::from_value::<AnalyzeShapeReport>(missing_relation_count)
        .expect_err("shape report should require counts for non-empty fact relations");
    assert!(
        err.to_string()
            .contains("relation_counts is missing shape_node=1"),
        "{err}"
    );

    let mut mismatched_relation_table = value.clone();
    let mut relation_tables = relation_tables_value_for_report(&mismatched_relation_table);
    relation_rows_mut(&mut relation_tables, "shape_node")[0]
        .as_array_mut()
        .expect("shape_node row should be an array")[2] = serde_json::json!("other-root");
    mismatched_relation_table["relation_tables"] = relation_tables;
    let err = serde_json::from_value::<AnalyzeShapeReport>(mismatched_relation_table)
        .expect_err("shape report should reject relation table rows that drift from facts");
    assert!(
        err.to_string()
            .contains("relation table shape_node rows do not match typed facts"),
        "{err}"
    );

    let mut relation_tables_without_facts = value.clone();
    relation_tables_without_facts["relation_tables"] =
        relation_tables_value_for_report(&relation_tables_without_facts);
    relation_tables_without_facts
        .as_object_mut()
        .expect("shape report should be an object")
        .remove("facts");
    let err = serde_json::from_value::<AnalyzeShapeReport>(relation_tables_without_facts)
        .expect_err("shape report relation tables should require emitted facts");
    assert!(
        err.to_string()
            .contains("relation_tables require emitted typed facts"),
        "{err}"
    );

    let mut bad_hash_count = value;
    bad_hash_count["relation_counts"] = serde_json::json!([{"relation": "shape_hash", "rows": 14}]);
    let err = serde_json::from_value::<AnalyzeShapeReport>(bad_hash_count)
        .expect_err("shape report should reject shape hash relation count drift");
    assert!(
        err.to_string()
            .contains("relation count shape_hash=14 does not match report count 15"),
        "{err}"
    );
}
