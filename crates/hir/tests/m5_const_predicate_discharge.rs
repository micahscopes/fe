//! FCO-M5: obligation-level discharge of `where`-clause const predicates.
//!
//! This is the end-to-end "platform fact" demo: a backend fact expressed as an
//! associated const (`B::WORD_BITS`) appears in a `where` predicate, becomes a
//! first-class obligation at the call site, and is discharged by CTFE under the
//! call's type substitution — never inside the trait solver. The satisfying
//! backend compiles and records evidence; the non-satisfying backend fails with
//! a named diagnostic; a generic caller that forwards its own type leaves the
//! predicate as its own assumption (no false discharge, no false error).

use common::diagnostics::{CompleteDiagnostic, cmp_complete_diagnostics};
use fe_hir::analysis::ty::ty_check::{DischargeRoute, check_func_body};
use fe_hir::hir_def::{Expr, Func, Partial, TopLevelMod};
use fe_hir::test_db::{HirAnalysisTestDb, initialize_analysis_pass};

/// A `Platform` trait carrying a backend fact (`WORD_BITS`), a satisfying
/// backend (`Evm`, 256) and a non-satisfying one (`Tiny`, 16), and a generic
/// function gated on the fact.
const PLATFORM: &str = r#"
trait Platform {
    const WORD_BITS: u256
}

struct Evm {}
struct Tiny {}

impl Platform for Evm {
    const WORD_BITS: u256 = 256
}

impl Platform for Tiny {
    const WORD_BITS: u256 = 16
}

fn word_op<B: Platform>() where B::WORD_BITS == 256 {
}
"#;

fn diagnostics_for<'db>(
    db: &'db HirAnalysisTestDb,
    top_mod: TopLevelMod<'db>,
) -> Vec<CompleteDiagnostic> {
    let mut manager = initialize_analysis_pass();
    let mut diags: Vec<_> = manager
        .run_on_module(db, top_mod)
        .into_iter()
        .map(|diag| diag.to_complete(db))
        .collect();
    diags.sort_by(cmp_complete_diagnostics);
    diags
}

fn func_named<'db>(db: &'db HirAnalysisTestDb, top_mod: TopLevelMod<'db>, name: &str) -> Func<'db> {
    top_mod
        .all_funcs(db)
        .iter()
        .copied()
        .find(|func| func.name(db).to_opt().is_some_and(|n| n.data(db) == name))
        .unwrap_or_else(|| panic!("missing `{name}` function"))
}

fn only_call_expr<'db>(db: &'db HirAnalysisTestDb, func: Func<'db>) -> fe_hir::hir_def::ExprId {
    let body = func.body(db).expect("function has a body");
    body.exprs(db)
        .iter()
        .find_map(|(expr, data)| matches!(data, Partial::Present(Expr::Call(..))).then_some(expr))
        .expect("body contains a call expression")
}

#[test]
fn platform_fact_discharges_for_satisfying_backend() {
    let mut db = HirAnalysisTestDb::default();
    let src = format!("{PLATFORM}\nfn caller() {{\n    word_op<Evm>()\n}}\n");
    let file = db.new_stand_alone("platform_fact_pass.fe".into(), &src);
    let (top_mod, _) = db.top_mod(file);

    // The satisfying program compiles clean.
    db.assert_no_diags(top_mod);

    // And it recorded const-predicate discharge evidence, keyed to the call,
    // discharged via the CTFE route with an (empty) premises slot.
    let caller = func_named(&db, top_mod, "caller");
    let call_expr = only_call_expr(&db, caller);
    let typed = &check_func_body(&db, caller).1;

    let records: Vec<_> = typed
        .discharged_const_predicates_for_call(call_expr)
        .collect();
    assert_eq!(
        records.len(),
        1,
        "expected exactly one const-predicate discharge for the call"
    );
    let record = records[0];
    assert_eq!(record.route, DischargeRoute::Ctfe);
    assert_eq!(record.generic_args.len(), 1, "B := Evm");
    assert!(
        record.premises.is_empty(),
        "the M5 CTFE route is premise-free; the premises slot must be empty"
    );
    assert_eq!(record.call_expr(), call_expr);
}

#[test]
fn platform_fact_fails_for_non_satisfying_backend() {
    let mut db = HirAnalysisTestDb::default();
    let src = format!("{PLATFORM}\nfn caller() {{\n    word_op<Tiny>()\n}}\n");
    let file = db.new_stand_alone("platform_fact_fail.fe".into(), &src);
    let (top_mod, _) = db.top_mod(file);

    let diags = diagnostics_for(&db, top_mod);
    assert!(
        diags.iter().any(|d| d.error_code.to_string() == "8-0085"
            && d.message.contains("const predicate is not satisfied")),
        "expected the unsatisfied-predicate diagnostic, got: {diags:#?}"
    );

    // The failing call discharges *no* evidence — the program does not proceed
    // as if the fact held.
    let caller = func_named(&db, top_mod, "caller");
    let typed = &check_func_body(&db, caller).1;
    assert!(
        typed.discharged_const_predicates().is_empty(),
        "a refuted predicate must not record a discharge"
    );
}

#[test]
fn symbolic_assoc_const_lowers_and_forwards_without_error() {
    // A generic caller forwards its own type parameter `B`. The predicate
    // `B::WORD_BITS == 256` stays symbolic at this call (it is `mid`'s own
    // assumption); it must lower without ICE and must not be falsely discharged
    // or falsely refuted.
    let mut db = HirAnalysisTestDb::default();
    let src = format!(
        "{PLATFORM}\nfn mid<B: Platform>() where B::WORD_BITS == 256 {{\n    word_op<B>()\n}}\n"
    );
    let file = db.new_stand_alone("platform_fact_symbolic.fe".into(), &src);
    let (top_mod, _) = db.top_mod(file);

    db.assert_no_diags(top_mod);

    let mid = func_named(&db, top_mod, "mid");
    let typed = &check_func_body(&db, mid).1;
    assert!(
        typed.discharged_const_predicates().is_empty(),
        "a symbolic forwarded predicate is the caller's own assumption, not a CTFE discharge"
    );
}
