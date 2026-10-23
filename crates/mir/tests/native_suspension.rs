use common::{InputDb, stdlib::HasBuiltinCore};
use driver::DriverDataBase;
use fe_mir::{
    RuntimeInstanceKey,
    instance::{RuntimeInstanceSource, get_or_build_runtime_instance},
    runtime::{
        RExpr, RStmt, RuntimeClass,
        suspension::{
            ScalarCapture, build_native_scalar_segments, compose_native_scalar_task,
            plan_native_scalar_task, prepare_native_scalar_cuts,
        },
    },
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
    let segments = build_native_scalar_segments(&db, &body).expect("typed entry and resume");
    assert_eq!(segments.resumes.len(), 1);
    let resume = &segments.resumes[0];
    assert_eq!(resume.signature.params.len(), cut.captures.len() + 1);
    assert_eq!(resume.signature.params.last().unwrap().local, cut.outcome);
    assert_eq!(segments.entry.signature.params, body.signature.params);
    for segment in std::iter::once(&segments.entry).chain(&segments.resumes) {
        assert_eq!(
            segment.signature.ret,
            Some(RuntimeClass::AggregateValue {
                layout: segments.step_layout
            })
        );
        assert!(
            prepare_native_scalar_cuts(&db, segment).unwrap().is_empty(),
            "segments cannot leave raw cuts behind"
        );
        assert!(segment.blocks.iter().flat_map(|block| &block.stmts).any(|stmt| matches!(stmt,
            RStmt::Assign { expr: RExpr::EnumMake { layout, .. }, .. } if *layout == segments.step_layout
        )));
    }
    let plan = plan_native_scalar_task(&db, instance).expect("registered task plan");
    let generated = plan.instances(&db);
    assert_eq!(generated.len(), 2);
    assert_ne!(generated[0], generated[1]);
    for (index, generated) in generated.iter().enumerate() {
        assert_ne!(*generated, instance);
        let registered = generated.body(&db);
        assert_eq!(registered.owner, *generated);
        assert_eq!(registered.key, generated.key(&db));
        assert_eq!(
            registered.signature,
            generated.interface_signature(&db).clone()
        );
        assert_eq!(registered.blocks, plan.segments(&db)[index].blocks);
        assert_eq!(
            registered.stmt_origins,
            plan.segments(&db)[index].stmt_origins
        );
    }
    assert_ne!(
        fe_mir::runtime::runtime_instance_stable_key(&db, generated[0]),
        fe_mir::runtime::runtime_instance_stable_key(&db, generated[1]),
    );
}
