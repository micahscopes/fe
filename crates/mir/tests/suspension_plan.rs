use common::InputDb;
use driver::DriverDataBase;
use fe_mir::{
    Layout, RExpr, RStmt, RuntimeFunctionOwner, RuntimeSuspensionCause,
    derive_runtime_resumable_plans, derive_runtime_suspension_points,
    materialize_runtime_resumable_machine, runtime::build_wasm_runtime_package_for_entry,
    verify_runtime_body,
};
use hir::analysis::ty::ty_check::BodyOwner;
use url::Url;

const SOURCE: &str = r#"
use core::pending::{Pending, TaskOutcome}
use std::host::raw::suspend
use std::wasm::WasmBackend

pub fn resumable(
    _ pending: own Pending<WasmBackend, u32>,
    _ kept: u32,
    _ dead: u32,
) -> u32 {
    let outcome: TaskOutcome<u32, u32> = suspend(pending)
    match outcome {
        TaskOutcome::Success(value) => value + kept
        TaskOutcome::Failure(error) => error + kept
        TaskOutcome::Cancelled => kept
    }
}
"#;

const EFFECT_SOURCE: &str = r#"
use core::pending::{Pending, Suspend, TaskOutcome}
use std::host::Resumable
use std::wasm::WasmBackend

fn wait_through_effect(
    _ pending: own Pending<WasmBackend, u32>,
) -> TaskOutcome<u32, u32>
    uses (s: Suspend<WasmBackend, u32>)
{
    s.suspend(pending)
}

fn wait_through_helper(
    _ pending: own Pending<WasmBackend, u32>,
) -> TaskOutcome<u32, u32>
    uses (s: Suspend<WasmBackend, u32>)
{
    wait_through_effect(pending)
}

pub fn effect_resumable(
    _ pending: own Pending<WasmBackend, u32>,
    _ kept: u32,
    _ dead: u32,
) -> u32 {
    with (Suspend<WasmBackend, u32> = Resumable {}) {
        let outcome = wait_through_helper(pending)
        match outcome {
            TaskOutcome::Success(value) => value + kept
            TaskOutcome::Failure(error) => error + kept
            TaskOutcome::Cancelled => kept
        }
    }
}
"#;

const BRANCHED_EFFECT_SOURCE: &str = r#"
use core::pending::{Pending, Suspend, TaskOutcome}
use std::host::Resumable
use std::wasm::WasmBackend

fn wait_on_either_branch(
    _ pending: own Pending<WasmBackend, u32>,
    _ choose_first: bool,
) -> TaskOutcome<u32, u32>
    uses (s: Suspend<WasmBackend, u32>)
{
    if choose_first {
        s.suspend(pending)
    } else {
        s.suspend(pending)
    }
}

pub fn branched_resumable(
    _ pending: own Pending<WasmBackend, u32>,
    _ choose_first: bool,
    _ kept: u32,
) -> u32 {
    with (Suspend<WasmBackend, u32> = Resumable {}) {
        let outcome = wait_on_either_branch(pending, choose_first)
        match outcome {
            TaskOutcome::Success(value) => value + kept
            TaskOutcome::Failure(error) => error + kept
            TaskOutcome::Cancelled => kept
        }
    }
}
"#;

#[test]
fn nominal_suspend_derives_state_pending_delivery_and_exact_live_values() {
    let mut db = DriverDataBase::default();
    let url = Url::parse("file:///compiler_derived_suspension.fe").unwrap();
    db.workspace()
        .touch(&mut db, url.clone(), Some(SOURCE.to_owned()));
    let file = db.workspace().get(&db, &url).unwrap();
    let top_mod = db.top_mod(file);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(
        diagnostics.is_empty(),
        "fixture diagnostics:\n{diagnostics}"
    );

    let package = build_wasm_runtime_package_for_entry(&db, top_mod, "resumable").unwrap();
    let function = package
        .functions(&db)
        .into_iter()
        .find(|function| {
            let RuntimeFunctionOwner::Semantic(semantic) = function.owner(&db) else {
                return false;
            };
            matches!(
                semantic.key(&db).owner(&db),
                BodyOwner::Func(func)
                    if func.name(&db).to_opt().is_some_and(|name| name.data(&db) == "resumable")
            )
        })
        .expect("resumable runtime function");
    let body = function.instance(&db).body(&db);
    let points = derive_runtime_suspension_points(&db, &body).unwrap();
    let [point] = points.as_slice() else {
        panic!("expected exactly one compiler-derived suspension point: {points:?}");
    };
    assert_eq!(point.continuation_state, 1);

    let params = &body.signature.params;
    assert_eq!(params.len(), 3);
    assert_eq!(
        point.cause,
        RuntimeSuspensionCause::Effect {
            pending: params[0].local
        }
    );
    assert_eq!(point.live_values.as_ref(), [params[1].local]);
    assert_ne!(point.delivery, params[0].local);
    assert!(
        !point.live_values.contains(&params[2].local),
        "an unused parameter must not inflate the continuation frame"
    );

    let plans = derive_runtime_resumable_plans(&db, package).unwrap();
    let plan = plans
        .iter()
        .find(|plan| plan.body == function.instance(&db))
        .unwrap();
    let machine = materialize_runtime_resumable_machine(&db, plan).unwrap();
    let Layout::Enum(step) = machine.step_layout.data(&db) else {
        panic!("the machine result must be a compiler-derived payload enum");
    };
    assert_eq!(step.variants.len(), 2);
    assert_eq!(step.variants[0].name, "Complete");
    assert_eq!(step.variants[1].name, "Suspended1");
    assert_eq!(step.variants[1].fields.len(), 2);
    assert_eq!(machine.continuations.len(), 1);
    assert_eq!(
        machine.continuations[0]
            .body
            .signature
            .params
            .iter()
            .map(|param| param.local)
            .collect::<Vec<_>>(),
        vec![params[1].local, point.delivery],
        "re-entry receives the exact live frame followed by the typed delivery"
    );
    let view: &dyn fe_mir::MirDb = &db;
    for segment in std::iter::once(&machine.entry).chain(machine.continuations.iter()) {
        verify_runtime_body(&db, &view, &segment.body).unwrap_or_else(|error| {
            panic!("materialized segment failed MIR verification: {error:?}")
        });
        assert!(
            !segment.body.blocks.iter().any(|block| {
                block.stmts.iter().any(|statement| {
                    matches!(
                        statement,
                        RStmt::Assign {
                            expr: RExpr::Call { callee, .. },
                            ..
                        } if fe_mir::runtime_control_effect_kind(&db, *callee).is_some()
                    )
                })
            }),
            "materialized segments must consume, not import, the nominal suspend operation"
        );
    }
}

#[test]
fn suspension_propagates_through_effect_provider_and_ordinary_helpers() {
    let mut db = DriverDataBase::default();
    let url = Url::parse("file:///transitive_effect_suspension.fe").unwrap();
    db.workspace()
        .touch(&mut db, url.clone(), Some(EFFECT_SOURCE.to_owned()));
    let file = db.workspace().get(&db, &url).unwrap();
    let top_mod = db.top_mod(file);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(
        diagnostics.is_empty(),
        "fixture diagnostics:\n{diagnostics}"
    );

    let package = build_wasm_runtime_package_for_entry(&db, top_mod, "effect_resumable").unwrap();
    let plans = derive_runtime_resumable_plans(&db, package).unwrap();
    let name = |plan: &fe_mir::RuntimeResumableBodyPlan<'_>| {
        package
            .functions(&db)
            .into_iter()
            .find(|function| function.instance(&db) == plan.body)
            .and_then(|function| match function.owner(&db) {
                RuntimeFunctionOwner::Semantic(semantic) => match semantic.key(&db).owner(&db) {
                    BodyOwner::Func(func) => func
                        .name(&db)
                        .to_opt()
                        .map(|name| name.data(&db).to_owned()),
                    _ => None,
                },
                RuntimeFunctionOwner::Synthetic(_) => None,
            })
            .unwrap_or_else(|| "<synthetic>".to_owned())
    };

    let names = plans.iter().map(name).collect::<Vec<_>>();
    assert!(names.iter().any(|name| name == "suspend"));
    assert!(names.iter().any(|name| name == "wait_through_effect"));
    assert!(names.iter().any(|name| name == "wait_through_helper"));
    assert!(names.iter().any(|name| name == "effect_resumable"));

    let provider = plans.iter().find(|plan| name(plan) == "suspend").unwrap();
    assert!(
        provider
            .points
            .iter()
            .any(|point| matches!(point.cause, RuntimeSuspensionCause::Effect { .. }))
            || !provider.suspending_tails.is_empty(),
        "the provider must retain its direct nominal suspend boundary even when it is tail-lowered"
    );

    let root = plans
        .iter()
        .find(|plan| name(plan) == "effect_resumable")
        .unwrap();
    let [point] = root.points.as_ref() else {
        panic!("root should contain one flattened nominal suspension: {root:?}");
    };
    assert!(
        matches!(point.cause, RuntimeSuspensionCause::Effect { .. }),
        "ordinary helpers and the selected effect provider must expand before liveness"
    );
    let root_body = &root.flattened_body;
    assert_eq!(
        point.live_values.as_ref(),
        [root_body.signature.params[1].local]
    );
    assert_eq!(root.frame.len(), 1);
    assert_eq!(root.frame[0].local, root_body.signature.params[1].local);
    assert_eq!(root.frame[0].class, root_body.signature.params[1].class);
    assert!(
        !point
            .live_values
            .contains(&root_body.signature.params[2].local)
    );

    let machine = materialize_runtime_resumable_machine(&db, root).unwrap();
    let view: &dyn fe_mir::MirDb = &db;
    for segment in std::iter::once(&machine.entry).chain(machine.continuations.iter()) {
        verify_runtime_body(&db, &view, &segment.body).unwrap_or_else(|error| {
            panic!("flattened transitive segment failed MIR verification: {error:?}")
        });
        assert!(
            !segment.body.blocks.iter().any(|block| {
                block.stmts.iter().any(|statement| {
                    matches!(
                        statement,
                        RStmt::Assign {
                            expr: RExpr::Call { callee, .. },
                            ..
                        } if fe_mir::runtime_control_effect_kind(&db, *callee).is_some()
                    )
                })
            }),
            "the transitive machine must consume its provider's nominal suspension"
        );
    }
}

#[test]
fn branched_provider_cfg_flattens_both_suspension_paths() {
    let mut db = DriverDataBase::default();
    let url = Url::parse("file:///branched_effect_suspension.fe").unwrap();
    db.workspace().touch(
        &mut db,
        url.clone(),
        Some(BRANCHED_EFFECT_SOURCE.to_owned()),
    );
    let file = db.workspace().get(&db, &url).unwrap();
    let top_mod = db.top_mod(file);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(
        diagnostics.is_empty(),
        "fixture diagnostics:\n{diagnostics}"
    );

    let package = build_wasm_runtime_package_for_entry(&db, top_mod, "branched_resumable").unwrap();
    let plans = derive_runtime_resumable_plans(&db, package).unwrap();
    let root = plans
        .iter()
        .find(|plan| {
            package
                .functions(&db)
                .into_iter()
                .find(|function| function.instance(&db) == plan.body)
                .is_some_and(|function| {
                    let RuntimeFunctionOwner::Semantic(semantic) = function.owner(&db) else {
                        return false;
                    };
                    matches!(
                        semantic.key(&db).owner(&db),
                        BodyOwner::Func(func)
                            if func.name(&db).to_opt().is_some_and(
                                |name| name.data(&db) == "branched_resumable"
                            )
                    )
                })
        })
        .expect("branched resumable root");
    assert_eq!(root.points.len(), 2);
    assert!(
        root.points
            .iter()
            .all(|point| matches!(point.cause, RuntimeSuspensionCause::Effect { .. })),
        "both branch-local provider calls must become direct nominal leaves"
    );
    let kept = root.flattened_body.signature.params[2].local;
    assert!(
        root.points
            .iter()
            .all(|point| point.live_values.contains(&kept)),
        "the caller's used value must survive either branch"
    );

    let machine = materialize_runtime_resumable_machine(&db, root).unwrap();
    assert_eq!(machine.continuations.len(), 2);
    let view: &dyn fe_mir::MirDb = &db;
    for segment in std::iter::once(&machine.entry).chain(machine.continuations.iter()) {
        verify_runtime_body(&db, &view, &segment.body).unwrap_or_else(|error| {
            panic!("branched materialized segment failed MIR verification: {error:?}")
        });
    }
}

#[test]
fn an_ordinary_function_named_suspend_has_no_control_semantics() {
    let mut db = DriverDataBase::default();
    let url = Url::parse("file:///ordinary_suspend_name.fe").unwrap();
    db.workspace().touch(
        &mut db,
        url.clone(),
        Some(
            r#"
fn suspend(_ value: u32) -> u32 { value + 1 }
pub fn entry(_ value: u32) -> u32 { suspend(value) }
"#
            .to_owned(),
        ),
    );
    let file = db.workspace().get(&db, &url).unwrap();
    let top_mod = db.top_mod(file);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(
        diagnostics.is_empty(),
        "fixture diagnostics:\n{diagnostics}"
    );

    let package = build_wasm_runtime_package_for_entry(&db, top_mod, "entry").unwrap();
    assert!(
        derive_runtime_resumable_plans(&db, package)
            .unwrap()
            .is_empty(),
        "control semantics come from the nominal std item, never its spelling"
    );
}
