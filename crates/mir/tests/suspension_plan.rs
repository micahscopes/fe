use common::InputDb;
use driver::DriverDataBase;
use fe_mir::{
    RuntimeFunctionOwner, RuntimeSuspensionCause, derive_runtime_resumable_plans,
    derive_runtime_suspension_points, runtime::build_wasm_runtime_package_for_entry,
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
        panic!("root should suspend at its ordinary helper call: {root:?}");
    };
    assert!(matches!(point.cause, RuntimeSuspensionCause::Callee { .. }));
    let root_body = root.body.body(&db);
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
