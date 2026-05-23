use super::*;

#[test]
fn typed_fact_json_rejects_unknown_schema_version() {
    let json = r#"{"schema_version":2,"facts":[]}"#;
    let err = serde_json::from_str::<OwnedTypedFactSetExport>(json)
        .expect_err("unknown typed fact schema versions must fail closed");

    assert!(
        err.to_string()
            .contains("unsupported typed fact schema_version 2"),
        "{err}"
    );
}

#[test]
fn typed_fact_json_rejects_unknown_export_fields() {
    let json = r#"{"schema_version":1,"facts":[],"extra":true}"#;
    let err = serde_json::from_str::<OwnedTypedFactSetExport>(json)
        .expect_err("unknown typed fact export fields must fail closed");

    assert!(err.to_string().contains("unknown field"), "{err}");
}

#[test]
fn typed_fact_json_rejects_unknown_fact_fields() {
    let json = r#"{
        "schema_version": 1,
        "facts": [{
            "type": "origin_link",
            "from": {"namespace": "origin_node", "ordinal": 0},
            "to": {"namespace": "origin_node", "ordinal": 1},
            "kind": "lowered",
            "extra": true
        }]
    }"#;
    let err = serde_json::from_str::<OwnedTypedFactSetExport>(json)
        .expect_err("unknown typed fact row fields must fail closed");

    assert!(err.to_string().contains("unknown field"), "{err}");
}

#[test]
fn typed_fact_json_rejects_unknown_nested_key_fields() {
    let json = r#"{
        "schema_version": 1,
        "facts": [{
            "type": "origin_node",
            "id": {"namespace": "origin_node", "ordinal": 0},
            "key": {
                "kind": "semantic",
                "owner_key": "semantic:a",
                "local_key": "expr:0",
                "extra": true
            }
        }]
    }"#;
    let err = serde_json::from_str::<OwnedTypedFactSetExport>(json)
        .expect_err("unknown nested origin key fields must fail closed");

    assert!(err.to_string().contains("unknown field"), "{err}");
}

#[test]
fn typed_fact_json_rejects_missing_origin_link_endpoint() {
    let json = r#"{
        "schema_version": 1,
        "facts": [{
            "type": "origin_node",
            "id": {"namespace": "origin_node", "ordinal": 0},
            "key": {
                "kind": "runtime.stmt",
                "owner_key": "runtime:a",
                "local_key": "block:0:stmt:0"
            }
        }, {
            "type": "origin_link",
            "from": {"namespace": "origin_node", "ordinal": 0},
            "to": {"namespace": "origin_node", "ordinal": 1},
            "kind": "lowered"
        }]
    }"#;
    let err = serde_json::from_str::<OwnedTypedFactSetExport>(json)
        .expect_err("typed fact JSON must reject missing origin link endpoints");

    assert!(
        err.to_string().contains(
            "invalid origin facts in typed fact export: origin link references missing endpoint origin_node:1"
        ),
        "{err}"
    );
}

#[test]
fn typed_fact_json_rejects_duplicate_origin_links() {
    let json = r#"{
        "schema_version": 1,
        "facts": [{
            "type": "origin_node",
            "id": {"namespace": "origin_node", "ordinal": 0},
            "key": {
                "kind": "semantic",
                "owner_key": "semantic:a",
                "local_key": "expr:0"
            }
        }, {
            "type": "origin_node",
            "id": {"namespace": "origin_node", "ordinal": 1},
            "key": {
                "kind": "runtime.stmt",
                "owner_key": "runtime:a",
                "local_key": "block:0:stmt:0"
            }
        }, {
            "type": "origin_link",
            "from": {"namespace": "origin_node", "ordinal": 0},
            "to": {"namespace": "origin_node", "ordinal": 1},
            "kind": "lowered"
        }, {
            "type": "origin_link",
            "from": {"namespace": "origin_node", "ordinal": 0},
            "to": {"namespace": "origin_node", "ordinal": 1},
            "kind": "lowered"
        }]
    }"#;
    let err = serde_json::from_str::<OwnedTypedFactSetExport>(json)
        .expect_err("typed fact JSON must reject duplicate origin links");

    assert!(
        err.to_string().contains(
            "invalid origin facts in typed fact export: duplicate origin link origin_node:0 -> origin_node:1 (lowered)"
        ),
        "{err}"
    );
}

#[test]
fn typed_fact_json_rejects_duplicate_origin_ids() {
    let json = r#"{
        "schema_version": 1,
        "facts": [{
            "type": "origin_node",
            "id": {"namespace": "origin_node", "ordinal": 0},
            "key": {
                "kind": "semantic",
                "owner_key": "semantic:a",
                "local_key": "expr:0"
            }
        }, {
            "type": "origin_node",
            "id": {"namespace": "origin_node", "ordinal": 0},
            "key": {
                "kind": "runtime.stmt",
                "owner_key": "runtime:a",
                "local_key": "block:0:stmt:0"
            }
        }]
    }"#;
    let err = serde_json::from_str::<OwnedTypedFactSetExport>(json)
        .expect_err("typed fact JSON must reject duplicate origin node ids");

    assert!(
        err.to_string()
            .contains("invalid origin facts in typed fact export: duplicate origin fact id"),
        "{err}"
    );
}

#[test]
fn typed_fact_json_rejects_missing_source_span_origin() {
    let json = r#"{
        "schema_version": 1,
        "facts": [{
            "type": "origin_node",
            "id": {"namespace": "origin_node", "ordinal": 0},
            "key": {
                "kind": "bytecode.pc",
                "owner_key": "object:Foo:section:runtime",
                "local_key": "pc:0..4"
            }
        }, {
            "type": "source_span",
            "origin": {"namespace": "origin_node", "ordinal": 1},
            "span_kind": "original",
            "file": "file:///missing_source_span.fe",
            "start_byte": 0,
            "end_byte": 4,
            "start_line": 0,
            "start_col": 0,
            "end_line": 0,
            "end_col": 4
        }]
    }"#;
    let err = serde_json::from_str::<OwnedTypedFactSetExport>(json)
        .expect_err("typed fact JSON must reject source spans for missing origins");

    assert!(
        err.to_string().contains(
            "invalid origin facts in typed fact export: source span references missing origin origin_node:1"
        ),
        "{err}"
    );
}

#[test]
fn typed_fact_json_rejects_empty_source_span_files() {
    let json = r#"{
        "schema_version": 1,
        "facts": [{
            "type": "origin_node",
            "id": {"namespace": "origin_node", "ordinal": 0},
            "key": {
                "kind": "bytecode.pc",
                "owner_key": "object:Foo:section:runtime",
                "local_key": "pc:0..4"
            }
        }, {
            "type": "source_span",
            "origin": {"namespace": "origin_node", "ordinal": 0},
            "span_kind": "original",
            "file": "",
            "start_byte": 0,
            "end_byte": 4,
            "start_line": 0,
            "start_col": 0,
            "end_line": 0,
            "end_col": 4
        }]
    }"#;
    let err = serde_json::from_str::<OwnedTypedFactSetExport>(json)
        .expect_err("typed fact JSON must reject empty source-span files");

    assert!(
        err.to_string()
            .contains("source span file must not be empty"),
        "{err}"
    );
}

#[test]
fn typed_fact_json_rejects_unknown_source_span_kind() {
    let json = r#"{
        "schema_version": 1,
        "facts": [{
            "type": "origin_node",
            "id": {"namespace": "origin_node", "ordinal": 0},
            "key": {
                "kind": "bytecode.pc",
                "owner_key": "object:Foo:section:runtime",
                "local_key": "pc:0..4"
            }
        }, {
            "type": "source_span",
            "origin": {"namespace": "origin_node", "ordinal": 0},
            "span_kind": "mystery",
            "file": "file:///unknown_span_kind.fe",
            "start_byte": 0,
            "end_byte": 4,
            "start_line": 0,
            "start_col": 0,
            "end_line": 0,
            "end_col": 4
        }]
    }"#;
    let err = serde_json::from_str::<OwnedTypedFactSetExport>(json)
        .expect_err("typed fact JSON must reject unknown source span kinds");

    assert!(err.to_string().contains("unknown variant"), "{err}");
}

#[test]
fn typed_fact_json_rejects_inverted_source_span_positions() {
    let json = r#"{
        "schema_version": 1,
        "facts": [{
            "type": "origin_node",
            "id": {"namespace": "origin_node", "ordinal": 0},
            "key": {
                "kind": "bytecode.pc",
                "owner_key": "object:Foo:section:runtime",
                "local_key": "pc:0..4"
            }
        }, {
            "type": "source_span",
            "origin": {"namespace": "origin_node", "ordinal": 0},
            "span_kind": "original",
            "file": "file:///bad_position.fe",
            "start_byte": 0,
            "end_byte": 4,
            "start_line": 1,
            "start_col": 0,
            "end_line": 0,
            "end_col": 4
        }]
    }"#;
    let err = serde_json::from_str::<OwnedTypedFactSetExport>(json)
        .expect_err("typed fact JSON must reject inverted source span positions");

    assert!(
        err.to_string()
            .contains("source span line/column range must be ordered: 1:0..0:4"),
        "{err}"
    );
}

#[test]
fn source_span_export_is_deterministic_and_keyed_by_origin() {
    let first = origin_key(
        OriginExportKind::BytecodePc,
        "object:A:section:runtime",
        "pc:0..4",
    );
    let second = origin_key(
        OriginExportKind::BytecodePc,
        "object:B:section:runtime",
        "pc:0..4",
    );
    let mut graph = OriginGraph::new();
    graph.push(first.clone(), second.clone(), OriginLinkKind::Alias);

    let source_spans = [
        SourceSpanExport::new(
            second.clone(),
            SourceSpanKind::Original,
            "file:///b.fe",
            8,
            12,
            1,
            0,
            1,
            4,
        ),
        SourceSpanExport::new(
            first.clone(),
            SourceSpanKind::Original,
            "file:///a.fe",
            0,
            4,
            0,
            0,
            0,
            4,
        ),
    ];

    let facts = origin_graph_facts(&graph, Clone::clone)
        .with_source_spans(source_spans.clone())
        .expect("source spans should attach to exported origin facts");
    let facts_with_reversed_input = origin_graph_facts(&graph, Clone::clone)
        .with_source_spans(source_spans.into_iter().rev())
        .expect("source spans should attach to exported origin facts");

    assert_eq!(facts, facts_with_reversed_input);
    let index = OriginFactIndex::new(&facts).expect("facts should index");
    assert_eq!(
        index
            .source_spans_for_key(&first)
            .map(|span| span.file())
            .collect::<Vec<_>>(),
        vec!["file:///a.fe"]
    );
    assert_eq!(
        index
            .source_spans_for_key(&second)
            .map(|span| span.file())
            .collect::<Vec<_>>(),
        vec!["file:///b.fe"]
    );
}

#[test]
fn typed_fact_json_rejects_missing_shape_child_endpoint() {
    let json = r#"{
        "schema_version": 1,
        "facts": [{
            "type": "shape_node",
            "id": {"namespace": "shape_node", "ordinal": 0},
            "source_id": 0,
            "stable_key": "root",
            "kind": "block"
        }, {
            "type": "shape_child",
            "parent": {"namespace": "shape_node", "ordinal": 0},
            "child": {"namespace": "shape_node", "ordinal": 1},
            "label": "missing",
            "order": 0
        }]
    }"#;
    let err = serde_json::from_str::<OwnedTypedFactSetExport>(json)
        .expect_err("typed fact JSON must reject missing shape child endpoints");

    assert!(
        err.to_string().contains(
            "invalid shape facts in typed fact export: shape fact references missing node shape_node:1"
        ),
        "{err}"
    );
}

#[test]
fn typed_fact_json_rejects_missing_trace_event_node() {
    let json = r#"{
        "schema_version": 1,
        "facts": [{
            "type": "trace_event",
            "node": {"namespace": "shape_node", "ordinal": 0},
            "event_kind": "runtime_code_region",
            "value": "runtime_code_region_ref"
        }]
    }"#;
    let err = serde_json::from_str::<OwnedTypedFactSetExport>(json)
        .expect_err("typed fact JSON must reject trace events for missing nodes");

    assert!(
        err.to_string().contains(
            "invalid shape facts in typed fact export: shape fact references missing node shape_node:0"
        ),
        "{err}"
    );
}

#[test]
fn typed_fact_json_rejects_missing_data_flow_endpoint() {
    let json = r#"{
        "schema_version": 1,
        "facts": [{
            "type": "data_flow",
            "source": {"namespace": "shape_node", "ordinal": 0},
            "target": {"namespace": "shape_node", "ordinal": 1},
            "kind": "data-flow:operand"
        }]
    }"#;
    let err = serde_json::from_str::<OwnedTypedFactSetExport>(json)
        .expect_err("typed fact JSON must reject data-flow rows with missing endpoints");

    assert!(
        err.to_string().contains(
            "invalid shape facts in typed fact export: shape fact references missing node shape_node:0"
        ),
        "{err}"
    );
}

#[test]
fn typed_fact_json_rejects_empty_shape_stable_keys() {
    let json = r#"{
        "schema_version": 1,
        "facts": [{
            "type": "shape_node",
            "id": {"namespace": "shape_node", "ordinal": 0},
            "source_id": 0,
            "stable_key": "",
            "kind": "block"
        }]
    }"#;
    let err = serde_json::from_str::<OwnedTypedFactSetExport>(json)
        .expect_err("typed fact JSON must reject empty shape stable keys");

    assert!(
        err.to_string()
            .contains("shape stable key must not be empty"),
        "{err}"
    );
}

#[test]
fn typed_fact_json_rejects_empty_shape_child_labels() {
    let json = r#"{
        "schema_version": 1,
        "facts": [{
            "type": "shape_node",
            "id": {"namespace": "shape_node", "ordinal": 0},
            "source_id": 0,
            "stable_key": "root",
            "kind": "block"
        }, {
            "type": "shape_node",
            "id": {"namespace": "shape_node", "ordinal": 1},
            "source_id": 1,
            "stable_key": "leaf",
            "kind": "literal"
        }, {
            "type": "shape_child",
            "parent": {"namespace": "shape_node", "ordinal": 0},
            "child": {"namespace": "shape_node", "ordinal": 1},
            "label": "",
            "order": 0
        }]
    }"#;
    let err = serde_json::from_str::<OwnedTypedFactSetExport>(json)
        .expect_err("typed fact JSON must reject empty shape child labels");

    assert!(
        err.to_string()
            .contains("shape child label must not be empty"),
        "{err}"
    );
}

#[test]
fn typed_fact_json_rejects_duplicate_shape_ids() {
    let json = r#"{
        "schema_version": 1,
        "facts": [{
            "type": "shape_node",
            "id": {"namespace": "shape_node", "ordinal": 0},
            "source_id": 0,
            "stable_key": "root",
            "kind": "block"
        }, {
            "type": "shape_node",
            "id": {"namespace": "shape_node", "ordinal": 0},
            "source_id": 1,
            "stable_key": "leaf",
            "kind": "literal"
        }]
    }"#;
    let err = serde_json::from_str::<OwnedTypedFactSetExport>(json)
        .expect_err("typed fact JSON must reject duplicate shape node ids");

    assert!(
        err.to_string()
            .contains("invalid shape facts in typed fact export: duplicate shape fact id"),
        "{err}"
    );
}

#[test]
fn typed_fact_json_rejects_shape_hash_scope_node_mismatch() {
    let json = r#"{
        "schema_version": 1,
        "facts": [{
            "type": "shape_node",
            "id": {"namespace": "shape_node", "ordinal": 0},
            "source_id": 0,
            "stable_key": "root",
            "kind": "block"
        }, {
            "type": "shape_hash",
            "node": null,
            "scope": "local",
            "dimension": "structure",
            "digest_hex": "0000000000000000"
        }]
    }"#;
    let err = serde_json::from_str::<OwnedTypedFactSetExport>(json)
        .expect_err("typed fact JSON must reject local/tree shape hashes without nodes");

    assert!(
        err.to_string()
            .contains("shape hash scope local has invalid node reference none"),
        "{err}"
    );
}

#[test]
fn typed_fact_json_rejects_malformed_shape_hash_digest() {
    let json = r#"{
        "schema_version": 1,
        "facts": [{
            "type": "shape_node",
            "id": {"namespace": "shape_node", "ordinal": 0},
            "source_id": 0,
            "stable_key": "root",
            "kind": "block"
        }, {
            "type": "shape_hash",
            "node": {"namespace": "shape_node", "ordinal": 0},
            "scope": "local",
            "dimension": "structure",
            "digest_hex": "ABCDEF0000000000"
        }]
    }"#;
    let err = serde_json::from_str::<OwnedTypedFactSetExport>(json)
        .expect_err("typed fact JSON must reject non-canonical shape hash digests");

    assert!(
        err.to_string()
            .contains("canonical 16-character lowercase hex"),
        "{err}"
    );
}

#[test]
fn typed_fact_json_rejects_duplicate_shape_hashes() {
    let json = r#"{
        "schema_version": 1,
        "facts": [{
            "type": "shape_hash",
            "node": null,
            "scope": "graph",
            "dimension": "structure",
            "digest_hex": "0000000000000000"
        }, {
            "type": "shape_hash",
            "node": null,
            "scope": "graph",
            "dimension": "structure",
            "digest_hex": "0000000000000001"
        }]
    }"#;
    let err = serde_json::from_str::<OwnedTypedFactSetExport>(json)
        .expect_err("typed fact JSON must reject duplicate shape hashes");

    assert!(
        err.to_string().contains(
            "invalid shape facts in typed fact export: duplicate shape hash for scope graph dimension structure at node graph"
        ),
        "{err}"
    );
}

#[test]
fn typed_fact_json_rejects_incomplete_shape_hash_sets() {
    let json = r#"{
        "schema_version": 1,
        "facts": [{
            "type": "shape_node",
            "id": {"namespace": "shape_node", "ordinal": 0},
            "source_id": 0,
            "stable_key": "root",
            "kind": "block"
        }]
    }"#;
    let err = serde_json::from_str::<OwnedTypedFactSetExport>(json)
        .expect_err("typed fact JSON must reject incomplete shape hash sets");

    assert!(
        err.to_string().contains(
            "invalid shape facts in typed fact export: missing shape hash for scope graph dimension structure at node graph"
        ),
        "{err}"
    );
}
