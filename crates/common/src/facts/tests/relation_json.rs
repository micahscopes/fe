use super::*;

#[test]
fn typed_fact_relation_json_rejects_unknown_schema_version() {
    let json = r#"{"schema_version":2,"relations":[]}"#;
    let err = serde_json::from_str::<TypedFactRelationSet>(json)
        .expect_err("unknown relation schema versions must fail closed");

    assert!(
        err.to_string()
            .contains("unsupported typed fact relation schema_version 2"),
        "{err}"
    );
}

#[test]
fn typed_fact_relation_json_rejects_unknown_export_fields() {
    let json = r#"{"schema_version":1,"relations":[],"extra":true}"#;
    let err = serde_json::from_str::<TypedFactRelationSet>(json)
        .expect_err("unknown relation export fields must fail closed");

    assert!(err.to_string().contains("unknown field"), "{err}");
}

#[test]
fn typed_fact_relation_json_rejects_unknown_relation_fields() {
    let json = r#"{
        "schema_version": 1,
        "relations": [{
            "name": "origin_node",
            "columns": ["id", "kind", "owner_key", "local_key"],
            "rows": [],
            "extra": true
        }]
    }"#;
    let err = serde_json::from_str::<TypedFactRelationSet>(json)
        .expect_err("unknown relation fields must fail closed");

    assert!(err.to_string().contains("unknown field"), "{err}");
}

#[test]
fn typed_fact_relation_json_rejects_unknown_relation_names() {
    let json = r#"{
        "schema_version": 1,
        "relations": [{
            "name": "unknown_relation",
            "columns": [],
            "rows": []
        }]
    }"#;
    let err = serde_json::from_str::<TypedFactRelationSet>(json)
        .expect_err("unknown relation names must fail closed");

    assert!(
        err.to_string()
            .contains("unknown typed fact relation `unknown_relation`"),
        "{err}"
    );
}

#[test]
fn typed_fact_relation_json_rejects_missing_relations() {
    let json = r#"{"schema_version":1,"relations":[]}"#;
    let err = serde_json::from_str::<TypedFactRelationSet>(json)
        .expect_err("missing relation tables must fail closed");

    assert!(
        err.to_string()
            .contains("missing typed fact relation `origin_node`"),
        "{err}"
    );
}

#[test]
fn typed_fact_relation_json_rejects_duplicate_relations() {
    let json = r#"{
        "schema_version": 1,
        "relations": [
            {
                "name": "origin_node",
                "columns": ["id", "kind", "owner_key", "local_key"],
                "rows": []
            },
            {
                "name": "origin_node",
                "columns": ["id", "kind", "owner_key", "local_key"],
                "rows": []
            }
        ]
    }"#;
    let err = serde_json::from_str::<TypedFactRelationSet>(json)
        .expect_err("duplicate relation tables must fail closed");

    assert!(
        err.to_string()
            .contains("duplicate typed fact relation `origin_node`"),
        "{err}"
    );
}

#[test]
fn typed_fact_relation_json_rejects_wrong_columns() {
    let json = r#"{
        "schema_version": 1,
        "relations": [{
            "name": "origin_node",
            "columns": ["id"],
            "rows": []
        }]
    }"#;
    let err = serde_json::from_str::<TypedFactRelationSet>(json)
        .expect_err("relation table columns must match fixed schema");

    assert!(
        err.to_string()
            .contains("typed fact relation `origin_node` has columns"),
        "{err}"
    );
}

#[test]
fn typed_fact_relation_json_rejects_wrong_row_width() {
    let json = r#"{
        "schema_version": 1,
        "relations": [{
            "name": "origin_link",
            "columns": ["from", "to", "kind"],
            "rows": [["origin_node:0", "origin_node:1"]]
        }]
    }"#;
    let err = serde_json::from_str::<TypedFactRelationSet>(json)
        .expect_err("relation table row widths must match fixed schema");

    assert!(
        err.to_string()
            .contains("typed fact relation `origin_link` row 0 has 2 columns; expected 3"),
        "{err}"
    );
}
