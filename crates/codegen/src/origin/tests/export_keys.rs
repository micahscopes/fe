use super::*;

#[test]
fn codegen_origin_export_keys_include_kind_owner_and_local_identity() {
    let inst_key = sonatina_inst_export_key(
        SonatinaInstOrigin::pre_opt(FuncRef::from_u32(0), InstId::from_u32(7)),
        &SonatinaFunctionExportKey::new("sonatina:func:a"),
    );
    let pc_key = bytecode_pc_export_key(BytecodePcOrigin::new(
        bytecode_section_key("Foo", "runtime"),
        BytecodePcRange::new(4, 8).expect("valid range"),
    ));

    assert_eq!(
        inst_key,
        origin_key(
            OriginExportKind::SonatinaInst,
            "sonatina:func:a",
            "pre_opt:inst:7"
        )
    );
    assert_eq!(
        pc_key,
        origin_key(
            OriginExportKind::BytecodePc,
            "object:Foo:section:runtime",
            "pc:4..8"
        )
    );
}

#[test]
fn codegen_origin_node_export_keys_cover_synthetic_unmapped_and_pc_nodes() {
    let synthetic = codegen_origin_node_export_key(
        &CodegenOriginNode::SonatinaSynthetic(SonatinaSyntheticOrigin::Prologue),
        |_| None,
    )
    .expect("synthetic node does not need a function key");
    let unmapped = codegen_origin_node_export_key(
        &CodegenOriginNode::BytecodeUnmapped(super::BytecodeUnmappedReason::NoIrInst),
        |_| None,
    )
    .expect("unmapped node does not need a function key");
    let pc_origin = BytecodePcOrigin::new(
        bytecode_section_key("Foo", "runtime"),
        BytecodePcRange::new(4, 8).expect("valid range"),
    );
    let pc =
        codegen_origin_node_export_key(&CodegenOriginNode::BytecodePc(pc_origin.clone()), |_| None)
            .expect("bytecode PC node does not need a function key");

    assert_eq!(
        synthetic,
        sonatina_synthetic_export_key(SonatinaSyntheticOrigin::Prologue)
    );
    assert_eq!(
        codegen_origin_node_export_key(
            &CodegenOriginNode::SonatinaSynthetic(SonatinaSyntheticOrigin::PreOptSnapshotLoss),
            |_| None,
        )
        .expect("snapshot-loss synthetic node does not need a function key"),
        sonatina_synthetic_export_key(SonatinaSyntheticOrigin::PreOptSnapshotLoss)
    );
    assert_eq!(
        unmapped,
        bytecode_unmapped_export_key(super::BytecodeUnmappedReason::NoIrInst)
    );
    assert_eq!(pc, bytecode_pc_export_key(pc_origin));
}

#[test]
fn end_to_end_runtime_synthetic_export_uses_typed_owner_and_local_keys() {
    let node = EndToEndOriginNode::RuntimeSynthetic {
        owner_key: EndToEndRuntimeOwnerKey::new("sonatina:func:test"),
        local_key: EndToEndRuntimeSyntheticLocalKey::new("block:0:stmt:0"),
    };

    assert_eq!(
        end_to_end_origin_node_export_key(&node, |_| None),
        Some(origin_key(
            OriginExportKind::RuntimeSynthetic,
            "sonatina:func:test",
            "block:0:stmt:0"
        ))
    );
}

#[test]
fn sonatina_inst_node_export_key_requires_function_key() {
    let node = CodegenOriginNode::SonatinaInst(SonatinaInstOrigin::post_opt(
        FuncRef::from_u32(2),
        InstId::from_u32(9),
    ));

    assert_eq!(codegen_origin_node_export_key(&node, |_| None), None);
    assert_eq!(
        codegen_origin_node_export_key(&node, |func| {
            assert_eq!(func, FuncRef::from_u32(2));
            Some(SonatinaFunctionExportKey::new("sonatina:func:foo"))
        }),
        Some(origin_key(
            OriginExportKind::SonatinaInst,
            "sonatina:func:foo",
            "post_opt:inst:9"
        ))
    );
}

#[test]
fn end_to_end_origin_owner_keys_are_derived_from_typed_function_key() {
    let function_key = SonatinaFunctionExportKey::new("sonatina:func:test");
    let owner_keys = EndToEndOriginOwnerKeys::for_function(&function_key);

    assert_eq!(owner_keys.semantic().as_str(), "sonatina:func:test");
    assert_eq!(owner_keys.runtime().as_str(), "sonatina:func:test");
}
