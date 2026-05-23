use super::*;

#[test]
fn origin_reachability_summary_roundtrips_through_fail_closed_schema() {
    let semantic_to_runtime = OriginReachableKindPairSummary::new(
        OriginExportKind::Semantic,
        OriginExportKind::RuntimeStmt,
        2,
    );
    let runtime_to_pc = OriginReachableKindPairSummary::new(
        OriginExportKind::RuntimeStmt,
        OriginExportKind::BytecodePc,
        3,
    );
    let summary =
        OriginReachabilitySummary::new(5, vec![semantic_to_runtime.clone(), runtime_to_pc.clone()]);
    let json = serde_json::to_string(&summary).expect("reachability summary should serialize");
    let decoded = serde_json::from_str::<OriginReachabilitySummary>(&json)
        .expect("reachability summary should decode");
    assert_eq!(decoded, summary);
    assert_eq!(
        OriginReachabilitySummary::default().reachable_pairs(),
        0,
        "empty reachability is valid for fact sets without reachable pairs"
    );

    assert_eq!(
        OriginReachableKindPairSummary::try_new(
            OriginExportKind::Semantic,
            OriginExportKind::RuntimeStmt,
            0,
        ),
        Err(OriginReachabilitySummaryError::ZeroReachablePairsForKind {
            from_kind: OriginExportKind::Semantic,
            to_kind: OriginExportKind::RuntimeStmt,
        })
    );
    assert_eq!(
        OriginReachabilitySummary::try_new(
            6,
            vec![semantic_to_runtime.clone(), runtime_to_pc.clone()],
        ),
        Err(OriginReachabilitySummaryError::ReachablePairTotalMismatch {
            declared: 6,
            actual: 5,
        })
    );
    assert_eq!(
        OriginReachabilitySummary::try_new(
            4,
            vec![semantic_to_runtime.clone(), semantic_to_runtime.clone()],
        ),
        Err(OriginReachabilitySummaryError::DuplicateKindPair {
            from_kind: OriginExportKind::Semantic,
            to_kind: OriginExportKind::RuntimeStmt,
        })
    );

    let zero_pair = r#"{
        "from_kind": "semantic",
        "to_kind": "runtime.stmt",
        "reachable_pairs": 0
    }"#;
    let err = serde_json::from_str::<OriginReachableKindPairSummary>(zero_pair)
        .expect_err("reachable kind-pair JSON should reject zero counts");
    assert!(
        err.to_string()
            .contains("must have at least one reachable pair"),
        "{err}"
    );

    let mismatched_total = r#"{
        "reachable_pairs": 6,
        "reachable_pairs_by_kind": [{
            "from_kind": "semantic",
            "to_kind": "runtime.stmt",
            "reachable_pairs": 2
        }, {
            "from_kind": "runtime.stmt",
            "to_kind": "bytecode.pc",
            "reachable_pairs": 3
        }]
    }"#;
    let err = serde_json::from_str::<OriginReachabilitySummary>(mismatched_total)
        .expect_err("reachability summary JSON should reject mismatched totals");
    assert!(
        err.to_string()
            .contains("total 6 does not match per-kind sum 5"),
        "{err}"
    );

    let duplicate_pair = r#"{
        "reachable_pairs": 4,
        "reachable_pairs_by_kind": [{
            "from_kind": "semantic",
            "to_kind": "runtime.stmt",
            "reachable_pairs": 2
        }, {
            "from_kind": "semantic",
            "to_kind": "runtime.stmt",
            "reachable_pairs": 2
        }]
    }"#;
    let err = serde_json::from_str::<OriginReachabilitySummary>(duplicate_pair)
        .expect_err("reachability summary JSON should reject duplicate kind pairs");
    assert!(
        err.to_string()
            .contains("duplicate reachable origin kind pair semantic -> runtime.stmt"),
        "{err}"
    );

    let unknown_field = r#"{
        "reachable_pairs": 0,
        "reachable_pairs_by_kind": [],
        "unexpected": true
    }"#;
    let err = serde_json::from_str::<OriginReachabilitySummary>(unknown_field)
        .expect_err("reachability summary JSON should reject unknown fields");
    assert!(err.to_string().contains("unknown field"), "{err}");
}

#[test]
fn origin_path_witnesses_roundtrip_through_fail_closed_schema() {
    let origin = FactId::new(FactNamespace::OriginNode, 0);
    let runtime = FactId::new(FactNamespace::OriginNode, 1);
    let shape = FactId::new(FactNamespace::ShapeNode, 0);
    let path = OriginPath::new(vec![origin, runtime], vec![OriginLinkKind::Lowered]);
    let json = serde_json::to_string(&path).expect("origin path should serialize");
    let decoded = serde_json::from_str::<OriginPath>(&json).expect("origin path should decode");
    assert_eq!(decoded, path);

    assert_eq!(
        OriginPath::try_new(vec![], vec![]),
        Err(OriginPathError::EmptyPath)
    );
    assert_eq!(
        OriginPath::try_new(vec![origin], vec![OriginLinkKind::Lowered]),
        Err(OriginPathError::LengthMismatch { nodes: 1, links: 1 })
    );
    assert_eq!(
        OriginPath::try_new(vec![shape], vec![]),
        Err(OriginPathError::WrongNamespace(
            FactNamespaceError::WrongNamespace {
                id: shape,
                expected: FactNamespace::OriginNode
            }
        ))
    );

    let err = serde_json::from_str::<OriginPath>(
        r#"{
            "nodes": [{"namespace": "shape_node", "ordinal": 0}],
            "links": []
        }"#,
    )
    .expect_err("path JSON should reject non-origin node namespaces");
    assert!(err.to_string().contains("expected origin_node"), "{err}");

    let semantic_key = origin_key(OriginExportKind::Semantic, "semantic:a", "expr:0");
    let runtime_key = origin_key(OriginExportKind::RuntimeStmt, "runtime:a", "bb0:stmt0");
    let pc_key = origin_key(
        OriginExportKind::BytecodePc,
        "object:A:section:runtime",
        "pc:0..2",
    );
    let witness = OriginPathWitnessExport::new(
        OriginExportKind::Semantic,
        OriginExportKind::BytecodePc,
        vec![semantic_key.clone(), runtime_key, pc_key.clone()],
        vec![OriginLinkKind::Lowered, OriginLinkKind::Lowered],
    );
    let json = serde_json::to_string(&witness).expect("path witness should serialize");
    let decoded =
        serde_json::from_str::<OriginPathWitnessExport>(&json).expect("path witness should decode");
    assert_eq!(decoded, witness);

    assert_eq!(
        OriginPathWitnessExport::try_new(
            OriginExportKind::Semantic,
            OriginExportKind::BytecodePc,
            vec![],
            vec![],
        ),
        Err(OriginPathWitnessExportError::EmptyPath)
    );
    assert_eq!(
        OriginPathWitnessExport::try_new(
            OriginExportKind::Semantic,
            OriginExportKind::BytecodePc,
            vec![semantic_key.clone()],
            vec![OriginLinkKind::Lowered],
        ),
        Err(OriginPathWitnessExportError::LengthMismatch { nodes: 1, links: 1 })
    );
    assert_eq!(
        OriginPathWitnessExport::try_new(
            OriginExportKind::RuntimeStmt,
            OriginExportKind::BytecodePc,
            vec![semantic_key.clone(), pc_key.clone()],
            vec![OriginLinkKind::Lowered],
        ),
        Err(OriginPathWitnessExportError::FromKindMismatch {
            expected: OriginExportKind::RuntimeStmt,
            actual: OriginExportKind::Semantic,
        })
    );
    assert_eq!(
        OriginPathWitnessExport::try_new(
            OriginExportKind::Semantic,
            OriginExportKind::RuntimeStmt,
            vec![semantic_key.clone(), pc_key.clone()],
            vec![OriginLinkKind::Lowered],
        ),
        Err(OriginPathWitnessExportError::ToKindMismatch {
            expected: OriginExportKind::RuntimeStmt,
            actual: OriginExportKind::BytecodePc,
        })
    );

    let mut bad_witness = serde_json::to_value(&witness).expect("witness should serialize");
    bad_witness["links"] = serde_json::Value::Array(vec![]);
    let err = serde_json::from_value::<OriginPathWitnessExport>(bad_witness)
        .expect_err("path witness JSON should reject mismatched link counts");
    assert!(
        err.to_string()
            .contains("expected exactly one more node than link"),
        "{err}"
    );

    let source_span = SourceSpanExport::new(
        pc_key.clone(),
        SourceSpanKind::Original,
        "file:///path-witness.fe",
        10,
        14,
        1,
        2,
        1,
        6,
    );
    let source_witness = OriginSourcePathWitnessExport::new(witness.clone(), source_span);
    let json =
        serde_json::to_string(&source_witness).expect("source path witness should serialize");
    let decoded = serde_json::from_str::<OriginSourcePathWitnessExport>(&json)
        .expect("source path witness should decode");
    assert_eq!(decoded, source_witness);

    let mismatched_span = SourceSpanExport::new(
        semantic_key.clone(),
        SourceSpanKind::Original,
        "file:///path-witness.fe",
        10,
        14,
        1,
        2,
        1,
        6,
    );
    assert_eq!(
        OriginSourcePathWitnessExport::try_new(witness.clone(), mismatched_span),
        Err(
            OriginSourcePathWitnessExportError::SourceSpanTargetMismatch {
                path_target: pc_key.clone(),
                source_origin: semantic_key.clone(),
            }
        )
    );

    let mut bad_source_witness =
        serde_json::to_value(&source_witness).expect("source witness should serialize");
    bad_source_witness["source_span"]["origin_key"] =
        serde_json::to_value(&semantic_key).expect("origin key should serialize");
    let err = serde_json::from_value::<OriginSourcePathWitnessExport>(bad_source_witness)
        .expect_err("source path witness JSON should reject mismatched terminal origin");
    assert!(
        err.to_string().contains("path ending at bytecode.pc"),
        "{err}"
    );
}

#[test]
fn origin_fact_index_answers_exact_reachability_oracle() {
    let semantic_a = origin_key(OriginExportKind::Semantic, "semantic:a", "expr:0");
    let runtime_a = origin_key(OriginExportKind::RuntimeStmt, "runtime:a", "block:0:stmt:0");
    let pre_a = origin_key(
        OriginExportKind::SonatinaInst,
        "sonatina:a",
        "pre_opt:inst:1",
    );
    let post_a = origin_key(
        OriginExportKind::SonatinaInst,
        "sonatina:a",
        "post_opt:inst:4",
    );
    let pc_a = origin_key(
        OriginExportKind::BytecodePc,
        "object:A:section:runtime",
        "pc:0..2",
    );

    let semantic_b = origin_key(OriginExportKind::Semantic, "semantic:b", "expr:0");
    let runtime_b = origin_key(OriginExportKind::RuntimeStmt, "runtime:b", "block:0:stmt:0");
    let pre_b = origin_key(
        OriginExportKind::SonatinaInst,
        "sonatina:b",
        "pre_opt:inst:1",
    );
    let post_b = origin_key(
        OriginExportKind::SonatinaInst,
        "sonatina:b",
        "post_opt:inst:4",
    );
    let pc_b = origin_key(
        OriginExportKind::BytecodePc,
        "object:B:section:runtime",
        "pc:0..2",
    );

    let unmapped = origin_key(OriginExportKind::BytecodeUnmapped, "bytecode", "no_ir_inst");
    let pc_unmapped = origin_key(
        OriginExportKind::BytecodePc,
        "object:C:section:runtime",
        "pc:8..9",
    );

    let mut graph = OriginGraph::new();
    graph.push(
        semantic_a.clone(),
        runtime_a.clone(),
        OriginLinkKind::Lowered,
    );
    graph.push(runtime_a.clone(), pre_a.clone(), OriginLinkKind::Lowered);
    graph.push(pre_a.clone(), post_a.clone(), OriginLinkKind::Transformed);
    graph.push(post_a.clone(), pc_a.clone(), OriginLinkKind::Lowered);
    graph.push(
        semantic_b.clone(),
        runtime_b.clone(),
        OriginLinkKind::Lowered,
    );
    graph.push(runtime_b.clone(), pre_b.clone(), OriginLinkKind::Lowered);
    graph.push(pre_b.clone(), post_b.clone(), OriginLinkKind::Transformed);
    graph.push(post_b.clone(), pc_b.clone(), OriginLinkKind::Lowered);
    graph.push(
        unmapped.clone(),
        pc_unmapped.clone(),
        OriginLinkKind::Synthetic,
    );

    let facts = origin_graph_facts(&graph, Clone::clone);
    let index = OriginFactIndex::new(&facts).expect("synthetic facts should index");
    let semantic_a_id = index
        .origin_id(&semantic_a)
        .expect("semantic A should be indexed");
    let pc_a_id = index.origin_id(&pc_a).expect("PC A should be indexed");
    let pc_b_id = index.origin_id(&pc_b).expect("PC B should be indexed");

    let path = index
        .shortest_path(semantic_a_id, pc_a_id)
        .expect("semantic A should have a path to PC A");
    assert_eq!(
        path.links(),
        &[
            OriginLinkKind::Lowered,
            OriginLinkKind::Lowered,
            OriginLinkKind::Transformed,
            OriginLinkKind::Lowered,
        ]
    );
    assert_eq!(
        path.nodes()
            .iter()
            .map(|id| index.origin_key(*id).expect("path node should be indexed"))
            .collect::<Vec<_>>(),
        vec![&semantic_a, &runtime_a, &pre_a, &post_a, &pc_a]
    );
    assert_eq!(
        index
            .shortest_path(semantic_a_id, semantic_a_id)
            .expect("identity path should exist")
            .nodes(),
        &[semantic_a_id]
    );
    assert!(index.shortest_path(semantic_a_id, pc_b_id).is_none());
    assert!(
        index.has_reachable_kind_pair(OriginExportKind::Semantic, OriginExportKind::BytecodePc)
    );
    assert!(!index.has_reachable_kind_pair(
        OriginExportKind::Semantic,
        OriginExportKind::BytecodeUnmapped
    ));

    let typed_witness = index
        .representative_path_for_kind_pair(OriginExportKind::Semantic, OriginExportKind::BytecodePc)
        .expect("semantic-to-bytecode kind pair should have a representative path");
    assert_eq!(
        typed_witness.path().links(),
        &[
            OriginLinkKind::Lowered,
            OriginLinkKind::Lowered,
            OriginLinkKind::Transformed,
            OriginLinkKind::Lowered,
        ]
    );
    assert!(
        index
            .representative_path_export_for_kind_pair(
                OriginExportKind::Semantic,
                OriginExportKind::BytecodeUnmapped,
            )
            .is_none()
    );
    assert!(index.has_path_between_keys(&semantic_a, &pc_a));
    assert!(!index.has_path_between_keys(&semantic_a, &pc_b));
    let key_path_export = index
        .path_export_between_keys(&semantic_a, &pc_a)
        .expect("stable keys should resolve to a semantic-to-bytecode path export");
    assert_eq!(key_path_export.from_kind(), OriginExportKind::Semantic);
    assert_eq!(key_path_export.to_kind(), OriginExportKind::BytecodePc);
    assert_eq!(
        key_path_export.nodes(),
        &[
            semantic_a.clone(),
            runtime_a.clone(),
            pre_a.clone(),
            post_a.clone(),
            pc_a.clone(),
        ]
    );
    assert!(
        index
            .path_export_between_keys(
                &semantic_a,
                &origin_key(
                    OriginExportKind::BytecodePc,
                    "object:missing:section:runtime",
                    "pc:0..1",
                ),
            )
            .is_none()
    );

    let reachable = index
        .reachable_keys_from(semantic_a_id)
        .into_iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    let expected = BTreeSet::from([
        runtime_a.clone(),
        pre_a.clone(),
        post_a.clone(),
        pc_a.clone(),
    ]);

    assert_eq!(reachable, expected);
    assert!(!index.has_path(semantic_a_id, pc_b_id));
    assert_eq!(
        index
            .reachable_from_with_kinds(semantic_a_id, |kind| kind == OriginLinkKind::Lowered)
            .into_iter()
            .filter_map(|id| index.origin_key(id).cloned())
            .collect::<BTreeSet<_>>(),
        BTreeSet::from([runtime_a, pre_a])
    );

    let summary = index.reachability_summary();
    assert_eq!(summary.reachable_pairs(), 21);
    assert_eq!(
        summary.pair_count(OriginExportKind::Semantic, OriginExportKind::BytecodePc),
        2
    );
    assert_eq!(
        summary.pair_count(OriginExportKind::RuntimeStmt, OriginExportKind::BytecodePc),
        2
    );
    assert_eq!(
        summary.pair_count(
            OriginExportKind::BytecodeUnmapped,
            OriginExportKind::BytecodePc
        ),
        1
    );

    let witnesses = index.representative_paths_by_kind(16);
    assert!(witnesses.iter().any(|witness| {
        witness.from_kind() == OriginExportKind::Semantic
            && witness.to_kind() == OriginExportKind::BytecodePc
            && witness.path().links()
                == &[
                    OriginLinkKind::Lowered,
                    OriginLinkKind::Lowered,
                    OriginLinkKind::Transformed,
                    OriginLinkKind::Lowered,
                ]
    }));
    assert_eq!(index.representative_paths_by_kind(1).len(), 1);

    let prioritized = index.representative_path_exports_with_priority(
        [
            (OriginExportKind::RuntimeStmt, OriginExportKind::BytecodePc),
            (OriginExportKind::Semantic, OriginExportKind::BytecodePc),
        ],
        1,
    );
    assert_eq!(prioritized.len(), 1);
    assert_eq!(
        prioritized[0].from_kind(),
        OriginExportKind::RuntimeStmt,
        "priority pairs should not be suppressed by generic witness ordering"
    );
    assert_eq!(prioritized[0].to_kind(), OriginExportKind::BytecodePc);
    assert!(
        index
            .representative_path_exports_with_priority(
                [(OriginExportKind::Semantic, OriginExportKind::BytecodePc)],
                0,
            )
            .is_empty()
    );

    let witness_exports = index.representative_path_exports(16);
    let semantic_to_pc = witness_exports
        .iter()
        .find(|witness| {
            witness.from_kind() == OriginExportKind::Semantic
                && witness.to_kind() == OriginExportKind::BytecodePc
        })
        .expect("semantic-to-bytecode path export should exist");
    assert_eq!(
        semantic_to_pc
            .nodes()
            .first()
            .expect("path should have a start node"),
        &semantic_a
    );
    assert_eq!(
        semantic_to_pc
            .nodes()
            .last()
            .expect("path should have an end node"),
        &pc_a
    );
    assert_eq!(
        semantic_to_pc.links(),
        &[
            OriginLinkKind::Lowered,
            OriginLinkKind::Lowered,
            OriginLinkKind::Transformed,
            OriginLinkKind::Lowered,
        ]
    );
    let json = serde_json::to_string(semantic_to_pc).expect("path export should serialize");
    let decoded = serde_json::from_str::<super::OriginPathWitnessExport>(&json)
        .expect("path export should deserialize");
    assert_eq!(&decoded, semantic_to_pc);

    let source_path = super::OriginSourcePathWitnessExport::new(
        (*semantic_to_pc).clone(),
        SourceSpanExport::new(
            pc_a,
            SourceSpanKind::Original,
            "file:///source-path-witness.fe",
            10,
            14,
            1,
            2,
            1,
            6,
        ),
    );
    let json = serde_json::to_string(&source_path).expect("source path export should serialize");
    let decoded = serde_json::from_str::<super::OriginSourcePathWitnessExport>(&json)
        .expect("source path export should deserialize");
    assert_eq!(decoded, source_path);
}

#[test]
fn origin_fact_index_rejects_missing_origin_link_endpoint() {
    let key = origin_key(OriginExportKind::RuntimeStmt, "runtime:a", "block:0:stmt:0");
    let origin_id = FactId::new(FactNamespace::OriginNode, 0);
    let missing_id = FactId::new(FactNamespace::OriginNode, 1);
    let facts = TypedFactSet::new(vec![
        TypedFact::OriginNode(OriginNodeFact::new(origin_id, key)),
        TypedFact::OriginLink(OriginLinkFactRow::new(
            origin_id,
            missing_id,
            OriginLinkKind::Lowered,
        )),
    ]);

    let err = OriginFactIndex::new(&facts)
        .expect_err("indexing should reject links to missing origin nodes");
    assert_eq!(
        err,
        FactIndexError::OriginLinkMissingEndpoint {
            endpoint: missing_id
        }
    );
}
