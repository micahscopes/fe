use fe_hir::test_db::HirAnalysisTestDb;
use fe_hir::{
    analysis::{
        semantic::{CtfeError, SemConstScalar, SemConstValue, eval_body_owner_const},
        ty::ty_check::BodyOwner,
    },
    hir_def::Partial,
};
use num_traits::ToPrimitive;

fn eval_named(
    source: &str,
    name: &str,
) -> (
    &'static HirAnalysisTestDb,
    Result<fe_hir::analysis::semantic::SemConstId<'static>, CtfeError<'static>>,
) {
    let db = Box::leak(Box::new(HirAnalysisTestDb::default()));
    let file = db.new_stand_alone("nested_ctfe_memo.fe".into(), source);
    let (top_mod, _) = db.top_mod(file);
    let func = top_mod
        .all_funcs(db)
        .iter()
        .find(|func| matches!(func.name(db), Partial::Present(found) if found.data(db) == name))
        .copied()
        .expect("missing requested function");
    (
        db,
        eval_body_owner_const(db, BodyOwner::Func(func), Vec::new()),
    )
}

fn has_recursion_limit(error: &CtfeError<'_>) -> bool {
    match error {
        CtfeError::RecursionLimitExceeded { .. } => true,
        CtfeError::CalleeError { source, .. } => has_recursion_limit(source),
        _ => false,
    }
}

#[test]
fn repeated_concrete_nested_call_preserves_value() {
    let (db, value) = eval_named(
        r#"
const fn scan(_ seed: usize) -> usize {
    let mut i: usize = 0
    let mut value: usize = seed
    while i < 80 {
        value = value + i
        i = i + 1
    }
    value
}
const fn root() -> usize { scan(7) + scan(7) }
"#,
        "root",
    );
    let value = value.expect("repeated concrete call should evaluate");
    let SemConstValue::Scalar {
        value: SemConstScalar::Int { value },
        ..
    } = value.value(db)
    else {
        panic!("expected integer")
    };
    assert_eq!(value.to_usize(), Some(2 * (7 + (0..80).sum::<usize>())));
}

#[test]
fn changing_argument_recursion_still_obeys_recursion_budget() {
    let (_, error) = eval_named(
        r#"
const fn descend(_ n: usize) -> usize {
    if n == 0 { 0 } else { descend(n - 1) }
}
const fn root() -> usize { descend(100) }
"#,
        "root",
    );
    let error = error.expect_err("deep changing-argument recursion must fail");
    assert!(
        has_recursion_limit(&error),
        "unexpected CTFE error: {error:?}"
    );
}

#[test]
fn same_key_recursion_falls_back_to_machine_cycle_guard() {
    let (_, error) = eval_named(
        r#"
const fn forever() -> usize { forever() }
const fn root() -> usize { forever() }
"#,
        "root",
    );
    let error = error.expect_err("same-key recursion must fail");
    assert!(
        has_recursion_limit(&error),
        "unexpected CTFE error: {error:?}"
    );
}

#[test]
fn failed_memo_probe_preserves_inline_callee_error() {
    let (_, error) = eval_named(
        r#"
const fn checked(_ value: usize) -> usize {
    let values: [usize; 1] = [7]
    values[value]
}
const fn root() -> usize { checked(1) }
"#,
        "root",
    );
    let error = error.expect_err("assertion must fail");
    assert!(
        matches!(error, CtfeError::CalleeError { ref source, .. }
        if matches!(**source, CtfeError::OutOfBounds { .. })),
        "{error:?}"
    );
}

#[test]
fn wrapper_around_deferred_extern_stays_type_level() {
    let (db, value) = eval_named(
        r#"
extern { const fn unknown(_: usize) -> usize }
const fn wrapper(_ value: usize) -> usize { unknown(value) }
const fn root() -> usize { wrapper(7) }
"#,
        "root",
    );
    let value = value.expect("deferred const call should remain representable");
    assert!(
        matches!(value.value(db), SemConstValue::TypeLevel { .. }),
        "memoization must not concretize deferred extern result: {:?}",
        value.value(db),
    );
}
