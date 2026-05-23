use super::*;

#[test]
fn origin_fact_ids_roundtrip_through_fail_closed_namespaces() {
    let origin = FactId::new(FactNamespace::OriginNode, 0);
    let runtime = FactId::new(FactNamespace::OriginNode, 1);
    let shape = FactId::new(FactNamespace::ShapeNode, 0);
    let key = origin_key(OriginExportKind::Semantic, "semantic:contract", "expr:0");
    let node = OriginNodeFact::new(origin, key.clone());
    let json = serde_json::to_string(&node).expect("origin node should serialize");
    let decoded = serde_json::from_str::<OriginNodeFact>(&json).expect("origin node should decode");

    assert_eq!(decoded, node);
    assert_eq!(
        OriginNodeFact::try_new(shape, key),
        Err(FactNamespaceError::WrongNamespace {
            id: shape,
            expected: FactNamespace::OriginNode,
        })
    );
    assert_eq!(
        OriginLinkFact::try_new(origin, shape, OriginLinkKind::Lowered),
        Err(FactNamespaceError::WrongNamespace {
            id: shape,
            expected: FactNamespace::OriginNode,
        })
    );
    assert!(OriginLinkFact::try_new(origin, runtime, OriginLinkKind::Lowered).is_ok());

    let wrong_link_namespace = r#"{
        "from": {"namespace": "origin_node", "ordinal": 0},
        "to": {"namespace": "shape_node", "ordinal": 0},
        "kind": "lowered"
    }"#;
    let err = serde_json::from_str::<OriginLinkFact>(wrong_link_namespace)
        .expect_err("origin link facts should reject non-origin endpoints");
    assert!(err.to_string().contains("expected origin_node"), "{err}");

    let wrong_typed_fact_namespace = r#"{
        "schema_version": 1,
        "facts": [{
            "type": "origin_node",
            "id": {"namespace": "shape_node", "ordinal": 0},
            "key": {
                "kind": "semantic",
                "owner_key": "semantic:contract",
                "local_key": "expr:0"
            }
        }]
    }"#;
    let err = serde_json::from_str::<OwnedTypedFactSetExport>(wrong_typed_fact_namespace)
        .expect_err("typed origin node facts should reject non-origin ids");
    assert!(
        err.to_string()
            .contains("fact id shape_node:0 has namespace shape_node, expected origin_node"),
        "{err}"
    );
}

#[test]
fn source_span_fact_roundtrips_through_fail_closed_schema() {
    let origin = FactId::new(FactNamespace::OriginNode, 0);
    let shape = FactId::new(FactNamespace::ShapeNode, 0);
    let span = SourceSpanFact::new(
        origin,
        SourceSpanKind::Original,
        "file:///a.fe",
        0,
        4,
        0,
        0,
        0,
        4,
    );
    let json = serde_json::to_string(&span).expect("source span fact should serialize");
    let decoded =
        serde_json::from_str::<SourceSpanFact>(&json).expect("source span fact should decode");
    assert_eq!(decoded, span);

    assert_eq!(
        SourceSpanFact::try_new(origin, SourceSpanKind::Original, "", 0, 4, 0, 0, 0, 4),
        Err(SourceSpanFactBuildError::InvalidSpan(
            SourceSpanExportError::EmptyFile
        ))
    );
    assert_eq!(
        SourceSpanFact::try_new(
            shape,
            SourceSpanKind::Original,
            "file:///a.fe",
            0,
            4,
            0,
            0,
            0,
            4
        ),
        Err(SourceSpanFactBuildError::WrongNamespace(
            FactNamespaceError::WrongNamespace {
                id: shape,
                expected: FactNamespace::OriginNode,
            }
        ))
    );
    assert_eq!(
        SourceSpanFact::try_new(
            origin,
            SourceSpanKind::Original,
            "file:///a.fe",
            4,
            0,
            0,
            0,
            0,
            4,
        ),
        Err(SourceSpanFactBuildError::InvalidSpan(
            SourceSpanExportError::InvalidByteRange {
                start_byte: 4,
                end_byte: 0,
            }
        ))
    );
    assert_eq!(
        SourceSpanFact::try_new(
            origin,
            SourceSpanKind::Original,
            "file:///a.fe",
            0,
            4,
            1,
            0,
            0,
            4,
        ),
        Err(SourceSpanFactBuildError::InvalidSpan(
            SourceSpanExportError::InvalidPositionRange {
                start_line: 1,
                start_col: 0,
                end_line: 0,
                end_col: 4,
            }
        ))
    );

    let unknown_field = r#"{
        "origin": {"namespace": "origin_node", "ordinal": 0},
        "span_kind": "original",
        "file": "file:///a.fe",
        "start_byte": 0,
        "end_byte": 4,
        "start_line": 0,
        "start_col": 0,
        "end_line": 0,
        "end_col": 4,
        "extra": true
    }"#;
    let err = serde_json::from_str::<SourceSpanFact>(unknown_field)
        .expect_err("source span facts should reject unknown fields");
    assert!(err.to_string().contains("unknown field"));
}

#[test]
fn source_span_file_count_roundtrips_through_fail_closed_schema() {
    let count = SourceSpanFileCount::new("file:///a.fe", 2);
    let json = serde_json::to_string(&count).expect("source span count should serialize");
    let decoded = serde_json::from_str::<SourceSpanFileCount>(&json).expect("count should decode");

    assert_eq!(decoded, count);
    assert_eq!(
        SourceSpanFileCount::try_new("", 1),
        Err(SourceSpanFileCountError::EmptyFile)
    );
    assert_eq!(
        SourceSpanFileCount::try_new("file:///a.fe", 0),
        Err(SourceSpanFileCountError::ZeroSpans)
    );

    let empty_file = r#"{"file":"","spans":1}"#;
    let err = serde_json::from_str::<SourceSpanFileCount>(empty_file)
        .expect_err("empty source-span file counts should fail closed");
    assert!(err.to_string().contains("file must not be empty"));

    let zero_spans = r#"{"file":"file:///a.fe","spans":0}"#;
    let err = serde_json::from_str::<SourceSpanFileCount>(zero_spans)
        .expect_err("zero-span file counts should fail closed");
    assert!(err.to_string().contains("greater than zero"));

    let unknown_field = r#"{"file":"file:///a.fe","spans":1,"extra":true}"#;
    let err = serde_json::from_str::<SourceSpanFileCount>(unknown_field)
        .expect_err("source-span file counts should reject unknown fields");
    assert!(err.to_string().contains("unknown field"));
}

#[test]
fn typed_fact_relation_count_roundtrips_through_fail_closed_schema() {
    let count = TypedFactRelationCount::new(TypedFactRelationName::OriginNode, 2);
    let json = serde_json::to_string(&count).expect("relation count should serialize");
    assert_eq!(json, r#"{"relation":"origin_node","rows":2}"#);
    let decoded =
        serde_json::from_str::<TypedFactRelationCount>(&json).expect("count should decode");

    assert_eq!(decoded, count);
    assert_eq!(
        TypedFactRelationCount::try_new(TypedFactRelationName::OriginNode, 0),
        Err(TypedFactRelationCountError::ZeroRows)
    );

    let zero_rows = r#"{"relation":"origin_node","rows":0}"#;
    let err = serde_json::from_str::<TypedFactRelationCount>(zero_rows)
        .expect_err("zero-row relation counts should fail closed");
    assert!(err.to_string().contains("greater than zero"));

    let unknown_relation = r#"{"relation":"unknown_relation","rows":1}"#;
    serde_json::from_str::<TypedFactRelationCount>(unknown_relation)
        .expect_err("unknown relation-count names should fail closed");

    let unknown_field = r#"{"relation":"origin_node","rows":1,"extra":true}"#;
    let err = serde_json::from_str::<TypedFactRelationCount>(unknown_field)
        .expect_err("relation counts should reject unknown fields");
    assert!(err.to_string().contains("unknown field"));
}

#[test]
fn shape_fact_text_fields_roundtrip_through_fail_closed_schema() {
    let node = FactId::new(FactNamespace::ShapeNode, 0);
    let child = FactId::new(FactNamespace::ShapeNode, 1);
    let origin = FactId::new(FactNamespace::OriginNode, 0);
    let shape_node = ShapeNodeFact::new(node, ShapeNodeId::from_u32(0), "root", "block");
    let json = serde_json::to_string(&shape_node).expect("shape node should serialize");
    let decoded = serde_json::from_str::<ShapeNodeFact>(&json).expect("shape node should decode");
    assert_eq!(decoded, shape_node);

    assert_eq!(
        ShapeNodeFact::try_new(node, ShapeNodeId::from_u32(0), "", "block"),
        Err(ShapeFactTextError::Empty {
            field: "shape stable key"
        })
    );
    assert_eq!(
        ShapeNodeFact::try_new(node, ShapeNodeId::from_u32(0), "root", ""),
        Err(ShapeFactTextError::Empty {
            field: "shape node kind"
        })
    );
    assert_eq!(
        ShapeNodeFact::try_new(origin, ShapeNodeId::from_u32(0), "root", "block"),
        Err(ShapeFactTextError::WrongNamespace(
            FactNamespaceError::WrongNamespace {
                id: origin,
                expected: FactNamespace::ShapeNode,
            }
        ))
    );
    assert_eq!(
        ShapeFieldFact::try_new(node, ShapeDimension::Structure, "", ""),
        Err(ShapeFactTextError::Empty {
            field: "shape field name"
        })
    );
    assert_eq!(
        ShapeFieldFact::try_new(origin, ShapeDimension::Structure, "constant", ""),
        Err(ShapeFactTextError::WrongNamespace(
            FactNamespaceError::WrongNamespace {
                id: origin,
                expected: FactNamespace::ShapeNode,
            }
        ))
    );
    assert!(ShapeFieldFact::try_new(node, ShapeDimension::Structure, "constant", "").is_ok());
    assert_eq!(
        ShapeChildFact::try_new(node, child, "", 0),
        Err(ShapeFactTextError::Empty {
            field: "shape child label"
        })
    );
    assert_eq!(
        ShapeEdgeFact::try_new(node, child, ""),
        Err(ShapeFactTextError::Empty {
            field: "shape edge label"
        })
    );
    assert_eq!(
        TraceEventFact::try_new(node, "", ""),
        Err(ShapeFactTextError::Empty {
            field: "trace event kind"
        })
    );
    assert!(TraceEventFact::try_new(node, "lowered", "").is_ok());
    assert_eq!(
        DataFlowFact::try_new(node, child, ""),
        Err(ShapeFactTextError::Empty {
            field: "data flow kind"
        })
    );

    let empty_field_name = r#"{
        "node": {"namespace": "shape_node", "ordinal": 0},
        "dimension": "structure",
        "name": "",
        "value": ""
    }"#;
    let err = serde_json::from_str::<ShapeFieldFact>(empty_field_name)
        .expect_err("empty shape field names should fail closed");
    assert!(
        err.to_string()
            .contains("shape field name must not be empty")
    );

    let unknown_field = r#"{
        "id": {"namespace": "shape_node", "ordinal": 0},
        "source_id": 0,
        "stable_key": "root",
        "kind": "block",
        "extra": true
    }"#;
    let err = serde_json::from_str::<ShapeNodeFact>(unknown_field)
        .expect_err("shape node facts should reject unknown fields");
    assert!(err.to_string().contains("unknown field"));

    let wrong_namespace = r#"{
        "node": {"namespace": "origin_node", "ordinal": 0},
        "dimension": "structure",
        "name": "constant",
        "value": ""
    }"#;
    let err = serde_json::from_str::<ShapeFieldFact>(wrong_namespace)
        .expect_err("shape field facts should reject non-shape node IDs");
    assert!(err.to_string().contains("expected shape_node"), "{err}");

    let wrong_typed_fact_namespace = r#"{
        "schema_version": 1,
        "facts": [{
            "type": "shape_node",
            "id": {"namespace": "origin_node", "ordinal": 0},
            "source_id": 0,
            "stable_key": "root",
            "kind": "block"
        }]
    }"#;
    let err = serde_json::from_str::<OwnedTypedFactSetExport>(wrong_typed_fact_namespace)
        .expect_err("typed shape node facts should reject non-shape ids");
    assert!(
        err.to_string()
            .contains("fact id origin_node:0 has namespace origin_node, expected shape_node"),
        "{err}"
    );
}

#[test]
fn shape_hash_fact_roundtrips_through_fail_closed_digest_schema() {
    let hash = ShapeHashFact::new(
        None,
        ShapeHashScope::Graph,
        ShapeDimension::Structure,
        ShapeHashDigest::new("0000000000000000"),
    );
    let json = serde_json::to_string(&hash).expect("shape hash fact should serialize");
    let decoded =
        serde_json::from_str::<ShapeHashFact>(&json).expect("shape hash fact should decode");

    assert_eq!(decoded, hash);
    assert_eq!(decoded.digest().as_str(), "0000000000000000");
    assert_eq!(decoded.digest_hex(), "0000000000000000");
    let digest = ShapeHashDigest::try_new("0000000000000000")
        .expect("canonical shape hash digest should validate");
    assert_eq!(digest.as_str(), "0000000000000000");
    assert_eq!(digest.to_string(), "0000000000000000");
    assert_eq!(
        ShapeHashDigest::try_new("000000000000000"),
        Err(ShapeHashDigestError::InvalidDigest {
            digest_hex: "000000000000000".to_string(),
        })
    );
    assert_eq!(
        ShapeHashDigest::try_new("ABCDEF0000000000"),
        Err(ShapeHashDigestError::InvalidDigest {
            digest_hex: "ABCDEF0000000000".to_string(),
        })
    );
    let invalid_digest_json = r#""ABCDEF0000000000""#;
    let err = serde_json::from_str::<ShapeHashDigest>(invalid_digest_json)
        .expect_err("shape hash digest JSON should reject non-canonical text");
    assert!(
        err.to_string()
            .contains("canonical 16-character lowercase hex"),
        "{err}"
    );
    assert_eq!(
        ShapeHashFact::try_from_digest_hex(
            None,
            ShapeHashScope::Graph,
            ShapeDimension::Structure,
            "ABCDEF0000000000",
        ),
        Err(ShapeHashFactError::InvalidDigest(
            ShapeHashDigestError::InvalidDigest {
                digest_hex: "ABCDEF0000000000".to_string(),
            }
        ))
    );
    let origin = FactId::new(FactNamespace::OriginNode, 0);
    assert_eq!(
        ShapeHashFact::try_new(
            Some(origin),
            ShapeHashScope::Local,
            ShapeDimension::Structure,
            ShapeHashDigest::new("0000000000000000"),
        ),
        Err(ShapeHashFactError::WrongNamespace(
            FactNamespaceError::WrongNamespace {
                id: origin,
                expected: FactNamespace::ShapeNode,
            }
        ))
    );

    let invalid_digest = r#"{
        "node": null,
        "scope": "graph",
        "dimension": "structure",
        "digest_hex": "ABCDEF0000000000"
    }"#;
    let err = serde_json::from_str::<ShapeHashFact>(invalid_digest)
        .expect_err("non-canonical shape hash digests should fail closed");
    assert!(
        err.to_string()
            .contains("canonical 16-character lowercase hex")
    );

    let unknown_field = r#"{
        "node": null,
        "scope": "graph",
        "dimension": "structure",
        "digest_hex": "0000000000000000",
        "extra": true
    }"#;
    let err = serde_json::from_str::<ShapeHashFact>(unknown_field)
        .expect_err("shape hash facts should reject unknown fields");
    assert!(err.to_string().contains("unknown field"));

    let wrong_namespace = r#"{
        "node": {"namespace": "origin_node", "ordinal": 0},
        "scope": "local",
        "dimension": "structure",
        "digest_hex": "0000000000000000"
    }"#;
    let err = serde_json::from_str::<ShapeHashFact>(wrong_namespace)
        .expect_err("shape hash facts should reject non-shape node IDs");
    assert!(err.to_string().contains("expected shape_node"), "{err}");

    let shape = FactId::new(FactNamespace::ShapeNode, 0);
    assert_eq!(
        ShapeHashFactKey::try_new(None, ShapeHashScope::Local, ShapeDimension::Structure),
        Err(ShapeHashNodeScopeError::new(ShapeHashScope::Local, None))
    );
    assert_eq!(
        ShapeHashFactKey::try_new(None, ShapeHashScope::Tree, ShapeDimension::Structure),
        Err(ShapeHashNodeScopeError::new(ShapeHashScope::Tree, None))
    );
    assert_eq!(
        ShapeHashFactKey::try_new(
            Some(shape),
            ShapeHashScope::Graph,
            ShapeDimension::Structure,
        ),
        Err(ShapeHashNodeScopeError::new(
            ShapeHashScope::Graph,
            Some(shape),
        ))
    );
    assert_eq!(
        ShapeHashFactKey::graph(ShapeDimension::Structure).node(),
        None
    );
    assert_eq!(
        ShapeHashFactKey::local(shape, ShapeDimension::Structure).node(),
        Some(shape)
    );
    assert_eq!(
        ShapeHashFact::try_new(
            Some(shape),
            ShapeHashScope::Graph,
            ShapeDimension::Structure,
            ShapeHashDigest::new("0000000000000000"),
        ),
        Err(ShapeHashFactError::InvalidNodeScope(
            ShapeHashNodeScopeError::new(ShapeHashScope::Graph, Some(shape))
        ))
    );

    let local_without_node = r#"{
        "node": null,
        "scope": "local",
        "dimension": "structure",
        "digest_hex": "0000000000000000"
    }"#;
    let err = serde_json::from_str::<ShapeHashFact>(local_without_node)
        .expect_err("shape hash facts should reject local hashes without nodes");
    assert!(
        err.to_string()
            .contains("shape hash scope local has invalid node reference none"),
        "{err}"
    );
}
