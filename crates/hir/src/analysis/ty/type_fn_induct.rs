//! Type-to-type CTFE induction engine PRECONDITIONS (spec sec 5.3, ladder
//! S2.2a; Fable steering-04 §5). This module is deliberately INERT: every item
//! here has ZERO non-test callers. Nothing is wired into any obligation or WF
//! discharge site (that is S2.2b), so it has zero proof power by construction
//! and cannot perturb the 2475 baseline.
//!
//! It provides the three trapdoor primitives the minimal induction engine will
//! consume, each executable and unit-tested now, before any proof power exists:
//!
//! 1. [`strict_prove`] / [`StrictResult`] — a STRICT satisfaction check that is
//!    a SEPARATE type from [`GoalSatisfiability`], so no engine result can ever
//!    be routed through the permissive `is_satisfied` / UnSat-only WF pattern by
//!    accident (steering-04 §1.2). It calls the existing tracked
//!    `is_query_satisfiable` READ-ONLY and maps only strict `Satisfied` to
//!    `Proven`; everything else (including the depth-cap give-up
//!    `NeedsConfirmation(empty)`) to `NotProven`, behind a HAS_INVALID/HAS_VAR
//!    pre-flight (because `Invalid` unifies with everything, `unify.rs`).
//!
//! 2. [`mint_induction_opaque`] — mints a fresh RIGID opaque as a dedicated
//!    [`Variant::Induction`](super::ty_def) `TyParam`, so it sets `HAS_PARAM`
//!    (load-bearing: the solver's assumptions leg only fires for param-carrying
//!    goals, `proof_forest.rs`) yet can never unify with a real param (identity
//!    unification over a distinct variant). Leak tripwires (`ty_def.rs`
//!    `original_idx`/`scope` panics + a mint-site collision assert) guard against
//!    an opaque escaping into a stored/MIR/source position.
//!
//! 3. [`minimal_class`] — a pure recognizer over `TypeFnWfData` + the requested
//!    goal deciding whether a symbolic type-fn goal is in the S2.2b minimal
//!    class, so S2.2b engages the engine ONLY there and otherwise falls back to
//!    the S2.1 assumption path. Every decline carries a [`ClassDecline`] reason.

// S2.2a INVARIANT: every item below is exercised ONLY by this module's tests
// until S2.2b wires the engine into a discharge site. The dead-code allow makes
// that "zero non-test callers" guarantee explicit; S2.2b's real callers remove
// the need for it.
#![allow(dead_code)]

use crate::Ingot;
use crate::analysis::HirAnalysisDb;
use crate::analysis::ty::const_ty::{ConstTyData, ConstTyId};
use crate::analysis::ty::trait_def::TraitInstId;
use crate::analysis::ty::trait_resolution::{
    CanonicalGoalQuery, GoalSatisfiability, PredicateListId, is_query_satisfiable,
};
use crate::analysis::ty::ty_def::{Kind, TyFlags, TyId, TyParam};
use crate::analysis::ty::type_fn::{
    TypeFnWfData, bare_path_ident, body_root_expr, collect_type_fn_heads, resolve_leaf_scope,
    type_fn_app_head,
};
use crate::core::hir_def::{
    ConstGenericArgValue, Expr, GenericArg, GenericParam, IdentId, ItemKind, LitKind, Partial,
    TypeFnDef, TypeId, TypeKind, scope_graph::ScopeId,
};

// ---------------------------------------------------------------------------
// 1. Strict satisfaction (steering-04 §1.2 / §1.3).
// ---------------------------------------------------------------------------

/// The result of a STRICT proof attempt. Deliberately NOT [`GoalSatisfiability`]:
/// a distinct type is what makes it impossible to route an engine result through
/// the permissive `GoalSatisfiability::is_satisfied` (which counts
/// `NeedsConfirmation` and `ContainsInvalid` as satisfied) or the pervasive
/// UnSat-only WF match. The engine may treat ONLY [`StrictResult::Proven`] as
/// proof; [`StrictResult::NotProven`] means "no lemma", NEVER "refuted for all n"
/// (the engine records no negative lemmas).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StrictResult {
    /// The goal is strictly `Satisfied` with a unique solution.
    Proven,
    /// Anything else: `NeedsConfirmation` (multi-solution OR the depth-cap
    /// give-up `NeedsConfirmation(empty)`), `ContainsInvalid`, `UnSat`, or a
    /// HAS_INVALID/HAS_VAR pre-flight decline.
    NotProven,
}

/// Strictly prove `goal` under `assumptions` in `origin_ingot`, READ-ONLY.
///
/// Reconciliation of the task shorthand `strict_prove(db, goal, assumptions)`
/// with the callable form: `origin_ingot` is required because impl visibility is
/// keyed on it in `is_query_satisfiable` (steering-04 §3.2/§4). The S2.2b
/// discharge site has it in hand (from its `TraitSolveCx`).
///
/// It (a) pre-checks HAS_INVALID/HAS_VAR on the whole canonical query (goal PLUS
/// assumptions) and declines before querying — `Invalid` unifies with everything
/// (`unify.rs:149-152`) and `is_query_satisfiable` converts a HAS_INVALID query
/// into `ContainsInvalid`, which `is_satisfied` then passes, so the two traps
/// compose; a free var likewise lets arbitrary impls apply — then (b) calls the
/// EXISTING tracked `is_query_satisfiable` and maps strict `Satisfied` to
/// `Proven`, everything else to `NotProven`.
pub(crate) fn strict_prove<'db>(
    db: &'db dyn HirAnalysisDb,
    origin_ingot: Ingot<'db>,
    goal: TraitInstId<'db>,
    assumptions: PredicateListId<'db>,
) -> StrictResult {
    // Build the SAME canonical query the ordinary solver builds, so the strict
    // check reads exactly the tracked entry every other consumer reads (no new
    // salsa writes, no perturbation).
    let query = CanonicalGoalQuery::new(db, goal, assumptions);
    let canonical = query.canonical();

    // PRE-FLIGHT (steering-04 §1.2): decline on any `Invalid` or free var in the
    // goal or the (bound-extended) assumptions BEFORE querying.
    let flags = canonical.flags(db);
    if flags.contains(TyFlags::HAS_INVALID) || flags.contains(TyFlags::HAS_VAR) {
        return StrictResult::NotProven;
    }

    match is_query_satisfiable(db, origin_ingot, canonical) {
        GoalSatisfiability::Satisfied(_) => StrictResult::Proven,
        // `NeedsConfirmation(empty)` is the depth-cap give-up
        // (`proof_forest.rs:161-164`); `NeedsConfirmation(non-empty)` is a
        // multi-solution / coexistence. `ContainsInvalid` is unreachable after
        // the pre-flight (belt-and-suspenders). All are NOT a strict proof.
        GoalSatisfiability::NeedsConfirmation(_)
        | GoalSatisfiability::ContainsInvalid
        | GoalSatisfiability::UnSat(_) => StrictResult::NotProven,
    }
}

// ---------------------------------------------------------------------------
// 2. Opaque IH minting (steering-04 §1.3 / C3).
// ---------------------------------------------------------------------------

/// Reserved index base for induction opaques, minted well past any real param
/// count so the opaque `idx` cannot alias a real param index (belt over the
/// dedicated-variant suspenders, which already guarantee non-collision).
const OPAQUE_IDX_BASE: usize = 1 << 20;

/// Stride separating arms in the opaque index space; `occurrence_idx` must stay
/// below it so distinct `(arm_idx, occurrence_idx)` pairs mint distinct indices.
const OPAQUE_ARM_STRIDE: usize = 1024;

/// Mint a fresh RIGID induction-hypothesis opaque for the `occurrence_idx`-th
/// self-call in arm `arm_idx` of `def` (steering-04 §2: fresh PER OCCURRENCE is
/// sound and sufficient; distinct rigids prove strictly less than a shared one,
/// the conservative direction).
///
/// Determinism: the opaque is a pure function of `(def, arm_idx, occurrence_idx,
/// kind)`, so it is stable across salsa revalidations (opaques become part of
/// arm-goal query keys in S2.2b).
///
/// Representation (C3): a [`TyParam`] with the dedicated
/// [`Variant::Induction`](super::ty_def) — `TyParam`-shaped so `visit_param`
/// sets `HAS_PARAM` (turns the assumptions leg ON), yet the distinct variant
/// makes it unequal to every real param under identity unification. A `TyVar`
/// would unify with anything (unsound); a non-`TyParam` shape would turn the
/// assumptions leg OFF (useless).
pub(crate) fn mint_induction_opaque<'db>(
    db: &'db dyn HirAnalysisDb,
    def: TypeFnDef<'db>,
    arm_idx: usize,
    occurrence_idx: usize,
    kind: Kind,
) -> TyId<'db> {
    debug_assert!(
        occurrence_idx < OPAQUE_ARM_STRIDE,
        "occurrence_idx {occurrence_idx} exceeds the per-arm opaque stride"
    );
    let idx = OPAQUE_IDX_BASE + arm_idx * OPAQUE_ARM_STRIDE + occurrence_idx;

    // Reserved marker name: the angle-bracket form is render-friendly for later
    // diagnostics ("... (induction hypothesis)") yet cannot be a source-level
    // identifier, so it cannot shadow a user param by name either.
    let name = IdentId::new(db, format!("<ih#{arm_idx}.{occurrence_idx}>"));
    let param = TyParam::induction_opaque(name, idx, kind, def.scope());
    let opaque = param.ty(db);

    // Collision tripwire (steering-04 §1.3 risk 1): assert the opaque is not
    // equal to any real param of the owner, and its index is past the real
    // count. The dedicated variant already guarantees this; the assert makes the
    // guarantee executable.
    #[cfg(debug_assertions)]
    {
        use crate::analysis::ty::ty_lower::collect_generic_params;
        use crate::core::hir_def::GenericParamOwner;
        let real = collect_generic_params(db, GenericParamOwner::TypeFn(def)).params(db);
        debug_assert!(
            idx >= real.len(),
            "induction opaque idx {idx} must be past the def's real param count {}",
            real.len()
        );
        for &p in real {
            debug_assert_ne!(
                p, opaque,
                "induction opaque collided with a real param of the def"
            );
        }
    }

    opaque
}

// ---------------------------------------------------------------------------
// 3. Minimal-class recognizer (steering-04 §5 S2.2a item 3).
// ---------------------------------------------------------------------------

/// Whether a symbolic type-fn goal is in the S2.2b minimal induction class.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MinimalClass {
    /// The engine may engage: all preconditions hold.
    InClass,
    /// The engine must NOT engage; fall back to the S2.1 assumption path. Carries
    /// the specific reason for diagnostics.
    Declined(ClassDecline),
}

/// Why a goal is outside the S2.2b minimal induction class.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ClassDecline {
    /// The goal's self type is not a live `recursive type fn` application.
    NotTypeFnGoal,
    /// The goal's self type is an application of a DIFFERENT type fn than the one
    /// whose `TypeFnWfData` was supplied.
    ForeignTypeFnGoal,
    /// The recursion subject is not a bare rigid const param (it is ground, a
    /// hole, a var, or a compound const expression).
    SubjectNotBareRigidConst,
    /// The goal carries a trait argument beyond the self type (non-unary trait).
    /// Subject-free multi-arg traits are a sound deferred widening.
    NonUnaryTraitGoal,
    /// The goal carries associated-type bindings (not a single bare predicate).
    AssocTypeBindings,
    /// A goal argument carries a free inference var.
    ArgHasVar,
    /// A goal argument carries `Invalid`.
    ArgHasInvalid,
    /// A goal argument carries a const hole.
    ArgHasHole,
    /// A goal argument is (or contains) a type-fn application.
    ArgHasTypeFn,
    /// A trait argument mentions the recursion subject.
    SubjectInTraitArgs,
    /// Some arm has more than one self-call (out of the v1 single-IH class).
    MultiSelfCallArm,
    /// Some arm body mentions the bare subject in a const-argument position (the
    /// G3 hazard: a value-indexed impl could diverge from the symbolic proof).
    BareSubjectConstArgInArm,
}

/// Decide whether `goal` is in the S2.2b minimal class for the type fn described
/// by `wf`. PURE over `(wf, goal)`; no solver call, no minting. Declining is
/// always sound (S2.2b falls back to the S2.1 assumption route), so every
/// uncertain shape declines.
pub(crate) fn minimal_class<'db>(
    db: &'db dyn HirAnalysisDb,
    wf: &TypeFnWfData<'db>,
    goal: TraitInstId<'db>,
) -> MinimalClass {
    // Single fixed goal: a bare predicate, no associated-type bindings.
    if !goal.assoc_type_bindings(db).is_empty() {
        return declined(ClassDecline::AssocTypeBindings);
    }

    // The self type must be a live application of THIS type fn.
    let self_ty = goal.self_ty(db);
    let Some(head_def) = type_fn_app_head(db, self_ty) else {
        return declined(ClassDecline::NotTypeFnGoal);
    };
    if head_def != wf.def {
        return declined(ClassDecline::ForeignTypeFnGoal);
    }

    // Decompose the saturated app: subject is the final arg, forwarded type args
    // precede it.
    let (_base, app_args) = self_ty.decompose_ty_app(db);
    let Some((&subject, type_args)) = app_args.split_last() else {
        return declined(ClassDecline::NotTypeFnGoal);
    };

    // Bare rigid const subject.
    if !is_bare_rigid_const_param(db, subject) {
        return declined(ClassDecline::SubjectNotBareRigidConst);
    }

    // Forwarded type args: var / invalid / hole / type-fn free.
    for &arg in type_args {
        if let Some(reason) = arg_decline(db, arg) {
            return declined(reason);
        }
    }

    // Single fixed goal / subject-free trait args: v1 admits only unary traits
    // (self only). Any extra trait arg declines (sound; deferred widening).
    let goal_args = goal.args(db);
    for &arg in &goal_args[1..] {
        if let Some(reason) = arg_decline(db, arg) {
            return declined(reason);
        }
        if ty_contains_subterm(db, arg, subject) {
            return declined(ClassDecline::SubjectInTraitArgs);
        }
        return declined(ClassDecline::NonUnaryTraitGoal);
    }

    // At most one self-call per arm (single-IH class).
    for arm in &wf.arms {
        if arm.self_calls.len() > 1 {
            return declined(ClassDecline::MultiSelfCallArm);
        }
    }

    // G3 guard: no arm body may mention the bare subject in a const-arg position
    // (a bare-subject generic argument to a non-self-call application). The bare
    // subject may parse as a Type-path arg (a bare identifier is syntactically a
    // type until lowering resolves it against the callee's const param), so the
    // subject name is needed to recognise it.
    let subject_name = subject_param_name(db, wf);
    for arm in &wf.arms {
        if arm_body_has_bare_subject_const_arg(db, wf.def, subject_name, arm.rhs_ty) {
            return declined(ClassDecline::BareSubjectConstArgInArm);
        }
    }

    MinimalClass::InClass
}

/// The declared name of the recursion subject const param (`N`), read from the
/// def's HIR generic params at `wf.subject_idx` (the WF-guaranteed last param).
fn subject_param_name<'db>(
    db: &'db dyn HirAnalysisDb,
    wf: &TypeFnWfData<'db>,
) -> Option<IdentId<'db>> {
    let params = wf.def.hir_generic_params(db).data(db);
    match params.get(wf.subject_idx)? {
        GenericParam::Const(c) => c.name.to_opt(),
        _ => None,
    }
}

fn declined(reason: ClassDecline) -> MinimalClass {
    MinimalClass::Declined(reason)
}

/// `true` iff `ty` is a bare rigid const param `ConstTy(TyParam(normal))`. An
/// induction opaque (`Variant::Induction`) is NOT `is_normal`, so it is correctly
/// rejected here.
fn is_bare_rigid_const_param<'db>(db: &'db dyn HirAnalysisDb, ty: TyId<'db>) -> bool {
    match ty.data(db) {
        crate::analysis::ty::ty_def::TyData::ConstTy(cid) => {
            matches!(cid.data(db), ConstTyData::TyParam(param, _) if param.is_normal())
        }
        _ => false,
    }
}

/// The arg-shape decline reason for a goal argument, if any.
fn arg_decline<'db>(db: &'db dyn HirAnalysisDb, arg: TyId<'db>) -> Option<ClassDecline> {
    if arg.has_invalid(db) {
        return Some(ClassDecline::ArgHasInvalid);
    }
    if arg.has_var(db) {
        return Some(ClassDecline::ArgHasVar);
    }
    if ty_has_const_hole(db, arg) {
        return Some(ClassDecline::ArgHasHole);
    }
    if !collect_type_fn_heads(db, arg).is_empty() {
        return Some(ClassDecline::ArgHasTypeFn);
    }
    None
}

/// `true` iff `ty` contains a `ConstTyData::Hole` anywhere (there is no HAS_HOLE
/// flag, so this is a dedicated walk).
fn ty_has_const_hole<'db>(db: &'db dyn HirAnalysisDb, ty: TyId<'db>) -> bool {
    use crate::analysis::ty::visitor::{TyVisitable, TyVisitor, walk_const_ty};
    struct HoleFinder<'db> {
        db: &'db dyn HirAnalysisDb,
        found: bool,
    }
    impl<'db> TyVisitor<'db> for HoleFinder<'db> {
        fn db(&self) -> &'db dyn HirAnalysisDb {
            self.db
        }
        fn visit_const_ty(&mut self, const_ty: &ConstTyId<'db>) {
            if matches!(const_ty.data(self.db), ConstTyData::Hole(..)) {
                self.found = true;
            }
            walk_const_ty(self, const_ty);
        }
    }
    let mut f = HoleFinder { db, found: false };
    ty.visit_with(&mut f);
    f.found
}

/// `true` iff `needle` occurs as a subterm of `haystack` (exact interned-id
/// containment).
fn ty_contains_subterm<'db>(
    db: &'db dyn HirAnalysisDb,
    haystack: TyId<'db>,
    needle: TyId<'db>,
) -> bool {
    use crate::analysis::ty::visitor::{TyVisitable, TyVisitor, walk_ty};
    struct Finder<'db> {
        db: &'db dyn HirAnalysisDb,
        needle: TyId<'db>,
        found: bool,
    }
    impl<'db> TyVisitor<'db> for Finder<'db> {
        fn db(&self) -> &'db dyn HirAnalysisDb {
            self.db
        }
        fn visit_ty(&mut self, ty: TyId<'db>) {
            if ty == self.needle {
                self.found = true;
            }
            walk_ty(self, ty);
        }
    }
    let mut f = Finder {
        db,
        needle,
        found: false,
    };
    haystack.visit_with(&mut f);
    f.found
}

/// `true` iff the arm-RHS HIR `ty` mentions the bare subject in a value
/// (const-argument) position on a NON-self-call path (the G3 hazard). Read-only
/// mirror of the WF checker's `walk_arm_ty` + `check_arm_const_arg`.
///
/// The bare subject can surface two ways as a generic argument: (a) a
/// `GenericArg::Const` whose body is the bare subject path (WF restricted a
/// non-self-call const arg to an int literal OR the bare subject, so "not an int
/// literal" is exactly the subject); or (b) a `GenericArg::Type` that is a bare
/// path equal to the subject name (a bare identifier parses as a type until
/// lowering slots it into the callee's const param). Both are the hazard on a
/// non-self-call path. A self-call's own const subject slot is the induction
/// step and is skipped.
fn arm_body_has_bare_subject_const_arg<'db>(
    db: &'db dyn HirAnalysisDb,
    def: TypeFnDef<'db>,
    subject_name: Option<IdentId<'db>>,
    ty: TypeId<'db>,
) -> bool {
    let TypeKind::Path(Partial::Present(path)) = ty.data(db) else {
        return false;
    };

    let is_self_call = path.parent(db).is_none()
        && matches!(
            resolve_leaf_scope(db, *path, def.scope()),
            Some(ScopeId::Item(ItemKind::TypeFn(d))) if d == def
        );

    for arg in path.generic_args(db).data(db) {
        match arg {
            GenericArg::Type(t) => {
                if let Partial::Present(inner) = t.ty {
                    // (b) A bare-subject type-path arg on a non-self-call path is
                    // the subject being passed into a value slot.
                    if !is_self_call
                        && subject_name.is_some()
                        && bare_path_ident(db, inner) == subject_name
                    {
                        return true;
                    }
                    if arm_body_has_bare_subject_const_arg(db, def, subject_name, inner) {
                        return true;
                    }
                }
            }
            GenericArg::AssocType(a) => {
                if let Partial::Present(inner) = a.ty
                    && arm_body_has_bare_subject_const_arg(db, def, subject_name, inner)
                {
                    return true;
                }
            }
            GenericArg::Const(c) => {
                // (a) A self-call's const arg is the recursion subject (opaqued in
                // S2.2b), never a hazard: skip it. On any other path, a
                // non-int-literal const arg is the bare subject.
                if is_self_call {
                    continue;
                }
                let is_int_lit = match c.value {
                    ConstGenericArgValue::Expr(Partial::Present(body)) => {
                        matches!(body_root_expr(db, body), Some(Expr::Lit(LitKind::Int(_))))
                    }
                    _ => false,
                };
                if !is_int_lit {
                    return true;
                }
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use camino::Utf8PathBuf;
    use num_bigint::BigUint;

    use crate::analysis::ty::adt_def::AdtRef;
    use crate::analysis::ty::const_ty::{ConstTyData, ConstTyId, EvaluatedConstTy};
    use crate::analysis::ty::trait_resolution::{GoalSatisfiability, TraitSolveCx, is_goal_satisfiable};
    use crate::analysis::ty::ty_def::{Kind, PrimTy, TyBase, TyData, TyId};
    use crate::analysis::ty::ty_lower::collect_generic_params;
    use crate::analysis::ty::type_fn::type_fn_wf;
    use crate::core::hir_def::{GenericParamOwner, IntegerId, ItemKind, TopLevelMod, TypeFnDef};
    use crate::test_db::HirAnalysisTestDb;

    const FIXTURES: &str = r#"
struct Par {}
struct Pair {}
struct Comp<F, G> {}
struct Wrapper<G, const K: usize> {}

trait Marker {}
impl Marker for Par {}
impl Marker for Pair {}
impl<F, G> Marker for Comp<F, G> {}

recursive type fn RPow<F, const N: usize>() -> (*) {
    match N {
        0 => Par
        _ => Comp<RPow<F, {N - 1}>, F>
    }
}

recursive type fn Bush<const N: usize>() -> (*) {
    match N {
        0 => Pair
        _ => Comp<Bush<{N - 1}>, Bush<{N - 1}>>
    }
}

recursive type fn G3Bad<F, const N: usize>() -> (*) {
    match N {
        0 => Par
        _ => Wrapper<G3Bad<F, {N - 1}>, N>
    }
}
"#;

    fn find_tf<'db>(db: &'db HirAnalysisTestDb, top_mod: TopLevelMod<'db>, name: &str) -> TypeFnDef<'db> {
        *top_mod
            .all_type_fns(db)
            .iter()
            .find(|tf| tf.name(db).to_opt().is_some_and(|i| i.data(db) == name))
            .unwrap_or_else(|| panic!("missing `{name}` type fn"))
    }

    fn marker_trait<'db>(
        db: &'db HirAnalysisTestDb,
        top_mod: TopLevelMod<'db>,
    ) -> crate::hir_def::Trait<'db> {
        *top_mod
            .all_traits(db)
            .iter()
            .find(|t| t.name(db).to_opt().is_some_and(|i| i.data(db) == "Marker"))
            .expect("missing Marker trait")
    }

    fn adt_ty<'db>(db: &'db HirAnalysisTestDb, top_mod: TopLevelMod<'db>, name: &str) -> TyId<'db> {
        let s = top_mod
            .all_structs(db)
            .iter()
            .copied()
            .find(|s| s.name(db).to_opt().is_some_and(|i| i.data(db) == name))
            .unwrap_or_else(|| panic!("missing struct `{name}`"));
        TyId::adt(db, AdtRef::try_from_item(ItemKind::Struct(s)).unwrap().as_adt(db))
    }

    fn usize_subject<'db>(db: &'db HirAnalysisTestDb, n: u32) -> TyId<'db> {
        let usize_ty = TyId::new(db, TyData::TyBase(TyBase::Prim(PrimTy::Usize)));
        let cid = ConstTyId::new(
            db,
            ConstTyData::Evaluated(
                EvaluatedConstTy::LitInt(IntegerId::new(db, BigUint::from(n))),
                usize_ty,
            ),
        );
        TyId::const_ty(db, cid)
    }

    // ------------------------------------------------------------------
    // strict_prove mapping + anti-vacuity divergence.
    // ------------------------------------------------------------------

    /// A satisfiable, unique goal maps to `Proven`, and here the permissive and
    /// strict checks AGREE (the non-divergent control): `Marker(Par)` has exactly
    /// one impl.
    #[test]
    fn strict_maps_unique_satisfied_to_proven() {
        let mut db = HirAnalysisTestDb::default();
        let file = db.new_stand_alone(Utf8PathBuf::from("induct_ok.fe"), FIXTURES);
        let (top_mod, _) = db.top_mod(file);
        let ingot = top_mod.scope().ingot(&db);
        let marker = marker_trait(&db, top_mod);
        let goal = TraitInstId::new_simple(&db, marker, vec![adt_ty(&db, top_mod, "Par")]);
        let empty = PredicateListId::empty_list(&db);

        let cx = TraitSolveCx::new(&db, top_mod.scope());
        assert!(is_goal_satisfiable(&db, cx, goal).is_satisfied());
        assert_eq!(strict_prove(&db, ingot, goal, empty), StrictResult::Proven);
    }

    /// An unsatisfiable goal maps to `NotProven`, and both checks AGREE it is not
    /// satisfied (`Marker(Comp<...>)`-shaped counterexample would satisfy, so use
    /// a type with no impl).
    #[test]
    fn strict_maps_unsat_to_not_proven() {
        let src = format!("{FIXTURES}\nstruct NoImpl {{}}\n");
        let mut db = HirAnalysisTestDb::default();
        let file = db.new_stand_alone(Utf8PathBuf::from("induct_unsat.fe"), &src);
        let (top_mod, _) = db.top_mod(file);
        let ingot = top_mod.scope().ingot(&db);
        let marker = marker_trait(&db, top_mod);
        let goal = TraitInstId::new_simple(&db, marker, vec![adt_ty(&db, top_mod, "NoImpl")]);
        let empty = PredicateListId::empty_list(&db);

        let cx = TraitSolveCx::new(&db, top_mod.scope());
        assert!(!is_goal_satisfiable(&db, cx, goal).is_satisfied());
        assert_eq!(strict_prove(&db, ingot, goal, empty), StrictResult::NotProven);
    }

    /// ANTI-VACUITY (mandatory). A genuine multi-solution `NeedsConfirmation`: two
    /// coexisting impls both satisfy `Marker(Amb)`. The permissive
    /// `is_satisfied()` counts it satisfied; the strict `strict_prove` returns
    /// `NotProven`. Asserted in ONE test so the DIVERGENCE is the tested fact,
    /// proving the strict check is genuinely stricter, not a synonym, and that the
    /// `NeedsConfirmation -> NotProven` mapping arm is exercised (var-free, so the
    /// pre-flight does not fire first).
    #[test]
    fn strict_diverges_from_permissive_on_needs_confirmation() {
        let src = r#"
trait Marker {}
struct Amb {}
impl Marker for Amb {}
impl Marker for Amb {}
"#;
        let mut db = HirAnalysisTestDb::default();
        let file = db.new_stand_alone(Utf8PathBuf::from("induct_needsconf.fe"), src);
        let (top_mod, _) = db.top_mod(file);
        let ingot = top_mod.scope().ingot(&db);
        let marker = marker_trait(&db, top_mod);
        let goal = TraitInstId::new_simple(&db, marker, vec![adt_ty(&db, top_mod, "Amb")]);
        let empty = PredicateListId::empty_list(&db);

        let cx = TraitSolveCx::new(&db, top_mod.scope());
        let permissive = is_goal_satisfiable(&db, cx, goal);
        assert!(
            matches!(permissive, GoalSatisfiability::NeedsConfirmation(_)),
            "expected a multi-solution NeedsConfirmation, got {permissive:?}"
        );
        // The divergence, asserted together:
        assert!(permissive.is_satisfied(), "permissive counts NeedsConfirmation satisfied");
        assert_eq!(
            strict_prove(&db, ingot, goal, empty),
            StrictResult::NotProven,
            "strict must decline NeedsConfirmation"
        );
    }

    /// An `Invalid` argument: `is_query_satisfiable` returns `ContainsInvalid`,
    /// which the permissive `is_satisfied()` passes, while the HAS_INVALID
    /// pre-flight makes `strict_prove` decline. A second divergence pin.
    #[test]
    fn strict_diverges_from_permissive_on_contains_invalid() {
        let mut db = HirAnalysisTestDb::default();
        let file = db.new_stand_alone(Utf8PathBuf::from("induct_invalid.fe"), FIXTURES);
        let (top_mod, _) = db.top_mod(file);
        let ingot = top_mod.scope().ingot(&db);
        let marker = marker_trait(&db, top_mod);
        let invalid =
            TyId::invalid(&db, crate::analysis::ty::ty_def::InvalidCause::Other);
        let goal = TraitInstId::new_simple(&db, marker, vec![invalid]);
        let empty = PredicateListId::empty_list(&db);

        let cx = TraitSolveCx::new(&db, top_mod.scope());
        let permissive = is_goal_satisfiable(&db, cx, goal);
        assert!(
            matches!(permissive, GoalSatisfiability::ContainsInvalid),
            "expected ContainsInvalid, got {permissive:?}"
        );
        assert!(permissive.is_satisfied());
        assert_eq!(strict_prove(&db, ingot, goal, empty), StrictResult::NotProven);
    }

    // ------------------------------------------------------------------
    // Opaque minting.
    // ------------------------------------------------------------------

    /// Two distinct occurrences mint distinct, rigid opaques: distinct TyIds,
    /// `unify(O1, O2)` fails, `unify(O1, O1)` succeeds, `O1` never unifies with a
    /// concrete type, and each sets HAS_PARAM (the assumptions leg would fire).
    #[test]
    fn opaque_mint_distinct_and_rigid() {
        use crate::analysis::ty::unify::UnificationTable;

        let mut db = HirAnalysisTestDb::default();
        let file = db.new_stand_alone(Utf8PathBuf::from("induct_opaque.fe"), FIXTURES);
        let (top_mod, _) = db.top_mod(file);
        let rpow = find_tf(&db, top_mod, "RPow");

        let o1 = mint_induction_opaque(&db, rpow, 1, 0, Kind::Star);
        let o2 = mint_induction_opaque(&db, rpow, 1, 1, Kind::Star);
        assert_ne!(o1, o2, "distinct occurrences must mint distinct opaques");

        // Determinism: same coordinates re-mint the same opaque.
        assert_eq!(o1, mint_induction_opaque(&db, rpow, 1, 0, Kind::Star));

        // HAS_PARAM is set (load-bearing for the assumptions leg).
        assert!(o1.has_param(&db), "an induction opaque must set HAS_PARAM");
        assert!(!o1.has_var(&db), "an induction opaque must NOT be a var");

        let mut table = UnificationTable::new(&db);
        assert!(table.unify(o1, o1).is_ok(), "an opaque unifies with itself");
        assert!(table.unify(o1, o2).is_err(), "distinct opaques must not unify");

        let concrete = adt_ty(&db, top_mod, "Par");
        assert!(
            table.unify(o1, concrete).is_err(),
            "an opaque must not unify with a concrete type"
        );
    }

    /// No collision with any real param of the def, and goals differing only in
    /// O1 vs O2 canonicalize (as solver queries) differently.
    #[test]
    fn opaque_no_collision_and_distinct_queries() {
        let mut db = HirAnalysisTestDb::default();
        let file = db.new_stand_alone(Utf8PathBuf::from("induct_opaque2.fe"), FIXTURES);
        let (top_mod, _) = db.top_mod(file);
        let rpow = find_tf(&db, top_mod, "RPow");

        let o1 = mint_induction_opaque(&db, rpow, 0, 0, Kind::Star);
        let o2 = mint_induction_opaque(&db, rpow, 0, 1, Kind::Star);

        let real = collect_generic_params(&db, GenericParamOwner::TypeFn(rpow)).params(&db);
        for &p in real {
            assert_ne!(p, o1, "opaque collided with a real param");
        }

        let marker = marker_trait(&db, top_mod);
        let g1 = TraitInstId::new_simple(&db, marker, vec![o1]);
        let g2 = TraitInstId::new_simple(&db, marker, vec![o2]);
        let empty = PredicateListId::empty_list(&db);
        let q1 = CanonicalGoalQuery::new(&db, g1, empty);
        let q2 = CanonicalGoalQuery::new(&db, g2, empty);
        assert_ne!(
            q1.canonical(),
            q2.canonical(),
            "goals differing only in O1 vs O2 must canonicalize differently"
        );
    }

    // ------------------------------------------------------------------
    // Minimal-class recognizer.
    // ------------------------------------------------------------------

    fn symbolic_app<'db>(
        db: &'db HirAnalysisTestDb,
        tf: TypeFnDef<'db>,
        n_type_params: usize,
    ) -> TyId<'db> {
        let params = collect_generic_params(db, GenericParamOwner::TypeFn(tf));
        let mut args = Vec::new();
        for i in 0..n_type_params {
            args.push(params.param_by_original_idx(db, i).unwrap_or_else(|| {
                panic!("missing type param {i}")
            }));
        }
        // The subject is the last original param.
        args.push(
            params
                .param_by_original_idx(db, n_type_params)
                .expect("missing subject param"),
        );
        TyId::foldl(db, TyId::type_fn(db, tf), &args)
    }

    #[test]
    fn minimal_class_admits_rpow() {
        let mut db = HirAnalysisTestDb::default();
        let file = db.new_stand_alone(Utf8PathBuf::from("induct_recog_ok.fe"), FIXTURES);
        let (top_mod, _) = db.top_mod(file);
        let rpow = find_tf(&db, top_mod, "RPow");
        let wf = type_fn_wf(&db, rpow).data.clone().expect("RPow well-formed");
        let marker = marker_trait(&db, top_mod);
        let goal = TraitInstId::new_simple(&db, marker, vec![symbolic_app(&db, rpow, 1)]);
        assert_eq!(minimal_class(&db, &wf, goal), MinimalClass::InClass);
    }

    #[test]
    fn minimal_class_declines_multi_self_call() {
        let mut db = HirAnalysisTestDb::default();
        let file = db.new_stand_alone(Utf8PathBuf::from("induct_recog_bush.fe"), FIXTURES);
        let (top_mod, _) = db.top_mod(file);
        let bush = find_tf(&db, top_mod, "Bush");
        let wf = type_fn_wf(&db, bush).data.clone().expect("Bush well-formed");
        let marker = marker_trait(&db, top_mod);
        let goal = TraitInstId::new_simple(&db, marker, vec![symbolic_app(&db, bush, 0)]);
        assert_eq!(
            minimal_class(&db, &wf, goal),
            MinimalClass::Declined(ClassDecline::MultiSelfCallArm)
        );
    }

    #[test]
    fn minimal_class_declines_g3_bare_subject_const_arg() {
        let mut db = HirAnalysisTestDb::default();
        let file = db.new_stand_alone(Utf8PathBuf::from("induct_recog_g3.fe"), FIXTURES);
        let (top_mod, _) = db.top_mod(file);
        let g3 = find_tf(&db, top_mod, "G3Bad");
        // The def is WELL-FORMED (bare `N` is whitelisted at WF), so the decline
        // is due to the G3 recognizer guard, not a WF failure.
        let wf = type_fn_wf(&db, g3).data.clone().expect("G3Bad well-formed");
        let marker = marker_trait(&db, top_mod);
        let goal = TraitInstId::new_simple(&db, marker, vec![symbolic_app(&db, g3, 1)]);
        assert_eq!(
            minimal_class(&db, &wf, goal),
            MinimalClass::Declined(ClassDecline::BareSubjectConstArgInArm)
        );
    }

    #[test]
    fn minimal_class_declines_ground_subject() {
        let mut db = HirAnalysisTestDb::default();
        let file = db.new_stand_alone(Utf8PathBuf::from("induct_recog_ground.fe"), FIXTURES);
        let (top_mod, _) = db.top_mod(file);
        let rpow = find_tf(&db, top_mod, "RPow");
        let wf = type_fn_wf(&db, rpow).data.clone().expect("RPow well-formed");
        let marker = marker_trait(&db, top_mod);
        let pair = adt_ty(&db, top_mod, "Pair");
        // Ground subject (3), not a bare rigid const param.
        let app = TyId::foldl(&db, TyId::type_fn(&db, rpow), &[pair, usize_subject(&db, 3)]);
        let goal = TraitInstId::new_simple(&db, marker, vec![app]);
        assert_eq!(
            minimal_class(&db, &wf, goal),
            MinimalClass::Declined(ClassDecline::SubjectNotBareRigidConst)
        );
    }

    #[test]
    fn minimal_class_declines_non_type_fn_goal() {
        let mut db = HirAnalysisTestDb::default();
        let file = db.new_stand_alone(Utf8PathBuf::from("induct_recog_nontf.fe"), FIXTURES);
        let (top_mod, _) = db.top_mod(file);
        let rpow = find_tf(&db, top_mod, "RPow");
        let wf = type_fn_wf(&db, rpow).data.clone().expect("RPow well-formed");
        let marker = marker_trait(&db, top_mod);
        let goal = TraitInstId::new_simple(&db, marker, vec![adt_ty(&db, top_mod, "Pair")]);
        assert_eq!(
            minimal_class(&db, &wf, goal),
            MinimalClass::Declined(ClassDecline::NotTypeFnGoal)
        );
    }

    #[test]
    fn minimal_class_declines_type_fn_in_type_arg() {
        let mut db = HirAnalysisTestDb::default();
        let file = db.new_stand_alone(Utf8PathBuf::from("induct_recog_tfarg.fe"), FIXTURES);
        let (top_mod, _) = db.top_mod(file);
        let rpow = find_tf(&db, top_mod, "RPow");
        let wf = type_fn_wf(&db, rpow).data.clone().expect("RPow well-formed");
        let marker = marker_trait(&db, top_mod);

        // Forwarded type arg is itself a (hand-built, unexpanded) type-fn app.
        let pair = adt_ty(&db, top_mod, "Pair");
        let inner = TyId::foldl(&db, TyId::type_fn(&db, rpow), &[pair, usize_subject(&db, 2)]);
        // Outer: RPow<inner, N> with N a bare rigid subject.
        let params = collect_generic_params(&db, GenericParamOwner::TypeFn(rpow));
        let n_param = params.param_by_original_idx(&db, 1).expect("subject");
        let outer = TyId::foldl(&db, TyId::type_fn(&db, rpow), &[inner, n_param]);
        let goal = TraitInstId::new_simple(&db, marker, vec![outer]);
        assert_eq!(
            minimal_class(&db, &wf, goal),
            MinimalClass::Declined(ClassDecline::ArgHasTypeFn)
        );
    }
}
