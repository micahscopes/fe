use super::*;

#[test]
fn post_opt_snapshot_origin_facts_include_preopt_losses() {
    let function = FuncRef::from_u32(2);
    let kept_pre_opt = SonatinaInstOriginRecord::new(
        SonatinaInstOrigin::pre_opt(function, InstId::from_u32(9)),
        SonatinaOriginSource::Synthetic(SonatinaSyntheticOrigin::Prologue),
    );
    let lost_pre_opt = SonatinaInstOriginRecord::new(
        SonatinaInstOrigin::pre_opt(function, InstId::from_u32(11)),
        SonatinaOriginSource::Synthetic(SonatinaSyntheticOrigin::Prologue),
    );
    let post_opt = SonatinaPostOptOriginRecord::new(
        SonatinaInstOrigin::post_opt(function, InstId::from_u32(9)),
        SonatinaPostOptOriginSource::SameInstId(kept_pre_opt),
    );
    let origins = SonatinaPostOptPackageOrigins {
        functions: vec![SonatinaPostOptFunctionOrigins::new(
            function,
            vec![post_opt],
        )],
        pre_opt_snapshot_losses: vec![SonatinaPreOptSnapshotLossRecord::new(
            lost_pre_opt,
            SonatinaPreOptSnapshotLossReason::ElidedOrRewrittenBeforePostOptSnapshot,
        )],
    };
    let loss_reason = origins
        .pre_opt_snapshot_losses()
        .next()
        .expect("expected pre-opt snapshot loss")
        .reason();
    assert_eq!(
        loss_reason.as_str(),
        "elided_or_rewritten_before_postopt_snapshot"
    );
    assert_eq!(
        loss_reason.to_string(),
        "elided_or_rewritten_before_postopt_snapshot"
    );

    let graph = origins.origin_graph();
    assert!(graph.links().iter().any(|link| {
        link.kind() == OriginLinkKind::Alias
            && matches!(
                link.from(),
                CodegenOriginNode::SonatinaInst(origin)
                    if origin.stage() == SonatinaInstStage::PreOpt
                        && origin.inst() == InstId::from_u32(9)
            )
            && matches!(
                link.to(),
                CodegenOriginNode::SonatinaInst(origin)
                    if origin.stage() == SonatinaInstStage::PostOpt
                        && origin.inst() == InstId::from_u32(9)
            )
    }));
    assert!(graph.links().iter().any(|link| {
        link.kind() == OriginLinkKind::Synthetic
            && matches!(
                link.from(),
                CodegenOriginNode::SonatinaInst(origin)
                    if origin.stage() == SonatinaInstStage::PreOpt
                        && origin.inst() == InstId::from_u32(11)
            )
            && matches!(
                link.to(),
                CodegenOriginNode::SonatinaSynthetic(SonatinaSyntheticOrigin::PreOptSnapshotLoss)
            )
    }));
    assert!(
        graph.links().iter().all(|link| {
            !(link.kind() == OriginLinkKind::Transformed
                && matches!(
                    link.to(),
                    CodegenOriginNode::SonatinaSynthetic(
                        SonatinaSyntheticOrigin::PreOptSnapshotLoss
                    )
                ))
        }),
        "snapshot-loss facts must not pretend to be precise pass transforms"
    );

    let facts = origins
        .origin_facts(|func| {
            assert_eq!(func, function);
            Some(SonatinaFunctionExportKey::new("sonatina:func:test"))
        })
        .expect("snapshot diff facts should export with a function key")
        .expect("non-empty snapshot diff should produce facts");

    assert!(facts.origin_nodes().any(|node| {
        node.key().kind() == OriginExportKind::SonatinaInst
            && node.key().owner_key() == "sonatina:func:test"
            && node.key().local_key() == "pre_opt:inst:11"
    }));
    assert!(facts.origin_nodes().any(|node| {
        node.key() == &sonatina_synthetic_export_key(SonatinaSyntheticOrigin::PreOptSnapshotLoss)
    }));
    assert!(
        facts
            .origin_links()
            .any(|link| link.kind() == OriginLinkKind::Synthetic)
    );
}

#[test]
fn post_opt_origin_coverage_filters_to_bytecode_object_functions() {
    let first_function = FuncRef::from_u32(2);
    let second_function = FuncRef::from_u32(3);
    let first_kept_pre_opt = SonatinaInstOriginRecord::new(
        SonatinaInstOrigin::pre_opt(first_function, InstId::from_u32(9)),
        SonatinaOriginSource::Synthetic(SonatinaSyntheticOrigin::Prologue),
    );
    let first_lost_pre_opt = SonatinaInstOriginRecord::new(
        SonatinaInstOrigin::pre_opt(first_function, InstId::from_u32(11)),
        SonatinaOriginSource::Synthetic(SonatinaSyntheticOrigin::Prologue),
    );
    let second_lost_pre_opt = SonatinaInstOriginRecord::new(
        SonatinaInstOrigin::pre_opt(second_function, InstId::from_u32(13)),
        SonatinaOriginSource::Synthetic(SonatinaSyntheticOrigin::Prologue),
    );
    let first_same = SonatinaPostOptOriginRecord::new(
        SonatinaInstOrigin::post_opt(first_function, InstId::from_u32(9)),
        SonatinaPostOptOriginSource::SameInstId(first_kept_pre_opt),
    );
    let first_created = SonatinaPostOptOriginRecord::new(
        SonatinaInstOrigin::post_opt(first_function, InstId::from_u32(10)),
        SonatinaPostOptOriginSource::CreatedOrUnmatchedAfterPreOptSnapshot,
    );
    let second_created = SonatinaPostOptOriginRecord::new(
        SonatinaInstOrigin::post_opt(second_function, InstId::from_u32(12)),
        SonatinaPostOptOriginSource::CreatedOrUnmatchedAfterPreOptSnapshot,
    );
    let post_opt_origins = SonatinaPostOptPackageOrigins {
        functions: vec![
            SonatinaPostOptFunctionOrigins::new(first_function, vec![first_same, first_created]),
            SonatinaPostOptFunctionOrigins::new(second_function, vec![second_created]),
        ],
        pre_opt_snapshot_losses: vec![
            SonatinaPreOptSnapshotLossRecord::new(
                first_lost_pre_opt,
                SonatinaPreOptSnapshotLossReason::ElidedOrRewrittenBeforePostOptSnapshot,
            ),
            SonatinaPreOptSnapshotLossRecord::new(
                second_lost_pre_opt,
                SonatinaPreOptSnapshotLossReason::ElidedOrRewrittenBeforePostOptSnapshot,
            ),
        ],
    };
    let first_section = bytecode_section_key("First", "runtime");
    let second_section = bytecode_section_key("Second", "runtime");
    let origins = BytecodePackageOrigins {
        records: vec![
            BytecodeOriginRecord::new(
                BytecodePcOrigin::new(
                    first_section.clone(),
                    BytecodePcRange::new(0, 4).expect("valid PC range"),
                ),
                BytecodeOriginSource::SonatinaPostOpt(first_same),
            ),
            BytecodeOriginRecord::new(
                BytecodePcOrigin::new(
                    second_section,
                    BytecodePcRange::new(0, 4).expect("valid PC range"),
                ),
                BytecodeOriginSource::SonatinaPostOpt(second_created),
            ),
        ],
    };

    let coverage = origins
        .post_opt_origin_coverage_for_object(&BytecodeObjectKey::new("First"), &post_opt_origins);
    assert_eq!(coverage.total(), 2);
    assert_eq!(coverage.same_inst_id(), 1);
    assert_eq!(coverage.created_or_unmatched_after_preopt_snapshot(), 1);
    assert_eq!(coverage.pre_opt_snapshot_losses(), 1);
    assert_eq!(coverage.observed_pre_opt_total(), 2);
    assert!(coverage.is_post_opt_partitioned());

    let section_coverage =
        origins.post_opt_origin_coverage_for_section(&first_section, &post_opt_origins);
    assert_eq!(section_coverage, coverage);
}
