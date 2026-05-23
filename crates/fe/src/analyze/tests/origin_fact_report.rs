use super::*;

fn origin_fact_report_value() -> serde_json::Value {
    serde_json::json!({
        "scope": "runtime",
        "label": "Foo",
        "total": 1,
        "origin_nodes": 1,
        "origin_links": 0,
        "source_spans": 0,
        "relation_counts": [{
            "relation": "origin_node",
            "rows": 1
        }],
        "facts": {
            "schema_version": 1,
            "facts": [{
                "type": "origin_node",
                "id": {"namespace": "origin_node", "ordinal": 0},
                "key": {
                    "kind": "semantic",
                    "owner_key": "semantic:Foo",
                    "local_key": "expr:0"
                }
            }]
        }
    })
}

fn origin_fact_report_with_source_span_value() -> serde_json::Value {
    serde_json::json!({
        "scope": "runtime_bytecode",
        "label": "Foo",
        "object": "Foo",
        "total": 2,
        "origin_nodes": 1,
        "origin_links": 0,
        "source_spans": 1,
        "source_span_files": [{
            "file": "file:///foo.fe",
            "spans": 1
        }],
        "relation_counts": [{
            "relation": "origin_node",
            "rows": 1
        }, {
            "relation": "source_span",
            "rows": 1
        }],
        "facts": {
            "schema_version": 1,
            "facts": [{
                "type": "origin_node",
                "id": {"namespace": "origin_node", "ordinal": 0},
                "key": {
                    "kind": "bytecode.pc",
                    "owner_key": "object:Foo:section:runtime",
                    "local_key": "pc:0..1"
                }
            }, {
                "type": "source_span",
                "origin": {"namespace": "origin_node", "ordinal": 0},
                "span_kind": "original",
                "file": "file:///foo.fe",
                "start_byte": 0,
                "end_byte": 4,
                "start_line": 0,
                "start_col": 0,
                "end_line": 0,
                "end_col": 4
            }]
        }
    })
}

#[test]
fn analyze_origin_fact_report_roundtrips_through_fail_closed_counts() {
    let value = origin_fact_report_value();
    let report = serde_json::from_value::<AnalyzeOriginFactReport>(value.clone())
        .expect("origin-fact report should decode");
    assert_eq!(report.total, 1);
    assert_eq!(report.origin_nodes, 1);
    assert_eq!(report.origin_links, 0);
    assert_eq!(report.source_spans, 0);

    let json = serde_json::to_string(&report).expect("origin-fact report should serialize");
    serde_json::from_str::<AnalyzeOriginFactReport>(&json)
        .expect("origin-fact report should roundtrip");

    for (field, message) in [
        ("scope", "analyze origin-fact scope must not be empty"),
        ("label", "analyze origin-fact label must not be empty"),
    ] {
        let mut empty_field = value.clone();
        empty_field[field] = serde_json::json!("");
        let err = serde_json::from_value::<AnalyzeOriginFactReport>(empty_field)
            .expect_err("origin-fact report should reject empty identity fields");
        assert!(err.to_string().contains(message), "{err}");
    }

    let mut empty_object = origin_fact_report_with_source_span_value();
    empty_object["object"] = serde_json::json!("");
    let err = serde_json::from_value::<AnalyzeOriginFactReport>(empty_object)
        .expect_err("origin-fact report should reject empty optional objects");
    assert!(
        err.to_string()
            .contains("analyze origin-fact object must not be empty"),
        "{err}"
    );

    let mut empty_query_error = value.clone();
    empty_query_error["query_error"] = serde_json::json!("");
    let err = serde_json::from_value::<AnalyzeOriginFactReport>(empty_query_error)
        .expect_err("origin-fact report should reject empty query errors");
    assert!(
        err.to_string()
            .contains("analyze origin-fact query_error must not be empty"),
        "{err}"
    );

    let mut bad_total = value.clone();
    bad_total["total"] = serde_json::json!(2);
    let err = serde_json::from_value::<AnalyzeOriginFactReport>(bad_total)
        .expect_err("origin-fact report should reject inconsistent totals");
    assert!(
        err.to_string()
            .contains("total 2 does not match origin fact count 1"),
        "{err}"
    );

    let mut bad_origin_nodes = value.clone();
    bad_origin_nodes["origin_nodes"] = serde_json::json!(2);
    let err = serde_json::from_value::<AnalyzeOriginFactReport>(bad_origin_nodes)
        .expect_err("origin-fact report should reject inconsistent origin node counts");
    assert!(
        err.to_string()
            .contains("origin_nodes 2 does not match typed fact count 1"),
        "{err}"
    );

    let mut bad_relation_count = value.clone();
    bad_relation_count["relation_counts"] = serde_json::json!([{
        "relation": "origin_node",
        "rows": 2
    }]);
    let err = serde_json::from_value::<AnalyzeOriginFactReport>(bad_relation_count)
        .expect_err("origin-fact report should reject inconsistent relation counts");
    assert!(
        err.to_string()
            .contains("relation count origin_node=2 does not match report count 1"),
        "{err}"
    );

    let mut missing_relation_count = value.clone();
    missing_relation_count
        .as_object_mut()
        .expect("origin-fact report should be an object")
        .remove("relation_counts");
    let err = serde_json::from_value::<AnalyzeOriginFactReport>(missing_relation_count)
        .expect_err("origin-fact report should require counts for non-empty fact relations");
    assert!(
        err.to_string()
            .contains("relation_counts is missing origin_node=1"),
        "{err}"
    );

    let mut duplicate_relation_count = value.clone();
    duplicate_relation_count["relation_counts"] = serde_json::json!([{
        "relation": "origin_node",
        "rows": 1
    }, {
        "relation": "origin_node",
        "rows": 1
    }]);
    let err = serde_json::from_value::<AnalyzeOriginFactReport>(duplicate_relation_count)
        .expect_err("origin-fact report should reject duplicate relation counts");
    assert!(
        err.to_string()
            .contains("relation_counts contains duplicate relation origin_node"),
        "{err}"
    );

    let mut bad_unexpected_relation = value.clone();
    bad_unexpected_relation["relation_counts"] = serde_json::json!([{
        "relation": "shape_node",
        "rows": 1
    }]);
    let err = serde_json::from_value::<AnalyzeOriginFactReport>(bad_unexpected_relation)
        .expect_err("origin-fact report should reject shape relation counts");
    assert!(
        err.to_string()
            .contains("relation_counts contains non-origin relation shape_node"),
        "{err}"
    );

    let mut bad_unexpected_relation_table = value.clone();
    let mut relation_tables = relation_tables_value_for_report(&bad_unexpected_relation_table);
    relation_rows_mut(&mut relation_tables, "shape_node").push(serde_json::json!([
        "shape_node:0",
        "0",
        "root",
        "block"
    ]));
    bad_unexpected_relation_table["relation_tables"] = relation_tables;
    let err = serde_json::from_value::<AnalyzeOriginFactReport>(bad_unexpected_relation_table)
        .expect_err("origin-fact report should reject populated shape relation tables");
    assert!(
        err.to_string()
            .contains("relation table shape_node has non-origin rows 1"),
        "{err}"
    );

    let mut mismatched_relation_table = value.clone();
    let mut relation_tables = relation_tables_value_for_report(&mismatched_relation_table);
    relation_rows_mut(&mut relation_tables, "origin_node")[0]
        .as_array_mut()
        .expect("origin_node row should be an array")[2] = serde_json::json!("semantic:Other");
    mismatched_relation_table["relation_tables"] = relation_tables;
    let err = serde_json::from_value::<AnalyzeOriginFactReport>(mismatched_relation_table)
        .expect_err("origin-fact report should reject relation table rows that drift from facts");
    assert!(
        err.to_string()
            .contains("relation table origin_node rows do not match typed facts"),
        "{err}"
    );

    let mut with_source_span = origin_fact_report_with_source_span_value();
    serde_json::from_value::<AnalyzeOriginFactReport>(with_source_span.clone())
        .expect("origin-fact report with source span should decode");

    let mut duplicate_source_span_file = with_source_span.clone();
    let duplicate_file = duplicate_source_span_file["source_span_files"][0].clone();
    duplicate_source_span_file["source_span_files"]
        .as_array_mut()
        .expect("test source-span file rows should be an array")
        .push(duplicate_file);
    let err = serde_json::from_value::<AnalyzeOriginFactReport>(duplicate_source_span_file)
        .expect_err("origin-fact report should reject duplicate source-span file summaries");
    assert!(
        err.to_string()
            .contains("source_span_files contains duplicate file `file:///foo.fe`"),
        "{err}"
    );

    with_source_span["source_span_files"][0]["spans"] = serde_json::json!(2);
    let err = serde_json::from_value::<AnalyzeOriginFactReport>(with_source_span)
        .expect_err("origin-fact report should reject source-span file summary drift");
    assert!(
        err.to_string()
            .contains("source_span_files total 2 does not match source_spans 1"),
        "{err}"
    );
}
