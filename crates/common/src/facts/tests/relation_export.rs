use super::*;

#[test]
fn typed_fact_relation_export_has_engine_agnostic_tables() {
    let semantic = origin_key(OriginExportKind::Semantic, "semantic:a", "expr:0");
    let runtime = origin_key(OriginExportKind::RuntimeStmt, "runtime:a", "block:0:stmt:0");
    let mut origin_graph = OriginGraph::new();
    origin_graph.push(semantic.clone(), runtime.clone(), OriginLinkKind::Lowered);
    let origin_facts = origin_graph_facts(&origin_graph, Clone::clone)
        .with_source_spans([SourceSpanExport::new(
            runtime,
            SourceSpanKind::Original,
            "file:///relations.fe",
            4,
            8,
            0,
            4,
            0,
            8,
        )])
        .expect("source spans should attach to exported origin facts");

    let origin_relations = origin_facts.relation_export();
    assert_eq!(
        origin_relations.schema_version(),
        OwnedTypedFactSetExport::SCHEMA_VERSION
    );
    assert_eq!(
        origin_relations
            .relation(TypedFactRelationName::OriginNode)
            .expect("origin_node relation")
            .columns()
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        vec!["id", "kind", "owner_key", "local_key"]
    );
    assert_eq!(
        origin_relations
            .relation(TypedFactRelationName::OriginLink)
            .expect("origin_link relation")
            .row_count(),
        1
    );
    assert!(
        origin_relations
            .relation(TypedFactRelationName::SourceSpan)
            .expect("source_span relation")
            .rows()
            .iter()
            .any(|row| row[1] == "original" && row[2] == "file:///relations.fe")
    );

    let mut shape_graph = ShapeGraph::new();
    let root = shape_graph.add_node("root", "block");
    let leaf = shape_graph.add_node("leaf", "literal");
    shape_graph.add_field(
        root,
        ShapeDimension::TraceEvents,
        "runtime_code_region",
        "runtime_code_region_ref",
    );
    shape_graph.add_child(root, "expr", leaf);
    shape_graph.add_edge(root, leaf, "data-flow:value");

    let shape_relations = shape_graph_facts(&shape_graph).relation_export();
    assert!(
        shape_relations
            .relation(TypedFactRelationName::TraceEvent)
            .expect("trace_event relation")
            .rows()
            .iter()
            .any(|row| row[1] == "runtime_code_region" && row[2] == "runtime_code_region_ref")
    );
    assert!(
        shape_relations
            .relation(TypedFactRelationName::DataFlow)
            .expect("data_flow relation")
            .rows()
            .iter()
            .any(|row| row[2] == "data-flow:value")
    );
    assert!(
        shape_relations
            .relation(TypedFactRelationName::ShapeHash)
            .expect("shape_hash relation")
            .rows()
            .iter()
            .any(|row| row[0] == "graph" && row[1] == "graph")
    );
}

#[test]
fn typed_fact_relation_schema_descriptor_exposes_wire_names_and_columns() {
    let schema = TypedFactRelationName::OriginNode.schema();

    assert_eq!(schema.name(), TypedFactRelationName::OriginNode);
    assert!(TypedFactRelationName::OriginNode.is_origin_relation());
    assert!(TypedFactRelationName::OriginLink.is_origin_relation());
    assert!(TypedFactRelationName::SourceSpan.is_origin_relation());
    assert!(!TypedFactRelationName::ShapeNode.is_origin_relation());
    assert!(TypedFactRelationName::ShapeNode.is_shape_relation());
    assert!(TypedFactRelationName::ShapeHash.is_shape_relation());
    assert!(TypedFactRelationName::DataFlow.is_shape_relation());
    assert!(!TypedFactRelationName::OriginNode.is_shape_relation());
    assert_eq!(
        schema.column_names().collect::<Vec<_>>(),
        vec!["id", "kind", "owner_key", "local_key"]
    );
    assert_eq!(
        TypedFactRelationName::OriginLink
            .column_index(TypedFactRelationColumnName::To)
            .expect("known column should have an index"),
        1
    );
    assert_eq!(
        TypedFactRelationName::OriginLink
            .column_index(TypedFactRelationColumnName::File)
            .expect_err("mismatched column should fail closed"),
        TypedFactRelationError::UnknownColumn {
            relation: "origin_link".to_string(),
            column: "file".to_string(),
        }
    );
    assert!(
        super::typed_fact_relation_schemas()
            .iter()
            .any(|schema| schema.name() == TypedFactRelationName::ShapeHash)
    );

    let relation = super::TypedFactRelation::new(TypedFactRelationName::OriginLink, Vec::new())
        .expect("declared relation should construct from schema");
    assert_eq!(relation.relation_name(), TypedFactRelationName::OriginLink);
    assert_eq!(relation.name(), "origin_link");
    assert_eq!(
        relation.typed_columns(),
        &[
            TypedFactRelationColumnName::From,
            TypedFactRelationColumnName::To,
            TypedFactRelationColumnName::Kind,
        ]
    );
    assert_eq!(
        relation
            .columns()
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        vec!["from", "to", "kind"]
    );
}

#[test]
fn typed_fact_relation_export_columns_follow_declared_schema() {
    let relations = TypedFactSet::new(Vec::new()).relation_export();
    assert_eq!(
        relations.relations().len(),
        super::typed_fact_relation_schemas().len()
    );

    for schema in super::typed_fact_relation_schemas() {
        let relation = relations
            .relation(schema.name())
            .expect("declared relation should be exported");
        assert_eq!(
            relation
                .columns()
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            schema.column_names().collect::<Vec<_>>()
        );
    }
}

#[test]
fn typed_fact_relation_export_is_deterministic_for_fact_order() {
    let semantic = origin_key(OriginExportKind::Semantic, "semantic:a", "expr:0");
    let runtime = origin_key(OriginExportKind::RuntimeStmt, "runtime:a", "block:0:stmt:0");
    let pc = origin_key(
        OriginExportKind::BytecodePc,
        "object:a:section:runtime",
        "pc:0..4",
    );
    let mut origin_graph = OriginGraph::new();
    origin_graph.push(runtime.clone(), pc, OriginLinkKind::Lowered);
    origin_graph.push(semantic, runtime.clone(), OriginLinkKind::Lowered);
    let origin_facts = origin_graph_facts(&origin_graph, Clone::clone)
        .with_source_spans([SourceSpanExport::new(
            runtime,
            SourceSpanKind::Original,
            "file:///deterministic.fe",
            4,
            8,
            0,
            4,
            0,
            8,
        )])
        .expect("source spans should attach to exported origin facts");
    let mut reversed_origin_facts = origin_facts.clone().into_facts();
    reversed_origin_facts.reverse();

    assert_eq!(
        origin_facts.relation_export(),
        TypedFactSet::new(reversed_origin_facts).relation_export()
    );

    let mut shape_graph = ShapeGraph::new();
    let root = shape_graph.add_node("root", "block");
    let first = shape_graph.add_node("first", "literal");
    let second = shape_graph.add_node("second", "literal");
    shape_graph.add_field(
        root,
        ShapeDimension::TraceEvents,
        "runtime_code_region",
        "region",
    );
    shape_graph.add_field(first, ShapeDimension::Constants, "value", "1");
    shape_graph.add_field(second, ShapeDimension::Constants, "value", "2");
    shape_graph.add_child(root, "right", second);
    shape_graph.add_child(root, "left", first);
    shape_graph.add_edge(root, second, "data-flow:right");
    shape_graph.add_edge(root, first, "data-flow:left");
    let shape_facts = shape_graph_facts(&shape_graph);
    let mut reversed_shape_facts = shape_facts.clone().into_facts();
    reversed_shape_facts.reverse();

    assert_eq!(
        shape_facts.relation_export(),
        TypedFactSet::new(reversed_shape_facts).relation_export()
    );
}

#[test]
fn typed_fact_relation_export_roundtrips_schema() {
    let semantic = origin_key(OriginExportKind::Semantic, "semantic:a", "expr:0");
    let runtime = origin_key(OriginExportKind::RuntimeStmt, "runtime:a", "block:0:stmt:0");
    let mut origin_graph = OriginGraph::new();
    origin_graph.push(semantic, runtime, OriginLinkKind::Lowered);
    let relations = origin_graph_facts(&origin_graph, Clone::clone).relation_export();
    let json = serde_json::to_string(&relations).expect("relations serialize");
    let decoded =
        serde_json::from_str::<TypedFactRelationSet>(&json).expect("relations deserialize");

    assert_eq!(decoded, relations);
    assert_eq!(
        decoded
            .relation(TypedFactRelationName::OriginLink)
            .expect("origin_link relation")
            .row_count(),
        1
    );
}

#[test]
fn typed_fact_relation_index_answers_exact_origin_join_oracle() {
    let semantic = origin_key(OriginExportKind::Semantic, "semantic:a", "expr:0");
    let runtime = origin_key(OriginExportKind::RuntimeStmt, "runtime:a", "block:0:stmt:0");
    let mut origin_graph = OriginGraph::new();
    origin_graph.push(semantic, runtime.clone(), OriginLinkKind::Lowered);
    let origin_facts = origin_graph_facts(&origin_graph, Clone::clone)
        .with_source_spans([SourceSpanExport::new(
            runtime,
            SourceSpanKind::Original,
            "file:///relation_index.fe",
            4,
            8,
            0,
            4,
            0,
            8,
        )])
        .expect("source spans should attach to exported origin facts");
    let relation_json =
        serde_json::to_string(&origin_facts.relation_export()).expect("relations serialize");
    let decoded_relations =
        serde_json::from_str::<TypedFactRelationSet>(&relation_json).expect("relations decode");
    let index = TypedFactRelationIndex::new(&decoded_relations)
        .expect("decoded relations should build a query index");

    assert_eq!(
        index
            .row_count(TypedFactRelationName::OriginLink)
            .expect("origin links"),
        1
    );
    let semantic_rows = index
        .rows_where(
            TypedFactRelationName::OriginNode,
            TypedFactRelationColumnName::Kind,
            "semantic",
        )
        .expect("semantic rows should query");
    let runtime_rows = index
        .rows_where(
            TypedFactRelationName::OriginNode,
            TypedFactRelationColumnName::Kind,
            "runtime.stmt",
        )
        .expect("runtime rows should query");
    assert_eq!(semantic_rows.len(), 1);
    assert_eq!(runtime_rows.len(), 1);
    assert_eq!(
        semantic_rows[0].relation(),
        TypedFactRelationName::OriginNode
    );
    assert_eq!(semantic_rows[0].relation_name(), "origin_node");
    let semantic_id = semantic_rows[0]
        .cell(TypedFactRelationColumnName::Id)
        .expect("semantic id");
    let runtime_id = runtime_rows[0]
        .cell(TypedFactRelationColumnName::Id)
        .expect("runtime id");

    let lowered_edges = index
        .rows_where(
            TypedFactRelationName::OriginLink,
            TypedFactRelationColumnName::Kind,
            "lowered",
        )
        .expect("lowered edges should query");
    assert_eq!(lowered_edges.len(), 1);
    assert_eq!(
        lowered_edges[0]
            .cell(TypedFactRelationColumnName::From)
            .expect("edge from"),
        semantic_id
    );
    assert_eq!(
        lowered_edges[0]
            .cell(TypedFactRelationColumnName::To)
            .expect("edge to"),
        runtime_id
    );

    let source_spans = index
        .rows_where(
            TypedFactRelationName::SourceSpan,
            TypedFactRelationColumnName::Origin,
            runtime_id,
        )
        .expect("source spans should query by origin");
    assert_eq!(source_spans.len(), 1);
    assert_eq!(
        source_spans[0]
            .cell(TypedFactRelationColumnName::File)
            .expect("span file"),
        "file:///relation_index.fe"
    );
    assert_eq!(
        source_spans[0]
            .cell(TypedFactRelationColumnName::SpanKind)
            .expect("span kind"),
        "original"
    );
}

#[test]
fn typed_fact_relation_index_counts_source_span_files() {
    let semantic = origin_key(OriginExportKind::Semantic, "semantic:a", "expr:0");
    let first_runtime = origin_key(OriginExportKind::RuntimeStmt, "runtime:a", "block:0:stmt:0");
    let second_runtime = origin_key(OriginExportKind::RuntimeStmt, "runtime:a", "block:0:stmt:1");
    let mut origin_graph = OriginGraph::new();
    origin_graph.push(
        semantic.clone(),
        first_runtime.clone(),
        OriginLinkKind::Lowered,
    );
    origin_graph.push(
        semantic.clone(),
        second_runtime.clone(),
        OriginLinkKind::Lowered,
    );
    let origin_facts = origin_graph_facts(&origin_graph, Clone::clone)
        .with_source_spans([
            SourceSpanExport::new(
                first_runtime,
                SourceSpanKind::Original,
                "file:///b.fe",
                0,
                1,
                0,
                0,
                0,
                1,
            ),
            SourceSpanExport::new(
                second_runtime.clone(),
                SourceSpanKind::Original,
                "file:///a.fe",
                2,
                3,
                0,
                2,
                0,
                3,
            ),
            SourceSpanExport::new(
                second_runtime.clone(),
                SourceSpanKind::Original,
                "file:///a.fe",
                4,
                5,
                0,
                4,
                0,
                5,
            ),
        ])
        .expect("source spans should attach to exported origin facts");
    let relation_json =
        serde_json::to_string(&origin_facts.relation_export()).expect("relations serialize");
    let decoded_relations =
        serde_json::from_str::<TypedFactRelationSet>(&relation_json).expect("relations decode");
    let index = TypedFactRelationIndex::new(&decoded_relations)
        .expect("decoded relations should build a query index");
    let fact_index = OriginFactIndex::new(&origin_facts).expect("origin facts should index");

    assert_eq!(
        index
            .source_span_file_counts()
            .expect("source span file counts should query"),
        vec![
            SourceSpanFileCount::new("file:///a.fe", 2),
            SourceSpanFileCount::new("file:///b.fe", 1),
        ]
    );
    assert_eq!(
        index
            .relation_counts()
            .expect("relation counts should query"),
        vec![
            TypedFactRelationCount::new(TypedFactRelationName::OriginNode, 3),
            TypedFactRelationCount::new(TypedFactRelationName::OriginLink, 2),
            TypedFactRelationCount::new(TypedFactRelationName::SourceSpan, 3),
        ]
    );
    assert_eq!(
        index
            .origin_reachability_summary()
            .expect("relation reachability should query"),
        fact_index.reachability_summary()
    );
    assert_eq!(
        index
            .origin_reachability_summary()
            .expect("relation reachability should query")
            .pair_count(OriginExportKind::Semantic, OriginExportKind::RuntimeStmt),
        2
    );
    assert_eq!(
        index
            .representative_path_exports_with_priority(
                [(OriginExportKind::Semantic, OriginExportKind::RuntimeStmt)],
                4,
            )
            .expect("relation path witnesses should query"),
        fact_index.representative_path_exports_with_priority(
            [(OriginExportKind::Semantic, OriginExportKind::RuntimeStmt)],
            4,
        )
    );
    assert_eq!(
        index
            .path_export_between_keys(&semantic, &second_runtime)
            .expect("relation stable-key path should query"),
        fact_index.path_export_between_keys(&semantic, &second_runtime)
    );

    let source_paths = index
        .representative_source_path_exports_with_priority(
            [(OriginExportKind::Semantic, OriginExportKind::RuntimeStmt)],
            4,
        )
        .expect("relation source path witnesses should query");
    assert_eq!(source_paths.len(), 1);
    let source_path = &source_paths[0];
    assert_eq!(source_path.path().from_kind(), OriginExportKind::Semantic);
    assert_eq!(source_path.path().to_kind(), OriginExportKind::RuntimeStmt);
    assert_eq!(
        source_path.source_span().origin_key().kind(),
        OriginExportKind::RuntimeStmt
    );
    assert!(source_path.source_span().file().starts_with("file:///"));
    assert_eq!(
        Some(source_path.path()),
        fact_index
            .path_export_between_keys(&semantic, source_path.source_span().origin_key())
            .as_ref()
    );
    assert!(
        index
            .representative_source_path_exports_with_priority(
                [(OriginExportKind::Semantic, OriginExportKind::RuntimeStmt)],
                0,
            )
            .expect("zero limit source path witness query should succeed")
            .is_empty()
    );
}
