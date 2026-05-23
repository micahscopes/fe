use super::*;

#[test]
fn sonatina_instruction_origin_includes_function_owner_and_stage() {
    let inst = InstId::from_u32(7);

    let first = SonatinaInstOrigin::pre_opt(FuncRef::from_u32(0), inst);
    let second = SonatinaInstOrigin::pre_opt(FuncRef::from_u32(1), inst);
    let post_opt = SonatinaInstOrigin::post_opt(FuncRef::from_u32(0), inst);
    let backend_prepared = SonatinaInstOrigin::backend_prepared(FuncRef::from_u32(0), inst);

    assert_ne!(first, second);
    assert_ne!(first, post_opt);
    assert_ne!(post_opt, backend_prepared);
    assert_eq!(first.stage(), SonatinaInstStage::PreOpt);
    assert_eq!(post_opt.stage(), SonatinaInstStage::PostOpt);
    assert_eq!(backend_prepared.stage(), SonatinaInstStage::BackendPrepared);
    assert_eq!(first.stage().as_str(), "pre_opt");
    assert_eq!(backend_prepared.stage().to_string(), "backend_prepared");
}

#[test]
#[should_panic(expected = "pre-opt Sonatina origin records must use pre-opt instruction origins")]
fn sonatina_pre_opt_records_reject_post_opt_instruction_origins() {
    SonatinaInstOriginRecord::new(
        SonatinaInstOrigin::post_opt(FuncRef::from_u32(0), InstId::from_u32(7)),
        SonatinaOriginSource::Synthetic(SonatinaSyntheticOrigin::Prologue),
    );
}

#[test]
#[should_panic(expected = "post-opt Sonatina origin records must use post-opt instruction origins")]
fn sonatina_post_opt_records_reject_pre_opt_instruction_origins() {
    let pre_opt = SonatinaInstOriginRecord::new(
        SonatinaInstOrigin::pre_opt(FuncRef::from_u32(0), InstId::from_u32(7)),
        SonatinaOriginSource::Synthetic(SonatinaSyntheticOrigin::Prologue),
    );

    SonatinaPostOptOriginRecord::new(
        SonatinaInstOrigin::pre_opt(FuncRef::from_u32(0), InstId::from_u32(7)),
        SonatinaPostOptOriginSource::SameInstId(pre_opt),
    );
}

#[test]
#[should_panic(
    expected = "same-inst-id post-opt origins must reference the matching pre-opt function and instruction ID"
)]
fn sonatina_post_opt_records_reject_same_inst_id_source_from_another_function() {
    let pre_opt = SonatinaInstOriginRecord::new(
        SonatinaInstOrigin::pre_opt(FuncRef::from_u32(0), InstId::from_u32(7)),
        SonatinaOriginSource::Synthetic(SonatinaSyntheticOrigin::Prologue),
    );

    SonatinaPostOptOriginRecord::new(
        SonatinaInstOrigin::post_opt(FuncRef::from_u32(1), InstId::from_u32(7)),
        SonatinaPostOptOriginSource::SameInstId(pre_opt),
    );
}

#[test]
#[should_panic(
    expected = "same-inst-id post-opt origins must reference the matching pre-opt function and instruction ID"
)]
fn sonatina_post_opt_records_reject_same_inst_id_source_from_another_inst() {
    let pre_opt = SonatinaInstOriginRecord::new(
        SonatinaInstOrigin::pre_opt(FuncRef::from_u32(0), InstId::from_u32(7)),
        SonatinaOriginSource::Synthetic(SonatinaSyntheticOrigin::Prologue),
    );

    SonatinaPostOptOriginRecord::new(
        SonatinaInstOrigin::post_opt(FuncRef::from_u32(0), InstId::from_u32(8)),
        SonatinaPostOptOriginSource::SameInstId(pre_opt),
    );
}

#[test]
#[should_panic(
    expected = "backend-prepared Sonatina origin records must use backend-prepared instruction origins"
)]
fn sonatina_backend_prepared_records_reject_post_opt_instruction_origins() {
    SonatinaBackendPreparedOriginRecord::new(
        SonatinaInstOrigin::post_opt(FuncRef::from_u32(0), InstId::from_u32(7)),
        SonatinaBackendPreparedOriginSource::MissingPostOptSnapshotRecord,
    );
}

#[test]
#[should_panic(
    expected = "post-opt Sonatina function origins cannot contain records from another function"
)]
fn sonatina_post_opt_function_origins_reject_wrong_function_records() {
    let pre_opt = SonatinaInstOriginRecord::new(
        SonatinaInstOrigin::pre_opt(FuncRef::from_u32(0), InstId::from_u32(7)),
        SonatinaOriginSource::Synthetic(SonatinaSyntheticOrigin::Prologue),
    );
    let post_opt = SonatinaPostOptOriginRecord::new(
        SonatinaInstOrigin::post_opt(FuncRef::from_u32(0), InstId::from_u32(7)),
        SonatinaPostOptOriginSource::SameInstId(pre_opt),
    );

    SonatinaPostOptFunctionOrigins::new(FuncRef::from_u32(1), vec![post_opt]);
}

#[test]
#[should_panic(
    expected = "post-opt Sonatina function origins cannot contain duplicate instruction origins"
)]
fn sonatina_post_opt_function_origins_reject_duplicate_instruction_records() {
    let post_opt = SonatinaPostOptOriginRecord::new(
        SonatinaInstOrigin::post_opt(FuncRef::from_u32(0), InstId::from_u32(7)),
        SonatinaPostOptOriginSource::CreatedOrUnmatchedAfterPreOptSnapshot,
    );

    SonatinaPostOptFunctionOrigins::new(FuncRef::from_u32(0), vec![post_opt, post_opt]);
}
