//! Derive-provider capability/witness GOAL diagnostics (FCO Level 1, "W-D").
//!
//! A derive provider's `derive` fn names a *goal* in type-argument position:
//! `Evidence<Eq<T>>` (the witness parameter/return) and `ImplBuilder<Eq<T>>` (a
//! `uses (..)` capability). The `Eq<T>` there is a CONSTRAINT, not an ordinary
//! `*`-kinded type.
//!
//! As of FCO R2/R3 the GOOD case needs no special handling: `Evidence` /
//! `ImplBuilder` are `Constraint`-kinded constructors (`Constraint -> *`, K04b),
//! a saturated concrete goal `Eq<T>` lowers to a `Constraint`-kinded
//! [`TyData::ConstraintTerm`] (R2), and the provider signature is no longer
//! exempt from the ordinary type walk (R3) — so a valid goal type-checks like any
//! other application and the concrete goal is carried by the `ConstraintTerm` in
//! the lowered signature. This module no longer REPRESENTS the goal (the old
//! `CapabilityGoal::ConcreteTrait(TraitInstId)` carrier value was never read);
//! it only retains the goal-DIAGNOSTIC pass for ILL-FORMED goals, which the
//! ordinary walk does not report well:
//!
//!   - a live `* -> Constraint` parameter head (`Evidence<P<T>>`) is kind-correct
//!     and lowers SILENTLY — only this pass catches it (`6-0008`);
//!   - a bare/unsaturated head (`Evidence<Eq>`) degrades to a generic `2-0006`
//!     instead of the provider-specific guidance (`6-0009`);
//!   - a goal in the `uses (..)` clause is not walked by the ordinary signature
//!     diag at all.
//!
//! The pass classifies each recognized goal position by re-lowering the inner
//! HIR goal exactly as the where-clause path does
//! (`WherePredicateView::constraint_application_diags`), so a provider goal is
//! held to the same boundary as `where Eq<T>`. A concrete saturated goal lowers
//! cleanly and yields no diagnostic; every other shape is classified to a typed
//! [`GoalError`] and reported.
//!
//! Goal positions are recognized by RESOLVED IDENTITY (`core::derive::Evidence` /
//! `::ImplBuilder`), not by the bare names: a user type merely *named* `Evidence`,
//! without `use core::derive::Evidence`, is not a goal position.
//!
//! Placement: this is an analysis-layer helper (post scope-graph merge), NOT the
//! expansion-stage `validate_provider` — [`lower_hir_constraint_application`]
//! reaches the merged scope graph, which the expansion stage must not read
//! (it would salsa-cycle). See `docs/dev/FCO_PROBE_provider_goal_representation.md`.

use crate::{
    analysis::{
        HirAnalysisDb,
        name_resolution::{ExpectedPathKind, PathRes, resolve_path},
        ty::{
            diagnostics::{TraitConstraintDiag, TyDiagCollection},
            trait_lower::lower_hir_constraint_application,
            trait_resolution::PredicateListId,
            ty_def::Kind,
        },
    },
    hir_def::{
        Func, GenericArg, IdentId, PathId,
        scope_graph::ScopeId,
        types::{TypeId as HirTypeId, TypeKind},
    },
    span::{DynLazySpan, path::LazyPathSpan},
};

/// The `core::derive` module that holds the canonical capability/witness types,
/// used for resolved-identity recognition of a goal position.
const DERIVE_MODULE: &str = "derive";
/// The canonical last-segment name of the witness type (`core::derive::Evidence`).
const EVIDENCE_TY: &str = "Evidence";
/// The canonical last-segment name of the builder type
/// (`core::derive::ImplBuilder`).
const IMPL_BUILDER_TY: &str = "ImplBuilder";
/// The canonical last-segment name of the unforgeable override-capability type
/// (`core::derive::Fix`, FCO increment B1a). Recognition of this name is gated by
/// the full `core::derive::Fix` resolved identity, never by the bare spelling.
const FIX_TY: &str = "Fix";

/// A `core::derive` capability/witness type recognized at a goal position by
/// RESOLVED IDENTITY (not by a bare head-identifier string). The position
/// recognition resolves the outer type's head through the func's scope and checks
/// it canonicalizes to one of these (`core::derive::Evidence` / `::ImplBuilder`);
/// a user type merely *named* `Evidence`, without `use core::derive::Evidence`,
/// is not recognized as a goal position.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CapabilityTy {
    Evidence,
    ImplBuilder,
}

impl CapabilityTy {
    /// The canonical last-segment name of this capability type, for diagnostics.
    fn name(self) -> &'static str {
        match self {
            CapabilityTy::Evidence => EVIDENCE_TY,
            CapabilityTy::ImplBuilder => IMPL_BUILDER_TY,
        }
    }
}

/// Where a provider names a goal — for naming the position in diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GoalPosition {
    /// The `Evidence<..>` witness parameter / return.
    Witness,
    /// An `ImplBuilder<..>` capability in the `uses (..)` clause.
    ImplBuilder,
}

impl GoalPosition {
    /// The capability type that names the goal, for diagnostics.
    fn capability_name(self) -> &'static str {
        match self {
            GoalPosition::Witness => CapabilityTy::Evidence.name(),
            GoalPosition::ImplBuilder => CapabilityTy::ImplBuilder.name(),
        }
    }
}

/// Why a provider goal argument is not a concrete constraint. Each maps to a
/// typed diagnostic (see [`goal_error_diag`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GoalError<'db> {
    /// The goal head does not resolve, or resolves outside the trait domain
    /// (e.g. `Evidence<MissingTrait<T>>`) → a name-resolution diagnostic.
    Unresolved { head: PathId<'db> },
    /// A live `* -> Constraint` parameter head (`Evidence<P<T>>`) → the
    /// abstract-head boundary, reusing `6-0008`.
    LiveHead { param: IdentId<'db> },
    /// The head resolves to a trait but the application is not a concrete,
    /// saturated constraint (`Evidence<Eq>`: missing subject / arity / kind) →
    /// an arity/kind diagnostic.
    Unsaturated { head: PathId<'db> },
}

/// One recognized provider goal position whose goal is ILL-FORMED, paired with
/// the data needed to report it.
struct GoalDiag<'db> {
    position: GoalPosition,
    /// The func scope the goal lowers in (for re-resolving on the diag path).
    scope: ScopeId<'db>,
    /// The inner goal's path span (`Eq<T>`), for diagnostics.
    goal_path_span: LazyPathSpan<'db>,
    err: GoalError<'db>,
}

/// Diagnostics for the capability/witness goals of `func`, recognized by position
/// and classified. Empty when `func` is not a derive-provider `derive` fn or when
/// every recognized goal is a well-formed concrete constraint.
///
/// This is the SSOT for the provider-goal boundary diagnostics. The ordinary
/// type walk (de-exempted in R3) type-checks the goal positions and carries a
/// good goal as a `ConstraintTerm`; this pass adds the precise FCO diagnostics
/// for the ill-formed shapes the ordinary walk reports poorly or not at all.
pub(crate) fn provider_goal_diags<'db>(
    db: &'db dyn HirAnalysisDb,
    func: Func<'db>,
) -> Vec<TyDiagCollection<'db>> {
    if !func.is_derive_provider_fn(db) {
        return Vec::new();
    }

    let scope = func.scope();
    let mut diags = Vec::new();
    let mut push = |slot: Option<GoalDiag<'db>>| {
        if let Some(slot) = slot
            && let Some(diag) = goal_error_diag(db, &slot)
        {
            diags.push(diag);
        }
    };

    // Witness parameters: ordinary func params whose type head resolves to
    // `core::derive::Evidence` (by identity, not by the bare name `Evidence`).
    for param in func.params(db) {
        let Some(hir_ty) = param.hir_ty(db) else {
            continue;
        };
        let Some(inner) = capability_position_inner(db, hir_ty, scope, CapabilityTy::Evidence)
        else {
            continue;
        };
        // outer path span: `Evidence<Eq<T>>` (mode-stripped param ty path).
        let outer = mode_stripped_ty_span(param.lazy_ty_span(db));
        let goal_path_span = inner_goal_path_span(db, hir_ty, outer);
        push(classify_goal(db, GoalPosition::Witness, inner, scope, goal_path_span));
    }

    // `uses (..)` capabilities: an `ImplBuilder<..>` key path carries the goal.
    // The head is recognized by RESOLVED identity (`core::derive::ImplBuilder`),
    // not by the bare name `ImplBuilder`. The ordinary signature walk does not
    // visit effect-param types, so this is the only pass that checks this slot.
    for (idx, effect) in func.effect_params(db).enumerate() {
        let Some(key_path) = effect.key_path(db) else {
            continue;
        };
        if !path_head_resolves_to_capability(db, key_path, scope, CapabilityTy::ImplBuilder) {
            continue;
        }
        let Some(inner) = inner_goal_of_path(db, key_path) else {
            continue;
        };
        let outer = func.span().effects().param_idx(idx).path();
        let goal_path_span = inner_goal_path_span_from_outer(db, key_path, outer);
        push(classify_goal(
            db,
            GoalPosition::ImplBuilder,
            inner,
            scope,
            goal_path_span,
        ));
    }

    // Witness RETURN position: `derive(..) -> Evidence<Eq<T>>`. The result is
    // also a witness whose goal must be a concrete constraint. Recognized by
    // resolved identity, exactly like the witness parameter.
    if let Some(ret_hir) = func.ret_ty_hir(db)
        && let Some(inner) = capability_position_inner(db, ret_hir, scope, CapabilityTy::Evidence)
    {
        // The return type carries no own/mut mode wrapper (modes are param-only),
        // so take the path span directly — `mode_stripped_ty_span`'s
        // `into_mode_type()` mis-resolves a non-mode return span to the file root.
        let outer = func.span().ret_ty().into_path_type().path();
        let goal_path_span = inner_goal_path_span(db, ret_hir, outer);
        push(classify_goal(db, GoalPosition::Witness, inner, scope, goal_path_span));
    }

    diags
}

/// Classify the inner goal HIR type: a concrete saturated constraint (`Eq<T>`)
/// is WELL-FORMED (returns `None` — no diagnostic; the ordinary walk lowers it
/// to a `ConstraintTerm`), everything else is classified to why it is not a
/// concrete constraint. Mirrors the where-clause `constraint_application`
/// classification (`WherePredicateView::constraint_application_diags`), so a
/// provider goal is held to the same boundary as `where Eq<T>`.
fn classify_goal<'db>(
    db: &'db dyn HirAnalysisDb,
    position: GoalPosition,
    goal_hir: HirTypeId<'db>,
    scope: ScopeId<'db>,
    goal_path_span: LazyPathSpan<'db>,
) -> Option<GoalDiag<'db>> {
    let err = goal_error(db, goal_hir, scope)?;
    Some(GoalDiag {
        position,
        scope,
        goal_path_span,
        err,
    })
}

/// `None` when the inner goal is a concrete saturated constraint (well-formed);
/// otherwise the classified [`GoalError`].
fn goal_error<'db>(
    db: &'db dyn HirAnalysisDb,
    goal_hir: HirTypeId<'db>,
    scope: ScopeId<'db>,
) -> Option<GoalError<'db>> {
    let assumptions = PredicateListId::empty_list(db);

    // The concrete projection: a saturated concrete constraint lowers via the
    // W-B lowering (the same lowering R2 uses to produce the `ConstraintTerm` in
    // the ordinary walk). If it lowers, the goal is well-formed — no diagnostic.
    if lower_hir_constraint_application(db, goal_hir, scope, assumptions).is_some() {
        return None;
    }

    // Declined. Classify by resolving the goal head, exactly like the
    // where-clause path, so the diagnostic distinguishes the failure modes.
    let TypeKind::Path(path) = goal_hir.data(db) else {
        // Not even a path (e.g. a tuple/array goal). No trait head to name.
        return Some(GoalError::Unresolved {
            head: empty_path(db),
        });
    };
    let Some(path) = path.to_opt() else {
        return Some(GoalError::Unresolved {
            head: empty_path(db),
        });
    };
    let head = path.strip_generic_args(db);

    Some(match resolve_path(db, head, scope, assumptions, false) {
        // Resolved to a trait, but the application was not a concrete saturated
        // constraint (no subject / arity / kind) — `lower_hir_constraint_application`
        // already declined it.
        Ok(PathRes::Trait(_)) => GoalError::Unsaturated { head },
        // A live `* -> Constraint` parameter head (`Evidence<P<T>>`): the
        // abstract-head boundary, named at a typed position.
        Ok(PathRes::Ty(ty)) if is_constraint_ctor(&ty.kind(db)) => match path.ident(db).to_opt() {
            Some(param) => GoalError::LiveHead { param },
            None => GoalError::Unresolved { head },
        },
        // Any other non-trait head, or a resolution failure.
        _ => GoalError::Unresolved { head },
    })
}

/// The typed diagnostic for a goal error. The unsaturated case names the
/// capability and the goal spelling; the live-head case reuses `6-0008`.
fn goal_error_diag<'db>(
    db: &'db dyn HirAnalysisDb,
    goal: &GoalDiag<'db>,
) -> Option<TyDiagCollection<'db>> {
    use crate::analysis::name_resolution::diagnostics::PathResDiag;

    let span: DynLazySpan = goal.goal_path_span.clone().into();

    match goal.err {
        GoalError::LiveHead { param } => {
            // Reuse the abstract-head boundary diagnostic (`6-0008`).
            Some(TraitConstraintDiag::ConstraintCtorParamUnsupported { span, param }.into())
        }
        GoalError::Unsaturated { head } => {
            // The head is a trait but the goal is not a saturated constraint
            // (`Evidence<Eq>`): a provider goal must name a concrete constraint.
            head.ident(db).to_opt().map(|goal_ident| {
                TraitConstraintDiag::ProviderGoalNotConcrete {
                    span,
                    capability: goal.position.capability_name(),
                    goal: goal_ident,
                }
                .into()
            })
        }
        GoalError::Unresolved { head } => {
            // Re-run resolution to produce a precise name-resolution diagnostic
            // (`2-xxxx`) / expected-trait diagnostic, pointing at the goal path.
            let assumptions = PredicateListId::empty_list(db);
            match resolve_path(db, head, goal.scope, assumptions, false) {
                Ok(res) => head
                    .ident(db)
                    .to_opt()
                    .map(|ident| PathResDiag::ExpectedTrait(span, ident, res.kind_name()).into()),
                Err(inner) => inner
                    .into_diag(db, head, goal.goal_path_span.clone(), ExpectedPathKind::Trait)
                    .map(|d| d.into()),
            }
        }
    }
}

// ----------------------------------------------------------------------------
// Position recognition + extraction helpers (position-scoped, K04a-aligned).
// ----------------------------------------------------------------------------

fn empty_path<'db>(db: &'db dyn HirAnalysisDb) -> PathId<'db> {
    PathId::from_ident(db, IdentId::new(db, ""))
}

/// Whether `path`'s head (its segments minus the final generic args) RESOLVES to
/// the canonical `core::derive` capability/witness type `expected`. This is the
/// analysis-layer resolved-identity recognition that retires the bare head-
/// identifier string match: the head is resolved through `scope` (the full merged-
/// graph resolver is available here), and accepted only when it names a struct
/// whose scope identity is `core::derive::<expected>`. A user type merely *named*
/// `Evidence` / `ImplBuilder`, without `use core::derive::..`, does not resolve to
/// the canonical type and is not recognized as a goal position.
fn path_head_resolves_to_capability<'db>(
    db: &'db dyn HirAnalysisDb,
    path: PathId<'db>,
    scope: ScopeId<'db>,
    expected: CapabilityTy,
) -> bool {
    let head = path.strip_generic_args(db);
    let assumptions = PredicateListId::empty_list(db);
    let Ok(PathRes::Ty(ty)) = resolve_path(db, head, scope, assumptions, false) else {
        return false;
    };
    let Some(def_scope) = ty.as_scope(db) else {
        return false;
    };
    scope_is_derive_capability(db, def_scope, expected)
}

/// Whether `def_scope` is the definition scope of the canonical `core::derive`
/// capability type `expected`: its own name is `expected`'s name, its parent
/// module is the `derive` module, and that module lives in the `core` ingot. The
/// `core` ingot + `derive` module qualifier is the resolved identity (stronger
/// than the spelling): a like-named user type in any other module is rejected.
fn scope_is_derive_capability<'db>(
    db: &'db dyn HirAnalysisDb,
    def_scope: ScopeId<'db>,
    expected: CapabilityTy,
) -> bool {
    use common::ingot::IngotKind;

    let name_matches = def_scope
        .name(db)
        .is_some_and(|name| name.data(db) == expected.name());
    if !name_matches {
        return false;
    }
    let Some(module) = def_scope.parent_module(db) else {
        return false;
    };
    let module_is_derive = module
        .name(db)
        .is_some_and(|name| name.data(db) == DERIVE_MODULE);
    let in_core = module.top_mod(db).ingot(db).kind(db) == IngotKind::Core;
    module_is_derive && in_core
}

/// Whether `def_scope` is the definition scope of the canonical unforgeable
/// override-capability type `core::derive::Fix` (FCO increment B1a). Mirrors
/// [`scope_is_derive_capability`] EXACTLY — name `Fix`, parent module `derive`,
/// `core` ingot — so it inherits the same resolved-identity guarantee.
///
/// UNFORGEABILITY (type-confusion blocked). Recognition is the full resolved
/// `core::derive::Fix` identity, NOT the bare spelling: all three of
///   1. the type's own name is `Fix`,
///   2. its parent module is `derive`, and
///   3. that module lives in the `core` ingot (`IngotKind::Core`)
/// must hold. A user type merely *named* `Fix` — in any other module or ingot,
/// imported or not — fails check (2) and/or (3), so it resolves to a DIFFERENT
/// `def_scope` and is granted ZERO override authority. The companion proof that
/// no `Fix<T>` value can be CONSTRUCTED (private field, no ctor, not in prelude)
/// lives on the `Fix` type in `ingots/core/src/derive.fe`.
///
/// INERT (FCO B1a): this is the recognition primitive only. Nothing in the
/// production path consumes it yet — the override gate that turns "scope holds a
/// `core::derive::Fix`" into authority is increment B1b, and minting-at-root /
/// the `fixed` tier are A2 / B2. The unit test
/// `scope_is_fix_capability_keys_on_core_identity_not_name` exercises both the
/// positive (`core::derive::Fix`) and negative (a local `struct Fix<T>`) cases.
// removal target: wired by B1b gate (the consumer that reads this recognition).
#[allow(dead_code)]
fn scope_is_fix_capability<'db>(db: &'db dyn HirAnalysisDb, def_scope: ScopeId<'db>) -> bool {
    use common::ingot::IngotKind;

    let name_matches = def_scope
        .name(db)
        .is_some_and(|name| name.data(db) == FIX_TY);
    if !name_matches {
        return false;
    }
    let Some(module) = def_scope.parent_module(db) else {
        return false;
    };
    let module_is_derive = module
        .name(db)
        .is_some_and(|name| name.data(db) == DERIVE_MODULE);
    let in_core = module.top_mod(db).ingot(db).kind(db) == IngotKind::Core;
    module_is_derive && in_core
}

/// If `hir_ty` is an `Evidence<goal>` / `ImplBuilder<goal>` capability/witness
/// position (after stripping an own/mut mode wrapper) whose head resolves to the
/// canonical capability type `expected`, return its single inner goal HIR type.
fn capability_position_inner<'db>(
    db: &'db dyn HirAnalysisDb,
    hir_ty: HirTypeId<'db>,
    scope: ScopeId<'db>,
    expected: CapabilityTy,
) -> Option<HirTypeId<'db>> {
    let path = capability_position_path(db, hir_ty, scope, expected)?;
    inner_goal_of_path(db, path)
}

/// The capability path of `hir_ty` (mode-wrapper stripped) whose head resolves to
/// the canonical capability type `expected`, or `None`.
fn capability_position_path<'db>(
    db: &'db dyn HirAnalysisDb,
    hir_ty: HirTypeId<'db>,
    scope: ScopeId<'db>,
    expected: CapabilityTy,
) -> Option<PathId<'db>> {
    let hir_ty = match hir_ty.data(db) {
        TypeKind::Mode(_, inner) => inner.to_opt()?,
        _ => hir_ty,
    };
    let TypeKind::Path(p) = hir_ty.data(db) else {
        return None;
    };
    let path = p.to_opt()?;
    path_head_resolves_to_capability(db, path, scope, expected).then_some(path)
}

/// The single inner type argument of a capability/witness path
/// (`Evidence<Eq<T>>` / `ImplBuilder<Eq<T>>` → `Eq<T>`).
fn inner_goal_of_path<'db>(
    db: &'db dyn HirAnalysisDb,
    path: PathId<'db>,
) -> Option<HirTypeId<'db>> {
    let args = path.generic_args(db);
    let GenericArg::Type(ta) = args.data(db).first()? else {
        return None;
    };
    ta.ty.to_opt()
}

/// Navigate the witness param's type span (`own Evidence<Eq<T>>`) down to the
/// inner goal's path span (`Eq<T>`). `hir_ty` is the (possibly mode-wrapped)
/// param HIR type; `outer` is the mode-stripped path span of the capability.
fn inner_goal_path_span<'db>(
    db: &'db dyn HirAnalysisDb,
    hir_ty: HirTypeId<'db>,
    outer: LazyPathSpan<'db>,
) -> LazyPathSpan<'db> {
    let hir_ty = match hir_ty.data(db) {
        TypeKind::Mode(_, inner) => inner.to_opt().unwrap_or(hir_ty),
        _ => hir_ty,
    };
    let last = match hir_ty.data(db) {
        TypeKind::Path(p) => p.to_opt().map(|path| path.len(db).saturating_sub(1)),
        _ => None,
    }
    .unwrap_or(0);
    descend_to_goal(outer, last)
}

/// Same as [`inner_goal_path_span`] but the outer span is already the capability
/// path span and `path` is the capability's HIR path (used for `uses`-key spans).
fn inner_goal_path_span_from_outer<'db>(
    db: &'db dyn HirAnalysisDb,
    path: PathId<'db>,
    outer: LazyPathSpan<'db>,
) -> LazyPathSpan<'db> {
    descend_to_goal(outer, path.len(db).saturating_sub(1))
}

/// `outer` (`Evidence<Eq<T>>`) → segment `last` → first generic arg → its type
/// → that type's path (`Eq<T>`). Falls back to `outer` if the structure does not
/// match (the lazy span machinery degrades to the nearest enclosing node).
fn descend_to_goal<'db>(outer: LazyPathSpan<'db>, last: usize) -> LazyPathSpan<'db> {
    outer
        .segment(last)
        .generic_args()
        .arg(0)
        .into_type_arg()
        .ty()
        .into_path_type()
        .path()
}

/// Strip an own/mut mode wrapper from a parameter's type span, returning the
/// underlying path span.
fn mode_stripped_ty_span<'db>(
    ty_span: crate::span::types::LazyTySpan<'db>,
) -> LazyPathSpan<'db> {
    // `into_mode_type().inner()` degrades to the same span when the type is not a
    // mode type, so this is safe for both `own Evidence<..>` and `Evidence<..>`.
    ty_span.into_mode_type().inner().into_path_type().path()
}

/// Is `kind` the kind `* -> Constraint` of a constraint constructor (a live
/// abstract head)?
fn is_constraint_ctor(kind: &Kind) -> bool {
    matches!(
        kind,
        Kind::Abs(inner)
            if inner.0.does_match(&Kind::Star) && inner.1.does_match(&Kind::Constraint)
    )
}

#[cfg(test)]
mod tests {
    use camino::Utf8PathBuf;

    use super::scope_is_fix_capability;
    use crate::{
        analysis::name_resolution::{PathRes, resolve_path},
        analysis::ty::trait_resolution::PredicateListId,
        hir_def::PathId,
        test_db::HirAnalysisTestDb,
    };

    /// Resolve `path` (as segment strings) from `text`'s file scope to its
    /// definition scope, asserting it resolves to a TYPE (anti-vacuous: a `None`
    /// would make either polarity of the recognition test pass for free).
    fn resolve_to_def_scope(
        db: &mut HirAnalysisTestDb,
        file_name: &str,
        text: &str,
        path: &[&str],
    ) -> bool {
        let file = db.new_stand_alone(Utf8PathBuf::from(file_name), text);
        let (top_mod, _) = db.top_mod(file);
        let scope = top_mod.scope();
        let assumptions = PredicateListId::empty_list(db);
        let path_id = PathId::from_segments(db, path);
        let ty = match resolve_path(db, path_id, scope, assumptions, false) {
            Ok(PathRes::Ty(ty)) | Ok(PathRes::TyAlias(_, ty)) => ty,
            res => panic!("expected {path:?} to resolve to a type, got {res:?}"),
        };
        let def_scope = ty
            .as_scope(db)
            .unwrap_or_else(|| panic!("{path:?} resolved type has no def scope"));
        scope_is_fix_capability(db, def_scope)
    }

    /// The crown-jewel unforgeability property (type-confusion arm): override
    /// authority keys on the FULL resolved `core::derive::Fix` identity
    /// (name + `derive` module + `core` ingot), NEVER on the bare spelling.
    ///
    /// Positive: the canonical `core::derive::Fix` resolves to the `core`-ingot
    /// capability scope and IS recognized.
    ///
    /// Negative (anti-vacuous): a LOCAL `struct Fix<T>` declared in the fixture,
    /// with NO `use core::derive::Fix`, resolves to a DIFFERENT def scope (wrong
    /// parent module / wrong ingot) and is NOT recognized — the identical `Fix`
    /// spelling grants ZERO authority. The `resolve_to_def_scope` helper asserts
    /// each path resolves to a real type, so neither arm can pass vacuously.
    #[test]
    fn scope_is_fix_capability_keys_on_core_identity_not_name() {
        let mut db = HirAnalysisTestDb::default();

        // Positive: the canonical capability type, by its absolute `core` path.
        let recognized = resolve_to_def_scope(
            &mut db,
            "fix_capability_positive.fe",
            "struct Anchor {}\n",
            &["core", "derive", "Fix"],
        );
        assert!(
            recognized,
            "core::derive::Fix must be recognized as the Fix override capability"
        );

        // Negative: a like-named LOCAL type, no import — same spelling, no authority.
        let mut db = HirAnalysisTestDb::default();
        let recognized_local = resolve_to_def_scope(
            &mut db,
            "fix_capability_negative.fe",
            "struct Fix<T> { x: T }\n",
            &["Fix"],
        );
        assert!(
            !recognized_local,
            "a local `struct Fix<T>` (not core::derive::Fix) must be granted no authority"
        );
    }
}
