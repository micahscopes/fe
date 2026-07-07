//! Type-to-type CTFE minimal induction engine (spec sec 5.3, ladder S2.2a/b;
//! Fable steering-04 §5). S2.2a landed the three trapdoor primitives; S2.2b wires
//! them into the WF discharge sites as [`try_prove_by_induction`] /
//! [`try_discharge_by_induction`], the ENGINE proper.
//!
//! The engine is consulted ONLY from the WF/obligation discharge layer
//! (`check_ty_wf` / `check_trait_inst_wf`'s `UnSat` branch), which is OUTSIDE the
//! tracked `is_query_satisfiable` proof-forest solve (C1). Baseline goals carry no
//! `recursive type fn` head, so the engine is never reached for them
//! (input-disjointness): the 2475 baseline is untouched.
//!
//! The primitives:
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
//!    class, so the engine engages ONLY there and otherwise falls back to the S2.1
//!    assumption path. Every decline carries a [`ClassDecline`] reason.
//!
//! 4. [`try_prove_by_induction`] — the engine: course-of-values induction over the
//!    WF-checked subject metric, one strict solve per arm, per-occurrence opaque
//!    IHs on step arms, ground discharge on base arms. All arms `Proven` ->
//!    lemma `Proven`; any arm `NotProven` -> decline (no negative lemma).

// A handful of the S2.2a `ClassDecline` reasons / primitives are only exercised by
// this module's tests (they gate shapes no in-tree fixture reaches yet); keep the
// dead-code allow so those documented decline reasons stay compiled without
// warnings. The engine entry points and their helpers have real callers.
#![allow(dead_code)]

use crate::Ingot;
use crate::analysis::HirAnalysisDb;
use crate::analysis::ty::const_ty::{ConstTyData, ConstTyId};
use crate::analysis::ty::fold::{TyFoldable, TyFolder};
use crate::analysis::ty::trait_def::TraitInstId;
use crate::analysis::ty::trait_resolution::{
    CanonicalGoalQuery, GoalSatisfiability, PredicateListId, TraitSolveCx, is_query_satisfiable,
};
use crate::analysis::ty::ty_def::{
    InvalidCause, Kind, TyBase, TyData, TyFlags, TyId, TyParam, type_fn_sig,
};
use crate::analysis::ty::ty_lower::lower_hir_ty;
use crate::analysis::ty::type_fn::{
    TypeFnWfData, bare_path_ident, body_root_expr, collect_type_fn_heads, resolve_leaf_scope,
    type_fn_app_head, type_fn_wf,
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

// ---------------------------------------------------------------------------
// 4. The minimal induction engine (steering-04 §5 S2.2b).
// ---------------------------------------------------------------------------

/// Attempt to discharge a symbolic type-fn membership obligation `P(F<..., N>)`
/// by course-of-values induction over the WF-checked subject metric.
///
/// PLACEMENT (C1): this is THE ENGINE. It is consulted ONLY from the WF /
/// obligation discharge layer ([`try_discharge_by_induction`], called from
/// `check_ty_wf` / `check_trait_inst_wf`'s `UnSat` branch), which is OUTSIDE the
/// tracked `is_query_satisfiable` proof-forest solve. It never runs inside a
/// `ProofForest`, and it only ever calls the STRICT [`strict_prove`] helper
/// (never the permissive solver, and never `check_ty_wf`/`check_trait_inst_wf`),
/// so the solver -> engine edge stays one-directional and no salsa cycle exists.
///
/// SCHEME: for each arm of `F` (from [`TypeFnWfData`]):
/// - a BASE arm (no self-call) discharges GROUND (C5): the arm RHS is lowered and
///   the forwarded type args substituted, giving exactly the ground normal form
///   at the matched subject, and `P(body)` is strictly proved under the caller
///   assumptions only;
/// - a STEP arm (with self-calls) replaces EACH self-call occurrence with a FRESH
///   per-occurrence rigid opaque `O_i` (steering-04 §2: distinct rigids prove
///   strictly less than a shared one, the conservative direction), injects the
///   induction hypotheses `{P(O_i)}` into the assumptions, and strictly proves
///   `P(body')` (body with the opaques substituted for the self-calls and the
///   forwarded args substituted for the def's type params).
///
/// Returns [`StrictResult::Proven`] ONLY when EVERY arm is `Proven`; otherwise
/// [`StrictResult::NotProven`], and the caller falls back to the S2.1 assumption
/// route unchanged (no negative lemma is ever recorded). Never emits a partial or
/// uncertain proof. Outside the S2.2b minimal class (per [`minimal_class`]) it
/// declines immediately.
pub(crate) fn try_prove_by_induction<'db>(
    db: &'db dyn HirAnalysisDb,
    solve_cx: TraitSolveCx<'db>,
    goal: TraitInstId<'db>,
) -> StrictResult {
    let self_ty = goal.self_ty(db);
    let Some(head_def) = type_fn_app_head(db, self_ty) else {
        return StrictResult::NotProven;
    };
    let wf_result = type_fn_wf(db, head_def);
    let Some(wf) = wf_result.data.as_ref() else {
        return StrictResult::NotProven;
    };
    // The class gate is the whole soundness argument (steering-04 §2): it rules
    // out multi-self-call arms (the shared-opaque hazard), var/hole/invalid/
    // type-fn args, non-unary/assoc goals, and the G3 bare-subject-value hazard.
    if minimal_class(db, wf, goal) != MinimalClass::InClass {
        return StrictResult::NotProven;
    }

    // Decompose the saturated app: forwarded type args precede the bare subject.
    // `app_args` is the substitution vector, indexed by the def's lowered param
    // index exactly as the S1.5 `Unfolder` indexes its `subst_args`.
    let (_base, app_args) = self_ty.decompose_ty_app(db);
    if app_args.is_empty() {
        return StrictResult::NotProven;
    }

    let ret_kind = type_fn_sig(db, head_def).ret_kind;
    let origin_ingot = solve_cx.origin_ingot();
    let caller_assumptions = solve_cx.assumptions();

    for (arm_idx, arm) in wf.arms.iter().enumerate() {
        // Lower the arm RHS in the def's scope: because that scope is inside a
        // type fn body, the path-lowering site leaves self-calls opaque (live
        // `TyBase::TypeFn` heads), which is exactly what the substitution needs
        // to intercept. Empty assumptions: the arm RHS lowering never solves.
        let body_ty = lower_hir_ty(
            db,
            arm.rhs_ty,
            head_def.scope(),
            PredicateListId::empty_list(db),
        );

        let mut subst = InductionSubst {
            db,
            def: head_def,
            subst_args: app_args,
            arm_idx,
            ret_kind: ret_kind.clone(),
            occurrence: 0,
            opaques: Vec::new(),
        };
        let body = body_ty.fold_with(db, &mut subst);

        // A foreign type-fn head (WF-impossible) tripwires to `Invalid`; decline.
        if body.has_invalid(db) {
            return StrictResult::NotProven;
        }

        let arm_goal = replace_self_ty(db, goal, body);

        let assumptions = if subst.opaques.is_empty() {
            // BASE arm (C5): ground discharge, no IH. The body carries no type-fn
            // head (no self-call, and foreign heads are WF-banned); assert it so a
            // future widening cannot silently strict-solve an un-normalized head.
            debug_assert!(
                collect_type_fn_heads(db, body).is_empty(),
                "a base arm body must be type-fn-head-free"
            );
            caller_assumptions
        } else {
            // STEP arm: inject the per-occurrence induction hypotheses `{P(O_i)}`.
            let mut preds = caller_assumptions.list(db).clone();
            for &opaque in &subst.opaques {
                preds.push(replace_self_ty(db, goal, opaque));
            }
            PredicateListId::new(db, preds)
        };

        if strict_prove(db, origin_ingot, arm_goal, assumptions) != StrictResult::Proven {
            return StrictResult::NotProven;
        }
    }

    StrictResult::Proven
}

/// Discharge site wrapper (C2). On engine `Proven`, consume the lemma by
/// RE-RUNNING the ordinary query with the goal INJECTED as an assumption,
/// accepting only a strict proof. For the opaque type-fn head that proof can come
/// ONLY from that injected assumption (the S2.0 impl-target ban leaves no impl
/// candidate whose self-type head is the opaque type fn), so the discharge is the
/// exact S2.1 assumption route (`ImplementorOrigin::Assumption`): gate-don't-select
/// holds by construction, and mono re-resolves ground. Returns `true` iff the
/// obligation is discharged; `false` falls back to the S2.1 `IllFormed` path.
pub(crate) fn try_discharge_by_induction<'db>(
    db: &'db dyn HirAnalysisDb,
    solve_cx: TraitSolveCx<'db>,
    goal: TraitInstId<'db>,
) -> bool {
    if try_prove_by_induction(db, solve_cx, goal) != StrictResult::Proven {
        return false;
    }
    let injected = {
        let mut preds = solve_cx.assumptions().list(db).clone();
        preds.push(goal);
        PredicateListId::new(db, preds)
    };
    strict_prove(db, solve_cx.origin_ingot(), goal, injected) == StrictResult::Proven
}

/// Rebuild `goal` with its self type replaced by `new_self`, preserving the trait
/// def, the remaining trait args, and any associated-type bindings.
fn replace_self_ty<'db>(
    db: &'db dyn HirAnalysisDb,
    goal: TraitInstId<'db>,
    new_self: TyId<'db>,
) -> TraitInstId<'db> {
    let mut args = goal.args(db).clone();
    args[0] = new_self;
    TraitInstId::new(db, goal.def(db), args, goal.assoc_type_bindings(db).clone())
}

/// One-arm substitution folder (steering-04 §2). It (a) replaces the WHOLE spine
/// of each self-call `F<...>` with a FRESH per-occurrence rigid induction opaque
/// (never recursing into the self-call's args), and (b) substitutes the def's
/// type params with the forwarded application args. Mirrors the S1.5 [`Unfolder`],
/// but mints an opaque hypothesis where the unfolder builds a smaller ground app.
struct InductionSubst<'db, 'a> {
    db: &'db dyn HirAnalysisDb,
    def: TypeFnDef<'db>,
    /// The application's arguments, indexed by the def's lowered param index
    /// (type params first, subject last), exactly as [`Unfolder`]'s `subst_args`.
    subst_args: &'a [TyId<'db>],
    arm_idx: usize,
    /// The def's return kind: the kind of a saturated self-call, hence of the
    /// opaque that abstracts it.
    ret_kind: Kind,
    /// Per-occurrence counter, incremented at each self-call in source order.
    occurrence: usize,
    /// The opaques minted in this arm, in occurrence order, for IH injection.
    opaques: Vec<TyId<'db>>,
}

impl<'db> TyFolder<'db> for InductionSubst<'db, '_> {
    fn fold_ty(&mut self, db: &'db dyn HirAnalysisDb, ty: TyId<'db>) -> TyId<'db> {
        // Intercept a self-call spine BEFORE its subject/args can be folded.
        let (base, _args) = ty.decompose_ty_app(db);
        if let TyData::TyBase(TyBase::TypeFn(d)) = base.data(db) {
            if *d != self.def {
                // Foreign head: WF forbids this in an arm; tripwire to Invalid so
                // the arm declines rather than proving over a foreign application.
                return TyId::invalid(db, InvalidCause::Other);
            }
            let opaque = mint_induction_opaque(
                db,
                self.def,
                self.arm_idx,
                self.occurrence,
                self.ret_kind.clone(),
            );
            self.occurrence += 1;
            self.opaques.push(opaque);
            return opaque;
        }

        match ty.data(db) {
            // The def's type params -> the forwarded application args. The subject
            // (a `ConstTy`-wrapped param, idx == subject_idx) is never a bare
            // `TyParam`, and the G3 class guard bans it from non-self-call value
            // positions, so it is not substituted here.
            TyData::TyParam(p) if !p.is_effect() && p.idx < self.subst_args.len() => {
                self.subst_args[p.idx]
            }
            _ => ty.super_fold_with(db, self),
        }
    }
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

    // ------------------------------------------------------------------
    // 4. The minimal induction engine (S2.2b).
    // ------------------------------------------------------------------

    /// CONSTRAINED-combinator fixture (steering-04 C4): `Comp`'s `Marker` impl is
    /// `impl<A: Marker, B: Marker> Marker for Comp<A, B>`, so the step arm goal
    /// `Marker(Comp<O, F>)` is UnSat without the IH `Marker(O)` and the caller's
    /// `F: Marker`, and Proven with them. This makes the induction hypothesis
    /// LOAD-BEARING (unlike the S2.1 unconstrained impl, which discharges the arm
    /// ignoring the IH). `Bush` is a two-self-call def used for the shared-opaque
    /// decline pin.
    const CONSTRAINED_FIXTURES: &str = r#"
struct Par {}
struct Pair {}
struct Comp<F, G> {}

trait Marker {}
impl Marker for Par {}
impl Marker for Pair {}
impl<A: Marker, B: Marker> Marker for Comp<A, B> {}

struct Requires<T> where T: Marker {}

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
"#;

    /// Full-pass diagnostics for the constrained fixture plus `tail`.
    fn constrained_diags(tail: &str) -> String {
        use crate::test_db::format_diagnostics;
        let src = format!("{CONSTRAINED_FIXTURES}\n{tail}\n");
        let mut db = HirAnalysisTestDb::default();
        let file = db.new_stand_alone(Utf8PathBuf::from("type_fn_s22b_e2e.fe"), &src);
        let (top_mod, _) = db.top_mod(file);
        let diags = db.run_on_top_mod(top_mod);
        format_diagnostics(&db, &diags)
    }

    /// END-TO-END POSITIVE (C4): a generic fn whose signature forces
    /// `RPow<F, N>: Marker` (via the `Requires` wrapper) type-checks with NO
    /// explicit `where RPow<F, N>: Marker` bound — the induction engine proves the
    /// membership from `F: Marker` alone. This is the new proof power.
    #[test]
    fn engine_proves_constrained_rpow_without_where_bound() {
        let rendered = constrained_diags(
            "fn use_it<F: Marker, const N: usize>(x: Requires<RPow<F, N>>) {}",
        );
        assert!(
            rendered.is_empty(),
            "the engine should prove `RPow<F, N>: Marker` from `F: Marker`, but got:\n{rendered}"
        );
    }

    /// END-TO-END NEGATIVE (C4 conservatism): the SAME fn with `F` NOT `Marker`
    /// is correctly DECLINED — the step arm's `Marker(F)` subgoal is UnSat, so the
    /// engine proves nothing and the ordinary trait-bound diagnostic fires. Proof
    /// power is gated on the real precondition, not vacuous.
    #[test]
    fn engine_declines_when_arg_not_marker() {
        let rendered =
            constrained_diags("fn use_it<F, const N: usize>(x: Requires<RPow<F, N>>) {}");
        assert!(
            rendered.contains("is not satisfied") || rendered.contains("doesn't implement"),
            "without `F: Marker` the engine must decline (UnSat), got:\n{rendered}"
        );
    }

    /// The IH ANTI-VACUITY twin (mandatory): build the step-arm goal
    /// `Marker(Comp<O, F>)` directly and show the injected IH `Marker(O)` is
    /// genuinely required — Proven WITH `{Marker(O), Marker(F)}`, NotProven with
    /// only `{Marker(F)}`. If this were vacuous (unconstrained `Comp` impl) both
    /// would pass; the constrained impl makes the hypothesis load-bearing.
    #[test]
    fn engine_ih_is_load_bearing() {
        let mut db = HirAnalysisTestDb::default();
        let file =
            db.new_stand_alone(Utf8PathBuf::from("type_fn_s22b_ih.fe"), CONSTRAINED_FIXTURES);
        let (top_mod, _) = db.top_mod(file);
        let ingot = top_mod.scope().ingot(&db);
        let rpow = find_tf(&db, top_mod, "RPow");
        let marker = marker_trait(&db, top_mod);

        let params = collect_generic_params(&db, GenericParamOwner::TypeFn(rpow));
        let f_param = params.param_by_original_idx(&db, 0).expect("RPow.F");
        // A fresh per-occurrence opaque, exactly as the engine mints for the single
        // self-call in the wildcard (arm index 1, occurrence 0).
        let o = mint_induction_opaque(&db, rpow, 1, 0, Kind::Star);
        let comp = adt_ty(&db, top_mod, "Comp");
        let comp_o_f = TyId::foldl(&db, comp, &[o, f_param]);
        let arm_goal = TraitInstId::new_simple(&db, marker, vec![comp_o_f]);

        let f_marker = TraitInstId::new_simple(&db, marker, vec![f_param]);
        let o_marker = TraitInstId::new_simple(&db, marker, vec![o]);

        let with_ih = PredicateListId::new(&db, vec![f_marker, o_marker]);
        assert_eq!(
            strict_prove(&db, ingot, arm_goal, with_ih),
            StrictResult::Proven,
            "the step arm must be Proven under the IH `Marker(O)` and `Marker(F)`"
        );

        let without_ih = PredicateListId::new(&db, vec![f_marker]);
        assert_eq!(
            strict_prove(&db, ingot, arm_goal, without_ih),
            StrictResult::NotProven,
            "removing the IH `Marker(O)` must make the step arm NotProven (anti-vacuity)"
        );
    }

    /// SHARED-OPAQUE NEGATIVE (steering-04 §2): the two-self-call `Bush` is OUTSIDE
    /// the minimal class (the shared-opaque hazard `Comp<A, A>` lives here), so the
    /// CLASS GATE declines it and the engine proves nothing — the gate, not luck,
    /// prevents an engine from proving a lemma a `Comp<A, A>`-style impl would let
    /// slip. (Per-occurrence distinctness itself is pinned by
    /// `opaque_mint_distinct_and_rigid`.)
    #[test]
    fn engine_declines_multi_self_call_shared_opaque() {
        let mut db = HirAnalysisTestDb::default();
        let file = db.new_stand_alone(Utf8PathBuf::from("type_fn_s22b_bush.fe"), CONSTRAINED_FIXTURES);
        let (top_mod, _) = db.top_mod(file);
        let bush = find_tf(&db, top_mod, "Bush");
        let wf = type_fn_wf(&db, bush).data.clone().expect("Bush well-formed");
        let marker = marker_trait(&db, top_mod);
        let goal = TraitInstId::new_simple(&db, marker, vec![symbolic_app(&db, bush, 0)]);

        assert_eq!(
            minimal_class(&db, &wf, goal),
            MinimalClass::Declined(ClassDecline::MultiSelfCallArm),
            "the class gate must decline the two-self-call def"
        );
        let cx = TraitSolveCx::new(&db, bush.scope());
        assert_eq!(
            try_prove_by_induction(&db, cx, goal),
            StrictResult::NotProven,
            "the engine must not prove a lemma over a multi-self-call def"
        );
    }

    /// The GATE-DON'T-SELECT cross-check (the core S2.2b soundness test). Two legs.
    ///
    /// GATE leg (symbolic): the engine PROVES `Marker(RPow<F, N>)` from `F: Marker`
    /// alone (no `RPow<F, N>: Marker` assumption), and the C2 discharge wrapper
    /// accepts it; drop `F: Marker` and the engine DECLINES (NotProven).
    ///
    /// SELECT leg (ground): for n in {0, 1, 2, 4, 7}, ground resolution of the
    /// un-normalized `Marker(RPow<Pair, n>)` and of the normal form `Marker(NF_n)`
    /// select the IDENTICAL unique implementor (equal `ImplementorId` implies equal
    /// origin + `SelDiscriminator`), pinning a real `Hir` impl, with `select_impl`
    /// Unique on both forms AND `default_tier_selection == None` at every n (the N1
    /// dedup / tier never engages: the engine never proves a coexistence shape).
    #[test]
    fn engine_cross_check_gate_matches_ground_select() {
        use crate::analysis::ty::trait_def::ImplementorOrigin;
        use crate::analysis::ty::trait_resolution::Selection;
        use crate::analysis::ty::type_fn::normalize_type_fn_app;

        let mut db = HirAnalysisTestDb::default();
        let file =
            db.new_stand_alone(Utf8PathBuf::from("type_fn_s22b_xcheck.fe"), CONSTRAINED_FIXTURES);
        let (top_mod, _) = db.top_mod(file);
        let rpow = find_tf(&db, top_mod, "RPow");
        let marker = marker_trait(&db, top_mod);
        let pair_ty = adt_ty(&db, top_mod, "Pair");

        // --- GATE leg: the engine proves the symbolic goal from `F: Marker`. ---
        let params = collect_generic_params(&db, GenericParamOwner::TypeFn(rpow));
        let f_param = params.param_by_original_idx(&db, 0).expect("RPow.F");
        let n_param = params.param_by_original_idx(&db, 1).expect("RPow.N");
        let sym_app = TyId::foldl(&db, TyId::type_fn(&db, rpow), &[f_param, n_param]);
        assert!(
            type_fn_app_head(&db, sym_app).is_some(),
            "symbolic app must keep a live type-fn head"
        );
        let sym_goal = TraitInstId::new_simple(&db, marker, vec![sym_app]);

        let f_marker = TraitInstId::new_simple(&db, marker, vec![f_param]);
        let assumptions = PredicateListId::new(&db, vec![f_marker]);
        let cx_f_marker = TraitSolveCx::new(&db, rpow.scope()).with_assumptions(assumptions);

        assert_eq!(
            try_prove_by_induction(&db, cx_f_marker, sym_goal),
            StrictResult::Proven,
            "the engine must prove `Marker(RPow<F, N>)` from `F: Marker`"
        );
        assert!(
            try_discharge_by_induction(&db, cx_f_marker, sym_goal),
            "the C2 discharge wrapper must accept the engine proof"
        );

        // Conservatism: without `F: Marker` the engine declines.
        let cx_bare = TraitSolveCx::new(&db, rpow.scope());
        assert_eq!(
            try_prove_by_induction(&db, cx_bare, sym_goal),
            StrictResult::NotProven,
            "without `F: Marker` the engine must decline"
        );

        // --- SELECT leg: ground resolution agrees at each n, tier never engages. ---
        let ground_cx = TraitSolveCx::new(&db, top_mod.scope());
        for n in [0u32, 1, 2, 4, 7] {
            let subject = usize_subject(&db, n);
            let app = TyId::foldl(&db, TyId::type_fn(&db, rpow), &[pair_ty, subject]);
            let nf = normalize_type_fn_app(&db, app);
            assert!(
                collect_type_fn_heads(&db, nf).is_empty(),
                "NF_{n} still carries a type-fn head: {}",
                nf.pretty_print(&db)
            );

            let goal_unnorm = TraitInstId::new_simple(&db, marker, vec![app]);
            let goal_norm = TraitInstId::new_simple(&db, marker, vec![nf]);

            let impl_unnorm = match is_goal_satisfiable(&db, ground_cx, goal_unnorm) {
                GoalSatisfiability::Satisfied(sol) => sol.value.implementor,
                other => panic!("Marker(RPow<Pair, {n}>) must be Satisfied ground, got {other:?}"),
            };
            let impl_norm = match is_goal_satisfiable(&db, ground_cx, goal_norm) {
                GoalSatisfiability::Satisfied(sol) => sol.value.implementor,
                other => panic!("Marker(NF_{n}) must be Satisfied ground, got {other:?}"),
            };
            // Equal ImplementorId => equal origin + SelDiscriminator.
            assert_eq!(
                impl_unnorm, impl_norm,
                "gate/select divergence at n={n}: un-normalized and normal form differ"
            );
            assert!(
                matches!(impl_norm.origin(&db), ImplementorOrigin::Hir(_)),
                "ground selection at n={n} must pin a real impl, not an assumption"
            );

            // The tier never engages: the engine never proves a coexistence shape.
            assert!(
                ground_cx.default_tier_selection(&db, goal_norm).is_none(),
                "default_tier_selection must be None on NF_{n} (tier never engaged)"
            );
            assert!(
                ground_cx.default_tier_selection(&db, goal_unnorm).is_none(),
                "default_tier_selection must be None on RPow<Pair, {n}> (tier never engaged)"
            );

            // select_impl returns Unique(that impl) on BOTH forms.
            for (label, goal) in [("normal form", goal_norm), ("un-normalized", goal_unnorm)] {
                match ground_cx.select_impl(&db, goal) {
                    Selection::Unique(sel) => assert_eq!(
                        sel, impl_norm,
                        "select_impl on the {label} at n={n} picked a different impl"
                    ),
                    other => panic!("select_impl on the {label} at n={n} not Unique: {other:?}"),
                }
            }
        }
    }
}
