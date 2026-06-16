//! Derive-provider capability/witness GOAL representation and kind-check
//! (FCO Level 1, "W-D").
//!
//! A derive provider's `derive` fn names a *goal* in type-argument position:
//! `Evidence<Eq<T>>` (the witness parameter) and `ImplBuilder<Eq<T>>` (a
//! `uses (..)` capability). The `Eq<T>` there is a CONSTRAINT, not an ordinary
//! `*`-kinded type, so the ordinary signature checker cannot lower it (it would
//! reject `Eq<T>` as a type application, `2-0011`). Level 0 dodged that by
//! exempting the whole provider signature — which made the goal argument pure
//! decoration: a nonsense trait in `Evidence<..>` compiled silently.
//!
//! This module retires that exemption FOR CONCRETE GOALS via a narrow,
//! analysis-layer carrier. It recognizes the `Evidence`/`ImplBuilder` argument
//! POSITION, extracts the single inner HIR type argument, and lowers THAT via
//! the existing [`lower_hir_constraint_application`] (the W-B lowering). A
//! concrete saturated constraint (`Eq<T>`) lowers to a
//! [`CapabilityGoal::ConcreteTrait`] ([`TraitInstId`]); every non-concrete shape
//! (missing trait, unsaturated `* -> Constraint` head, live `* -> Constraint`
//! param head) is declined to a typed [`GoalError`].
//!
//! This is POSITION-SCOPED: `Evidence`/`ImplBuilder` are NOT modeled as general
//! `Constraint -> *` type constructors (that would require a constraint to be a
//! kind-`Constraint` `TyId`, i.e. `TyData::ConstraintTerm`, which is forbidden
//! here). The inner constraint never travels through the `*`-kinded type walk,
//! never becomes a `TyData` node, and never reaches the solver as a live head:
//! [`CapabilityGoal`] has no variable-head variant by construction.
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
            trait_def::TraitInstId,
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

/// The head-identifier the witness parameter type carries (`Evidence<..>`).
const EVIDENCE_KEY: &str = "Evidence";
/// The head-identifier a generated-impl-builder capability carries
/// (`ImplBuilder<..>`).
const IMPL_BUILDER_KEY: &str = "ImplBuilder";

/// Where a provider names a goal — for naming the position in diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GoalPosition {
    /// The `Evidence<..>` witness parameter.
    Witness,
    /// An `ImplBuilder<..>` capability in the `uses (..)` clause.
    ImplBuilder,
}

impl GoalPosition {
    /// The capability type that names the goal, for diagnostics.
    fn capability_name(self) -> &'static str {
        match self {
            GoalPosition::Witness => EVIDENCE_KEY,
            GoalPosition::ImplBuilder => IMPL_BUILDER_KEY,
        }
    }
}

/// The concrete constraint a provider capability/witness names (`Eq<T>` in
/// `Evidence<Eq<T>>` / `ImplBuilder<Eq<T>>`).
///
/// Compile-time only; never a runtime value; eliminated to a concrete trait
/// obligation before the solver runs. By construction it has NO variable-head
/// variant, so a live head can never be carried.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CapabilityGoal<'db> {
    /// A single applied trait: `Eq<T>` → `TraitInstId{Eq, [T]}` (Self = T).
    ConcreteTrait(TraitInstId<'db>),
}

/// Why a provider goal argument is not a concrete constraint. Each maps to a
/// typed diagnostic (see [`goal_error_diag`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GoalError<'db> {
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

/// One recognized provider goal position, with the result of lowering its goal.
pub(crate) struct ProviderGoal<'db> {
    pub(crate) position: GoalPosition,
    /// The func scope the goal lowers in (for re-resolving on the diag path).
    scope: ScopeId<'db>,
    /// The inner goal's path span (`Eq<T>`), for diagnostics.
    goal_path_span: LazyPathSpan<'db>,
    pub(crate) result: Result<CapabilityGoal<'db>, GoalError<'db>>,
}

/// The provider capability/witness goals for `func`, recognized by position and
/// lowered/kind-checked. Empty when `func` is not a derive-provider `derive` fn.
///
/// This is the SSOT for de-exempting the provider signature: the caller routes
/// the recognized goal positions through here (emitting [`goal_error_diag`])
/// INSTEAD of the ordinary `*`-kinded type walk for those slots.
pub(crate) fn provider_capability_goals<'db>(
    db: &'db dyn HirAnalysisDb,
    func: Func<'db>,
) -> Vec<ProviderGoal<'db>> {
    if !func.is_derive_provider_fn(db) {
        return Vec::new();
    }

    let scope = func.scope();
    let mut goals = Vec::new();

    // Witness parameters: ordinary func params whose type head is `Evidence`.
    for param in func.params(db) {
        let Some(hir_ty) = param.hir_ty(db) else {
            continue;
        };
        let Some(inner) = capability_position_inner(db, hir_ty, EVIDENCE_KEY) else {
            continue;
        };
        // outer path span: `Evidence<Eq<T>>` (mode-stripped param ty path).
        let outer = mode_stripped_ty_span(param.lazy_ty_span(db));
        let goal_path_span = inner_goal_path_span(db, hir_ty, outer);
        let result = lower_goal(db, inner, scope);
        goals.push(ProviderGoal {
            position: GoalPosition::Witness,
            scope,
            goal_path_span,
            result,
        });
    }

    // `uses (..)` capabilities: an `ImplBuilder<..>` key path carries the goal.
    for (idx, effect) in func.effect_params(db).enumerate() {
        let Some(key_path) = effect.key_path(db) else {
            continue;
        };
        if !path_head_is(db, key_path, IMPL_BUILDER_KEY) {
            continue;
        }
        let Some(inner) = inner_goal_of_path(db, key_path) else {
            continue;
        };
        let outer = func.span().effects().param_idx(idx).path();
        let goal_path_span = inner_goal_path_span_from_outer(db, key_path, outer);
        let result = lower_goal(db, inner, scope);
        goals.push(ProviderGoal {
            position: GoalPosition::ImplBuilder,
            scope,
            goal_path_span,
            result,
        });
    }

    goals
}

/// Lower the inner goal HIR type to a [`CapabilityGoal`], or classify why it is
/// not a concrete constraint. Mirrors the where-clause `constraint_application`
/// classification (`WherePredicateView::constraint_application_diags`), so a
/// provider goal is held to the same boundary as `where Eq<T>`.
fn lower_goal<'db>(
    db: &'db dyn HirAnalysisDb,
    goal_hir: HirTypeId<'db>,
    scope: ScopeId<'db>,
) -> Result<CapabilityGoal<'db>, GoalError<'db>> {
    let assumptions = PredicateListId::empty_list(db);

    // The concrete projection: feed the inner type into the W-B lowering. A
    // concrete saturated constraint becomes a `TraitInstId`; everything else
    // declines to `None` and is classified below.
    if let Some(inst) = lower_hir_constraint_application(db, goal_hir, scope, assumptions) {
        return Ok(CapabilityGoal::ConcreteTrait(inst));
    }

    // Declined. Classify by resolving the goal head, exactly like the
    // where-clause path, so the diagnostic distinguishes the failure modes.
    let TypeKind::Path(path) = goal_hir.data(db) else {
        // Not even a path (e.g. a tuple/array goal). No trait head to name.
        return Err(GoalError::Unresolved {
            head: empty_path(db),
        });
    };
    let Some(path) = path.to_opt() else {
        return Err(GoalError::Unresolved {
            head: empty_path(db),
        });
    };
    let head = path.strip_generic_args(db);

    match resolve_path(db, head, scope, assumptions, false) {
        // Resolved to a trait, but the application was not a concrete saturated
        // constraint (no subject / arity / kind) — `lower_hir_constraint_application`
        // already declined it.
        Ok(PathRes::Trait(_)) => Err(GoalError::Unsaturated { head }),
        // A live `* -> Constraint` parameter head (`Evidence<P<T>>`): the
        // abstract-head boundary, named at a typed position.
        Ok(PathRes::Ty(ty)) if is_constraint_ctor(&ty.kind(db)) => match path.ident(db).to_opt() {
            Some(param) => Err(GoalError::LiveHead { param }),
            None => Err(GoalError::Unresolved { head }),
        },
        // Any other non-trait head, or a resolution failure.
        _ => Err(GoalError::Unresolved { head }),
    }
}

/// The typed diagnostic for a goal error. The missing/unsaturated cases name the
/// capability and the goal spelling; the live-head case reuses `6-0008`.
pub(crate) fn goal_error_diag<'db>(
    db: &'db dyn HirAnalysisDb,
    goal: &ProviderGoal<'db>,
) -> Option<TyDiagCollection<'db>> {
    use crate::analysis::name_resolution::diagnostics::PathResDiag;

    let Err(err) = goal.result else {
        return None;
    };
    let span: DynLazySpan = goal.goal_path_span.clone().into();

    match err {
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

/// Whether `path`'s last segment is `name` (head-identifier recognition for the
/// capability/witness *position*, mirroring `validate_provider`'s key match).
fn path_head_is<'db>(db: &'db dyn HirAnalysisDb, path: PathId<'db>, name: &str) -> bool {
    path.ident(db)
        .to_opt()
        .is_some_and(|ident| ident.data(db) == name)
}

/// If `hir_ty` is a `<name><goal>` capability/witness position (after stripping
/// an own/mut mode wrapper), return its single inner goal HIR type.
fn capability_position_inner<'db>(
    db: &'db dyn HirAnalysisDb,
    hir_ty: HirTypeId<'db>,
    name: &str,
) -> Option<HirTypeId<'db>> {
    let path = capability_position_path(db, hir_ty, name)?;
    inner_goal_of_path(db, path)
}

/// The `<name><..>` capability path of `hir_ty`, mode-wrapper stripped.
fn capability_position_path<'db>(
    db: &'db dyn HirAnalysisDb,
    hir_ty: HirTypeId<'db>,
    name: &str,
) -> Option<PathId<'db>> {
    let hir_ty = match hir_ty.data(db) {
        TypeKind::Mode(_, inner) => inner.to_opt()?,
        _ => hir_ty,
    };
    let TypeKind::Path(p) = hir_ty.data(db) else {
        return None;
    };
    let path = p.to_opt()?;
    path_head_is(db, path, name).then_some(path)
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
