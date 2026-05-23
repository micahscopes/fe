use super::*;

#[test]
fn pc_map_entries_missing_postopt_snapshot_use_backend_prepared_origin() {
    let function = FuncRef::from_u32(2);
    let missing_inst = InstId::from_u32(99);
    let post_opt_origins = SonatinaPostOptPackageOrigins {
        functions: vec![SonatinaPostOptFunctionOrigins::new(function, Vec::new())],
        pre_opt_snapshot_losses: Vec::new(),
    };
    let pc_entry = PcMapEntry {
        pc_start: 4,
        pc_end: 8,
        func: function,
        func_name: "test_func".to_string(),
        block: BlockId::from_u32(0),
        vcode_inst: VCodeInst(0),
        ir_inst: Some(missing_inst),
        frontend_provenance: None,
        unmapped_reason: None,
    };

    let source = bytecode_source_from_pc_entry(&pc_entry, &post_opt_origins);
    let BytecodeOriginSource::SonatinaBackendPrepared(backend_prepared) = source else {
        panic!("PC-map entries missing the post-opt snapshot must not fake post-opt origins");
    };
    assert_eq!(
        backend_prepared.origin(),
        SonatinaInstOrigin::backend_prepared(function, missing_inst)
    );
    assert_eq!(
        backend_prepared.source(),
        SonatinaBackendPreparedOriginSource::MissingPostOptSnapshotRecord
    );

    let object = BytecodeObjectKey::new("Foo");
    let pc = BytecodePcOrigin::new(
        BytecodeSectionKey::new(object.clone(), BytecodeSectionNameKey::new("runtime")),
        BytecodePcRange::new(4, 8).expect("valid PC range"),
    );
    let origins = BytecodePackageOrigins {
        records: vec![BytecodeOriginRecord::new(
            pc,
            BytecodeOriginSource::SonatinaBackendPrepared(backend_prepared),
        )],
    };
    assert_eq!(origins.coverage().total(), 1);
    assert_eq!(origins.coverage().sonatina_backend_prepared(), 1);
    assert_eq!(origins.coverage().sonatina_post_opt(), 0);
    assert_eq!(origins.coverage().unmapped(), 0);
    assert!(origins.coverage().is_partitioned());

    let graph = origins.origin_graph();
    assert!(graph.links().iter().any(|link| {
        link.kind() == OriginLinkKind::Synthetic
            && matches!(
                link.from(),
                CodegenOriginNode::SonatinaSynthetic(
                    SonatinaSyntheticOrigin::PostPreOptSnapshotGap
                )
            )
            && matches!(
                link.to(),
                CodegenOriginNode::SonatinaInst(origin)
                    if origin.stage() == SonatinaInstStage::BackendPrepared
                        && origin.inst() == missing_inst
            )
    }));
    assert!(graph.links().iter().any(|link| {
        link.kind() == OriginLinkKind::Lowered
            && matches!(
                link.from(),
                CodegenOriginNode::SonatinaInst(origin)
                    if origin.stage() == SonatinaInstStage::BackendPrepared
                        && origin.inst() == missing_inst
            )
            && matches!(link.to(), CodegenOriginNode::BytecodePc(_))
    }));
    assert!(graph.links().iter().all(|link| {
        !matches!(
            link.from(),
            CodegenOriginNode::SonatinaInst(origin)
                if origin.stage() == SonatinaInstStage::PostOpt
                    && origin.inst() == missing_inst
        ) && !matches!(
            link.to(),
            CodegenOriginNode::SonatinaInst(origin)
                if origin.stage() == SonatinaInstStage::PostOpt
                    && origin.inst() == missing_inst
        )
    }));

    let facts = origins
        .origin_facts_for_object(&object, |func| {
            assert_eq!(func, function);
            Some(SonatinaFunctionExportKey::new("sonatina:func:test"))
        })
        .expect("backend-prepared bytecode facts should export with a function key")
        .expect("backend-prepared bytecode facts should be non-empty");
    assert!(facts.origin_nodes().any(|node| {
        node.key().kind() == OriginExportKind::SonatinaInst
            && node.key().owner_key() == "sonatina:func:test"
            && node.key().local_key() == "backend_prepared:inst:99"
    }));
}
