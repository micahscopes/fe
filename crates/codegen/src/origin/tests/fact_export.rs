use super::*;

#[test]
fn codegen_origin_graph_facts_require_stable_function_keys() {
    let inst = CodegenOriginNode::SonatinaInst(SonatinaInstOrigin::post_opt(
        FuncRef::from_u32(2),
        InstId::from_u32(9),
    ));
    let pc = CodegenOriginNode::BytecodePc(BytecodePcOrigin::new(
        bytecode_section_key("Foo", "runtime"),
        BytecodePcRange::new(4, 8).expect("valid range"),
    ));
    let mut graph = CodegenOriginGraph::new();
    graph.push(inst, pc, OriginLinkKind::Lowered);

    let err = codegen_origin_graph_facts(&graph, |_| None)
        .expect_err("Sonatina instruction nodes require stable function keys");
    assert_eq!(err.function(), FuncRef::from_u32(2));

    let facts = codegen_origin_graph_facts(&graph, |func| {
        assert_eq!(func, FuncRef::from_u32(2));
        Some(SonatinaFunctionExportKey::new("sonatina:func:foo"))
    })
    .expect("stable function key should export codegen origin facts");

    assert!(facts.origin_nodes().any(|node| {
        node.key().kind() == OriginExportKind::SonatinaInst
            && node.key().owner_key() == "sonatina:func:foo"
    }));
    assert!(
        facts
            .origin_links()
            .any(|link| link.kind() == OriginLinkKind::Lowered)
    );
}

#[test]
fn codegen_origin_graph_facts_resolve_each_function_key_once() {
    let function = FuncRef::from_u32(2);
    let first_inst =
        CodegenOriginNode::SonatinaInst(SonatinaInstOrigin::pre_opt(function, InstId::from_u32(8)));
    let second_inst = CodegenOriginNode::SonatinaInst(SonatinaInstOrigin::post_opt(
        function,
        InstId::from_u32(8),
    ));
    let pc = CodegenOriginNode::BytecodePc(BytecodePcOrigin::new(
        bytecode_section_key("Foo", "runtime"),
        BytecodePcRange::new(4, 8).expect("valid range"),
    ));
    let mut graph = CodegenOriginGraph::new();
    graph.push(first_inst, second_inst.clone(), OriginLinkKind::Alias);
    graph.push(second_inst, pc, OriginLinkKind::Lowered);

    let mut calls = 0;
    let facts = codegen_origin_graph_facts(&graph, |func| {
        calls += 1;
        assert_eq!(func, function);
        Some(SonatinaFunctionExportKey::new("sonatina:func:foo"))
    })
    .expect("stable function key should export repeated-function graph facts");

    assert_eq!(calls, 1);
    assert!(facts.origin_nodes().any(|node| {
        node.key().kind() == OriginExportKind::SonatinaInst
            && node.key().owner_key() == "sonatina:func:foo"
    }));
}

#[test]
fn end_to_end_origin_graph_facts_require_stable_function_keys() {
    let inst = EndToEndOriginNode::SonatinaInst(SonatinaInstOrigin::post_opt(
        FuncRef::from_u32(2),
        InstId::from_u32(9),
    ));
    let pc = EndToEndOriginNode::BytecodePc(BytecodePcOrigin::new(
        bytecode_section_key("Foo", "runtime"),
        BytecodePcRange::new(4, 8).expect("valid range"),
    ));
    let mut graph = EndToEndOriginGraph::new();
    graph.push(inst, pc, OriginLinkKind::Lowered);

    let err = end_to_end_origin_graph_facts(&graph, |_| None)
        .expect_err("end-to-end Sonatina nodes require stable function keys");
    assert_eq!(err.function(), FuncRef::from_u32(2));

    let facts = end_to_end_origin_graph_facts(&graph, |func| {
        assert_eq!(func, FuncRef::from_u32(2));
        Some(SonatinaFunctionExportKey::new("sonatina:func:foo"))
    })
    .expect("stable function key should export end-to-end origin facts");

    assert!(facts.origin_nodes().any(|node| {
        node.key().kind() == OriginExportKind::SonatinaInst
            && node.key().owner_key() == "sonatina:func:foo"
    }));
    assert!(
        facts
            .origin_links()
            .any(|link| link.kind() == OriginLinkKind::Lowered)
    );
}
