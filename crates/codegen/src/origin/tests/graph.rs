use super::*;

#[test]
fn codegen_origin_graph_uses_typed_codegen_nodes() {
    let inst = CodegenOriginNode::SonatinaInst(SonatinaInstOrigin::new(
        SonatinaInstStage::PostOpt,
        FuncRef::from_u32(0),
        InstId::from_u32(7),
    ));
    let pc = CodegenOriginNode::BytecodePc(BytecodePcOrigin::new(
        bytecode_section_key("Foo", "runtime"),
        BytecodePcRange::new(4, 8).expect("valid range"),
    ));
    let mut graph = CodegenOriginGraph::new();

    graph.push(inst.clone(), pc.clone(), OriginLinkKind::Lowered);

    let link = graph
        .links()
        .first()
        .expect("origin graph should have a link");

    assert_eq!(link.from(), &inst);
    assert_eq!(link.to(), &pc);
    assert_eq!(link.kind(), OriginLinkKind::Lowered);
}
