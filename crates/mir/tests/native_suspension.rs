use common::{InputDb, stdlib::HasBuiltinCore};
use driver::DriverDataBase;
use fe_mir::{
    RuntimeInstanceKey,
    instance::{RuntimeInstanceSource, get_or_build_runtime_instance},
    runtime::suspension::{ScalarCapture, compose_native_scalar_task, prepare_native_scalar_cuts},
};
use hir::analysis::{
    semantic::{get_or_build_semantic_instance, root_semantic_instance_key},
    ty::ty_check::BodyOwner,
};
use url::Url;

#[test]
fn composed_native_cut_retains_the_callers_scalar_and_delivery_origin() {
    let mut db = DriverDataBase::default();
    db.initialize_builtin_core();
    let file = db.workspace().touch(
        &mut db,
        Url::parse("file:///native_suspension.fe").unwrap(),
        Some(
            r#"
use core::task::{Native, Pending, Suspend, TaskOutcome}
// A trusted fixture source of a pending identity. This fixture tests MIR
// construction, not a host ABI or runtime delivery.
extern { unsafe fn fixture_pending() -> Pending<Native, u64> }
fn hop(_ cap: mut Native, _ pending: own Pending<Native, u64>) -> TaskOutcome<u64, u64> {
    cap.suspend(pending)
}
fn entry(_ native: own Native) -> u64 {
    var cap = native
    let pending = unsafe { fixture_pending() }
    let keep: u64 = 5
    match hop(mut cap, pending) {
        TaskOutcome::Success(value) => keep + value
        TaskOutcome::Failure(error) => error
        TaskOutcome::Cancelled => 0
    }
}
"#
            .into(),
        ),
    );
    let module = db.top_mod(file);
    let func = module
        .all_funcs(&db)
        .iter()
        .copied()
        .find(|func| {
            func.name(&db)
                .to_opt()
                .is_some_and(|name| name.data(&db) == "entry")
        })
        .expect("entry function");
    let semantic_key =
        root_semantic_instance_key(&db, BodyOwner::Func(func, None)).expect("closed entry");
    let semantic = SemanticInstance::new(&db, semantic_key);
    let runtime_key =
        RuntimeInstanceKey::new(&db, RuntimeInstanceSource::Semantic(semantic), vec![]);
    let instance = get_or_build_runtime_instance(&db, runtime_key);
    assert!(
        prepare_native_scalar_cuts(&db, instance.body(&db)).is_err(),
        "ordinary suspending helper calls must be composed first"
    );
    let body =
        compose_native_scalar_task(&db, instance).expect("compose native adapter and helper");
    let cuts = prepare_native_scalar_cuts(&db, &body).expect("owned scalar cut");
    assert_eq!(cuts.len(), 1);
    let cut = &cuts[0];
    assert!(
        !cut.captures.is_empty(),
        "the caller's keep value must survive delivery"
    );
    for capture in &cut.captures {
        let ScalarCapture::Value { local, .. } = capture;
        assert_ne!(*local, cut.outcome, "delivery defines its own result");
    }
    assert_eq!(
        cut.origin,
        body.stmt_origins[cut.block.as_u32() as usize][cut.statement]
    );
}
