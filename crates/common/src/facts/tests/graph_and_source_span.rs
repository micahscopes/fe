use super::*;

#[test]
fn origin_graph_export_uses_typed_namespaced_ids() {
    let hir = origin_key(OriginExportKind::HirExpr, "body:a", "expr:0");
    let first_stmt = origin_key(OriginExportKind::RuntimeStmt, "runtime:f", "bb0:stmt0");
    let second_stmt = origin_key(OriginExportKind::RuntimeStmt, "runtime:f", "bb0:stmt1");
    let mut graph = OriginGraph::new();
    graph.push(hir.clone(), first_stmt.clone(), OriginLinkKind::Lowered);
    graph.push(hir.clone(), second_stmt.clone(), OriginLinkKind::Lowered);

    let facts = origin_graph_facts(&graph, Clone::clone);
    let nodes = facts.origin_nodes().collect::<Vec<_>>();
    let links = facts.origin_links().collect::<Vec<_>>();

    assert_eq!(nodes.len(), 3);
    assert!(
        nodes
            .iter()
            .all(|node| node.id().namespace() == FactNamespace::OriginNode)
    );
    assert_eq!(links.len(), 2);
    assert!(links.iter().all(|link| {
        link.from().namespace() == FactNamespace::OriginNode
            && link.to().namespace() == FactNamespace::OriginNode
            && link.kind() == OriginLinkKind::Lowered
    }));

    let hir_id = nodes
        .iter()
        .find(|node| node.key() == &hir)
        .expect("HIR node should be exported")
        .id();
    assert_eq!(
        links
            .iter()
            .filter(|link| link.from() == hir_id)
            .collect::<Vec<&&OriginLinkFact>>()
            .len(),
        2
    );
}

#[test]
fn fallible_origin_graph_export_propagates_key_errors() {
    let hir = origin_key(OriginExportKind::HirExpr, "body:a", "expr:0");
    let stmt = origin_key(OriginExportKind::RuntimeStmt, "runtime:f", "bb0:stmt0");
    let mut graph = OriginGraph::new();
    graph.push(hir.clone(), stmt, OriginLinkKind::Lowered);

    let err = try_origin_graph_facts(&graph, |key| {
        if key.kind() == OriginExportKind::RuntimeStmt {
            Err("missing runtime export owner")
        } else {
            Ok(key.clone())
        }
    })
    .expect_err("fallible origin export should return key errors");

    assert_eq!(err, "missing runtime export owner");
}

#[test]
fn typed_fact_json_roundtrips_stable_origin_and_shape_keys() {
    let semantic = origin_key(OriginExportKind::Semantic, "semantic:a", "expr:0");
    let runtime = origin_key(OriginExportKind::RuntimeStmt, "runtime:a", "block:0:stmt:0");
    let mut origin_graph = OriginGraph::new();
    origin_graph.push(semantic.clone(), runtime.clone(), OriginLinkKind::Lowered);

    let origin_facts = origin_graph_facts(&origin_graph, Clone::clone)
        .with_source_spans([SourceSpanExport::new(
            runtime.clone(),
            SourceSpanKind::Original,
            "file:///roundtrip.fe",
            4,
            8,
            0,
            4,
            0,
            8,
        )])
        .expect("source spans should attach to exported origin facts");
    let origin_export = origin_facts.to_owned_export();
    let origin_json = serde_json::to_string(&origin_export).expect("origin facts serialize");
    assert!(origin_json.contains("\"kind\":\"runtime.stmt\""));
    assert!(origin_json.contains("\"kind\":\"lowered\""));
    assert!(origin_json.contains("\"type\":\"source_span\""));

    let decoded_origin_export = serde_json::from_str::<OwnedTypedFactSetExport>(&origin_json)
        .expect("origin facts deserialize");
    assert_eq!(decoded_origin_export, origin_export);

    let decoded_origin_facts = TypedFactSet::new(decoded_origin_export.facts().to_vec());
    let index =
        OriginFactIndex::new(&decoded_origin_facts).expect("roundtripped facts should index");
    let semantic_id = index
        .origin_id(&semantic)
        .expect("semantic key should roundtrip");
    let runtime_id = index
        .origin_id(&runtime)
        .expect("runtime key should roundtrip");
    assert!(index.has_path(semantic_id, runtime_id));
    let source_spans = index.source_spans_for_key(&runtime).collect::<Vec<_>>();
    assert_eq!(source_spans.len(), 1);
    assert_eq!(source_spans[0].file(), "file:///roundtrip.fe");
    assert_eq!(source_spans[0].start_byte(), 4);

    let mut shape_graph = ShapeGraph::new();
    let expr = shape_graph.add_node("expr:0", "literal");
    shape_graph.add_field(expr, ShapeDimension::Constants, "value", "1");
    let shape_export = shape_graph_facts(&shape_graph).to_owned_export();
    let shape_json = serde_json::to_string(&shape_export).expect("shape facts serialize");
    assert!(shape_json.contains("\"dimension\":\"constants\""));
    assert!(shape_json.contains("\"scope\":\"graph\""));

    let decoded_shape_export = serde_json::from_str::<OwnedTypedFactSetExport>(&shape_json)
        .expect("shape facts deserialize");
    assert_eq!(decoded_shape_export, shape_export);
}

#[test]
#[should_panic(expected = "source span file must not be empty")]
fn source_span_export_rejects_empty_files() {
    SourceSpanExport::new(
        origin_key(
            OriginExportKind::BytecodePc,
            "object:Foo:section:runtime",
            "pc:0..4",
        ),
        SourceSpanKind::Original,
        "",
        0,
        4,
        0,
        0,
        0,
        4,
    );
}

#[test]
#[should_panic(expected = "source span byte range must be ordered")]
fn source_span_export_rejects_inverted_byte_ranges() {
    SourceSpanExport::new(
        origin_key(
            OriginExportKind::BytecodePc,
            "object:Foo:section:runtime",
            "pc:0..4",
        ),
        SourceSpanKind::Original,
        "file:///bad-byte-range.fe",
        4,
        0,
        0,
        0,
        0,
        4,
    );
}

#[test]
#[should_panic(expected = "source span line/column range must be ordered")]
fn source_span_export_rejects_inverted_line_column_ranges() {
    SourceSpanExport::new(
        origin_key(
            OriginExportKind::BytecodePc,
            "object:Foo:section:runtime",
            "pc:0..4",
        ),
        SourceSpanKind::Original,
        "file:///bad-line-range.fe",
        0,
        4,
        1,
        4,
        1,
        0,
    );
}

#[test]
fn source_span_export_json_roundtrips() {
    let span = SourceSpanExport::new(
        origin_key(
            OriginExportKind::BytecodePc,
            "object:Foo:section:runtime",
            "pc:0..4",
        ),
        SourceSpanKind::Original,
        "file:///roundtrip.fe",
        0,
        4,
        1,
        0,
        1,
        4,
    );

    let json = serde_json::to_string(&span).expect("source span should serialize");
    let decoded =
        serde_json::from_str::<SourceSpanExport>(&json).expect("source span should decode");

    assert_eq!(decoded, span);
}

#[test]
fn source_span_export_json_rejects_empty_files() {
    let json = r#"{
        "origin_key": {
            "kind": "bytecode.pc",
            "owner_key": "object:Foo:section:runtime",
            "local_key": "pc:0..4"
        },
        "span_kind": "original",
        "file": "",
        "start_byte": 0,
        "end_byte": 4,
        "start_line": 1,
        "start_col": 0,
        "end_line": 1,
        "end_col": 4
    }"#;
    let err = serde_json::from_str::<SourceSpanExport>(json)
        .expect_err("source span JSON should reject empty files");

    assert!(
        err.to_string()
            .contains("source span file must not be empty"),
        "{err}"
    );
}

#[test]
fn source_span_export_json_rejects_inverted_ranges() {
    let json = r#"{
        "origin_key": {
            "kind": "bytecode.pc",
            "owner_key": "object:Foo:section:runtime",
            "local_key": "pc:0..4"
        },
        "span_kind": "original",
        "file": "file:///bad-range.fe",
        "start_byte": 4,
        "end_byte": 0,
        "start_line": 1,
        "start_col": 0,
        "end_line": 1,
        "end_col": 4
    }"#;
    let err = serde_json::from_str::<SourceSpanExport>(json)
        .expect_err("source span JSON should reject inverted byte ranges");

    assert!(
        err.to_string()
            .contains("source span byte range must be ordered: 4..0"),
        "{err}"
    );
}
