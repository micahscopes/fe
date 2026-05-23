use super::*;

#[test]
fn typed_fact_relation_index_answers_exact_shape_relation_oracle() {
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

    let relation_json = serde_json::to_string(&shape_graph_facts(&shape_graph).relation_export())
        .expect("relations serialize");
    let decoded_relations =
        serde_json::from_str::<TypedFactRelationSet>(&relation_json).expect("relations decode");
    let index = TypedFactRelationIndex::new(&decoded_relations)
        .expect("decoded relations should build a query index");

    let root_rows = index
        .rows_where(
            TypedFactRelationName::ShapeNode,
            TypedFactRelationColumnName::StableKey,
            "root",
        )
        .expect("root rows should query");
    let leaf_rows = index
        .rows_where(
            TypedFactRelationName::ShapeNode,
            TypedFactRelationColumnName::StableKey,
            "leaf",
        )
        .expect("leaf rows should query");
    assert_eq!(root_rows.len(), 1);
    assert_eq!(leaf_rows.len(), 1);
    assert_eq!(root_rows[0].relation(), TypedFactRelationName::ShapeNode);
    assert_eq!(
        root_rows[0]
            .cell(TypedFactRelationColumnName::Kind)
            .expect("root kind"),
        "block"
    );
    let root_id = root_rows[0]
        .cell(TypedFactRelationColumnName::Id)
        .expect("root id");
    let leaf_id = leaf_rows[0]
        .cell(TypedFactRelationColumnName::Id)
        .expect("leaf id");

    let trace_events = index
        .rows_where(
            TypedFactRelationName::TraceEvent,
            TypedFactRelationColumnName::Node,
            root_id,
        )
        .expect("trace events should query by node");
    assert_eq!(trace_events.len(), 1);
    assert_eq!(
        trace_events[0]
            .cell(TypedFactRelationColumnName::EventKind)
            .expect("event kind"),
        "runtime_code_region"
    );
    assert_eq!(
        trace_events[0]
            .cell(TypedFactRelationColumnName::Value)
            .expect("event value"),
        "runtime_code_region_ref"
    );

    let data_flows = index
        .rows_where(
            TypedFactRelationName::DataFlow,
            TypedFactRelationColumnName::Source,
            root_id,
        )
        .expect("data-flow rows should query by source");
    assert_eq!(data_flows.len(), 1);
    assert_eq!(
        data_flows[0]
            .cell(TypedFactRelationColumnName::Target)
            .expect("flow target"),
        leaf_id
    );
    assert_eq!(
        data_flows[0]
            .cell(TypedFactRelationColumnName::Kind)
            .expect("flow kind"),
        "data-flow:value"
    );

    let graph_hashes = index
        .rows_where(
            TypedFactRelationName::ShapeHash,
            TypedFactRelationColumnName::Node,
            "graph",
        )
        .expect("graph hashes should query");
    assert!(graph_hashes.iter().any(|row| {
        row.cell(TypedFactRelationColumnName::Scope)
            .expect("hash scope")
            == "graph"
            && row
                .cell(TypedFactRelationColumnName::Dimension)
                .expect("hash dimension")
                == "structure"
            && row
                .cell(TypedFactRelationColumnName::DigestHex)
                .expect("hash digest")
                .len()
                == 16
    }));
}

#[test]
fn typed_fact_relation_index_rejects_malformed_or_mismatched_queries() {
    let err = TypedFactRelationSet::new(Vec::new())
        .expect_err("publicly constructed incomplete relation sets must fail");
    assert_eq!(
        err,
        TypedFactRelationError::MissingRelation {
            relation: "origin_node".to_string(),
        }
    );

    let semantic = origin_key(OriginExportKind::Semantic, "semantic:a", "expr:0");
    let runtime = origin_key(OriginExportKind::RuntimeStmt, "runtime:a", "block:0:stmt:0");
    let mut origin_graph = OriginGraph::new();
    origin_graph.push(semantic, runtime, OriginLinkKind::Lowered);
    let relations = origin_graph_facts(&origin_graph, Clone::clone).relation_export();
    let index = TypedFactRelationIndex::new(&relations).expect("relations should index");

    let err = index
        .rows_where(
            TypedFactRelationName::OriginNode,
            TypedFactRelationColumnName::File,
            "x",
        )
        .expect_err("column/relation mismatches must fail closed");
    assert_eq!(
        err,
        TypedFactRelationError::UnknownColumn {
            relation: "origin_node".to_string(),
            column: "file".to_string(),
        }
    );
}

#[test]
fn typed_fact_relation_index_rejects_missing_origin_references() {
    let semantic = origin_key(OriginExportKind::Semantic, "semantic:a", "expr:0");
    let runtime = origin_key(OriginExportKind::RuntimeStmt, "runtime:a", "block:0:stmt:0");
    let mut origin_graph = OriginGraph::new();
    origin_graph.push(semantic, runtime, OriginLinkKind::Lowered);
    let mut relation_json =
        serde_json::to_value(origin_graph_facts(&origin_graph, Clone::clone).relation_export())
            .expect("relations serialize to value");
    relation_rows_mut(&mut relation_json, "origin_link")[0]
        .as_array_mut()
        .expect("origin_link row should be an array")[1] =
        serde_json::Value::String("origin_node:99".to_string());
    let decoded_relations = serde_json::from_value::<TypedFactRelationSet>(relation_json)
        .expect("relation schema should still decode");

    let err = TypedFactRelationIndex::new(&decoded_relations)
        .expect_err("query index should reject missing origin endpoints");
    assert_eq!(
        err,
        TypedFactRelationError::MissingRelationReference {
            relation: "origin_link".to_string(),
            column: "to".to_string(),
            value: "origin_node:99".to_string(),
            target_relation: "origin_node".to_string(),
        }
    );
}

#[test]
fn typed_fact_relation_index_rejects_invalid_closed_values() {
    let semantic = origin_key(OriginExportKind::Semantic, "semantic:a", "expr:0");
    let runtime = origin_key(OriginExportKind::RuntimeStmt, "runtime:a", "block:0:stmt:0");
    let mut origin_graph = OriginGraph::new();
    origin_graph.push(semantic, runtime, OriginLinkKind::Lowered);
    let mut relation_json =
        serde_json::to_value(origin_graph_facts(&origin_graph, Clone::clone).relation_export())
            .expect("relations serialize to value");
    relation_rows_mut(&mut relation_json, "origin_link")[0]
        .as_array_mut()
        .expect("origin_link row should be an array")[2] =
        serde_json::Value::String("not-a-link-kind".to_string());
    let decoded_relations = serde_json::from_value::<TypedFactRelationSet>(relation_json)
        .expect("relation schema should still decode");

    let err = TypedFactRelationIndex::new(&decoded_relations)
        .expect_err("query index should reject invalid closed values");
    assert_eq!(
        err,
        TypedFactRelationError::InvalidRelationValue {
            relation: "origin_link".to_string(),
            column: "kind".to_string(),
            value: "not-a-link-kind".to_string(),
        }
    );
}

#[test]
fn typed_fact_relation_index_rejects_missing_shape_references() {
    let mut shape_graph = ShapeGraph::new();
    let root = shape_graph.add_node("root", "block");
    let leaf = shape_graph.add_node("leaf", "literal");
    shape_graph.add_edge(root, leaf, "data-flow:value");
    let mut relation_json = serde_json::to_value(shape_graph_facts(&shape_graph).relation_export())
        .expect("relations serialize to value");
    relation_rows_mut(&mut relation_json, "data_flow")[0]
        .as_array_mut()
        .expect("data_flow row should be an array")[1] =
        serde_json::Value::String("shape_node:99".to_string());
    let decoded_relations = serde_json::from_value::<TypedFactRelationSet>(relation_json)
        .expect("relation schema should still decode");

    let err = TypedFactRelationIndex::new(&decoded_relations)
        .expect_err("query index should reject missing shape endpoints");
    assert_eq!(
        err,
        TypedFactRelationError::MissingRelationReference {
            relation: "data_flow".to_string(),
            column: "target".to_string(),
            value: "shape_node:99".to_string(),
            target_relation: "shape_node".to_string(),
        }
    );
}

#[test]
fn typed_fact_relation_index_rejects_duplicate_origin_export_keys() {
    let semantic = origin_key(OriginExportKind::Semantic, "semantic:a", "expr:0");
    let runtime = origin_key(OriginExportKind::RuntimeStmt, "runtime:a", "block:0:stmt:0");
    let mut origin_graph = OriginGraph::new();
    origin_graph.push(semantic, runtime, OriginLinkKind::Lowered);
    let mut relation_json =
        serde_json::to_value(origin_graph_facts(&origin_graph, Clone::clone).relation_export())
            .expect("relations serialize to value");
    let expected_values = {
        let rows = relation_rows_mut(&mut relation_json, "origin_node");
        let first = rows[0]
            .as_array()
            .expect("origin_node row should be an array")
            .clone();
        let values = (1..=3)
            .map(|idx| {
                first[idx]
                    .as_str()
                    .expect("origin_node key cells should be strings")
                    .to_string()
            })
            .collect::<Vec<_>>();
        let second = rows[1]
            .as_array_mut()
            .expect("origin_node row should be an array");
        for idx in 1..=3 {
            second[idx] = first[idx].clone();
        }
        values
    };
    let decoded_relations = serde_json::from_value::<TypedFactRelationSet>(relation_json)
        .expect("relation schema should still decode");

    let err = TypedFactRelationIndex::new(&decoded_relations)
        .expect_err("query index should reject duplicate origin export keys");
    assert_eq!(
        err,
        TypedFactRelationError::DuplicateRelationKey {
            relation: "origin_node".to_string(),
            columns: vec![
                "kind".to_string(),
                "owner_key".to_string(),
                "local_key".to_string()
            ],
            values: expected_values,
        }
    );
}

#[test]
fn typed_fact_relation_index_rejects_duplicate_origin_links() {
    let semantic = origin_key(OriginExportKind::Semantic, "semantic:a", "expr:0");
    let runtime = origin_key(OriginExportKind::RuntimeStmt, "runtime:a", "block:0:stmt:0");
    let mut origin_graph = OriginGraph::new();
    origin_graph.push(semantic, runtime, OriginLinkKind::Lowered);
    let mut relation_json =
        serde_json::to_value(origin_graph_facts(&origin_graph, Clone::clone).relation_export())
            .expect("relations serialize to value");
    let expected_values = {
        let rows = relation_rows_mut(&mut relation_json, "origin_link");
        let duplicate = rows[0]
            .as_array()
            .expect("origin_link row should be an array")
            .clone();
        let values = duplicate
            .iter()
            .map(|cell| {
                cell.as_str()
                    .expect("origin_link cells should be strings")
                    .to_string()
            })
            .collect::<Vec<_>>();
        rows.push(serde_json::Value::Array(duplicate));
        values
    };
    let decoded_relations = serde_json::from_value::<TypedFactRelationSet>(relation_json)
        .expect("relation schema should still decode");

    let err = TypedFactRelationIndex::new(&decoded_relations)
        .expect_err("query index should reject duplicate origin links");
    assert_eq!(
        err,
        TypedFactRelationError::DuplicateRelationKey {
            relation: "origin_link".to_string(),
            columns: vec!["from".to_string(), "to".to_string(), "kind".to_string()],
            values: expected_values,
        }
    );
}

#[test]
fn typed_fact_relation_index_rejects_empty_origin_key_parts() {
    let semantic = origin_key(OriginExportKind::Semantic, "semantic:a", "expr:0");
    let runtime = origin_key(OriginExportKind::RuntimeStmt, "runtime:a", "block:0:stmt:0");
    let mut origin_graph = OriginGraph::new();
    origin_graph.push(semantic, runtime, OriginLinkKind::Lowered);
    let mut relation_json =
        serde_json::to_value(origin_graph_facts(&origin_graph, Clone::clone).relation_export())
            .expect("relations serialize to value");
    relation_rows_mut(&mut relation_json, "origin_node")[0]
        .as_array_mut()
        .expect("origin_node row should be an array")[2] = serde_json::Value::String(String::new());
    let decoded_relations = serde_json::from_value::<TypedFactRelationSet>(relation_json)
        .expect("relation schema should still decode");

    let err = TypedFactRelationIndex::new(&decoded_relations)
        .expect_err("query index should reject empty origin owner keys");
    assert_eq!(
        err,
        TypedFactRelationError::InvalidRelationValue {
            relation: "origin_node".to_string(),
            column: "owner_key".to_string(),
            value: String::new(),
        }
    );
}

#[test]
fn typed_fact_relation_index_rejects_reserved_origin_key_separators() {
    let semantic = origin_key(OriginExportKind::Semantic, "semantic:a", "expr:0");
    let runtime = origin_key(OriginExportKind::RuntimeStmt, "runtime:a", "block:0:stmt:0");
    let mut origin_graph = OriginGraph::new();
    origin_graph.push(semantic, runtime, OriginLinkKind::Lowered);
    let mut relation_json =
        serde_json::to_value(origin_graph_facts(&origin_graph, Clone::clone).relation_export())
            .expect("relations serialize to value");
    relation_rows_mut(&mut relation_json, "origin_node")[0]
        .as_array_mut()
        .expect("origin_node row should be an array")[3] =
        serde_json::Value::String("expr\u{1f}0".to_string());
    let decoded_relations = serde_json::from_value::<TypedFactRelationSet>(relation_json)
        .expect("relation schema should still decode");

    let err = TypedFactRelationIndex::new(&decoded_relations)
        .expect_err("query index should reject reserved origin key separators");
    assert_eq!(
        err,
        TypedFactRelationError::InvalidRelationValue {
            relation: "origin_node".to_string(),
            column: "local_key".to_string(),
            value: "expr\u{1f}0".to_string(),
        }
    );
}

#[test]
fn typed_fact_relation_index_rejects_wrong_relation_id_namespace() {
    let semantic = origin_key(OriginExportKind::Semantic, "semantic:a", "expr:0");
    let runtime = origin_key(OriginExportKind::RuntimeStmt, "runtime:a", "block:0:stmt:0");
    let mut origin_graph = OriginGraph::new();
    origin_graph.push(semantic, runtime, OriginLinkKind::Lowered);
    let mut relation_json =
        serde_json::to_value(origin_graph_facts(&origin_graph, Clone::clone).relation_export())
            .expect("relations serialize to value");
    relation_rows_mut(&mut relation_json, "origin_node")[0]
        .as_array_mut()
        .expect("origin_node row should be an array")[0] =
        serde_json::Value::String("shape_node:0".to_string());
    let decoded_relations = serde_json::from_value::<TypedFactRelationSet>(relation_json)
        .expect("relation schema should still decode");

    let err = TypedFactRelationIndex::new(&decoded_relations)
        .expect_err("query index should reject wrong id namespace");
    assert_eq!(
        err,
        TypedFactRelationError::InvalidRelationValue {
            relation: "origin_node".to_string(),
            column: "id".to_string(),
            value: "shape_node:0".to_string(),
        }
    );
}

#[test]
fn typed_fact_relation_index_rejects_inverted_source_span_ranges() {
    let semantic = origin_key(OriginExportKind::Semantic, "semantic:a", "expr:0");
    let runtime = origin_key(OriginExportKind::RuntimeStmt, "runtime:a", "block:0:stmt:0");
    let mut origin_graph = OriginGraph::new();
    origin_graph.push(semantic, runtime.clone(), OriginLinkKind::Lowered);
    let origin_facts = origin_graph_facts(&origin_graph, Clone::clone)
        .with_source_spans([SourceSpanExport::new(
            runtime,
            SourceSpanKind::Original,
            "file:///bad-span.fe",
            4,
            8,
            0,
            4,
            0,
            8,
        )])
        .expect("source spans should attach to exported origin facts");
    let mut relation_json =
        serde_json::to_value(origin_facts.relation_export()).expect("relations serialize");
    let expected_origin = {
        let rows = relation_rows_mut(&mut relation_json, "source_span");
        let row = rows[0]
            .as_array_mut()
            .expect("source_span row should be an array");
        let origin = row[0]
            .as_str()
            .expect("source_span origin should be a string")
            .to_string();
        row[3] = serde_json::Value::String("9".to_string());
        row[4] = serde_json::Value::String("4".to_string());
        origin
    };
    let decoded_relations = serde_json::from_value::<TypedFactRelationSet>(relation_json)
        .expect("relation schema should still decode");

    let err = TypedFactRelationIndex::new(&decoded_relations)
        .expect_err("query index should reject inverted byte ranges");
    assert_eq!(
        err,
        TypedFactRelationError::InvalidSourceSpanRange {
            origin: expected_origin,
            start_byte: 9,
            end_byte: 4,
        }
    );
}

#[test]
fn typed_fact_relation_index_rejects_empty_source_span_files() {
    let semantic = origin_key(OriginExportKind::Semantic, "semantic:a", "expr:0");
    let runtime = origin_key(OriginExportKind::RuntimeStmt, "runtime:a", "block:0:stmt:0");
    let mut origin_graph = OriginGraph::new();
    origin_graph.push(semantic, runtime.clone(), OriginLinkKind::Lowered);
    let origin_facts = origin_graph_facts(&origin_graph, Clone::clone)
        .with_source_spans([SourceSpanExport::new(
            runtime,
            SourceSpanKind::Original,
            "file:///empty-file.fe",
            4,
            8,
            0,
            4,
            0,
            8,
        )])
        .expect("source spans should attach to exported origin facts");
    let mut relation_json =
        serde_json::to_value(origin_facts.relation_export()).expect("relations serialize");
    let expected_origin = {
        let rows = relation_rows_mut(&mut relation_json, "source_span");
        let row = rows[0]
            .as_array_mut()
            .expect("source_span row should be an array");
        let origin = row[0]
            .as_str()
            .expect("source_span origin should be a string")
            .to_string();
        row[2] = serde_json::Value::String(String::new());
        origin
    };
    let decoded_relations = serde_json::from_value::<TypedFactRelationSet>(relation_json)
        .expect("relation schema should still decode");

    let err = TypedFactRelationIndex::new(&decoded_relations)
        .expect_err("query index should reject empty source-span files");
    assert_eq!(
        err,
        TypedFactRelationError::InvalidSourceSpanFile {
            origin: expected_origin,
        }
    );
}

#[test]
fn typed_fact_relation_index_rejects_non_numeric_relation_cells() {
    let mut shape_graph = ShapeGraph::new();
    let root = shape_graph.add_node("root", "block");
    let leaf = shape_graph.add_node("leaf", "literal");
    shape_graph.add_child(root, "expr", leaf);
    let mut relation_json = serde_json::to_value(shape_graph_facts(&shape_graph).relation_export())
        .expect("relations serialize to value");
    relation_rows_mut(&mut relation_json, "shape_child")[0]
        .as_array_mut()
        .expect("shape_child row should be an array")[3] =
        serde_json::Value::String("not-an-order".to_string());
    let decoded_relations = serde_json::from_value::<TypedFactRelationSet>(relation_json)
        .expect("relation schema should still decode");

    let err = TypedFactRelationIndex::new(&decoded_relations)
        .expect_err("query index should reject non-numeric relation cells");
    assert_eq!(
        err,
        TypedFactRelationError::InvalidRelationValue {
            relation: "shape_child".to_string(),
            column: "order".to_string(),
            value: "not-an-order".to_string(),
        }
    );
}

#[test]
fn typed_fact_relation_index_rejects_empty_shape_identity_cells() {
    let mut shape_graph = ShapeGraph::new();
    shape_graph.add_node("root", "block");
    let mut relation_json = serde_json::to_value(shape_graph_facts(&shape_graph).relation_export())
        .expect("relations serialize to value");
    relation_rows_mut(&mut relation_json, "shape_node")[0]
        .as_array_mut()
        .expect("shape_node row should be an array")[2] = serde_json::Value::String(String::new());
    let decoded_relations = serde_json::from_value::<TypedFactRelationSet>(relation_json)
        .expect("relation schema should still decode");

    let err = TypedFactRelationIndex::new(&decoded_relations)
        .expect_err("query index should reject empty shape identity cells");
    assert_eq!(
        err,
        TypedFactRelationError::InvalidRelationValue {
            relation: "shape_node".to_string(),
            column: "stable_key".to_string(),
            value: String::new(),
        }
    );
}

#[test]
fn typed_fact_relation_index_rejects_empty_shape_label_cells() {
    let mut shape_graph = ShapeGraph::new();
    let root = shape_graph.add_node("root", "block");
    let leaf = shape_graph.add_node("leaf", "literal");
    shape_graph.add_child(root, "expr", leaf);
    let mut relation_json = serde_json::to_value(shape_graph_facts(&shape_graph).relation_export())
        .expect("relations serialize to value");
    relation_rows_mut(&mut relation_json, "shape_child")[0]
        .as_array_mut()
        .expect("shape_child row should be an array")[2] = serde_json::Value::String(String::new());
    let decoded_relations = serde_json::from_value::<TypedFactRelationSet>(relation_json)
        .expect("relation schema should still decode");

    let err = TypedFactRelationIndex::new(&decoded_relations)
        .expect_err("query index should reject empty shape label cells");
    assert_eq!(
        err,
        TypedFactRelationError::InvalidRelationValue {
            relation: "shape_child".to_string(),
            column: "label".to_string(),
            value: String::new(),
        }
    );
}

#[test]
fn typed_fact_relation_index_rejects_duplicate_shape_stable_keys() {
    let mut shape_graph = ShapeGraph::new();
    shape_graph.add_node("root", "block");
    shape_graph.add_node("leaf", "literal");
    let mut relation_json = serde_json::to_value(shape_graph_facts(&shape_graph).relation_export())
        .expect("relations serialize to value");
    let expected_value = {
        let rows = relation_rows_mut(&mut relation_json, "shape_node");
        let first_stable_key = relation_cell(&rows[0], 2);
        rows[1]
            .as_array_mut()
            .expect("shape_node row should be an array")[2] =
            serde_json::Value::String(first_stable_key.clone());
        first_stable_key
    };
    let decoded_relations = serde_json::from_value::<TypedFactRelationSet>(relation_json)
        .expect("relation schema should still decode");

    let err = TypedFactRelationIndex::new(&decoded_relations)
        .expect_err("query index should reject duplicate shape stable keys");
    assert_eq!(
        err,
        TypedFactRelationError::DuplicateRelationKey {
            relation: "shape_node".to_string(),
            columns: vec!["stable_key".to_string()],
            values: vec![expected_value],
        }
    );
}

#[test]
fn typed_fact_relation_index_rejects_duplicate_shape_hash_keys() {
    let mut shape_graph = ShapeGraph::new();
    shape_graph.add_node("root", "block");
    let mut relation_json = serde_json::to_value(shape_graph_facts(&shape_graph).relation_export())
        .expect("relations serialize to value");
    let (expected_node, expected_scope, expected_dimension) = {
        let rows = relation_rows_mut(&mut relation_json, "shape_hash");
        let duplicate = rows[0].clone();
        let expected = (
            relation_cell(&duplicate, 0),
            relation_cell(&duplicate, 1),
            relation_cell(&duplicate, 2),
        );
        rows.push(duplicate);
        expected
    };
    let decoded_relations = serde_json::from_value::<TypedFactRelationSet>(relation_json)
        .expect("relation schema should still decode");

    let err = TypedFactRelationIndex::new(&decoded_relations)
        .expect_err("query index should reject duplicate shape hash keys");
    assert_eq!(
        err,
        TypedFactRelationError::DuplicateShapeHash {
            node: expected_node,
            scope: expected_scope,
            dimension: expected_dimension,
        }
    );
}

#[test]
fn typed_fact_relation_index_rejects_incomplete_shape_hash_sets() {
    let mut shape_graph = ShapeGraph::new();
    shape_graph.add_node("root", "block");
    let mut relation_json = serde_json::to_value(shape_graph_facts(&shape_graph).relation_export())
        .expect("relations serialize to value");
    {
        let rows = relation_rows_mut(&mut relation_json, "shape_hash");
        let graph_structure = rows
            .iter()
            .position(|row| {
                relation_cell(row, 0) == "graph"
                    && relation_cell(row, 1) == "graph"
                    && relation_cell(row, 2) == "structure"
            })
            .expect("graph structure hash should exist");
        rows.remove(graph_structure);
    }
    let decoded_relations = serde_json::from_value::<TypedFactRelationSet>(relation_json)
        .expect("relation schema should still decode");

    let err = TypedFactRelationIndex::new(&decoded_relations)
        .expect_err("query index should reject incomplete shape hash coverage");
    assert_eq!(
        err,
        TypedFactRelationError::MissingShapeHash {
            node: "graph".to_string(),
            scope: "graph".to_string(),
            dimension: "structure".to_string(),
        }
    );
}
