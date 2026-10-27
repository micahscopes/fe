use common::{InputDb, stdlib::HasBuiltinCore};
use cranelift_entity::EntityRef;
use driver::DriverDataBase;
use fe_mir::{
    RuntimeInstance, RuntimeInstanceKey,
    instance::{RuntimeInstanceSource, get_or_build_runtime_instance},
    runtime::{
        ConstScalar, RBlock, RBlockId, RExpr, RLocal, RLocalId, RStmt, RTerminator, RuntimeBody,
        RuntimeCarrier, RuntimeClass, RuntimeLocalRoot, ScalarClass, ScalarRepr, ScalarRole,
        suspension::{
            NativeScalarTaskPlan, ScalarCapture, build_native_scalar_segments,
            check_native_step_protocol, compose_native_scalar_task, plan_native_scalar_abi,
            plan_native_scalar_task, prepare_native_scalar_cuts,
        },
    },
};
use hir::{
    analysis::{
        semantic::{SemanticInstance, root_semantic_instance_key},
        ty::{ty_check::BodyOwner, ty_def::TyId},
    },
    hir_def::TopLevelMod,
};
use url::Url;

// A trusted fixture source of a pending identity. This fixture tests MIR
// construction, not a host ABI or runtime delivery.
const FIXTURE: &str = r#"
use core::task::{Native, Pending, Suspend, TaskOutcome}
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
fn entry_twice(_ native: own Native) -> u64 {
    var cap = native
    let first = unsafe { fixture_pending() }
    let keep: u64 = 5
    let subtotal = match hop(mut cap, first) {
        TaskOutcome::Success(value) => keep + value
        TaskOutcome::Failure(error) => error
        TaskOutcome::Cancelled => 0
    }
    let second = unsafe { fixture_pending() }
    match hop(mut cap, second) {
        TaskOutcome::Success(value) => subtotal + value
        TaskOutcome::Failure(error) => error
        TaskOutcome::Cancelled => 0
    }
}
fn entry_loop(_ native: own Native) -> u64 {
    var cap = native
    var total: u64 = 5
    while total < 20 {
        let pending = unsafe { fixture_pending() }
        total = match hop(mut cap, pending) {
            TaskOutcome::Success(value) => total + value
            TaskOutcome::Failure(error) => error
            TaskOutcome::Cancelled => 20
        }
    }
    total
}
"#;

#[test]
fn composed_native_cut_retains_the_callers_scalar_and_delivery_origin() {
    let mut db = DriverDataBase::default();
    db.initialize_builtin_core();
    let file = db.workspace().touch(
        &mut db,
        Url::parse("file:///native_suspension.fe").unwrap(),
        Some(FIXTURE.into()),
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
    let package = fe_mir::runtime::build_native_task_runtime_package(&db, module, plan)
        .expect("generated segment package");
    assert!(
        !package
            .functions(&db)
            .iter()
            .any(|function| function.instance(&db) == instance)
    );
    for segment in &generated {
        assert!(
            package
                .functions(&db)
                .iter()
                .any(|function| function.instance(&db) == *segment)
        );
    }
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
    let twice = module
        .all_funcs(&db)
        .iter()
        .copied()
        .find(|func| {
            func.name(&db)
                .to_opt()
                .is_some_and(|name| name.data(&db) == "entry_twice")
        })
        .unwrap();
    let semantic = SemanticInstance::new(
        &db,
        root_semantic_instance_key(&db, BodyOwner::Func(twice, None)).unwrap(),
    );
    let twice = get_or_build_runtime_instance(
        &db,
        RuntimeInstanceKey::new(&db, RuntimeInstanceSource::Semantic(semantic), vec![]),
    );
    let twice_plan = plan_native_scalar_task(&db, twice).expect("two sequential cuts");
    let twice_segments = twice_plan.instances(&db);
    assert_eq!(twice_segments.len(), 3);
    for segment in twice_segments {
        assert!(
            prepare_native_scalar_cuts(&db, segment.body(&db))
                .unwrap()
                .is_empty()
        );
    }
    let loop_func = module
        .all_funcs(&db)
        .iter()
        .copied()
        .find(|func| {
            func.name(&db)
                .to_opt()
                .is_some_and(|name| name.data(&db) == "entry_loop")
        })
        .unwrap();
    let semantic = SemanticInstance::new(
        &db,
        root_semantic_instance_key(&db, BodyOwner::Func(loop_func, None)).unwrap(),
    );
    let loop_instance = get_or_build_runtime_instance(
        &db,
        RuntimeInstanceKey::new(&db, RuntimeInstanceSource::Semantic(semantic), vec![]),
    );
    let loop_plan = plan_native_scalar_task(&db, loop_instance).expect("loop-carried scalar cut");
    assert_eq!(loop_plan.segments(&db).len(), 2);
    // The resume suffix must retain the loop backedge and reach the same cut
    // again, rather than treating the cut's block as permanently skipped.
    let resume = &loop_plan.segments(&db)[1];
    assert!(
        resume
            .blocks
            .iter()
            .flat_map(|block| &block.stmts)
            .any(|stmt| matches!(stmt,
                RStmt::Assign { expr: RExpr::EnumMake { layout, variant, .. }, .. }
                    if *layout == loop_plan.step_layout(&db) && variant.index == 1
            ))
    );
}

fn fixture_instance<'db>(
    db: &'db DriverDataBase,
    module: TopLevelMod<'db>,
    name: &str,
) -> RuntimeInstance<'db> {
    let func = module
        .all_funcs(db)
        .iter()
        .copied()
        .find(|func| {
            func.name(db)
                .to_opt()
                .is_some_and(|func| func.data(db) == name)
        })
        .unwrap_or_else(|| panic!("fixture function {name}"));
    let semantic = SemanticInstance::new(
        db,
        root_semantic_instance_key(db, BodyOwner::Func(func, None)).expect("closed entry"),
    );
    get_or_build_runtime_instance(
        db,
        RuntimeInstanceKey::new(db, RuntimeInstanceSource::Semantic(semantic), vec![]),
    )
}

#[test]
fn step_variants_and_segment_captures_must_agree_before_a_frame_is_planned() {
    let mut db = DriverDataBase::default();
    db.initialize_builtin_core();
    let file = db.workspace().touch(
        &mut db,
        Url::parse("file:///native_suspension.fe").unwrap(),
        Some(FIXTURE.into()),
    );
    let module = db.top_mod(file);
    for name in ["entry", "entry_twice", "entry_loop"] {
        let plan = plan_native_scalar_task(&db, fixture_instance(&db, module, name)).unwrap();
        let segments = plan.segments(&db);
        check_native_step_protocol(&db, plan.step_layout(&db), &segments[0], &segments[1..])
            .unwrap_or_else(|error| panic!("{name}: {error:?}"));
        plan_native_scalar_abi(&db, plan).unwrap_or_else(|error| panic!("{name}: {error:?}"));
    }

    // The adapters read resume arguments from the frame by index, and the
    // frame is sized from the Step variants. A plan whose segments disagree
    // with the variants must be refused rather than read out of bounds.
    let plan = plan_native_scalar_task(&db, fixture_instance(&db, module, "entry")).unwrap();
    let with = |segments| {
        NativeScalarTaskPlan::new(&db, plan.source(&db), plan.step_layout(&db), segments)
    };
    let mut extra_capture = plan.segments(&db).clone();
    let params = &mut extra_capture[1].signature.params;
    assert!(params.len() >= 2, "the fixture retains a scalar");
    params.insert(0, params[0].clone());
    assert!(
        plan_native_scalar_abi(&db, with(extra_capture)).is_err(),
        "a resume capture beyond its variant's fields"
    );
    let mut missing_capture = plan.segments(&db).clone();
    missing_capture[1].signature.params.remove(0);
    assert!(
        plan_native_scalar_abi(&db, with(missing_capture)).is_err(),
        "a variant field no resume takes"
    );
    let mut missing_resume = plan.segments(&db).clone();
    missing_resume.truncate(1);
    assert!(
        plan_native_scalar_abi(&db, with(missing_resume)).is_err(),
        "a Step variant without a segment"
    );
}

/// Rewrite `body` so that a fresh scalar `x` is assigned before the first
/// suspension, either on one branch of a new entry test or, for the control,
/// before it. `x` is also read after the cut, so it is live across it.
fn with_scalar_defined<'db>(
    db: &'db DriverDataBase,
    body: &RuntimeBody<'db>,
    cut_block: RBlockId,
    on_every_path: bool,
) -> (RuntimeBody<'db>, RLocalId) {
    let mut body = body.clone();
    let origin = body.terminator_origins[0];
    let word = ScalarClass {
        repr: ScalarRepr::Int {
            bits: 64,
            signed: false,
        },
        role: ScalarRole::Plain,
    };
    let flag = ScalarClass {
        repr: ScalarRepr::Bool,
        role: ScalarRole::Plain,
    };
    let mut local = |class: ScalarClass<'db>| {
        body.locals.push(RLocal {
            semantic_ty: TyId::unit(db),
            carrier: RuntimeCarrier::Value(RuntimeClass::Scalar(class)),
            root: RuntimeLocalRoot::None,
        });
        RLocalId::new(body.locals.len() - 1)
    };
    let (x, sink, cond) = (local(word.clone()), local(word), local(flag));
    let define = RStmt::Assign {
        dst: x,
        expr: RExpr::ConstScalar(ConstScalar::Int {
            bits: 64,
            signed: false,
            words: vec![7],
        }),
    };
    let test = RStmt::Assign {
        dst: cond,
        expr: RExpr::ConstScalar(ConstScalar::Bool(true)),
    };
    // The old entry block moves to the end so that block zero stays the entry.
    let moved = RBlockId::new(body.blocks.len());
    let define_block = RBlockId::new(body.blocks.len() + 1);
    let old_entry = body.blocks[0].clone();
    let old_stmt_origins = body.stmt_origins[0].clone();
    let old_terminator_origin = body.terminator_origins[0];
    body.blocks.push(old_entry);
    body.stmt_origins.push(old_stmt_origins);
    body.terminator_origins.push(old_terminator_origin);
    body.blocks.push(RBlock {
        stmts: vec![define.clone()],
        terminator: RTerminator::Goto(moved),
    });
    body.stmt_origins.push(vec![origin]);
    body.terminator_origins.push(origin);
    body.blocks[0] = if on_every_path {
        RBlock {
            stmts: vec![define, test],
            terminator: RTerminator::Goto(moved),
        }
    } else {
        RBlock {
            stmts: vec![test],
            terminator: RTerminator::Branch {
                cond,
                then_bb: define_block,
                else_bb: moved,
            },
        }
    };
    body.stmt_origins[0] = vec![origin; body.blocks[0].stmts.len()];
    let cut_block = if cut_block.index() == 0 {
        moved
    } else {
        cut_block
    };
    body.blocks[cut_block.index()].stmts.push(RStmt::Assign {
        dst: sink,
        expr: RExpr::Use(x),
    });
    body.stmt_origins[cut_block.index()].push(origin);
    (body, x)
}

#[test]
fn a_scalar_defined_on_only_one_path_to_a_cut_is_not_captured() {
    let mut db = DriverDataBase::default();
    db.initialize_builtin_core();
    let file = db.workspace().touch(
        &mut db,
        Url::parse("file:///native_suspension.fe").unwrap(),
        Some(FIXTURE.into()),
    );
    let module = db.top_mod(file);
    let instance = fixture_instance(&db, module, "entry");
    let composed = compose_native_scalar_task(&db, instance).unwrap();
    let cut_block = prepare_native_scalar_cuts(&db, &composed).unwrap()[0].block;

    // Control: the same extra scalar assigned before the branch is captured.
    let (always, x) = with_scalar_defined(&db, &composed, cut_block, true);
    let cuts = prepare_native_scalar_cuts(&db, &always).expect("defined on every path");
    assert!(cuts[0].captures.iter().any(|capture| {
        let ScalarCapture::Value { local, .. } = capture;
        *local == x
    }));
    build_native_scalar_segments(&db, &always).expect("segments for the control");

    // A scalar assigned on one branch only has no value on the other path, so
    // the entry segment would store an undefined value in the frame.
    let (sometimes, x) = with_scalar_defined(&db, &composed, cut_block, false);
    let error = prepare_native_scalar_cuts(&db, &sometimes)
        .expect_err("a scalar undefined on one path cannot be captured");
    assert!(
        format!("{error:?}").contains(&format!("{x:?}")),
        "{error:?}"
    );
    assert!(build_native_scalar_segments(&db, &sometimes).is_err());
}
