use std::path::Path;

use common::diagnostics::{CompleteDiagnostic, cmp_complete_diagnostics};
use dir_test::{Fixture, dir_test};
use fe_hir::analysis::ty::{
    ty_check::{TraitObligationOrigin, check_contract_recv_arm_body, check_func_body},
    ty_def::{Kind, TyId},
};
use fe_hir::hir_def::{Expr, Partial, TopLevelMod};
use fe_hir::span::LazySpan;
use fe_hir::test_db::{HirAnalysisTestDb, initialize_analysis_pass};
use test_utils::snap_test;

#[dir_test(
    dir: "$CARGO_MANIFEST_DIR/test_files/ty_check",
    glob: "**/*.fe"
)]
fn ty_check_standalone(fixture: Fixture<&str>) {
    let mut db = HirAnalysisTestDb::default();
    let path = Path::new(fixture.path());
    let file_name = path.file_name().and_then(|file| file.to_str()).unwrap();
    let file = db.new_stand_alone(file_name.into(), fixture.content());
    let (top_mod, mut prop_formatter) = db.top_mod(file);

    db.assert_no_diags(top_mod);

    for &func in top_mod.all_funcs(&db) {
        if let Some(body) = func.body(&db) {
            let typed_body = &check_func_body(&db, func).1;
            collect_body_props(&db, body, typed_body, &mut prop_formatter);
        }
    }

    for &contract in top_mod.all_contracts(&db) {
        let recvs = contract.recvs(&db);
        for (recv_idx, recv) in recvs.data(&db).iter().enumerate() {
            for (arm_idx, arm) in recv.arms.data(&db).iter().enumerate() {
                let typed_body =
                    &check_contract_recv_arm_body(&db, contract, recv_idx as u32, arm_idx as u32).1;
                collect_body_props(&db, arm.body, typed_body, &mut prop_formatter);
            }
        }
    }

    let res = prop_formatter.finish(&db);
    snap_test!(res, fixture.path());
}

#[test]
fn never_type_is_not_type_applicable() {
    let db = HirAnalysisTestDb::default();
    let never = TyId::never(&db);

    assert!(matches!(never.kind(&db), Kind::Star));
    assert!(never.applicable_ty(&db).is_none());
}

/// D1 Sub-PR 2 milestone: a trait associated type's kind is derived from its
/// own generic params. `type Ptr<T>` (one param) has kind `* -> *`; `type Word`
/// (no params) has kind `*`.
#[test]
fn gat_kind_from_generic_params() {
    use fe_hir::analysis::ty::trait_def::TraitInstId;
    use fe_hir::hir_def::IdentId;

    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "gat_kind_from_generic_params.fe".into(),
        r#"
trait Backend {
    type Ptr<T>
    type Word
}
"#,
    );
    let (top_mod, _) = db.top_mod(file);

    let backend = *top_mod
        .all_traits(&db)
        .iter()
        .find(|t| {
            t.name(&db)
                .to_opt()
                .is_some_and(|i| i.data(&db) == "Backend")
        })
        .expect("missing Backend trait");
    let inst = TraitInstId::new_simple(&db, backend, vec![]);

    let ptr = inst
        .assoc_ty(&db, IdentId::new(&db, "Ptr".to_string()))
        .expect("missing assoc ty Ptr");
    assert_eq!(
        *ptr.kind(&db),
        Kind::Abs(Box::new((Kind::Star, Kind::Star))),
        "type Ptr<T> should have kind * -> *, got {}",
        ptr.kind(&db)
    );

    let word = inst
        .assoc_ty(&db, IdentId::new(&db, "Word".to_string()))
        .expect("missing assoc ty Word");
    assert_eq!(
        *word.kind(&db),
        Kind::Star,
        "type Word should have kind *, got {}",
        word.kind(&db)
    );
}

#[test]
fn explicit_return_normalizes_ground_typefn_receiver_projection() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "explicit_return_normalizes_ground_typefn_receiver_projection.fe".into(),
        r#"
struct Done {}
struct Step<T> {}

recursive type fn Chain<const N: usize>() -> (*) {
    match N {
        0 => Done
        _ => Step<Chain<{N - 1}>>
    }
}

struct End {}
struct Node<T> { value: T }

trait Shape { type Out }
impl Shape for Done { type Out = End }
impl<T: Shape> Shape for Step<T> { type Out = Node<T::Out> }

type Derived = <Chain<2> as Shape>::Out

fn convert(_ value: Node<Node<End>>) -> Derived {
    return value
}
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    db.assert_no_diags(top_mod);
    let convert = top_mod
        .all_funcs(&db)
        .iter()
        .copied()
        .find(|func| {
            func.name(&db)
                .to_opt()
                .is_some_and(|name| name.data(&db) == "convert")
        })
        .expect("missing `convert` function");
    let (diags, _) = check_func_body(&db, convert).clone();
    assert!(diags.is_empty(), "{diags:?}");
}

/// A2b.1 core contract: an impl-side GAT param used in the RHS resolves to a
/// rigid `TyParam` with the PINNED representation (steering-06 sec 1.1): LOCAL
/// index 0 and `owner == ScopeId::ImplTraitType(imp, assoc_idx)` (the
/// assoc-type def node, an owner class no other `TyParam` mint site produces).
/// A2b.2 and A3 projection depend on exactly this shape.
#[test]
fn gat_impl_rhs_param_resolves_to_pinned_ty_param() {
    use fe_hir::analysis::ty::ty_def::TyData;
    use fe_hir::hir_def::scope_graph::ScopeId;

    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "gat_impl_rhs_param_resolves_to_pinned_ty_param.fe".into(),
        r#"
trait Backend {
    type Buffer<T>
}

struct Store<T> {
    value: T,
}

struct EvmBackend {}

impl Backend for EvmBackend {
    type Buffer<T> = Store<T>
}
"#,
    );
    let (top_mod, _) = db.top_mod(file);

    // No name-resolution (or any other) diagnostics: `T` in the RHS resolves.
    db.assert_no_diags(top_mod);

    // Locate the `impl Backend for EvmBackend`'s `Buffer` assoc-type view.
    let (imp, assoc_idx, view) = top_mod
        .all_impl_traits(&db)
        .iter()
        .find_map(|&imp| {
            imp.assoc_types(&db)
                .enumerate()
                .find(|(_, v)| v.name(&db).is_some_and(|n| n.data(&db) == "Buffer"))
                .map(|(idx, v)| (imp, idx, v))
        })
        .expect("missing `type Buffer<T>` in an impl-trait block");

    let ty = view
        .ty(&db)
        .expect("`type Buffer<T> = Store<T>` should lower");

    // RHS is `Store<T>`: an application of the `Store` ADT to the pinned param.
    let (_base, args) = ty.decompose_ty_app(&db);
    assert_eq!(
        args.len(),
        1,
        "expected `Store<T>` with one type arg, got `{}`",
        ty.pretty_print(&db)
    );

    let TyData::TyParam(param) = args[0].data(&db) else {
        panic!(
            "expected the RHS arg to be a rigid TyParam, got `{}`",
            args[0].pretty_print(&db)
        );
    };

    assert_eq!(param.idx, 0, "GAT param must carry LOCAL index 0");
    assert_eq!(
        param.owner,
        ScopeId::ImplTraitType(imp, assoc_idx as u16),
        "GAT param owner must be the assoc-type def node (ImplTraitType), not the impl item scope"
    );
}

/// A2b.1 trait side: a GAT param used in the assoc type's own default resolves
/// to a rigid `TyParam` owned by the trait-side def node
/// (`ScopeId::TraitType(t, assoc_idx)`), local idx 0. Mirrors the impl-side
/// contract; proves the trait-side scope re-anchor (default/bounds lower under
/// the `TraitType` def node).
#[test]
fn gat_trait_default_param_resolves_to_pinned_ty_param() {
    use fe_hir::analysis::ty::ty_def::TyData;
    use fe_hir::hir_def::scope_graph::ScopeId;

    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "gat_trait_default_param_resolves_to_pinned_ty_param.fe".into(),
        r#"
struct Store<T> {
    value: T,
}

trait Backend {
    type Buffer<T> = Store<T>
}
"#,
    );
    let (top_mod, _) = db.top_mod(file);

    let (backend, assoc_idx, view) = top_mod
        .all_traits(&db)
        .iter()
        .find(|t| {
            t.name(&db)
                .to_opt()
                .is_some_and(|i| i.data(&db) == "Backend")
        })
        .and_then(|&t| {
            t.assoc_types(&db)
                .enumerate()
                .find(|(_, v)| v.name(&db).is_some_and(|n| n.data(&db) == "Buffer"))
                .map(|(idx, v)| (t, idx, v))
        })
        .expect("missing `type Buffer<T>` in trait `Backend`");

    let ty = view
        .default_ty(&db)
        .expect("`type Buffer<T> = Store<T>` default should lower");

    let (_base, args) = ty.decompose_ty_app(&db);
    assert_eq!(
        args.len(),
        1,
        "expected `Store<T>` with one type arg, got `{}`",
        ty.pretty_print(&db)
    );

    let TyData::TyParam(param) = args[0].data(&db) else {
        panic!(
            "expected the default's arg to be a rigid TyParam, got `{}`",
            args[0].pretty_print(&db)
        );
    };

    assert_eq!(
        param.idx, 0,
        "trait-side GAT param must carry LOCAL index 0"
    );
    assert_eq!(
        param.owner,
        ScopeId::TraitType(backend, assoc_idx as u16),
        "trait-side GAT param owner must be the `TraitType` def node"
    );
}

#[test]
fn never_for_iterator_reports_type_must_be_known() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "never_for_iterator_reports_type_must_be_known.fe".into(),
        r#"
extern {
    fn revert() -> !
}

fn trigger() {
    for x in revert() {}
}
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    let diags = diagnostics_for(&db, top_mod);

    assert!(
        diags
            .iter()
            .any(|diag| diag.message == "type must be known"),
        "{diags:#?}"
    );
    assert!(
        !diags
            .iter()
            .any(|diag| diag.message == "`Seq` needs to be implemented for !"),
        "{diags:#?}"
    );
}

#[test]
fn invalid_const_fn_body_diagnostics_do_not_panic_during_const_eval() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "invalid_const_fn_body_diagnostics_do_not_panic_during_const_eval.fe".into(),
        r#"
const fn invalid_const() -> usize {
    pass
    missing_value
}

struct NeedsConst<const N: usize> {}

fn trigger() {
    let _x: NeedsConst<{ invalid_const() }>
}
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    let diags = diagnostics_for(&db, top_mod);

    assert!(
        diagnostics_contain(&diags, "undefined variable `pass`"),
        "{diags:#?}"
    );
    assert!(
        diagnostics_contain(&diags, "undefined variable `missing_value`"),
        "{diags:#?}"
    );
    assert!(!diagnostics_contain(&diags, "const eval"), "{diags:#?}");
}

#[test]
fn generic_operator_ambiguity_preserves_checked_candidates() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "generic_operator_ambiguity_preserves_checked_candidates.fe".into(),
        r#"
use core::ops::Add

struct Box<T> {
    value: T,
}

impl<T: Copy> Copy for Box<T> {}

impl<T> Box<T> {
    fn new(value: T) -> Box<T> {
        Box { value: value }
    }
}

impl<T: Copy> Add for Box<T> {
    fn add(own self, _ other: own Box<T>) -> Box<T> {
        self
    }
}

impl<T: Copy> Add<T> for Box<T> {
    fn add(own self, _ other: own T) -> Box<T> {
        self + Box::new(value: other)
    }
}

fn probe() -> u256 {
    let box: Box<u256> = Box::new(value: 1)
    (box + 2).value
}
"#,
    );
    let (top_mod, _) = db.top_mod(file);

    db.assert_no_diags(top_mod);
}

#[test]
fn discharged_trait_obligations_are_recorded() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "discharged_trait_obligations_are_recorded.fe".into(),
        r#"
trait Marker {}

struct Point {}

impl Marker for Point {}

fn requires_marker<T: Marker>(_ t: T) {}

fn caller(p: Point) {
    requires_marker(p)
}
"#,
    );
    let (top_mod, _) = db.top_mod(file);

    db.assert_no_diags(top_mod);

    let caller = top_mod
        .all_funcs(&db)
        .iter()
        .copied()
        .find(|func| {
            func.name(&db)
                .to_opt()
                .is_some_and(|name| name.data(&db) == "caller")
        })
        .expect("missing `caller` function");

    let typed_body = &check_func_body(&db, caller).1;
    let records = typed_body.discharged_obligations();
    assert!(
        !records.is_empty(),
        "expected discharge records for `caller`, found none"
    );

    let record = records
        .iter()
        .find(|record| record.solution.is_some())
        .expect("expected a discharge record with a committed solution");
    assert_eq!(record.goal.pretty_print(&db, true), "Point: Marker");

    let solution = record.solution.unwrap();
    assert_eq!(solution.describe_implementor(&db), "impl Marker for Point");

    // The record is keyed to the `requires_marker(p)` call expression.
    let body = caller.body(&db).expect("`caller` has a body");
    let call_expr = body
        .exprs(&db)
        .iter()
        .find_map(|(expr, data)| {
            if let Partial::Present(Expr::Call(..)) = data {
                Some(expr)
            } else {
                None
            }
        })
        .expect("`caller` body contains a call expression");
    let for_call: Vec<_> = typed_body
        .discharged_obligations_for_call(call_expr)
        .collect();
    assert_eq!(for_call.len(), 1);
    assert!(matches!(
        for_call[0].origin,
        TraitObligationOrigin::CallConstraint {
            call_expr: origin_expr,
            ..
        } if origin_expr == call_expr
    ));
}

#[test]
fn discharged_obligation_solution_notes_derive_provenance() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        "discharged_obligation_solution_notes_derive_provenance.fe".into(),
        r#"
use core::ops::Eq

#[derive(Eq)]
struct Point {
    x: u256,
    y: u256,
}

fn requires_eq<T: Eq>(_ t: T) {}

fn caller(p: Point) {
    requires_eq(p)
}
"#,
    );
    let (top_mod, _) = db.top_mod(file);

    db.assert_no_diags(top_mod);

    let caller = top_mod
        .all_funcs(&db)
        .iter()
        .copied()
        .find(|func| {
            func.name(&db)
                .to_opt()
                .is_some_and(|name| name.data(&db) == "caller")
        })
        .expect("missing `caller` function");

    let typed_body = &check_func_body(&db, caller).1;
    let record = typed_body
        .discharged_obligations()
        .iter()
        .find(|record| record.solution.is_some())
        .expect("expected a discharge record with a committed solution");
    // `Eq<T = Self>` elaborates its defaulted parameter, so the recorded
    // goal and impl both carry the explicit `<Point>` argument.
    assert_eq!(record.goal.pretty_print(&db, true), "Point: Eq<Point>");
    assert_eq!(
        record.solution.unwrap().describe_implementor(&db),
        "impl Eq<Point> for Point (derived)"
    );
}

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

fn diagnostics_contain(diags: &[CompleteDiagnostic], needle: &str) -> bool {
    diags.iter().any(|diag| {
        diag.message.contains(needle)
            || diag
                .sub_diagnostics
                .iter()
                .any(|sub_diag| sub_diag.message.contains(needle))
            || diag.notes.iter().any(|note| note.contains(needle))
    })
}

fn collect_body_props<'db>(
    db: &'db HirAnalysisTestDb,
    body: fe_hir::hir_def::Body<'db>,
    typed_body: &fe_hir::analysis::ty::ty_check::TypedBody<'db>,
    prop_formatter: &mut fe_hir::test_db::HirPropertyFormatter<'db>,
) {
    for expr in body.exprs(db).keys() {
        let span = expr.span(body);
        if span.resolve(db).is_none() {
            continue;
        }

        let ty = typed_body.expr_ty(db, expr);
        prop_formatter.push_prop(
            body.top_mod(db),
            span.into(),
            ty.pretty_print(db).to_string(),
        );
    }

    for pat in body.pats(db).keys() {
        let span = pat.span(body);
        if span.resolve(db).is_none() {
            continue;
        }

        let ty = typed_body.pat_ty(db, pat);
        prop_formatter.push_prop(
            body.top_mod(db),
            span.into(),
            ty.pretty_print(db).to_string(),
        );
    }
}
