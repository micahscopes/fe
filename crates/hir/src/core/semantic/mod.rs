//! Semantic traversal surface for HIR items.
//!
//! This module hosts the externally‑facing, semantic methods that callers
//! should use when walking the HIR. Keep raw, syntactic accessors and
//! #[salsa::tracked] implementations in `item.rs`; provide ergonomic,
//! context‑aware helpers here that compose the internal lowering and
//! resolution layers.
//!
//! Design notes
//! - Prefer returning semantic IDs (`TyId`, `TraitInstId`, etc.) or diagnostic
//!   collections from here; do not expose raw HIR nodes.
//! - Avoid env/engine types (`PredicateListId`, solver tables, etc.) in the
//!   public API. Keep environment plumbing internal to semantic helpers or
//!   the analysis layer; callers should ask items/views for semantic answers
//!   instead of pushing assumption lists around.
//! - Keep methods small and capability‑oriented (e.g., generic params,
//!   where‑clauses, signature types). Push per‑node, context‑rich logic into
//!   views when a single method signature becomes unwieldy.
//! - Let the compiler guide additions: ablate public syntactic accessors in
//!   `item.rs` and replace call sites by adding only the minimal semantic
//!   method(s) here.

mod context;
pub(crate) mod header;
pub(crate) use context::{
    ContextAuthority, associated_type_bound_context, captured_assoc_const_context,
    instance_position_context, provisional_opened_method,
};
pub use context::{ImplEnv, SelectedApplication, SolverPremises};
mod context_independence;
pub(crate) use context_independence::WithoutCallerPremises;
pub mod index;
mod provision;
pub use provision::Provision;
pub(crate) use provision::{Chain, Frame};
pub(crate) use provision::{lookup_assumptions, recorded_chain_at, recorded_chain_from_openings};
pub mod reference;
mod storage_layout;
pub mod symbol;
use crate::analysis::HirAnalysisDb;
use crate::analysis::ty::corelib::{resolve_core_trait, resolve_lib_func_path};
use crate::analysis::ty::diagnostics::{ImplDiag, TyLowerDiag};
use crate::analysis::ty::fold::TyFoldable;
use crate::analysis::ty::normalize::normalize_ty;
use crate::analysis::ty::ty_def::Kind;
use crate::analysis::ty::ty_error::collect_hir_ty_diags;
use crate::hir_def::GenericParamListId;
use crate::hir_def::params::KindBound as HirKindBound;
use crate::hir_def::scope_graph::ScopeId;
use crate::{HirDb, SpannedHirDb};
pub use reference::{
    FieldAccessView, HasReferences, MethodCallView, PathView, ReferenceView, Target, UsePathView,
};
use rustc_hash::{FxHashMap, FxHashSet};
pub use storage_layout::{
    AllocatedContractStorageLayout, AssignedLayoutTy, AssignedRootValue, ConcreteRootOccurrence,
    ConcreteRootOccurrenceId, ContractFieldId, ContractLayoutEntry, ContractLayoutEntryKind,
    ContractLayoutError, ContractLayoutParameterOrigin, ContractLayoutPath,
    ContractLayoutPathSegment, ContractLayoutReport, ContractLayoutValue,
    ContractStorageLayoutResult, EnumOverlayGroup, ExplicitRootReservation, FieldStorageLayout,
    LayoutBinding, LayoutBindingLeaf, LayoutInvariantError, LayoutProjection, LayoutSelection,
    LayoutViewError, LayoutViewKind, PlaceStep, RootAllocation, RootCell, RootCellId,
    RootOccurrence, RootOccurrenceId, RootRole, StoragePlace, ValidatedFieldLayoutPlan,
    validate_allocated_contract_layout,
};
pub use symbol::{
    IndexedReference, ReferenceIndex, SignatureWithSpan, SourceLocation, SymbolKind, SymbolView,
    item_kind_to_url_suffix, qualify_path_with_ingot_name, scope_to_doc_path,
};

pub fn lower_hir_kind_local<'db>(
    db: &'db dyn HirAnalysisDb,
    scope: ScopeId<'db>,
    kind: &HirKindBound,
) -> Kind {
    crate::analysis::ty::ty_lower::lower_kind(db, scope, kind)
}
use crate::analysis::ty::binder::Binder;
use crate::hir_def::*;
// When adding real methods, prefer calling internal lowering/normalization here
// rather than exposing raw syntax.
use crate::analysis::ty::adt_def::{AdtCycleMember, AdtDef, AdtField, AdtRef};
use crate::analysis::ty::const_ty::{
    CallableInputLayoutHoleOrigin, ConstTyData, ConstTyId, HoleAnchor, LoweringContext,
};
use crate::analysis::ty::effects::{
    EffectKeyKind, place_effect_provider_param_index_map, resolve_effect_key,
};
use crate::analysis::ty::layout_holes::{
    LayoutPlaceholderPolicy, LayoutRootUse, callable_input_layout_bindings_by_origin,
    collect_layout_hole_tys_in_order, collect_unique_layout_placeholders_in_order_with_policy,
    layout_hole_fallback_ty, substitute_layout_holes_by_placeholder,
    substitute_layout_holes_by_placeholder_in, substitute_layout_placeholders_by_placeholder,
};
use crate::analysis::ty::trait_def::{
    ImplementorId, ImplementorOrigin, TraitInstId, does_impl_trait_conflict, impls_for_trait_def,
};
use crate::analysis::ty::trait_lower::{
    TraitRefLowerError, collect_trait_impls, lower_trait_ref, lower_trait_ref_deferred,
};
use crate::analysis::ty::trait_resolution::constraint::{
    collect_candidate_constraints, collect_constraints,
};
use crate::analysis::ty::ty_def::{TyBase, TyData, TyParam};
use crate::analysis::ty::ty_lower::{GenericParamTypeSet, collect_generic_params};
use crate::analysis::ty::visitor::{TyVisitable, TyVisitor, walk_ty};
use crate::analysis::ty::{
    diagnostics::{TraitConstraintDiag, TyDiagCollection},
    provider::{
        ProviderLayoutEvidence, ProviderSemantics, RootProviderRegistration, RootProviderScope,
        provider_semantics, registered_root_providers,
    },
    trait_resolution::{PredicateListId, TraitSolveCx, WellFormedness, check_ty_wf},
    ty_check::EffectParamSite,
    ty_contains_const_hole,
    ty_def::{InvalidCause, PrimTy, TyId},
    ty_error::{collect_ty_lower_errors, demanded_ground_const_cause, diag_from_invalid_cause},
    ty_lower::{
        TyAlias, lower_callable_input_param_ty, lower_hir_ty, lower_hir_ty_deferred,
        lower_hir_ty_with_minter, lower_layout_root_uses_in_hir_ty, lower_opt_hir_ty,
        lower_type_alias, lower_type_alias_from_hir, lower_type_alias_from_hir_deferred,
        resolve_callable_input_effect_key,
    },
};
use crate::core::adt_lower::{lower_adt, lower_contract_fields};
use common::indexmap::IndexMap;
use indexmap::IndexSet;
use salsa::Update;
// Re-export from crate root for backwards compatibility
pub use crate::diagnosable as diagnostics;

/// Core-exposed entry point for alias lowering. Reads the HIR type_ref (core-visible)
/// and delegates to the analysis helper to keep visibility tight without shims.
pub(crate) fn lower_type_alias_body<'db>(
    db: &'db dyn HirAnalysisDb,
    alias: TypeAlias<'db>,
) -> TyAlias<'db> {
    let hir_ty_opt = alias.type_ref(db).to_opt();
    lower_type_alias_from_hir(db, alias, hir_ty_opt)
}

/// The types a type alias's right-hand side applies as written, over the
/// alias's own parameters; see `TyAlias::applied_types`.
pub(crate) fn alias_applied_types_body<'db>(
    db: &'db dyn HirAnalysisDb,
    alias: TypeAlias<'db>,
) -> Vec<crate::analysis::ty::ty_def::TyId<'db>> {
    let Some(hir_ty) = alias.type_ref(db).to_opt() else {
        return Vec::new();
    };
    // The alias's own lowering context, so constants in its right-hand side
    // are the ones its lowering produced.
    let premises = SolverPremises::for_alias_written_type(db, alias);
    let minter = crate::analysis::ty::const_ty::LoweringContext::for_const_bodies(
        crate::analysis::ty::const_ty::HoleAnchor::AliasTemplate(alias),
        crate::analysis::ty::const_ty::ConstBodyLowering::Eager,
    );
    crate::analysis::ty::ty_error::written_type_uses(
        db,
        hir_ty,
        alias.span().ty(),
        premises,
        &minter,
    )
}

pub(crate) fn lower_type_alias_body_deferred<'db>(
    db: &'db dyn HirAnalysisDb,
    alias: TypeAlias<'db>,
) -> TyAlias<'db> {
    let hir_ty_opt = alias.type_ref(db).to_opt();
    lower_type_alias_from_hir_deferred(db, alias, hir_ty_opt)
}

/// The trait's implicit `Self: Trait` predicate.
pub(crate) fn trait_self_predicate<'db>(
    db: &'db dyn HirAnalysisDb,
    trait_: Trait<'db>,
) -> TraitInstId<'db> {
    TraitInstId::new(db, trait_, trait_.params(db).to_vec(), IndexMap::new())
}

/// The bounds implied by a trait's own `Self: Trait` predicate.
///
/// Header positions such as a trait method signature deliberately withhold the
/// self-predicate from `assumptions`, because assuming it while the trait's
/// interface is still being lowered can recurse through the in-progress
/// definition (see `header_constraints_for`). Its implied bounds are still
/// sound for naming and comparing associated types, which never discharges a
/// goal, so they are reconstructed locally where they are needed.
pub(crate) fn trait_self_implied_bounds<'db>(
    db: &'db dyn HirAnalysisDb,
    trait_: Trait<'db>,
) -> PredicateListId<'db> {
    PredicateListId::new(db, vec![trait_self_predicate(db, trait_)]).extend_all_bounds(db)
}

/// Appends `pred` to `preds` unless it is already present.
fn push_predicate<'db>(
    db: &'db dyn HirAnalysisDb,
    preds: PredicateListId<'db>,
    pred: TraitInstId<'db>,
) -> PredicateListId<'db> {
    if preds.list(db).contains(&pred) {
        return preds;
    }
    let mut merged = preds.list(db).to_vec();
    merged.push(pred);
    PredicateListId::new(db, merged)
}

/// Consolidated assumptions for any item kind. Bounds rejected at their
/// declaration are never assumed (they are still required of callers).
pub fn constraints_for<'db>(
    db: &'db dyn HirAnalysisDb,
    item: ItemKind<'db>,
) -> PredicateListId<'db> {
    use crate::analysis::ty::trait_resolution::constraint::decl_premises;
    match item {
        // Contracts have no generic parameters, so no constraints
        ItemKind::Contract(_) => PredicateListId::empty_list(db),
        ItemKind::Trait(t) => {
            push_predicate(db, decl_premises(db, t.into()), trait_self_predicate(db, t))
        }
        item => GenericParamOwner::from_item_opt(item)
            .filter(|_| {
                matches!(
                    item,
                    ItemKind::Struct(_)
                        | ItemKind::Enum(_)
                        | ItemKind::Func(_)
                        | ItemKind::Impl(_)
                        | ItemKind::ImplTrait(_)
                )
            })
            .map_or_else(
                || PredicateListId::empty_list(db),
                |owner| decl_premises(db, owner),
            ),
    }
}

/// The full assumption set in scope for proving obligations *inside* `item`:
/// the item's declared constraints (including its parents'), the implicit
/// `Self: Trait` predicate for traits and trait items, and all implied bounds
/// (super-trait closure and associated-type bounds).
///
/// Use this for well-formedness and satisfiability *checks*. Lowering an
/// item's own header (super-trait refs, where-clause subjects, generic-param
/// bounds) must use [`header_constraints_for`] instead: assuming `Self: Trait`
/// while the trait's own interface is still being lowered can recurse through
/// the in-progress definition.
pub fn param_env<'db>(db: &'db dyn HirAnalysisDb, item: ItemKind<'db>) -> PredicateListId<'db> {
    item_body_assumptions(db, item, true).extend_all_bounds(db)
}

/// Unelaborated assumptions before effect-provider bounds are available.
pub(crate) fn func_body_assumptions<'db>(
    db: &'db dyn HirAnalysisDb,
    func: Func<'db>,
) -> PredicateListId<'db> {
    item_body_assumptions(db, func.into(), false)
}

/// The single body-premise builder. During inference, effect-provider bounds
/// are supplied by the checker; completed item contexts collect them here.
/// Header formation uses declaration constraints instead of this body view.
fn item_body_assumptions<'db>(
    db: &'db dyn HirAnalysisDb,
    item: ItemKind<'db>,
    include_effect_bounds: bool,
) -> PredicateListId<'db> {
    // Bounds rejected at their declaration are never assumed.
    use crate::analysis::ty::trait_resolution::constraint::{
        collect_func_decl_premises, decl_premises,
    };
    let preds = match item {
        ItemKind::Func(func) if include_effect_bounds => decl_premises(db, func.into()),
        ItemKind::Func(func) => collect_func_decl_premises(db, func.into()),
        item => GenericParamOwner::from_item_opt(item).map_or_else(
            || PredicateListId::empty_list(db),
            |owner| decl_premises(db, owner),
        ),
    };
    let trait_ = match item {
        ItemKind::Trait(trait_) => Some(trait_),
        _ => match item.scope().parent_item(db) {
            Some(ItemKind::Trait(trait_)) => Some(trait_),
            _ => None,
        },
    };
    trait_.map_or(preds, |trait_| {
        push_predicate(db, preds, trait_self_predicate(db, trait_))
    })
}

/// Solver view of the canonical premises at an item position.
fn item_solver_context<'db>(db: &'db dyn HirAnalysisDb, scope: ScopeId<'db>) -> TraitSolveCx<'db> {
    let context = context_at(db, scope, None);
    context.solver_context(db)
}

/// Declared body premises at an anonymous constant's source position.
pub(crate) fn body_assumptions_at<'db>(
    db: &'db dyn HirAnalysisDb,
    scope: ScopeId<'db>,
) -> PredicateListId<'db> {
    let mut item = scope.parent_item(db);
    while let Some(ItemKind::Body(body)) = item {
        item = body.scope().parent_item(db);
    }
    item.map_or_else(
        || PredicateListId::empty_list(db),
        |item| item_body_assumptions(db, item, false),
    )
}

/// Read the item premises and retained lexical selections at a source position.
/// A position outside a body has only its canonical item context. Body choices
/// are read from the existing decision record, never solved again here.
pub fn context_at<'db>(
    db: &'db dyn HirAnalysisDb,
    scope: ScopeId<'db>,
    position: Option<(
        &crate::analysis::ty::ty_check::TypedBody<'db>,
        crate::hir_def::ExprId,
    )>,
) -> crate::analysis::semantic::ImplEnv<'db> {
    if let Some((typed, expr)) = position {
        return provision::recorded_chain_at(db, typed, expr).context(db, scope);
    }
    let premises = param_env(db, enclosing_item(db, scope));
    let premises = match associated_type_at(scope) {
        Some(owner) => associated_type_premises(db, owner, premises),
        None => premises,
    };
    Chain::<()>::new(db, premises).context(db, scope)
}

/// Header formation reads declared bounds before implicit trait Self and
/// effect-provider bounds are available. Keep the unelaborated declaration
/// stage: proof queries elaborate it, without reading completed body output.
pub(crate) fn header_context_at<'db>(
    db: &'db dyn HirAnalysisDb,
    scope: ScopeId<'db>,
) -> crate::analysis::semantic::ImplEnv<'db> {
    use crate::analysis::ty::trait_resolution::constraint::{
        collect_func_decl_premises, decl_premises,
    };
    let premises = match enclosing_item(db, scope) {
        ItemKind::Func(func) => collect_func_decl_premises(db, func.into()),
        item => GenericParamOwner::from_item_opt(item).map_or_else(
            || PredicateListId::empty_list(db),
            |owner| decl_premises(db, owner),
        ),
    };
    Chain::<()>::new(db, premises).context(db, scope)
}

/// Unelaborated declaration predicates. No caller or effect-provider predicates
/// are allowed to influence name resolution within a default.
pub(crate) fn generic_default_context_at<'db>(
    db: &'db dyn HirAnalysisDb,
    owner: GenericParamOwner<'db>,
) -> ImplEnv<'db> {
    let declared = collect_candidate_constraints(db, owner).instantiate_identity();
    let trait_ = match owner {
        GenericParamOwner::Trait(trait_) => Some(trait_),
        GenericParamOwner::Func(func) => match func.scope().parent_item(db) {
            Some(ItemKind::Trait(trait_)) => Some(trait_),
            _ => None,
        },
        _ => None,
    };
    let premises = trait_.map_or(declared, |trait_| {
        declared.merge(
            db,
            PredicateListId::new(db, vec![trait_self_predicate(db, trait_)]),
        )
    });
    Chain::<()>::new(db, premises).context(db, owner.scope())
}

/// Callable input formation keeps declaration-only source numbering before
/// discovered layout/effect slots exist. Completed lowering retains its
/// existing declaration premises; neither stage reads the completed body.
pub(crate) fn callable_input_context_at<'db>(
    db: &'db dyn HirAnalysisDb,
    func: Func<'db>,
    const_bodies: crate::analysis::ty::const_ty::ConstBodyLowering,
) -> ImplEnv<'db> {
    use crate::analysis::ty::const_ty::ConstBodyLowering;
    use crate::analysis::ty::trait_resolution::constraint::{
        collect_callable_shape_constraints, collect_func_decl_premises,
    };
    let premises = match const_bodies {
        ConstBodyLowering::Eager => collect_func_decl_premises(db, func.into()),
        ConstBodyLowering::Deferred => collect_callable_shape_constraints(db, func),
    };
    Chain::<()>::new(db, premises).context(db, func.scope())
}

/// Declaration-phase premises at a position, for readers that run while a
/// function's effect-provider bounds are still being computed. Asking for the
/// completed premises there would re-enter the query that builds them, so a
/// function position reads its declared bounds only. Other positions have no
/// such phase and read `context_at`.
pub(crate) fn declaration_context_at<'db>(
    db: &'db dyn HirAnalysisDb,
    scope: ScopeId<'db>,
) -> crate::analysis::semantic::ImplEnv<'db> {
    match enclosing_item(db, scope) {
        ItemKind::Func(func) => {
            Chain::<()>::new(db, func_body_assumptions(db, func).extend_all_bounds(db))
                .context(db, scope)
        }
        _ => context_at(db, scope, None),
    }
}

/// The item whose premises hold at `scope`; a body inherits its owner's.
fn enclosing_item<'db>(db: &'db dyn HirAnalysisDb, scope: ScopeId<'db>) -> ItemKind<'db> {
    let mut enclosing = scope;
    while matches!(enclosing.item(), ItemKind::Body(_)) {
        let Some(parent) = enclosing.parent(db) else {
            break;
        };
        enclosing = parent;
    }
    enclosing.item()
}

/// The associated type whose own parameters are in scope at `scope`.
fn associated_type_at<'db>(
    scope: ScopeId<'db>,
) -> Option<crate::hir_def::scope_graph::AssocTypeOwner<'db>> {
    match scope {
        ScopeId::AssocTypeParam(owner, _) => Some(owner),
        scope => scope.assoc_type_owner(),
    }
}

/// An associated type's own parameter bounds hold wherever its parameters are
/// in scope. They come from the trait's declaration; an impl's definition gets
/// the trait's bounds with the impl's arguments, never its own written bounds,
/// so an impl cannot assume more than the trait's callers guarantee.
fn associated_type_premises<'db>(
    db: &'db dyn HirAnalysisDb,
    owner: crate::hir_def::scope_graph::AssocTypeOwner<'db>,
    premises: PredicateListId<'db>,
) -> PredicateListId<'db> {
    use crate::hir_def::scope_graph::AssocTypeOwner;
    match owner {
        AssocTypeOwner::Trait(owner, idx) => TraitAssocTypeView {
            owner,
            idx: idx as usize,
        }
        .with_parameter_bounds(db, premises),
        AssocTypeOwner::Impl(owner, idx) => ImplAssocTypeView {
            owner,
            idx: idx as usize,
        }
        .with_parameter_bounds(db, premises),
    }
}

/// The direct trait goal belongs to the accepted record, not a copied callable link.
fn call_trait_goal_at<'db>(
    db: &'db dyn HirAnalysisDb,
    typed: &crate::analysis::ty::ty_check::TypedBody<'db>,
    expr: ExprId,
) -> Option<TraitInstId<'db>> {
    let callable = typed.callable_expr(expr)?;
    let CallableDef::Func(func) = callable.callable_def() else {
        return None;
    };
    if func.containing_trait(db).is_none() && func.containing_impl_trait(db).is_none() {
        return None;
    }
    let body = typed.body().expect("a source call must retain its body");
    Some(
        body.decision_at(typed, expr.span(body).into())
            .expect("a source trait call must retain its accepted choice")
            .data(db)
            .goal(),
    )
}

/// Project the declared bounds of a recorded answer without selecting again.
pub(crate) fn recorded_impl_bounds<'db>(
    db: &'db dyn HirAnalysisDb,
    context: crate::analysis::semantic::ImplEnv<'db>,
    goal: TraitInstId<'db>,
    via: crate::analysis::ty::ty_check::TraitChoiceVia<'db>,
) -> PredicateListId<'db> {
    let selected = match via {
        crate::analysis::ty::ty_check::TraitChoiceVia::Implementation(selected) => selected,
        crate::analysis::ty::ty_check::TraitChoiceVia::Selected(selection) => selection.candidate(),
    };
    let ImplementorOrigin::Hir(item) = selected.origin(db) else {
        return PredicateListId::empty_list(db);
    };
    let mut normalizer =
        crate::analysis::ty::normalize::TypeNormalizer::new(db, context.premises(db));
    let goal = goal.fold_with(db, &mut normalizer);
    let selected = selected.fold_with(db, &mut normalizer);
    let instance = crate::analysis::ty::trait_def::instantiate_selected_impl_with_normalizer(
        db,
        selected,
        goal,
        Some(&mut normalizer),
    )
    .unwrap_or_else(|| {
        panic!(
            "an accepted implementation must match its recorded goal: answer={}, goal={}",
            selected.trait_(db).pretty_print(db, true),
            goal.pretty_print(db, true),
        )
    });
    collect_candidate_constraints(db, item.into()).instantiate(db, instance.impl_args(db))
}

/// Read nominal requirements and bounds of the already accepted implementation.
pub(crate) fn call_requirements_at<'db>(
    db: &'db dyn HirAnalysisDb,
    typed: &crate::analysis::ty::ty_check::TypedBody<'db>,
    expr: ExprId,
) -> PredicateListId<'db> {
    // A function value has path-bound records even without a call entry.
    let declared = typed
        .callable_expr(expr)
        .map(|callable| callable.selection_requirements(db))
        .unwrap_or_else(|| PredicateListId::empty_list(db));
    let Some(body) = typed.body() else {
        return declared;
    };
    let mut requirements = declared.list(db).to_vec();
    // Proving a goal consumes its visible provision just like a trait call.
    // Retain the operand through the existing call-evidence projection.
    if let Some(callable) = typed.callable_expr(expr)
        && let crate::hir_def::CallableDef::Func(func) = callable.callable_def()
        && crate::analysis::ty::corelib::lib_func_matches(db, func, "core::obligations::prove")
        && let [term] = callable.generic_args()
        && let crate::analysis::ty::ty_def::TyData::ConstraintTerm(goal) = term.data(db)
    {
        requirements.push(*goal);
    }
    for choice in typed.trait_choices(db).filter(|choice| {
        choice
            .call_constraint()
            .is_some_and(|(call, _, _)| call == expr)
            || choice.site()
                == crate::analysis::ty::ty_check::DecisionSite::trait_reference(
                    expr.span(body).into(),
                )
    }) {
        requirements.push(choice.goal());
        requirements.extend(
            recorded_impl_bounds(
                db,
                context_at(db, body.scope(), Some((typed, expr))),
                choice.goal(),
                choice.via(),
            )
            .list(db),
        );
    }
    PredicateListId::new(db, requirements).extend_all_bounds(db)
}

/// Read lexical evidence operands restricted to the checked call's requirements.
pub(crate) fn call_evidence_at<'db>(
    db: &'db dyn HirAnalysisDb,
    typed: &crate::analysis::ty::ty_check::TypedBody<'db>,
    expr: ExprId,
) -> Vec<(ExprId, TraitInstId<'db>)> {
    if typed.with_choices().len() == 0 {
        return Vec::new();
    }
    if typed.callable_expr(expr).is_none() {
        return Vec::new();
    }
    let evidence = recorded_chain_at(db, typed, expr)
        .opened_evidence(db)
        .collect::<Vec<_>>();
    if evidence.is_empty() {
        return evidence;
    }
    let requirements = call_requirements_at(db, typed, expr);
    let Some(body) = typed.body() else {
        return Vec::new();
    };
    let ingot = body.scope().ingot(db);
    evidence
        .into_iter()
        .filter(|(_, goal)| requirements_reach_goal(db, ingot, requirements, *goal))
        .collect()
}

fn matching_opened_call<'db>(
    db: &'db dyn HirAnalysisDb,
    goal: TraitInstId<'db>,
    provisions: Vec<(ExprId, TraitInstId<'db>)>,
) -> Option<(ExprId, TraitInstId<'db>)> {
    provisions.into_iter().rev().find(|(_, opened)| {
        opened.def(db) == goal.def(db)
            && opened.args(db) == goal.args(db)
            && goal
                .assoc_type_bindings(db)
                .iter()
                .all(|(name, ty)| opened.assoc_type_bindings(db).get(name) == Some(ty))
    })
}

pub(crate) fn opened_call_at<'db>(
    db: &'db dyn HirAnalysisDb,
    typed: &crate::analysis::ty::ty_check::TypedBody<'db>,
    expr: ExprId,
) -> Option<(ExprId, TraitInstId<'db>)> {
    let provisions = call_evidence_at(db, typed, expr);
    if provisions.is_empty() {
        return None;
    }
    matching_opened_call(db, call_trait_goal_at(db, typed, expr)?, provisions)
}

/// Freeze applicability in the checked source, then read instantiated operands.
/// Substitution must not let a concrete opening capture a formerly generic bound.
pub(crate) fn instance_call_evidence_at<'db>(
    db: &'db dyn HirAnalysisDb,
    key: crate::analysis::semantic::SemanticInstanceKey<'db>,
    expr: ExprId,
) -> Vec<(ExprId, TraitInstId<'db>)> {
    let template = crate::analysis::semantic::typed_body_template(db, key.owner(db));
    let applicable = call_evidence_at(db, &template.body, expr);
    if applicable.is_empty() {
        return applicable;
    }
    let typed = key.typed_body(db);
    applicable
        .into_iter()
        .map(|(value, _)| {
            let (_, goal) = typed
                .with_choice(value)
                .expect("an applicable opening must retain its record")
                .outcome()
                .expect("an applicable opening must remain accepted")
                .evidence_operand()
                .expect("an evidence opening must retain its source kind");
            (value, goal)
        })
        .collect()
}

pub(crate) fn instance_opened_call_at<'db>(
    db: &'db dyn HirAnalysisDb,
    key: crate::analysis::semantic::SemanticInstanceKey<'db>,
    expr: ExprId,
) -> Option<(ExprId, TraitInstId<'db>)> {
    let provisions = instance_call_evidence_at(db, key, expr);
    if provisions.is_empty() {
        return None;
    }
    matching_opened_call(
        db,
        call_trait_goal_at(db, key.typed_body(db), expr)?,
        provisions,
    )
}

/// The selections a call is allowed to pass on at its recorded position.
/// Freeze against the checked declaration before generic specialization, so
/// an unrelated concrete choice cannot become applicable to a rigid parameter.
pub(crate) fn call_selections_at<'db>(
    db: &'db dyn HirAnalysisDb,
    typed: &crate::analysis::ty::ty_check::TypedBody<'db>,
    expr: crate::hir_def::ExprId,
) -> Vec<crate::analysis::semantic::SelectedApplication<'db>> {
    let selections = typed.selections_for_expr(db, expr);
    if selections.is_empty() {
        return Vec::new();
    }
    let requirements = call_requirements_at(db, typed, expr);
    selections
        .iter()
        .copied()
        .filter(|selection| requirements_reach_selection(db, requirements, *selection))
        .collect()
}

/// A condition on a recorded call sees precisely the named applications that
/// the call can transport, frozen before generic specialization.
pub(crate) fn call_condition_context_at<'db>(
    db: &'db dyn HirAnalysisDb,
    scope: ScopeId<'db>,
    typed: &crate::analysis::ty::ty_check::TypedBody<'db>,
    expr: crate::hir_def::ExprId,
) -> crate::analysis::semantic::ImplEnv<'db> {
    context_at(db, scope, Some((typed, expr)))
        .with_selections(db, call_selections_at(db, typed, expr))
}

/// Whether proving `requirements` can read `selection`; see
/// [`requirements_reach_goal`].
pub(crate) fn requirements_reach_selection<'db>(
    db: &'db dyn HirAnalysisDb,
    requirements: PredicateListId<'db>,
    selection: crate::analysis::semantic::SelectedApplication<'db>,
) -> bool {
    requirements_reach_goal(
        db,
        selection.scope().ingot(db),
        requirements,
        selection.goal(),
    )
}

/// Whether proving `requirements` can read a provision for `goal`, of either
/// spelling: `goal` is one of them or a prerequisite they reach through impl
/// bounds, at any depth. A concrete goal written in a callee's body cannot read
/// its caller's provisions, and a goal written with parameters is not the
/// provided concrete goal, so a provision no requirement reaches is not
/// carried into the callee.
pub(crate) fn requirements_reach_goal<'db>(
    db: &'db dyn HirAnalysisDb,
    ingot: crate::Ingot<'db>,
    requirements: PredicateListId<'db>,
    goal: TraitInstId<'db>,
) -> bool {
    let matches = |required: &TraitInstId<'db>| {
        required.def(db) == goal.def(db) && required.args(db) == goal.args(db)
    };
    requirements.list(db).iter().any(matches)
        || crate::analysis::ty::trait_def::goals_reached_through_impl_bounds(
            db,
            ingot,
            requirements,
        )
        .iter()
        .any(matches)
}

/// Trait header validation must not assume `Self: Trait` yet.
///
/// The synthetic self-predicate from [`constraints_for`] is only valid once the trait
/// interface has been established. Using it while lowering the trait's own super-traits or
/// where-predicate headers can make projection resolution recurse through the in-progress trait
/// definition.
pub(crate) fn header_constraints_for<'db>(
    db: &'db dyn HirAnalysisDb,
    item: ItemKind<'db>,
) -> PredicateListId<'db> {
    header_context_at(db, item.scope()).assumptions(db)
}

type CallableInputLayoutArgs<'db> =
    FxHashMap<CallableInputLayoutHoleOrigin, FxHashMap<TyId<'db>, TyId<'db>>>;

fn callable_input_layout_args<'db>(
    db: &'db dyn HirAnalysisDb,
    func: Func<'db>,
) -> CallableInputLayoutArgs<'db> {
    callable_input_layout_bindings_by_origin(db, CallableDef::Func(func))
        .into_iter()
        .map(|(origin, bindings)| (origin, bindings.into_iter().collect()))
        .collect()
}

fn canonicalize_effect_binding_trait_inst<'db>(
    db: &'db dyn HirAnalysisDb,
    trait_inst: TraitInstId<'db>,
) -> TraitInstId<'db> {
    let mut assoc = trait_inst
        .assoc_ty_bindings(db)
        .into_iter()
        .collect::<Vec<_>>();
    assoc.sort_by_key(|(lhs, _)| *lhs);
    TraitInstId::new(
        db,
        trait_inst.def(db),
        trait_inst.args(db).to_vec(),
        assoc.into_iter().collect::<IndexMap<_, _>>(),
    )
}

fn contract_effect_hidden_param_scope<'db>(
    _db: &'db dyn HirDb,
    site: EffectParamSite<'db>,
) -> ScopeId<'db> {
    match site {
        EffectParamSite::Contract(contract)
        | EffectParamSite::ContractInit { contract }
        | EffectParamSite::ContractRecvArm { contract, .. } => contract.scope(),
        EffectParamSite::Func(_) => {
            unreachable!("contract effect hidden params must use a contract-scoped site")
        }
    }
}

fn contract_effect_layout_param_name<'db>(
    db: &'db dyn HirDb,
    site: EffectParamSite<'db>,
    binding_idx: u32,
    layout_idx: usize,
) -> IdentId<'db> {
    let site_name = |ident: Option<IdentId<'db>>, fallback: &str| {
        ident.map_or_else(|| fallback.to_string(), |ident| ident.data(db).to_string())
    };
    let prefix = match site {
        EffectParamSite::Contract(contract) => {
            format!(
                "__contract{}_efflayout",
                site_name(contract.name(db).to_opt(), "contract")
            )
        }
        EffectParamSite::ContractInit { contract } => {
            format!(
                "__init{}_efflayout",
                site_name(contract.name(db).to_opt(), "contract")
            )
        }
        EffectParamSite::ContractRecvArm {
            contract,
            recv_idx,
            arm_idx,
        } => format!(
            "__recv{}_{}_{}_efflayout",
            site_name(contract.name(db).to_opt(), "contract"),
            recv_idx,
            arm_idx
        ),
        EffectParamSite::Func(_) => {
            unreachable!("contract effect hidden params must use a contract-scoped site")
        }
    };
    IdentId::new(db, format!("{prefix}{binding_idx}_{layout_idx}"))
}

fn contract_effect_hidden_param_ty<'db>(
    db: &'db dyn HirAnalysisDb,
    site: EffectParamSite<'db>,
    binding_idx: u32,
    layout_idx: usize,
    placeholder: TyId<'db>,
) -> TyId<'db> {
    let TyData::ConstTy(const_ty) = placeholder.data(db) else {
        return placeholder;
    };
    let fallback_ty = match const_ty.data(db) {
        ConstTyData::Hole(hole_ty, _) => layout_hole_fallback_ty(db, *hole_ty),
        ConstTyData::TyParam(_, fallback_ty) => *fallback_ty,
        _ => return placeholder,
    };
    let param = TyParam::implicit_param(
        contract_effect_layout_param_name(db, site, binding_idx, layout_idx),
        layout_idx,
        fallback_ty.kind(db).clone(),
        contract_effect_hidden_param_scope(db, site),
    );
    TyId::new(
        db,
        TyData::ConstTy(ConstTyId::new(db, ConstTyData::TyParam(param, fallback_ty))),
    )
}

fn canonicalize_contract_effect_key_value<'db, T>(
    db: &'db dyn HirAnalysisDb,
    site: EffectParamSite<'db>,
    binding_idx: u32,
    value: T,
) -> T
where
    T: TyFoldable<'db> + TyVisitable<'db> + Copy,
{
    let placeholders = collect_unique_layout_placeholders_in_order_with_policy(
        db,
        value,
        LayoutPlaceholderPolicy::HolesAndImplicitParams,
    );
    if placeholders.is_empty() {
        return value;
    }

    let layout_args = placeholders
        .into_iter()
        .enumerate()
        .map(|(layout_idx, placeholder)| {
            (
                placeholder,
                contract_effect_hidden_param_ty(db, site, binding_idx, layout_idx, placeholder),
            )
        })
        .collect::<FxHashMap<_, _>>();
    substitute_layout_placeholders_by_placeholder(
        db,
        value,
        &layout_args,
        LayoutPlaceholderPolicy::HolesAndImplicitParams,
    )
}

fn canonicalize_contract_effect_key<'db>(
    db: &'db dyn HirAnalysisDb,
    site: EffectParamSite<'db>,
    binding_idx: u32,
    key_ty: Option<TyId<'db>>,
    key_trait: Option<TraitInstId<'db>>,
) -> (Option<TyId<'db>>, Option<TraitInstId<'db>>) {
    let key_ty = key_ty.map(|ty| canonicalize_contract_effect_key_value(db, site, binding_idx, ty));
    let key_trait = key_trait
        .map(|trait_inst| canonicalize_contract_effect_key_value(db, site, binding_idx, trait_inst))
        .map(|trait_inst| canonicalize_effect_binding_trait_inst(db, trait_inst));
    (key_ty, key_trait)
}

fn func_effect_requirements_canonical<'db>(
    db: &'db dyn HirAnalysisDb,
    func: Func<'db>,
) -> Vec<EffectRequirement<'db>> {
    let assumptions = crate::analysis::ty::trait_resolution::constraint::collect_func_decl_premises(
        db,
        func.into(),
    );
    let layout_args = callable_input_layout_args(db, func);
    func.effects(db)
        .data(db)
        .iter()
        .enumerate()
        .filter_map(|(idx, effect)| {
            let key_syntax = effect.key_ty.to_opt()?;
            let binding_name = effect
                .name
                .or_else(|| key_syntax.as_path(db)?.ident(db).to_opt())
                .unwrap_or_else(|| IdentId::new(db, "_effect".to_string()));
            let (key_kind, key_ty, key_trait) =
                resolve_callable_input_effect_key(db, func, idx, key_syntax, assumptions)
                    .into_parts(db);
            let effect_layout_args = layout_args.get(&CallableInputLayoutHoleOrigin::Effect(idx));
            let key_ty = key_ty.map(|ty| {
                if !ty_contains_const_hole(db, ty) {
                    return ty;
                }
                let Some(effect_layout_args) = effect_layout_args else {
                    return ty;
                };
                let ty = substitute_layout_holes_by_placeholder(db, ty, effect_layout_args);
                debug_assert!(
                    !ty_contains_const_hole(db, ty) || ty.has_invalid(db),
                    "unelaborated layout hole remained in callable effect key type"
                );
                ty
            });
            let key_trait = key_trait.map(|trait_inst| {
                if collect_layout_hole_tys_in_order(db, trait_inst).is_empty() {
                    return canonicalize_effect_binding_trait_inst(db, trait_inst);
                }
                let Some(effect_layout_args) = effect_layout_args else {
                    return canonicalize_effect_binding_trait_inst(db, trait_inst);
                };
                canonicalize_effect_binding_trait_inst(
                    db,
                    substitute_layout_holes_by_placeholder_in(db, trait_inst, effect_layout_args),
                )
            });
            Some(EffectRequirement {
                binding_name,
                key: EffectRequirementKey::from_parts(key_kind, key_ty, key_trait),
                is_mut: effect.is_mut,
                binding_site: EffectParamSite::Func(func),
                binding_idx: idx as u32,
                binding_ty: key_syntax,
            })
        })
        .collect()
}

fn func_provider_bindings_canonical<'db>(
    db: &'db dyn HirAnalysisDb,
    func: Func<'db>,
) -> Vec<ProviderBinding<'db>> {
    let requirements = func_effect_requirements_canonical(db, func);
    let assumptions = crate::analysis::ty::trait_resolution::constraint::collect_func_decl_premises(
        db,
        func.into(),
    );
    let scope = func.scope();
    let provider_map = place_effect_provider_param_index_map(db, func);
    let provider_params = CallableDef::Func(func).params(db);
    let mut providers = requirements
        .iter()
        .filter(|requirement| {
            matches!(
                requirement.key.kind(),
                EffectKeyKind::Type | EffectKeyKind::Trait
            )
        })
        .filter_map(|requirement| {
            let provider_param_idx = provider_map
                .get(requirement.binding_idx as usize)
                .copied()
                .flatten()?;
            let provider_ty = *provider_params.get(provider_param_idx)?;
            Some(ProviderBinding {
                provider_idx: requirement.binding_idx,
                provider_ty,
                is_mut: requirement.is_mut,
                source: ProviderSource::UsesParam {
                    site: EffectParamSite::Func(func),
                    requirement_idx: requirement.binding_idx,
                },
                semantics: provider_semantics(db, scope, assumptions, provider_ty),
                layout_env: None,
            })
        })
        .collect::<Vec<_>>();

    let base_provider_idx = providers.len();
    providers.extend(
        registered_root_providers(db, EffectParamSite::Func(func))
            .iter()
            .cloned()
            .enumerate()
            .map(|(idx, registration)| {
                let provider_ty = registration.provider_ty;
                ProviderBinding {
                    provider_idx: (base_provider_idx + idx) as u32,
                    provider_ty,
                    is_mut: true,
                    source: ProviderSource::RootProvider {
                        scope: RootProviderScope::Func(func),
                        registration,
                    },
                    semantics: provider_semantics(db, scope, assumptions, provider_ty),
                    layout_env: None,
                }
            }),
    );
    providers
}

fn func_effect_resolutions_canonical<'db>(
    db: &'db dyn HirAnalysisDb,
    func: Func<'db>,
) -> Vec<ResolvedEffectBinding> {
    let providers = provider_bindings_for_site(db, EffectParamSite::Func(func));
    let explicit_provider_idx = providers
        .iter()
        .filter_map(|provider| match provider.source {
            ProviderSource::UsesParam {
                requirement_idx, ..
            } => Some((
                requirement_idx,
                (provider.provider_idx, provider.semantics.kind),
            )),
            ProviderSource::RootProvider { .. } | ProviderSource::ContractField { .. } => None,
        })
        .collect::<FxHashMap<_, _>>();
    let root_provider_idx = providers.iter().find_map(|provider| match provider.source {
        ProviderSource::RootProvider { .. } => Some(provider.provider_idx),
        ProviderSource::UsesParam { .. } | ProviderSource::ContractField { .. } => None,
    });

    func_effect_requirements_canonical(db, func)
        .into_iter()
        .filter(|requirement| {
            matches!(
                requirement.key.kind(),
                EffectKeyKind::Type | EffectKeyKind::Trait
            )
        })
        .filter_map(|requirement| {
            explicit_provider_idx
                .get(&requirement.binding_idx)
                .map(|(provider_idx, _)| *provider_idx)
                .or(root_provider_idx)
                .map(|provider_idx| ResolvedEffectBinding {
                    requirement_idx: requirement.binding_idx,
                    provider_idx,
                })
        })
        .collect()
}

fn contract_effect_requirements_canonical<'db>(
    db: &'db dyn HirAnalysisDb,
    contract: Contract<'db>,
) -> Vec<EffectRequirement<'db>> {
    let assumptions = PredicateListId::empty_list(db);
    let site = EffectParamSite::Contract(contract);
    contract
        .effects(db)
        .data(db)
        .iter()
        .enumerate()
        .filter_map(|(idx, effect)| {
            let key_syntax = effect.key_ty.to_opt()?;
            let binding_name = effect
                .name
                .or_else(|| key_syntax.as_path(db)?.ident(db).to_opt())
                .unwrap_or_else(|| IdentId::new(db, "_effect".to_string()));
            let (key_kind, key_ty, key_trait) =
                resolve_effect_key(db, key_syntax, contract.scope(), assumptions).into_parts(db);
            let (key_ty, key_trait) =
                canonicalize_contract_effect_key(db, site, idx as u32, key_ty, key_trait);
            Some(EffectRequirement {
                binding_name,
                key: EffectRequirementKey::from_parts(key_kind, key_ty, key_trait),
                is_mut: effect.is_mut,
                binding_site: site,
                binding_idx: idx as u32,
                binding_ty: key_syntax,
            })
        })
        .collect()
}

fn lower_self_fallback_param_ty<'db>(
    db: &'db dyn HirAnalysisDb,
    func: Func<'db>,
    hir_ty: TypeId<'db>,
) -> TyId<'db> {
    match hir_ty.data(db) {
        TypeKind::Path(path) if path.to_opt().is_some_and(|path| path.is_self_ty(db)) => func
            .expected_self_ty(db)
            .unwrap_or_else(|| lower_hir_ty(db, hir_ty, func.scope(), func.assumptions(db))),
        TypeKind::Mode(mode, inner) => {
            let Some(inner) = inner.to_opt() else {
                return TyId::invalid(db, InvalidCause::ParseError);
            };

            let inner = lower_self_fallback_param_ty(db, func, inner);
            match mode {
                TypeMode::Mut => TyId::borrow_mut_of(db, inner),
                TypeMode::Ref => TyId::borrow_ref_of(db, inner),
                TypeMode::Own => inner,
            }
        }
        _ => lower_hir_ty(db, hir_ty, func.scope(), func.assumptions(db)),
    }
}

/// Call checking and callee planning elaborate the same parameters repeatedly.
#[salsa::tracked(return_ref)]
fn func_arg_tys<'db>(db: &'db dyn HirAnalysisDb, func: Func<'db>) -> Vec<Binder<'db, TyId<'db>>> {
    func.params(db)
        .enumerate()
        .map(|(idx, _)| func_arg_ty(db, func, idx).expect("function parameter exists"))
        .collect()
}

#[salsa::tracked]
fn func_arg_ty<'db>(
    db: &'db dyn HirAnalysisDb,
    func: Func<'db>,
    idx: usize,
) -> Option<Binder<'db, TyId<'db>>> {
    let param = func.params_list(db).to_opt()?.data(db).get(idx)?;
    let ty = elaborate_func_param_ty(db, func, idx, param, true);
    Some(Binder::bind(func.into(), ty))
}

#[salsa::tracked(return_ref)]
fn variant_ctor_arg_tys<'db>(
    db: &'db dyn HirAnalysisDb,
    enum_: Enum<'db>,
    idx: usize,
) -> Vec<Binder<'db, TyId<'db>>> {
    enum_.as_adt(db).fields(db)[idx].iter_types(db).collect()
}

fn elaborate_func_param_ty<'db>(
    db: &'db dyn HirAnalysisDb,
    func: Func<'db>,
    param_idx: usize,
    param: &FuncParam<'db>,
    apply_view: bool,
) -> TyId<'db> {
    let mut ty = match (
        param.ty.to_opt(),
        param.is_self_param(db),
        param.self_ty_fallback,
    ) {
        (Some(hir_ty), true, true) => lower_self_fallback_param_ty(db, func, hir_ty),
        (Some(hir_ty), true, false) => lower_callable_input_param_ty(
            db,
            func,
            CallableInputLayoutHoleOrigin::Receiver,
            hir_ty,
            SolverPremises::for_completed_callable_input(db, func),
        ),
        (Some(hir_ty), false, _) => lower_callable_input_param_ty(
            db,
            func,
            CallableInputLayoutHoleOrigin::ValueParam(param_idx),
            hir_ty,
            SolverPremises::for_completed_callable_input(db, func),
        ),
        (None, _, _) => TyId::invalid(db, InvalidCause::ParseError),
    };
    let had_layout_hole = ty_contains_const_hole(db, ty);

    let origin = if param.is_self_param(db) {
        CallableInputLayoutHoleOrigin::Receiver
    } else {
        CallableInputLayoutHoleOrigin::ValueParam(param_idx)
    };
    if had_layout_hole && let Some(layout_args) = callable_input_layout_args(db, func).get(&origin)
    {
        ty = substitute_layout_holes_by_placeholder(db, ty, layout_args);
    }

    let ty = if apply_view
        && param.mode == crate::hir_def::params::FuncParamMode::View
        && ty.as_capability(db).is_none()
    {
        TyId::view_of(db, ty)
    } else {
        ty
    };

    if had_layout_hole {
        let func_name = func
            .name(db)
            .to_opt()
            .map_or_else(|| "<anonymous>".to_string(), |name| name.data(db).clone());
        debug_assert!(
            !ty_contains_const_hole(db, ty) || ty.has_invalid(db),
            "unelaborated layout hole remained in callable parameter type for {func_name} param {param_idx}: {}",
            ty.pretty_print(db),
        );
    }
    ty
}
// Top‑level module items ----------------------------------------------------

impl<'db> TopLevelMod<'db> {
    // Note: callers currently use `scope_graph`-based traversal for modules.
    // If we find repetition in analysis or diagnostics, consider adding
    // semantic child-iteration helpers here instead of reaching into HIR.
}

impl<'db> Mod<'db> {
    // Note: semantic child iteration and module-scoped diagnostics can be
    // added here if direct `scope_graph` traversal in analysis becomes noisy.
    pub fn arithmetic_mode(self, db: &'db dyn HirDb) -> Option<ArithmeticMode> {
        self.attributes(db).arithmetic_mode(db)
    }
}

// Function items ------------------------------------------------------------

impl<'db> Func<'db> {
    pub fn arithmetic_mode(self, db: &'db dyn HirDb) -> ArithmeticMode {
        if let Some(mode) = self.attributes(db).arithmetic_mode(db) {
            return mode;
        }

        let mut scope = self.scope().parent_module(db);
        while let Some(module_scope) = scope {
            if let ItemKind::Mod(mod_) = module_scope.item()
                && let Some(mode) = mod_.attributes(db).arithmetic_mode(db)
            {
                return mode;
            }
            scope = module_scope.parent_module(db);
        }

        if let Some(mode) = self.top_mod(db).attributes(db).arithmetic_mode(db) {
            return mode;
        }

        if let Some(mode) = self.top_mod(db).ingot(db).arithmetic_mode(db) {
            return match mode {
                common::config::ArithmeticMode::Checked => ArithmeticMode::Checked,
                common::config::ArithmeticMode::Unchecked => ArithmeticMode::Unchecked,
            };
        }

        ArithmeticMode::Checked
    }

    /// Semantic predicate list (assumptions) for this function.
    pub(crate) fn assumptions(self, db: &'db dyn HirAnalysisDb) -> PredicateListId<'db> {
        constraints_for(db, self.into())
    }

    /// Returns true if this function declares an explicit return type.
    pub fn has_explicit_return_ty(self, db: &'db dyn HirDb) -> bool {
        self.ret_type_ref(db).is_some()
    }

    /// Explicit return type if annotated in source; `None` when the
    /// function has no explicit return type.
    fn explicit_return_ty(self, db: &'db dyn HirAnalysisDb) -> Option<TyId<'db>> {
        let assumptions = self.assumptions(db);
        let hir = self.ret_type_ref(db)?;
        Some(lower_hir_ty(db, hir, self.scope(), assumptions))
    }

    /// Semantic return type. When absent in source, this is `unit`.
    pub fn return_ty(self, db: &'db dyn HirAnalysisDb) -> TyId<'db> {
        self.explicit_return_ty(db)
            .unwrap_or_else(|| TyId::unit(db))
    }

    /// Semantic argument types bound to identity parameters.
    pub fn arg_tys(self, db: &'db dyn HirAnalysisDb) -> &'db [Binder<'db, TyId<'db>>] {
        func_arg_tys(db, self)
    }

    /// Semantic type of argument `idx` bound to identity parameters, lowering
    /// only that parameter.
    pub fn arg_ty(self, db: &'db dyn HirAnalysisDb, idx: usize) -> Option<Binder<'db, TyId<'db>>> {
        func_arg_ty(db, self, idx)
    }

    /// Semantic receiver type if this is a method (first argument), else None.
    pub fn receiver_ty(self, db: &'db dyn HirAnalysisDb) -> Option<Binder<'db, TyId<'db>>> {
        if self.is_method(db) {
            self.arg_ty(db, 0)
        } else {
            None
        }
    }

    /// Expected `Self` type for this function when it is an associated method.
    /// - In trait: the trait's `Self` parameter (identity instantiation)
    /// - In impl/impl_trait: the implementor type
    pub fn expected_self_ty(self, db: &'db dyn HirAnalysisDb) -> Option<TyId<'db>> {
        match self.scope().parent(db)? {
            ScopeId::Item(ItemKind::Trait(tr)) => Some(tr.self_param(db)),
            ScopeId::Item(ItemKind::ImplTrait(it)) => Some(it.ty(db)),
            ScopeId::Item(ItemKind::Impl(im)) => Some(im.ty(db)),
            _ => None,
        }
    }

    /// Return type lowering errors for functions with explicit return types.
    pub fn ret_ty_errors(self, db: &'db dyn HirAnalysisDb) -> Vec<TyDiagCollection<'db>> {
        let Some(hir_ty) = self.ret_type_ref(db) else {
            return Vec::new();
        };
        collect_ty_lower_errors(
            db,
            hir_ty,
            self.span().sig().ret_ty(),
            context_at(db, self.scope(), None).premises(db),
        )
    }

    /// Returns the containing `impl Trait` block if this function is a method
    /// inside an impl trait block.
    pub fn containing_impl_trait(self, db: &'db dyn HirDb) -> Option<ImplTrait<'db>> {
        match self.scope().parent(db)? {
            ScopeId::Item(ItemKind::ImplTrait(impl_trait)) => Some(impl_trait),
            _ => None,
        }
    }

    /// Returns the containing trait if this function is a method inside a trait definition.
    pub fn containing_trait(self, db: &'db dyn HirDb) -> Option<Trait<'db>> {
        match self.scope().parent(db)? {
            ScopeId::Item(ItemKind::Trait(trait_)) => Some(trait_),
            _ => None,
        }
    }

    /// Returns the containing inherent `impl` block if this function is a method
    /// inside an `impl` block (not an `impl Trait` block).
    pub fn containing_impl(self, db: &'db dyn HirDb) -> Option<Impl<'db>> {
        match self.scope().parent(db)? {
            ScopeId::Item(ItemKind::Impl(impl_)) => Some(impl_),
            _ => None,
        }
    }

    /// If this function is a method inside an `impl Trait` block, returns the
    /// corresponding trait method definition.
    ///
    /// Returns `None` if:
    /// - This function is not inside an impl trait block
    /// - The impl trait block's trait cannot be resolved
    /// - No matching method exists in the trait definition
    pub fn trait_method_def(self, db: &'db dyn HirAnalysisDb) -> Option<Func<'db>> {
        let impl_trait = self.containing_impl_trait(db)?;
        let trait_ = impl_trait.trait_def(db)?;
        let method_name = self.name(db).to_opt()?;

        trait_
            .methods(db)
            .find(|m| m.name(db).to_opt() == Some(method_name))
    }
}

/// Find the innermost provision for a trait slot. Associated equalities are
/// checked against the accepted answer, rather than selecting another slot.
/// The caller supplies the full ordered stream, including evidence entries.
pub(crate) fn lookup_trait_goal<'db, T: Copy>(
    db: &'db dyn HirAnalysisDb,
    goal: crate::analysis::ty::trait_def::TraitInstId<'db>,
    entries: impl DoubleEndedIterator<Item = T>,
    goal_of: impl Fn(T) -> crate::analysis::ty::trait_def::TraitInstId<'db>,
) -> Option<T> {
    entries.rev().find(|entry| {
        let provided = goal_of(*entry);
        provided.def(db) == goal.def(db) && provided.args(db) == goal.args(db)
    })
}

// Call site analysis --------------------------------------------------------

/// A call site found inside a function body.
#[derive(Clone, Debug)]
pub struct CallSiteView<'db> {
    pub body: Body<'db>,
    pub expr_id: ExprId,
    pub kind: CallSiteKind<'db>,
}

/// Discriminant for the shape of a call expression.
#[derive(Clone, Debug)]
pub enum CallSiteKind<'db> {
    FnCall,
    MethodCall { method_name: Partial<IdentId<'db>> },
}

impl<'db> CallSiteView<'db> {
    /// Read this call's retained choice from an already checked body.
    pub fn decision(
        &self,
        typed_body: &crate::analysis::ty::ty_check::TypedBody<'db>,
    ) -> Option<crate::analysis::ty::ty_check::TraitChoiceId<'db>> {
        self.body.decision_at(typed_body, self.call_span())
    }

    /// Resolve the callee of this call site via type checking.
    pub fn target(&self, db: &'db dyn HirAnalysisDb) -> Option<CallableDef<'db>> {
        use crate::analysis::ty::ty_check::check_func_body;
        let func = self.body.containing_func(db)?;
        let (_, typed_body) = check_func_body(db, func);
        typed_body
            .callable_expr(self.expr_id)
            .map(|c| c.callable_def)
    }

    /// Span of the callee name at the call site (function path or method name).
    pub fn callee_span(&self) -> crate::span::DynLazySpan<'db> {
        let expr_span = self.expr_id.span(self.body);
        match &self.kind {
            CallSiteKind::FnCall => expr_span.into_call_expr().callee().into(),
            CallSiteKind::MethodCall { .. } => {
                expr_span.into_method_call_expr().method_name().into()
            }
        }
    }

    /// Span of the entire call expression.
    pub fn call_span(&self) -> crate::span::DynLazySpan<'db> {
        self.expr_id.span(self.body).into()
    }

    /// The function containing this call site.
    pub fn containing_func(&self, db: &'db dyn HirDb) -> Option<Func<'db>> {
        self.body.containing_func(db)
    }
}

impl<'db> Body<'db> {
    /// Read the choice at an exact semantic reference site, without solving.
    pub fn decision_at(
        self,
        typed_body: &crate::analysis::ty::ty_check::TypedBody<'db>,
        span: crate::span::DynLazySpan<'db>,
    ) -> Option<crate::analysis::ty::ty_check::TraitChoiceId<'db>> {
        if typed_body.body()? != self {
            return None;
        }
        typed_body
            .decisions()
            .choice_ref(&crate::analysis::ty::ty_check::DecisionSite::trait_reference(span))
    }

    /// Enumerate all call sites (function calls and method calls) in this body.
    pub fn call_sites(self, db: &'db dyn HirDb) -> Vec<CallSiteView<'db>> {
        let mut sites = Vec::new();
        for (expr_id, partial_expr) in self.exprs(db).iter() {
            let Partial::Present(expr) = partial_expr else {
                continue;
            };
            match expr {
                Expr::Call(_, _) => {
                    sites.push(CallSiteView {
                        body: self,
                        expr_id,
                        kind: CallSiteKind::FnCall,
                    });
                }
                Expr::MethodCall(_, method_name, _, _) => {
                    sites.push(CallSiteView {
                        body: self,
                        expr_id,
                        kind: CallSiteKind::MethodCall {
                            method_name: *method_name,
                        },
                    });
                }
                _ => {}
            }
        }
        sites
    }
}

impl<'db> CallableDef<'db> {
    /// The declaration whose parameter schema interprets this callable's
    /// signature: the function itself, or a variant constructor's enum.
    pub fn generic_owner(self) -> GenericParamOwner<'db> {
        match self {
            Self::Func(func) => func.into(),
            Self::VariantCtor(variant) => variant.enum_.into(),
        }
    }

    pub fn name_span(self) -> crate::span::DynLazySpan<'db> {
        match self {
            Self::Func(func) => func.span().name().into(),
            Self::VariantCtor(v) => v.span().name().into(),
        }
    }

    pub fn is_method(self, db: &dyn HirDb) -> bool {
        match self {
            Self::Func(func) => func.is_method(db),
            Self::VariantCtor(..) => false,
        }
    }

    pub fn has_body(self, db: &dyn HirDb) -> bool {
        match self {
            Self::Func(func) => func.body(db).is_some(),
            Self::VariantCtor(..) => false,
        }
    }

    pub fn is_must_use(self, db: &'db dyn HirDb) -> bool {
        match self {
            Self::Func(func) => func.is_must_use(db),
            Self::VariantCtor(..) => false,
        }
    }

    pub fn ingot(self, db: &'db dyn HirDb) -> common::ingot::Ingot<'db> {
        match self {
            Self::Func(func) => func.top_mod(db).ingot(db),
            Self::VariantCtor(v) => v.enum_.top_mod(db).ingot(db),
        }
    }

    pub fn scope(self) -> ScopeId<'db> {
        match self {
            Self::Func(func) => func.scope(),
            Self::VariantCtor(v) => ScopeId::Variant(v),
        }
    }

    pub fn param_list_span(self) -> crate::span::DynLazySpan<'db> {
        match self {
            Self::Func(func) => func.span().params().into(),
            Self::VariantCtor(v) => v.span().tuple_type().into(),
        }
    }

    pub fn param_span(self, idx: usize) -> crate::span::DynLazySpan<'db> {
        match self {
            Self::Func(func) => func.span().params().param(idx).into(),
            Self::VariantCtor(var) => var.span().tuple_type().elem_ty(idx).into(),
        }
    }

    pub fn params(self, db: &'db dyn HirAnalysisDb) -> &'db [TyId<'db>] {
        match self {
            Self::Func(func) => collect_generic_params(db, func.into()).params(db),
            Self::VariantCtor(var) => {
                let adt = var.enum_.as_adt(db);
                adt.params(db)
            }
        }
    }

    pub fn explicit_params(self, db: &'db dyn HirAnalysisDb) -> &'db [TyId<'db>] {
        match self {
            Self::Func(func) => collect_generic_params(db, func.into()).explicit_params(db),
            Self::VariantCtor(var) => {
                let adt = var.enum_.as_adt(db);
                adt.params(db)
            }
        }
    }

    pub fn offset_to_explicit_params_position(self, db: &'db dyn HirAnalysisDb) -> usize {
        match self {
            Self::Func(func) => {
                collect_generic_params(db, func.into()).offset_to_explicit_params_position(db)
            }
            Self::VariantCtor(_) => 0, // Variant constructors don't have implicit self parameters
        }
    }

    /// Callable name (if present). Variant ctors may be absent when the name is elided.
    pub fn name(self, db: &'db dyn HirDb) -> Option<IdentId<'db>> {
        match self {
            Self::Func(func) => func.name(db).to_opt(),
            Self::VariantCtor(var) => var.ident(db),
        }
    }

    pub fn param_label(self, db: &'db dyn HirDb, idx: usize) -> Option<IdentId<'db>> {
        match self {
            Self::Func(func) => func.param_label(db, idx),
            Self::VariantCtor(_) => None,
        }
    }

    pub fn param_label_or_name(self, db: &'db dyn HirDb, idx: usize) -> Option<FuncParamName<'db>> {
        match self {
            Self::Func(func) => func.param_label_or_name(db, idx),
            Self::VariantCtor(_) => None,
        }
    }

    pub fn arg_tys(self, db: &'db dyn HirAnalysisDb) -> &'db [Binder<'db, TyId<'db>>] {
        match self {
            Self::Func(func) => func.arg_tys(db),
            Self::VariantCtor(var) => variant_ctor_arg_tys(db, var.enum_, var.idx as usize),
        }
    }

    /// Semantic type of argument `idx`, lowering only that argument.
    pub fn arg_ty(self, db: &'db dyn HirAnalysisDb, idx: usize) -> Option<Binder<'db, TyId<'db>>> {
        match self {
            Self::Func(func) => func.arg_ty(db, idx),
            Self::VariantCtor(var) => {
                let adt = var.enum_.as_adt(db);
                adt.fields(db)[var.idx as usize].iter_types(db).nth(idx)
            }
        }
    }

    pub fn ret_ty(self, db: &'db dyn HirAnalysisDb) -> Binder<'db, TyId<'db>> {
        match self {
            Self::Func(func) => Binder::bind(func.into(), func.return_ty(db)),
            Self::VariantCtor(var) => {
                let adt = var.enum_.as_adt(db);
                let mut ty = TyId::adt(db, adt);
                for &param in adt.params(db) {
                    ty = TyId::app(db, ty, param);
                }
                Binder::bind(var.enum_.into(), ty)
            }
        }
    }

    pub fn receiver_ty(self, db: &'db dyn HirAnalysisDb) -> Option<Binder<'db, TyId<'db>>> {
        match self {
            Self::Func(func) => func.receiver_ty(db),
            Self::VariantCtor(_) => None,
        }
    }
}

// ADT items -----------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
pub struct FuncParamView<'db> {
    func: Func<'db>,
    idx: usize,
}

impl<'db> FuncParamView<'db> {
    pub(crate) fn hir_ty(self, db: &'db dyn HirDb) -> Option<TypeId<'db>> {
        self.func
            .params_list(db)
            .to_opt()
            .and_then(|l| l.data(db).get(self.idx))
            .and_then(|param| param.ty.to_opt())
    }

    pub fn name(self, db: &'db dyn HirDb) -> Option<IdentId<'db>> {
        let list = self.func.params_list(db).to_opt()?;
        list.data(db).get(self.idx)?.name()
    }

    pub fn label(self, db: &'db dyn HirDb) -> Option<IdentId<'db>> {
        let list = self.func.params_list(db).to_opt()?;
        let param = list.data(db).get(self.idx)?;
        (!param.is_label_suppressed && !param.is_self_param(db))
            .then(|| param.name())
            .flatten()
    }

    pub fn is_label_suppressed(self, db: &'db dyn HirDb) -> bool {
        let list = self.func.params_list(db).to_opt();
        match list.and_then(|l| l.data(db).get(self.idx)) {
            Some(p) => p.is_label_suppressed,
            None => false,
        }
    }

    pub fn label_eagerly(self, db: &'db dyn HirDb) -> Option<IdentId<'db>> {
        let list = self.func.params_list(db).to_opt()?;
        list.data(db).get(self.idx)?.label_eagerly()
    }

    pub fn is_self_param(self, db: &'db dyn HirDb) -> bool {
        let list = self.func.params_list(db).to_opt();
        match list.and_then(|l| l.data(db).get(self.idx)) {
            Some(p) => p.is_self_param(db),
            None => false,
        }
    }

    pub fn is_mut(self, db: &'db dyn HirDb) -> bool {
        let list = self.func.params_list(db).to_opt();
        match list.and_then(|l| l.data(db).get(self.idx)) {
            Some(p) => p.is_mut,
            None => false,
        }
    }

    pub fn mode(self, db: &'db dyn HirDb) -> crate::hir_def::params::FuncParamMode {
        let list = self.func.params_list(db).to_opt();
        match list.and_then(|l| l.data(db).get(self.idx)) {
            Some(p) => p.mode,
            None => crate::hir_def::params::FuncParamMode::View,
        }
    }

    pub fn span(self) -> crate::span::params::LazyFuncParamSpan<'db> {
        self.func.span().params().param(self.idx)
    }

    pub fn self_ty_fallback(self, db: &'db dyn HirDb) -> bool {
        let list = self.func.params_list(db).to_opt();
        match list.and_then(|l| l.data(db).get(self.idx)) {
            Some(p) => p.self_ty_fallback,
            None => false,
        }
    }

    /// Semantic type of this parameter, bound to identity parameters.
    pub fn ty_binder(self, db: &'db dyn HirAnalysisDb) -> Binder<'db, TyId<'db>> {
        // Delegate to the function-level lowering to keep behavior consistent.
        // `idx` was derived from the function's own parameter list.
        self.func
            .arg_ty(db, self.idx)
            .expect("parameter of its own function")
    }

    /// Semantic type of this parameter (binder removed).
    pub fn ty(self, db: &'db dyn HirAnalysisDb) -> TyId<'db> {
        *self.ty_binder(db).skip_binder()
    }

    /// Returns the owning function.
    pub fn func(self) -> Func<'db> {
        self.func
    }

    /// Returns the parameter index.
    pub fn index(self) -> usize {
        self.idx
    }

    /// Returns the lazy span for this parameter's type in `LazyTySpan` form.
    /// Handles self-parameter fallback span correctly.
    pub fn lazy_ty_span(self, db: &'db dyn HirDb) -> crate::span::types::LazyTySpan<'db> {
        if self.is_self_param(db) && self.self_ty_fallback(db) {
            self.span().fallback_self_ty()
        } else {
            self.span().ty()
        }
    }

    /// Returns the span for error reporting on this parameter's type.
    /// Handles self-parameter fallback span correctly.
    pub fn ty_span(self, db: &'db dyn HirDb) -> crate::span::DynLazySpan<'db> {
        if self.is_self_param(db) && self.self_ty_fallback(db) {
            self.span().name().into()
        } else {
            self.span().ty().into()
        }
    }

    /// All type-related diagnostics for this parameter.
    pub fn ty_diags(self, db: &'db dyn HirAnalysisDb) -> Vec<TyDiagCollection<'db>> {
        let func = self.func;
        let context = context_at(db, func.scope(), None);

        let Some(param) = self
            .func
            .params_list(db)
            .to_opt()
            .and_then(|l| l.data(db).get(self.idx))
        else {
            return Vec::new();
        };
        let Some(hir_ty) = param.ty.to_opt() else {
            return Vec::new();
        };

        if self.is_self_param(db) && !param.self_ty_fallback {
            if param.has_ref_prefix {
                return vec![
                    TyLowerDiag::MixedRefSelfPrefixWithExplicitType {
                        span: self.span().ref_kw().into(),
                    }
                    .into(),
                ];
            }

            if param.has_own_prefix {
                return vec![
                    TyLowerDiag::MixedOwnSelfPrefixWithExplicitType {
                        span: self.span().own_kw().into(),
                    }
                    .into(),
                ];
            }

            if param.is_mut && !allows_mut_self_prefix_with_explicit_ty(db, hir_ty) {
                let span = self.span().mut_kw().into();
                return vec![TyLowerDiag::InvalidMutSelfPrefixWithExplicitType { span }.into()];
            }
        }

        if !self.is_self_param(db)
            && param.is_mut
            && !matches!(hir_ty.data(db), TypeKind::Mode(TypeMode::Own, _))
        {
            let span = self.span().mut_kw().into();
            return vec![TyLowerDiag::InvalidMutParamPrefixWithoutOwnType { span }.into()];
        }

        // Surface name-resolution errors for the parameter type first
        let errs = collect_hir_ty_diags(db, hir_ty, self.lazy_ty_span(db), context.premises(db));
        if !errs.is_empty() {
            return errs;
        }

        let semantic_ty = self.ty(db);
        let ty = if semantic_ty.has_invalid(db) {
            elaborate_func_param_ty(db, func, self.idx, param, false)
        } else if self.mode(db) == crate::hir_def::params::FuncParamMode::View {
            semantic_ty.as_view(db).unwrap_or(semantic_ty)
        } else {
            semantic_ty
        };
        let ty_span = self.ty_span(db);

        let mut out = Vec::new();

        if !ty.has_star_kind(db) {
            out.push(TyDiagCollection::from(TyLowerDiag::ExpectedStarKind(
                ty_span.clone(),
            )));
            return out;
        }
        if ty.is_const_ty(db) {
            out.push(
                TyLowerDiag::NormalTypeExpected {
                    span: ty_span.clone(),
                    given: ty,
                }
                .into(),
            );
            return out;
        }

        if self.mode(db) == crate::hir_def::params::FuncParamMode::Own && ty.as_borrow(db).is_some()
        {
            out.push(
                TyLowerDiag::OwnParamCannotBeBorrow {
                    span: ty_span.clone(),
                    ty,
                }
                .into(),
            );
        }

        // Well-formedness / trait-bound satisfaction for parameter type
        if let Some(diag) =
            check_ty_wf(db, context.solver_context(db), ty).into_diag(ty_span.clone())
        {
            out.push(diag);
        }

        // Self-parameter type shape check
        if self.is_self_param(db)
            && let Some(mut expected) = func.expected_self_ty(db)
            && !ty.has_invalid(db)
            && !expected.has_invalid(db)
        {
            if ty_contains_const_hole(db, expected) {
                let layout_args = callable_input_layout_args(db, func);
                if let Some(receiver_layout_args) =
                    layout_args.get(&CallableInputLayoutHoleOrigin::Receiver)
                {
                    expected =
                        substitute_layout_holes_by_placeholder(db, expected, receiver_layout_args);
                }
            }
            let ty_norm = normalize_ty(db, ty, context.premises(db));

            let matches_expected = |candidate: TyId<'db>| {
                let (exp_base, exp_args) = expected.decompose_ty_app(db);
                let (cand_base, cand_args) = candidate.decompose_ty_app(db);
                cand_base == exp_base
                    && cand_args.len() >= exp_args.len()
                    && exp_args.iter().zip(cand_args.iter()).all(|(a, b)| a == b)
            };

            let is_allowed_self_ty = matches_expected(ty_norm)
                || ty_norm
                    .as_capability(db)
                    .is_some_and(|(_, inner)| matches_expected(inner));

            if !is_allowed_self_ty {
                out.push(
                    ImplDiag::InvalidSelfType {
                        span: ty_span,
                        expected,
                        given: ty_norm,
                    }
                    .into(),
                );
            }
        }

        out
    }
}

fn allows_mut_self_prefix_with_explicit_ty<'db>(db: &'db dyn HirDb, hir_ty: TypeId<'db>) -> bool {
    if let TypeKind::Mode(TypeMode::Own, inner) = hir_ty.data(db)
        && let Some(inner) = inner.to_opt()
    {
        !is_bare_self_ty(db, inner)
    } else {
        false
    }
}

fn is_bare_self_ty<'db>(db: &'db dyn HirDb, hir_ty: TypeId<'db>) -> bool {
    if let TypeKind::Path(path) = hir_ty.data(db)
        && let Some(path) = path.to_opt()
    {
        path.is_self_ty(db) && path.generic_args(db).is_empty(db)
    } else {
        false
    }
}

// Effect param views --------------------------------------------------------

#[salsa::interned]
#[derive(Debug)]
pub struct RecvView<'db> {
    pub contract: Contract<'db>,
    pub recv_idx: u32,
}

impl<'db> RecvView<'db> {
    pub fn index(self, db: &'db dyn HirDb) -> u32 {
        self.recv_idx(db)
    }

    pub fn msg_path(self, db: &'db dyn HirDb) -> Option<PathId<'db>> {
        self.contract(db)
            .recvs(db)
            .data(db)
            .get(self.recv_idx(db) as usize)
            .and_then(|r| r.msg_path)
    }

    pub fn arm(self, db: &'db dyn HirDb, arm_idx: u32) -> Option<RecvArmView<'db>> {
        self.contract(db)
            .recv_arm(db, self.recv_idx(db) as usize, arm_idx as usize)
            .map(|_| RecvArmView::new(db, self, arm_idx))
    }

    pub fn arms(self, db: &'db dyn HirDb) -> impl Iterator<Item = RecvArmView<'db>> + 'db {
        let len = self
            .contract(db)
            .recvs(db)
            .data(db)
            .get(self.recv_idx(db) as usize)
            .map_or(0, |r| r.arms.data(db).len());
        (0..len).map(move |arm_idx| RecvArmView::new(db, self, arm_idx as u32))
    }
}

#[salsa::interned]
#[derive(Debug)]
pub struct RecvArmView<'db> {
    pub recv: RecvView<'db>,
    pub arm_idx: u32,
}

impl<'db> RecvArmView<'db> {
    pub fn index(self, db: &'db dyn HirDb) -> u32 {
        self.arm_idx(db)
    }

    pub fn contract(self, db: &'db dyn HirDb) -> Contract<'db> {
        self.recv(db).contract(db)
    }

    pub fn arm(self, db: &'db dyn HirDb) -> Option<ContractRecvArm<'db>> {
        let recv = self.recv(db);
        recv.contract(db)
            .recv_arm(db, recv.recv_idx(db) as usize, self.arm_idx(db) as usize)
    }

    pub fn is_fallback(self, db: &'db dyn HirDb) -> bool {
        self.arm(db).is_some_and(|arm| arm.is_fallback(db))
    }

    pub fn effects(self, db: &'db dyn HirDb) -> EffectParamListId<'db> {
        self.arm(db)
            .map_or_else(|| EffectParamListId::new(db, Vec::new()), |a| a.effects)
    }

    pub fn effective_effect_env(self, db: &'db dyn HirAnalysisDb) -> EffectEnvView<'db> {
        let contract = self.contract(db);
        let recv_idx = self.recv(db).recv_idx(db);
        let arm_idx = self.arm_idx(db);
        EffectEnvView {
            site: EffectParamSite::ContractRecvArm {
                contract,
                recv_idx,
                arm_idx,
            },
        }
    }
}

#[salsa::tracked]
impl<'db> RecvArmView<'db> {
    #[salsa::tracked]
    pub fn variant_ty(self, db: &'db dyn HirAnalysisDb) -> TyId<'db> {
        let assumptions = PredicateListId::empty_list(db);
        let recv = self.recv(db);
        let contract = recv.contract(db);

        let recv_is_bare = recv.msg_path(db).is_none();
        let msg_mod = recv
            .msg_path(db)
            .and_then(|p| resolve_msg_mod(db, contract, p, assumptions));

        let Some(hir_arm) = self.arm(db) else {
            return TyId::invalid(db, InvalidCause::Other);
        };

        let (_variant_struct, variant_ty) =
            resolve_recv_variant(db, contract, msg_mod, recv_is_bare, hir_arm, assumptions);

        variant_ty.unwrap_or_else(|| TyId::invalid(db, InvalidCause::Other))
    }

    #[salsa::tracked]
    pub fn abi_info(self, db: &'db dyn HirAnalysisDb, abi: TyId<'db>) -> RecvArmAbiInfo<'db> {
        if self.is_fallback(db) {
            return RecvArmAbiInfo {
                is_fallback: true,
                selector_value: None,
                selector_signature: None,
                args_ty: TyId::unit(db),
                ret_ty: None,
            };
        }

        let recv = self.recv(db);
        let contract = recv.contract(db);

        let variant_ty = self.variant_ty(db);
        let selector_info = get_variant_selector_info(db, variant_ty, contract.scope());

        let Some(msg_variant_trait) =
            resolve_core_trait(db, contract.scope(), &["message", "MsgVariant"])
        else {
            return RecvArmAbiInfo {
                is_fallback: false,
                selector_value: selector_info.value,
                selector_signature: selector_info.signature,
                args_ty: variant_ty,
                ret_ty: None,
            };
        };
        let return_ident = IdentId::new(db, "Return".to_string());

        let variant_ret_ty = if !variant_ty.has_invalid(db) && !abi.has_invalid(db) {
            let inst = TraitInstId::new(
                db,
                msg_variant_trait,
                vec![variant_ty, abi],
                IndexMap::new(),
            );
            let return_proj = TyId::assoc_ty(db, inst.trait_ref(db), return_ident);
            normalize_ty(
                db,
                return_proj,
                header_context_at(db, contract.scope()).premises(db),
            )
        } else {
            TyId::invalid(db, InvalidCause::Other)
        };

        let args_ty = variant_ty;
        let ret_ty = (variant_ret_ty != TyId::unit(db)).then_some(variant_ret_ty);

        RecvArmAbiInfo {
            is_fallback: false,
            selector_value: selector_info.value,
            selector_signature: selector_info.signature,
            args_ty,
            ret_ty,
        }
    }

    #[salsa::tracked(return_ref)]
    pub fn arg_bindings(self, db: &'db dyn HirAnalysisDb) -> Vec<ArgBinding<'db>> {
        if self.is_fallback(db) {
            return Vec::new();
        }

        let assumptions = PredicateListId::empty_list(db);
        let recv = self.recv(db);
        let contract = recv.contract(db);

        let Some(sol_ty) = resolve_sol_abi_ty(db, contract.scope(), assumptions) else {
            return Vec::new();
        };

        let variant_ty = self.variant_ty(db);
        let Some(variant_struct) = variant_struct_from_ty(db, variant_ty) else {
            return Vec::new();
        };

        let Some(hir_arm) = self.arm(db) else {
            return Vec::new();
        };

        let abi_info = self.abi_info(db, sol_ty);
        compute_arg_bindings(
            db,
            variant_struct,
            abi_info.args_ty,
            hir_arm.pat,
            hir_arm.body,
        )
        .unwrap_or_default()
    }

    #[salsa::tracked(return_ref)]
    pub fn effective_effect_requirements(
        self,
        db: &'db dyn HirAnalysisDb,
    ) -> Vec<EffectRequirement<'db>> {
        let contract = self.contract(db);
        let recv_idx = self.recv(db).recv_idx(db);
        let arm_idx = self.arm_idx(db);
        let Some(arm) = self.arm(db) else {
            return Vec::new();
        };

        let site = EffectParamSite::ContractRecvArm {
            contract,
            recv_idx,
            arm_idx,
        };
        contract_scoped_effect_requirements_canonical(db, contract, site, arm.effects)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Update)]
pub struct ContractFieldInfo<'db> {
    pub index: u32,
    pub name: IdentId<'db>,
    pub declared: AssignedLayoutTy<'db>,
    pub is_mut: bool,
    pub is_provider: bool,
    pub target: AssignedLayoutTy<'db>,
}

impl<'db> ContractFieldInfo<'db> {
    pub fn declared_shape_ty(&self) -> TyId<'db> {
        self.declared.shape_ty()
    }

    pub fn target_shape_ty(&self) -> TyId<'db> {
        self.target.shape_ty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Update)]
pub struct ArgBinding<'db> {
    pub pat: PatId,
    pub tuple_index: u32,
    pub ty: TyId<'db>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Update)]
pub enum EffectRequirementKey<'db> {
    Type(TyId<'db>),
    Trait(TraitInstId<'db>),
    Other,
}

impl<'db> EffectRequirementKey<'db> {
    pub fn from_parts(
        key_kind: EffectKeyKind,
        key_ty: Option<TyId<'db>>,
        key_trait: Option<TraitInstId<'db>>,
    ) -> Self {
        match key_kind {
            EffectKeyKind::Type => key_ty.map_or(Self::Other, Self::Type),
            EffectKeyKind::Trait => key_trait.map_or(Self::Other, Self::Trait),
            EffectKeyKind::Other => Self::Other,
        }
    }

    pub fn kind(&self) -> EffectKeyKind {
        match self {
            Self::Type(_) => EffectKeyKind::Type,
            Self::Trait(_) => EffectKeyKind::Trait,
            Self::Other => EffectKeyKind::Other,
        }
    }

    pub fn key_ty(&self) -> Option<TyId<'db>> {
        match self {
            Self::Type(ty) => Some(*ty),
            Self::Trait(_) | Self::Other => None,
        }
    }

    pub fn key_trait(&self) -> Option<TraitInstId<'db>> {
        match self {
            Self::Trait(trait_inst) => Some(*trait_inst),
            Self::Type(_) | Self::Other => None,
        }
    }

    pub fn binding_ty(&self, db: &'db dyn HirAnalysisDb) -> Option<TyId<'db>> {
        match self {
            Self::Type(ty) => Some(*ty),
            Self::Trait(trait_inst) => Some(trait_inst.self_ty(db)),
            Self::Other => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Update)]
pub struct EffectRequirement<'db> {
    pub binding_name: IdentId<'db>,
    pub key: EffectRequirementKey<'db>,
    pub is_mut: bool,
    pub binding_site: EffectParamSite<'db>,
    pub binding_idx: u32,
    /// The type syntax written at the binding site.
    ///
    /// This is not necessarily the semantic key. For contract-scoped named imports, it is the
    /// import name, while the resolved key is captured by `key`.
    pub binding_ty: TypeId<'db>,
}

impl<'db> EffectRequirement<'db> {
    pub fn binding_path(&self, db: &'db dyn HirDb) -> Option<PathId<'db>> {
        self.binding_ty.as_path(db)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Update)]
pub enum ProviderSource<'db> {
    UsesParam {
        site: EffectParamSite<'db>,
        requirement_idx: u32,
    },
    ContractField {
        field: ContractFieldId<'db>,
    },
    RootProvider {
        scope: RootProviderScope<'db>,
        registration: RootProviderRegistration<'db>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Update)]
pub struct ProviderBinding<'db> {
    pub provider_idx: u32,
    pub provider_ty: TyId<'db>,
    pub is_mut: bool,
    pub source: ProviderSource<'db>,
    pub semantics: ProviderSemantics<'db>,
    pub layout_env: Option<AssignedLayoutBindingEnv<'db>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Update)]
pub struct AssignedLayoutBindingEnv<'db> {
    pub field: ContractFieldId<'db>,
    pub view: LayoutViewKind,
}

impl<'db> ProviderBinding<'db> {
    pub fn effective_target_ty(&self) -> TyId<'db> {
        self.semantics.target_ty.unwrap_or(self.provider_ty)
    }

    pub fn assigned_field_layout(
        &self,
        db: &'db dyn HirAnalysisDb,
    ) -> Option<(&'db FieldStorageLayout<'db>, LayoutViewKind)> {
        let env = self.layout_env?;
        let field = env
            .field
            .contract
            .storage_layout(db)
            .values()
            .find(|field| field.field == env.field)?;
        Some((field, env.view))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Update)]
pub struct ResolvedEffectBinding {
    pub requirement_idx: u32,
    pub provider_idx: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Update)]
pub struct ResolvedEffectBindingInfo<'db> {
    pub requirement: EffectRequirement<'db>,
    pub provider: ProviderBinding<'db>,
}

#[salsa::interned]
pub struct EffectEnvSite<'db> {
    pub site: EffectParamSite<'db>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Update)]
pub struct EffectEnvView<'db> {
    site: EffectParamSite<'db>,
}

impl<'db> EffectEnvView<'db> {
    pub fn new(site: EffectParamSite<'db>) -> Self {
        Self { site }
    }

    pub fn site(self) -> EffectParamSite<'db> {
        self.site
    }

    pub fn requirements(self, db: &'db dyn HirAnalysisDb) -> Vec<EffectRequirement<'db>> {
        effect_requirements_for_site(db, self.site)
    }

    pub fn resolved_binding(
        self,
        db: &'db dyn HirAnalysisDb,
        idx: usize,
    ) -> Option<ResolvedEffectBindingInfo<'db>> {
        resolved_effect_binding_infos_for_site(db, EffectEnvSite::new(db, self.site))
            .get(idx)
            .and_then(|binding| binding.clone())
    }

    /// Returns the type of the effect binding as it is visible in the body.
    ///
    /// For a type-key effect like `uses (evm: Evm)`, this is the visible key
    /// type `Evm`. For a trait-key effect, the binding's value has the hidden
    /// provider type that implements the trait key.
    pub fn visible_effect_binding_ty(
        self,
        db: &'db dyn HirAnalysisDb,
        idx: usize,
    ) -> Option<TyId<'db>> {
        let requirement = self
            .requirements(db)
            .into_iter()
            .find(|binding| binding.binding_idx as usize == idx)?;
        let provider = self
            .resolved_binding(db, idx)
            .map(|binding| binding.provider);

        match requirement.key {
            EffectRequirementKey::Trait(_) => provider
                .map(|binding| binding.provider_ty)
                .or_else(|| requirement.key.binding_ty(db)),
            EffectRequirementKey::Type(_) | EffectRequirementKey::Other => requirement
                .key
                .binding_ty(db)
                .or_else(|| provider.map(|binding| binding.provider_ty)),
        }
    }

    /// Returns the hidden provider type associated with an effect binding.
    pub fn hidden_provider_binding_ty(
        self,
        db: &'db dyn HirAnalysisDb,
        idx: usize,
    ) -> Option<TyId<'db>> {
        self.resolved_binding(db, idx)
            .map(|binding| binding.provider.provider_ty)
    }

    pub fn providers(self, db: &'db dyn HirAnalysisDb) -> Vec<ProviderBinding<'db>> {
        provider_bindings_for_site(db, self.site).to_vec()
    }

    pub fn resolutions(self, db: &'db dyn HirAnalysisDb) -> Vec<ResolvedEffectBinding> {
        effect_resolutions_for_site(db, self.site).to_vec()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Update)]
pub struct RecvArmAbiInfo<'db> {
    pub is_fallback: bool,
    pub selector_value: Option<u32>,
    pub selector_signature: Option<String>,
    pub args_ty: TyId<'db>,
    pub ret_ty: Option<TyId<'db>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Update)]
pub(crate) struct VariantSelectorInfo {
    pub(crate) value: Option<u32>,
    pub(crate) signature: Option<String>,
}

fn variant_struct_from_ty<'db>(db: &'db dyn HirAnalysisDb, ty: TyId<'db>) -> Option<Struct<'db>> {
    match ty.base_ty(db).data(db) {
        TyData::TyBase(TyBase::Adt(adt)) => match adt.adt_ref(db) {
            AdtRef::Struct(struct_) => Some(struct_),
            _ => None,
        },
        _ => None,
    }
}

#[salsa::tracked]
impl<'db> Contract<'db> {
    pub fn recv(self, db: &'db dyn HirDb, recv_idx: u32) -> Option<RecvView<'db>> {
        self.recvs(db)
            .data(db)
            .get(recv_idx as usize)
            .map(|_| RecvView::new(db, self, recv_idx))
    }

    pub fn init_effect_env(self, db: &'db dyn HirDb) -> Option<EffectEnvView<'db>> {
        self.init(db).map(|_| EffectEnvView {
            site: EffectParamSite::ContractInit { contract: self },
        })
    }

    pub fn recv_views(self, db: &'db dyn HirDb) -> impl Iterator<Item = RecvView<'db>> + 'db {
        let len = self.recvs(db).data(db).len();
        (0..len).map(move |idx| RecvView::new(db, self, idx as u32))
    }

    pub fn effect_params(
        self,
        db: &'db dyn HirDb,
    ) -> impl Iterator<Item = EffectParamView<'db>> + 'db {
        let len = self.effects(db).data(db).len();
        let owner = EffectParamOwner::Contract(self);
        (0..len).map(move |idx| EffectParamView { owner, idx })
    }

    pub fn storage_layout(
        self,
        db: &'db dyn HirAnalysisDb,
    ) -> &'db ContractStorageLayoutResult<'db> {
        storage_layout::build_contract_storage_layout(db, self)
    }

    pub fn layout_report(
        self,
        db: &'db dyn HirAnalysisDb,
    ) -> Option<&'db ContractLayoutReport<'db>> {
        storage_layout::build_contract_layout_report(db, self).as_ref()
    }

    /// Total code-address-space high-water mark. Invalid contracts publish no
    /// provisional allocation and therefore have no deployable immutable
    /// buffer size.
    pub fn code_address_space_slot_count(self, db: &'db dyn HirAnalysisDb) -> usize {
        self.storage_layout(db)
            .allocated
            .as_ref()
            .and_then(|layout| {
                layout
                    .high_water_by_address_space
                    .get(&crate::analysis::ty::ProviderAddressSpace::Code)
                    .copied()
            })
            .unwrap_or(0)
    }

    #[salsa::tracked(return_ref)]
    pub fn fields(
        self,
        db: &'db dyn HirAnalysisDb,
    ) -> IndexMap<IdentId<'db>, ContractFieldInfo<'db>> {
        self.storage_layout(db)
            .semantic_fields(db)
            .into_iter()
            .map(|(name, field, is_mut, is_provider, declared, target)| {
                (
                    name,
                    ContractFieldInfo {
                        index: field.index,
                        name,
                        declared,
                        is_mut,
                        is_provider,
                        target,
                    },
                )
            })
            .collect()
    }

    #[salsa::tracked]
    pub fn init_args_ty(self, db: &'db dyn HirAnalysisDb) -> TyId<'db> {
        let Some(init) = self.init(db) else {
            return TyId::unit(db);
        };

        let assumptions = PredicateListId::empty_list(db);
        let param_tys: Vec<TyId<'db>> = init
            .params(db)
            .data(db)
            .iter()
            .map(|p| match p.ty.to_opt() {
                Some(hir_ty) => {
                    lower_opt_hir_ty(db, Partial::Present(hir_ty), self.scope(), assumptions)
                }
                None => TyId::invalid(db, InvalidCause::ParseError),
            })
            .collect();

        let base = TyId::new(
            db,
            TyData::TyBase(TyBase::Prim(PrimTy::Tuple(param_tys.len()))),
        );
        param_tys
            .into_iter()
            .fold(base, |acc, elem| TyId::app(db, acc, elem))
    }

    #[salsa::tracked(return_ref)]
    pub fn effect_requirements(self, db: &'db dyn HirAnalysisDb) -> Vec<EffectRequirement<'db>> {
        contract_effect_requirements_canonical(db, self)
    }

    #[salsa::tracked(return_ref)]
    pub fn init_effect_requirements(
        self,
        db: &'db dyn HirAnalysisDb,
    ) -> Vec<EffectRequirement<'db>> {
        let Some(init) = self.init(db) else {
            return Vec::new();
        };
        contract_scoped_effect_requirements_canonical(
            db,
            self,
            EffectParamSite::ContractInit { contract: self },
            init.effects(db),
        )
    }
}

#[salsa::tracked]
impl<'db> Func<'db> {
    #[salsa::tracked(return_ref)]
    pub fn effect_requirements(self, db: &'db dyn HirAnalysisDb) -> Vec<EffectRequirement<'db>> {
        func_effect_requirements_canonical(db, self)
    }

    #[salsa::tracked(return_ref)]
    pub fn effective_effect_requirements(
        self,
        db: &'db dyn HirAnalysisDb,
    ) -> Vec<EffectRequirement<'db>> {
        func_effect_requirements_canonical(db, self)
    }
}

fn contract_effect_decl_map<'db>(
    db: &'db dyn HirAnalysisDb,
    contract: Contract<'db>,
) -> FxHashMap<IdentId<'db>, EffectRequirement<'db>> {
    contract
        .effect_requirements(db)
        .iter()
        .cloned()
        .map(|binding| (binding.binding_name, binding))
        .collect()
}

#[derive(Clone, Copy)]
enum ContractResolutionSource {
    RootProvider,
    Field(u32),
    Unresolved,
}

pub fn effect_requirements_for_site<'db>(
    db: &'db dyn HirAnalysisDb,
    site: EffectParamSite<'db>,
) -> Vec<EffectRequirement<'db>> {
    match site {
        EffectParamSite::Func(func) => func.effective_effect_requirements(db).clone(),
        EffectParamSite::Contract(contract) => contract.effect_requirements(db).clone(),
        EffectParamSite::ContractInit { contract } => contract.init_effect_requirements(db).clone(),
        EffectParamSite::ContractRecvArm {
            contract,
            recv_idx,
            arm_idx,
        } => {
            let recv = RecvView::new(db, contract, recv_idx);
            let arm = RecvArmView::new(db, recv, arm_idx);
            arm.effective_effect_requirements(db).clone()
        }
    }
}

pub fn provider_bindings_for_site<'db>(
    db: &'db dyn HirAnalysisDb,
    site: EffectParamSite<'db>,
) -> &'db [ProviderBinding<'db>] {
    provider_bindings_for_site_query(db, EffectEnvSite::new(db, site)).as_slice()
}

#[salsa::tracked(return_ref)]
fn provider_bindings_for_site_query<'db>(
    db: &'db dyn HirAnalysisDb,
    site: EffectEnvSite<'db>,
) -> Vec<ProviderBinding<'db>> {
    let site = site.site(db);
    match site {
        EffectParamSite::Func(func) => func_provider_bindings_canonical(db, func),
        EffectParamSite::Contract(contract)
        | EffectParamSite::ContractInit { contract }
        | EffectParamSite::ContractRecvArm { contract, .. } => {
            contract_provider_bindings_canonical(db, contract, site)
        }
    }
}

pub fn effect_resolutions_for_site<'db>(
    db: &'db dyn HirAnalysisDb,
    site: EffectParamSite<'db>,
) -> &'db [ResolvedEffectBinding] {
    effect_resolutions_for_site_query(db, EffectEnvSite::new(db, site)).as_slice()
}

#[salsa::tracked(return_ref)]
fn effect_resolutions_for_site_query<'db>(
    db: &'db dyn HirAnalysisDb,
    site: EffectEnvSite<'db>,
) -> Vec<ResolvedEffectBinding> {
    let site = site.site(db);
    match site {
        EffectParamSite::Func(func) => func_effect_resolutions_canonical(db, func),
        EffectParamSite::Contract(contract) => {
            contract_effect_resolutions_canonical(db, contract, site, contract.effects(db))
        }
        EffectParamSite::ContractInit { contract } => {
            let Some(init) = contract.init(db) else {
                return Vec::new();
            };
            contract_effect_resolutions_canonical(db, contract, site, init.effects(db))
        }
        EffectParamSite::ContractRecvArm {
            contract,
            recv_idx,
            arm_idx,
        } => {
            let Some(arm) = contract.recv_arm(db, recv_idx as usize, arm_idx as usize) else {
                return Vec::new();
            };
            contract_effect_resolutions_canonical(db, contract, site, arm.effects)
        }
    }
}

#[salsa::tracked(return_ref)]
pub fn resolved_effect_binding_infos_for_site<'db>(
    db: &'db dyn HirAnalysisDb,
    site: EffectEnvSite<'db>,
) -> Vec<Option<ResolvedEffectBindingInfo<'db>>> {
    let site = site.site(db);
    let requirements = effect_requirements_for_site(db, site);
    let resolutions = effect_resolutions_for_site(db, site);
    let providers = provider_bindings_for_site(db, site);
    let max_idx = requirements
        .iter()
        .map(|requirement| requirement.binding_idx as usize)
        .max()
        .map_or(0, |idx| idx + 1);
    let mut requirements_by_idx = vec![None; max_idx];
    for requirement in requirements {
        let idx = requirement.binding_idx as usize;
        requirements_by_idx[idx] = Some(requirement);
    }
    let providers_by_idx = providers
        .iter()
        .cloned()
        .map(|provider| (provider.provider_idx, provider))
        .collect::<FxHashMap<_, _>>();
    let mut resolved = vec![None; requirements_by_idx.len()];
    for resolution in resolutions {
        let idx = resolution.requirement_idx as usize;
        let Some(requirement) = requirements_by_idx
            .get(idx)
            .and_then(|requirement| requirement.clone())
        else {
            continue;
        };
        let Some(provider) = providers_by_idx.get(&resolution.provider_idx).cloned() else {
            continue;
        };
        resolved[idx] = Some(ResolvedEffectBindingInfo {
            requirement,
            provider,
        });
    }
    resolved
}

fn contract_scoped_effect_requirements_canonical<'db>(
    db: &'db dyn HirAnalysisDb,
    contract: Contract<'db>,
    list_site: EffectParamSite<'db>,
    list: EffectParamListId<'db>,
) -> Vec<EffectRequirement<'db>> {
    if matches!(list_site, EffectParamSite::Func(_)) {
        unreachable!("contract-scoped effect bindings require a contract init/recv/decl site");
    }

    let fields = contract.fields(db);
    let contract_named_effects = contract_effect_decl_map(db, contract);
    let assumptions = PredicateListId::empty_list(db);

    let mut out = Vec::new();
    for (idx, effect) in list.data(db).iter().enumerate() {
        let Some(key_syntax) = effect.key_ty.to_opt() else {
            continue;
        };
        let key_path = key_syntax.as_path(db);

        if let Some(binding_name) = effect.name {
            let forwarded_key =
                key_path
                    .filter(|key_path| key_path.len(db) == 1)
                    .and_then(|key_path| {
                        key_path
                            .ident(db)
                            .to_opt()
                            .and_then(|name| contract_named_effects.get(&name))
                            .map(|binding| binding.key.clone())
                    });
            let (key_kind, key_ty, key_trait) = forwarded_key.as_ref().map_or_else(
                || resolve_effect_key(db, key_syntax, contract.scope(), assumptions).into_parts(db),
                |key| (key.kind(), key.key_ty(), key.key_trait()),
            );
            let (key_ty, key_trait) = forwarded_key.as_ref().map_or_else(
                || canonicalize_contract_effect_key(db, list_site, idx as u32, key_ty, key_trait),
                |_| (key_ty, key_trait),
            );

            out.push(EffectRequirement {
                binding_name,
                key: EffectRequirementKey::from_parts(key_kind, key_ty, key_trait),
                is_mut: effect.is_mut,
                binding_site: list_site,
                binding_idx: idx as u32,
                binding_ty: key_syntax,
            });
            continue;
        }

        let Some(key_path) = key_path else {
            out.push(EffectRequirement {
                binding_name: IdentId::new(db, "_effect".to_string()),
                key: EffectRequirementKey::Other,
                is_mut: effect.is_mut,
                binding_site: list_site,
                binding_idx: idx as u32,
                binding_ty: key_syntax,
            });
            continue;
        };

        if key_path.len(db) == 1
            && let Some(name) = key_path.ident(db).to_opt()
            && fields.contains_key(&name)
        {
            let key = match contract.storage_layout(db).field(&name) {
                Some(layout) => match layout.target_effect_binding_ty(db) {
                    Ok(ty) => EffectRequirementKey::Type(ty),
                    Err(_) => EffectRequirementKey::Other,
                },
                None => EffectRequirementKey::Other,
            };

            out.push(EffectRequirement {
                binding_name: name,
                key,
                is_mut: effect.is_mut,
                binding_site: list_site,
                binding_idx: idx as u32,
                binding_ty: key_syntax,
            });
            continue;
        }

        if key_path.len(db) == 1
            && let Some(name) = key_path.ident(db).to_opt()
            && let Some(binding) = contract_named_effects.get(&name)
        {
            out.push(EffectRequirement {
                binding_name: name,
                key: binding.key.clone(),
                is_mut: binding.is_mut,
                binding_site: list_site,
                binding_idx: idx as u32,
                binding_ty: key_syntax,
            });
            continue;
        }

        // Unlabeled contract-scoped effects are either references (field/contract effect) or
        // invalid. Diagnostics are emitted by the type checker; keep lowering conservative.
        let binding_name = key_path
            .ident(db)
            .to_opt()
            .unwrap_or_else(|| IdentId::new(db, "_effect".to_string()));

        out.push(EffectRequirement {
            binding_name,
            key: EffectRequirementKey::Other,
            is_mut: effect.is_mut,
            binding_site: list_site,
            binding_idx: idx as u32,
            binding_ty: key_syntax,
        });
    }

    out
}

fn contract_provider_bindings_canonical<'db>(
    db: &'db dyn HirAnalysisDb,
    contract: Contract<'db>,
    site: EffectParamSite<'db>,
) -> Vec<ProviderBinding<'db>> {
    let scope = contract.scope();
    let assumptions = PredicateListId::empty_list(db);
    let is_init_site = matches!(site, EffectParamSite::ContractInit { .. });
    let mut providers = contract
        .storage_layout(db)
        .values()
        .cloned()
        .enumerate()
        .filter_map(|(idx, field)| {
            let provider_ty = field.target_effect_binding_ty(db).ok()?;
            Some(ProviderBinding {
                provider_idx: idx as u32,
                provider_ty,
                // Immutable contract fields (i.e. code-backed) are writable during `init`.
                // They are materialized into memory during initialization and later embedded into code.
                is_mut: field.is_mut
                    || (is_init_site
                        && field.address_space == crate::analysis::ty::ProviderAddressSpace::Code),
                source: ProviderSource::ContractField { field: field.field },
                semantics: ProviderSemantics {
                    provider_ty,
                    kind: if provider_ty.is_struct(db)
                        || provider_ty.is_array(db)
                        || provider_ty.is_tuple(db)
                        || provider_ty.as_enum(db).is_some()
                    {
                        crate::analysis::ty::ProviderKind::Handle
                    } else {
                        crate::analysis::ty::ProviderKind::RawAddress
                    },
                    address_space: Some(field.address_space).map(|space| {
                        if is_init_site && space == crate::analysis::ty::ProviderAddressSpace::Code
                        {
                            crate::analysis::ty::ProviderAddressSpace::Memory
                        } else {
                            space
                        }
                    }),
                    target_ty: Some(provider_ty),
                    transport: crate::analysis::ty::ProviderTransport::ByValue,
                    evidence: ProviderLayoutEvidence::ContractField,
                },
                layout_env: Some(AssignedLayoutBindingEnv {
                    field: field.field,
                    view: LayoutViewKind::Target,
                }),
            })
        })
        .collect::<Vec<_>>();

    let base_provider_idx = providers.len();
    providers.extend(
        registered_root_providers(db, site)
            .iter()
            .cloned()
            .enumerate()
            .map(|(idx, registration)| {
                let provider_ty = registration.provider_ty;
                ProviderBinding {
                    provider_idx: (base_provider_idx + idx) as u32,
                    provider_ty,
                    is_mut: true,
                    source: ProviderSource::RootProvider {
                        scope: RootProviderScope::of_site(site),
                        registration,
                    },
                    semantics: provider_semantics(db, scope, assumptions, provider_ty),
                    layout_env: None,
                }
            }),
    );
    providers
}

fn contract_effect_decl_resolution_sources<'db>(
    db: &'db dyn HirAnalysisDb,
    contract: Contract<'db>,
) -> FxHashMap<IdentId<'db>, ContractResolutionSource> {
    contract
        .effect_requirements(db)
        .iter()
        .map(|requirement| {
            (
                requirement.binding_name,
                if matches!(requirement.key, EffectRequirementKey::Other) {
                    ContractResolutionSource::Unresolved
                } else {
                    ContractResolutionSource::RootProvider
                },
            )
        })
        .collect()
}

fn contract_effect_resolutions_canonical<'db>(
    db: &'db dyn HirAnalysisDb,
    contract: Contract<'db>,
    site: EffectParamSite<'db>,
    list: EffectParamListId<'db>,
) -> Vec<ResolvedEffectBinding> {
    let providers = provider_bindings_for_site(db, site);
    let root_provider_idx = providers.iter().find_map(|provider| {
        matches!(provider.source, ProviderSource::RootProvider { .. })
            .then_some(provider.provider_idx)
    });
    let field_provider_idx = providers
        .iter()
        .filter_map(|provider| match provider.source {
            ProviderSource::ContractField { field } => Some((field.index, provider.provider_idx)),
            ProviderSource::UsesParam { .. } | ProviderSource::RootProvider { .. } => None,
        })
        .collect::<FxHashMap<_, _>>();
    let contract_named_sources = contract_effect_decl_resolution_sources(db, contract);
    let fields = contract.fields(db);

    effect_requirements_for_site(db, site)
        .iter()
        .filter_map(|requirement| {
            let effect = list.data(db).get(requirement.binding_idx as usize)?;
            let key_path = effect.key_ty.to_opt()?.as_path(db)?;
            let source = if effect.name.is_some() {
                ContractResolutionSource::RootProvider
            } else if key_path.len(db) == 1
                && let Some(name) = key_path.ident(db).to_opt()
                && let Some(field) = fields.get(&name)
            {
                ContractResolutionSource::Field(field.index)
            } else if key_path.len(db) == 1
                && let Some(name) = key_path.ident(db).to_opt()
            {
                contract_named_sources
                    .get(&name)
                    .copied()
                    .unwrap_or(ContractResolutionSource::Unresolved)
            } else {
                ContractResolutionSource::Unresolved
            };

            let provider_idx = match source {
                ContractResolutionSource::RootProvider => root_provider_idx?,
                ContractResolutionSource::Field(field_idx) => {
                    *field_provider_idx.get(&field_idx)?
                }
                ContractResolutionSource::Unresolved => return None,
            };
            Some(ResolvedEffectBinding {
                requirement_idx: requirement.binding_idx,
                provider_idx,
            })
        })
        .collect()
}

fn compute_arg_bindings<'db>(
    db: &'db dyn HirAnalysisDb,
    variant_struct: Struct<'db>,
    args_ty: TyId<'db>,
    arm_pat: PatId,
    arm_body: Body<'db>,
) -> Option<Vec<ArgBinding<'db>>> {
    let Partial::Present(pat) = arm_pat.data(db, arm_body) else {
        return None;
    };

    let elem_tys = args_ty.field_types(db);

    let bindings = match pat {
        Pat::Record(_, fields) => {
            let mut out = Vec::new();
            for f in fields {
                let Some(label) = f.label(db, arm_body) else {
                    continue;
                };
                let Some(field_idx) = variant_struct.hir_fields(db).field_idx(db, label) else {
                    continue;
                };
                let Some(elem_ty) = elem_tys.get(field_idx).copied() else {
                    continue;
                };
                out.push(ArgBinding {
                    pat: f.pat,
                    tuple_index: field_idx as u32,
                    ty: elem_ty,
                });
            }
            out
        }
        Pat::PathTuple(_, pats) | Pat::Tuple(pats) => pats
            .iter()
            .enumerate()
            .filter_map(|(idx, pat)| {
                let ty = elem_tys.get(idx).copied()?;
                Some(ArgBinding {
                    pat: *pat,
                    tuple_index: idx as u32,
                    ty,
                })
            })
            .collect(),
        _ => Vec::new(),
    };

    Some(bindings)
}

fn resolve_msg_mod<'db>(
    db: &'db dyn HirAnalysisDb,
    contract: Contract<'db>,
    msg_path: PathId<'db>,
    assumptions: PredicateListId<'db>,
) -> Option<Mod<'db>> {
    use crate::analysis::name_resolution::{PathRes, resolve_path};
    use crate::hir_def::ItemKind;

    if let PathRes::Mod(ScopeId::Item(ItemKind::Mod(m))) =
        resolve_path(db, msg_path, contract.scope(), assumptions, false).ok()?
    {
        Some(m)
    } else {
        None
    }
}

fn resolve_recv_variant<'db>(
    db: &'db dyn HirAnalysisDb,
    contract: Contract<'db>,
    msg_mod: Option<Mod<'db>>,
    recv_is_bare: bool,
    arm: ContractRecvArm<'db>,
    assumptions: PredicateListId<'db>,
) -> (Option<Struct<'db>>, Option<TyId<'db>>) {
    let Some(variant_path) = arm.variant_path(db) else {
        return (None, None);
    };

    if let Some(msg_mod) = msg_mod {
        match crate::analysis::ty::ty_check::resolve_variant_in_msg(
            db,
            msg_mod,
            variant_path,
            assumptions,
        ) {
            Ok(resolved) => (Some(resolved.variant_struct), Some(resolved.ty)),
            _ => (None, None),
        }
    } else if recv_is_bare {
        match crate::analysis::ty::ty_check::resolve_variant_bare(
            db,
            contract,
            variant_path,
            assumptions,
        ) {
            Ok(resolved) => (Some(resolved.variant_struct), Some(resolved.ty)),
            _ => (None, None),
        }
    } else {
        (None, None)
    }
}

fn resolve_sol_abi_ty<'db>(
    db: &'db dyn HirAnalysisDb,
    scope: ScopeId<'db>,
    assumptions: PredicateListId<'db>,
) -> Option<TyId<'db>> {
    use crate::analysis::name_resolution::{PathRes, resolve_path};
    use common::ingot::IngotKind;

    let ingot = scope.ingot(db);
    let std_root = if ingot.kind(db) == IngotKind::Std {
        IdentId::make_ingot(db)
    } else {
        IdentId::new(db, "std".to_string())
    };

    let sol_path = PathId::from_ident(db, std_root)
        .push_ident(db, IdentId::new(db, "abi".to_string()))
        .push_ident(db, IdentId::new(db, "Sol".to_string()));

    match resolve_path(db, sol_path, scope, assumptions, false).ok()? {
        PathRes::Ty(ty) | PathRes::TyAlias(_, ty, _) => Some(ty),
        _ => None,
    }
}

pub(crate) fn get_variant_selector_info<'db>(
    db: &'db dyn HirAnalysisDb,
    variant_ty: TyId<'db>,
    scope: ScopeId<'db>,
) -> VariantSelectorInfo {
    use crate::analysis::semantic::{
        EvalOutcome, GenericSubst, SemConstScalar, SemConstValue, eval_body_owner_const,
    };
    use crate::analysis::ty::{
        canonical::Canonical,
        corelib::resolve_core_trait,
        trait_def::impls_for_ty,
        ty_def::{PrimTy, TyBase, TyData},
    };
    use num_traits::ToPrimitive;

    let Some(msg_variant_trait) = resolve_core_trait(db, scope, &["message", "MsgVariant"]) else {
        return VariantSelectorInfo {
            value: None,
            signature: None,
        };
    };
    let canonical_ty = Canonical::new(db, variant_ty);
    let scope_ingot = scope.ingot(db);
    let search_ingots = [
        Some(scope_ingot),
        variant_ty.ingot(db).filter(|&ingot| ingot != scope_ingot),
    ];

    let Some(impl_) = search_ingots.into_iter().flatten().find_map(|ingot| {
        impls_for_ty(db, ingot, canonical_ty)
            .iter()
            .find(|impl_| impl_.trait_def(db).eq(&msg_variant_trait))
            .copied()
    }) else {
        return VariantSelectorInfo {
            value: None,
            signature: None,
        };
    };

    let selector_name = IdentId::new(db, "SELECTOR".to_string());
    let hir_impl = impl_.hir_impl_trait(db);
    let Some(selector_const) = hir_impl
        .hir_consts(db)
        .iter()
        .find(|c| c.name.to_opt() == Some(selector_name))
    else {
        return VariantSelectorInfo {
            value: None,
            signature: None,
        };
    };

    let Some(body) = selector_const.value.to_opt() else {
        return VariantSelectorInfo {
            value: None,
            signature: None,
        };
    };
    if matches!(
        body.expr(db).data(db, body),
        crate::hir_def::Partial::Absent
    ) {
        return VariantSelectorInfo {
            value: None,
            signature: None,
        };
    }
    let signature = selector_signature_from_body(db, body, hir_impl.scope());
    let expected_ty = TyId::new(db, TyData::TyBase(TyBase::Prim(PrimTy::U32)));
    let value = match eval_body_owner_const(
        db,
        crate::analysis::ty::ty_check::BodyOwner::AnonConstBody {
            body,
            expected: expected_ty,
        },
        GenericSubst::none(db),
    ) {
        EvalOutcome::Ready(value) => match value.value(db) {
            SemConstValue::Scalar {
                value: SemConstScalar::Int { value },
                ..
            } => value.to_u32(),
            _ => None,
        },
        EvalOutcome::Blocked(_) | EvalOutcome::Failed(_) => None,
    };

    VariantSelectorInfo { value, signature }
}

fn selector_signature_from_body<'db>(
    db: &'db dyn HirAnalysisDb,
    body: Body<'db>,
    scope: ScopeId<'db>,
) -> Option<String> {
    use crate::analysis::ty::ty_def::{PrimTy, TyBase, TyData};

    let expected_ty = TyId::new(db, TyData::TyBase(TyBase::Prim(PrimTy::U32)));
    let (_, typed_body) =
        crate::analysis::ty::ty_check::check_anon_const_body(db, body, expected_ty);
    let mut visited = SelectorSearchVisited::new();

    selector_signature_from_typed_body(db, body, scope, typed_body, &mut visited)
}

/// Tracks already-visited items to prevent cycles when following const/fn references.
struct SelectorSearchVisited<'db> {
    consts: FxHashSet<Const<'db>>,
    inherent_consts: FxHashSet<Body<'db>>,
    funcs: FxHashSet<Func<'db>>,
    locals: FxHashSet<PatId>,
}

impl<'db> SelectorSearchVisited<'db> {
    fn new() -> Self {
        Self {
            consts: FxHashSet::default(),
            inherent_consts: FxHashSet::default(),
            funcs: FxHashSet::default(),
            locals: FxHashSet::default(),
        }
    }
}

fn selector_signature_from_typed_body<'db>(
    db: &'db dyn HirAnalysisDb,
    body: Body<'db>,
    scope: ScopeId<'db>,
    typed_body: &crate::analysis::ty::ty_check::TypedBody<'db>,
    visited: &mut SelectorSearchVisited<'db>,
) -> Option<String> {
    let expr_id = body.expr(db);
    selector_signature_from_expr(db, body, scope, typed_body, visited, expr_id)
}

fn selector_signature_from_expr<'db>(
    db: &'db dyn HirAnalysisDb,
    body: Body<'db>,
    scope: ScopeId<'db>,
    typed_body: &crate::analysis::ty::ty_check::TypedBody<'db>,
    visited: &mut SelectorSearchVisited<'db>,
    expr_id: ExprId,
) -> Option<String> {
    match expr_id.data(db, body) {
        Partial::Present(Expr::Call(_callee, args)) => {
            if args.len() == 1
                && args[0].label.is_none()
                && let Partial::Present(Expr::Lit(LitKind::String(signature))) =
                    args[0].expr.data(db, body)
            {
                let resolved_sol = resolve_lib_func_path(db, scope, "std::abi::sol")?;
                if let crate::hir_def::CallableDef::Func(func) =
                    typed_body.callable_expr(expr_id)?.callable_def
                    && func == resolved_sol
                {
                    return Some(signature.data(db).to_string());
                }
            }

            // Follow const fn calls
            if let Some(callable) = typed_body.callable_expr(expr_id)
                && let crate::hir_def::CallableDef::Func(func) = callable.callable_def
                && func.is_const(db)
                && visited.funcs.insert(func)
            {
                let (_, func_typed_body) = crate::analysis::ty::ty_check::check_func_body(db, func);
                if let Some(func_body) = func_typed_body.body() {
                    return selector_signature_from_typed_body(
                        db,
                        func_body,
                        func.scope(),
                        func_typed_body,
                        visited,
                    );
                }
            }
        }
        Partial::Present(Expr::Block(stmts)) => {
            if let Some(last_stmt) = stmts.last()
                && let Partial::Present(Stmt::Expr(inner_expr)) = last_stmt.data(db, body)
            {
                return selector_signature_from_expr(
                    db,
                    body,
                    scope,
                    typed_body,
                    visited,
                    *inner_expr,
                );
            }
        }
        _ => {}
    }

    if let Some(signature) =
        selector_signature_from_local_binding(db, body, scope, typed_body, visited, expr_id)
    {
        return Some(signature);
    }

    match typed_body.expr_const_ref(expr_id)? {
        crate::analysis::ty::ty_check::ConstRef::Const(const_) => {
            if !visited.consts.insert(const_) {
                return None;
            }
            let (_, const_typed_body) = crate::analysis::ty::ty_check::check_const_body(db, const_);
            let const_body = const_typed_body.body()?;
            selector_signature_from_typed_body(
                db,
                const_body,
                const_.scope(),
                const_typed_body,
                visited,
            )
        }
        crate::analysis::ty::ty_check::ConstRef::InherentConst(use_) => {
            // Follow the inherent const's value body so a selector that
            // delegates to `Selectors::TRANSFER` resolves the underlying
            // `sol("...")` signature, mirroring the top-level `Const` branch.
            let (const_body, _) = crate::analysis::ty::const_ty::inherent_const_body_and_impl_args(
                db,
                use_.impl_(),
                use_.receiver_ty(),
                use_.name(),
            )?;
            if !visited.inherent_consts.insert(const_body) {
                return None;
            }
            let expected = crate::analysis::ty::const_ty::inherent_const_expected_ty(
                db,
                use_.impl_(),
                use_.receiver_ty(),
                use_.name(),
            )?;
            let (_, const_typed_body) =
                crate::analysis::ty::ty_check::check_anon_const_body(db, const_body, expected);
            selector_signature_from_typed_body(
                db,
                const_body,
                use_.impl_().scope(),
                const_typed_body,
                visited,
            )
        }
        crate::analysis::ty::ty_check::ConstRef::TraitConst { .. } => None,
    }
}

fn selector_signature_from_local_binding<'db>(
    db: &'db dyn HirAnalysisDb,
    body: Body<'db>,
    scope: ScopeId<'db>,
    typed_body: &crate::analysis::ty::ty_check::TypedBody<'db>,
    visited: &mut SelectorSearchVisited<'db>,
    expr_id: ExprId,
) -> Option<String> {
    let crate::analysis::ty::ty_check::LocalBinding::Local { pat, .. } =
        typed_body.expr_binding(expr_id)?
    else {
        return None;
    };

    if !visited.locals.insert(pat) {
        return None;
    }

    let init_expr = selector_local_binding_init_expr(db, body, pat)?;
    selector_signature_from_expr(db, body, scope, typed_body, visited, init_expr)
}

fn selector_local_binding_init_expr<'db>(
    db: &'db dyn HirAnalysisDb,
    body: Body<'db>,
    pat: PatId,
) -> Option<ExprId> {
    for (_, stmt) in body.stmts(db).iter() {
        if let Partial::Present(Stmt::Let(stmt_pat, _, Some(init_expr))) = stmt
            && *stmt_pat == pat
        {
            return Some(*init_expr);
        }
    }

    None
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Update)]
pub enum EffectParamOwner<'db> {
    Func(Func<'db>),
    Contract(Contract<'db>),
    RecvArm(RecvArmView<'db>),
}
impl<'db> EffectParamOwner<'db> {
    pub fn scope(self, db: &'db dyn HirDb) -> ScopeId<'db> {
        match self {
            EffectParamOwner::Func(func) => func.scope(),
            EffectParamOwner::Contract(contract) => contract.scope(),
            EffectParamOwner::RecvArm(arm) => arm.contract(db).scope(),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct EffectParamView<'db> {
    pub owner: EffectParamOwner<'db>,
    idx: usize,
}

impl<'db> EffectParamView<'db> {
    fn effect(self, db: &'db dyn HirDb) -> &'db crate::core::hir_def::EffectParam<'db> {
        match self.owner {
            EffectParamOwner::Func(func) => &func.effects(db).data(db)[self.idx],
            EffectParamOwner::Contract(contract) => &contract.effects(db).data(db)[self.idx],
            EffectParamOwner::RecvArm(arm) => &arm.effects(db).data(db)[self.idx],
        }
    }

    /// Optional name for this effect parameter.
    pub fn name(self, db: &'db dyn HirDb) -> Option<IdentId<'db>> {
        self.effect(db).name
    }

    /// The type syntax identifying the effect key.
    pub fn key_ty(self, db: &'db dyn HirDb) -> Option<TypeId<'db>> {
        self.effect(db).key_ty.to_opt()
    }

    /// The path identifying a nominal effect key, if the key is path-shaped.
    pub fn key_path(self, db: &'db dyn HirDb) -> Option<PathId<'db>> {
        self.key_ty(db)?
            .as_path(db)
            .filter(|path| path.ident(db).is_present())
    }

    /// Whether this effect requires mutation.
    pub fn is_mut(self, db: &'db dyn HirDb) -> bool {
        self.effect(db).is_mut
    }

    /// Index of this effect in the function's effect list.
    pub fn index(self) -> usize {
        self.idx
    }
}

impl<'db> Func<'db> {
    /// Iterate parameters as contextual views (semantic traversal helper).
    pub fn params(self, db: &'db dyn HirDb) -> impl Iterator<Item = FuncParamView<'db>> + 'db {
        let len = self
            .params_list(db)
            .to_opt()
            .map_or(0, |l| l.data(db).len());
        (0..len).map(move |idx| FuncParamView { func: self, idx })
    }

    /// Iterate effect parameters as contextual views.
    pub fn effect_params(
        self,
        db: &'db dyn HirDb,
    ) -> impl Iterator<Item = EffectParamView<'db>> + 'db {
        let len = self.effects(db).data(db).len();
        let owner = EffectParamOwner::Func(self);
        (0..len).map(move |idx| EffectParamView { owner, idx })
    }

    /// Returns true if this function has any effect parameters.
    pub fn has_effects(self, db: &'db dyn HirDb) -> bool {
        !self.effects(db).data(db).is_empty()
    }
}

/// Helper to check if a type's base matches a given ADT.
fn matches_adt<'db>(db: &'db dyn HirAnalysisDb, ty: TyId<'db>, adt: AdtDef<'db>) -> bool {
    match ty.base_ty(db).data(db) {
        TyData::TyBase(TyBase::Adt(ty_adt)) => *ty_adt == adt,
        _ => false,
    }
}

impl<'db> Enum<'db> {
    pub fn len_variants(&self, db: &'db dyn HirDb) -> usize {
        self.variants_list(db).data(db).len()
    }

    /// Iterates variants as contextual views (structural traversal helper).
    pub fn variants(self, db: &'db dyn HirDb) -> impl Iterator<Item = VariantView<'db>> + 'db {
        let list = self.variants_list(db);
        list.data(db)
            .iter()
            .enumerate()
            .map(move |(idx, _)| VariantView { owner: self, idx })
    }

    /// Semantic ADT definition for this enum (cached via tracked query).
    pub fn as_adt(self, db: &'db dyn HirAnalysisDb) -> AdtDef<'db> {
        lower_adt(db, AdtRef::from(self))
    }

    /// Returns all inherent `impl` blocks for this enum within the same ingot.
    pub fn all_impls(self, db: &'db dyn HirAnalysisDb) -> Vec<Impl<'db>> {
        let adt = self.as_adt(db);
        self.top_mod(db)
            .ingot(db)
            .all_impls(db)
            .iter()
            .copied()
            .filter(|impl_| matches_adt(db, impl_.ty(db), adt))
            .collect()
    }

    /// Returns all `impl Trait for Enum` blocks for this enum within the same ingot.
    pub fn all_impl_traits(self, db: &'db dyn HirAnalysisDb) -> Vec<ImplTrait<'db>> {
        let adt = self.as_adt(db);
        self.top_mod(db)
            .ingot(db)
            .all_impl_traits(db)
            .iter()
            .copied()
            .filter(|impl_trait| matches_adt(db, impl_trait.ty(db), adt))
            .collect()
    }
}

impl<'db> Struct<'db> {
    /// Returns semantic types of all fields, bound to identity parameters.
    pub fn field_tys(self, db: &'db dyn HirAnalysisDb) -> Vec<Binder<'db, TyId<'db>>> {
        use crate::analysis::ty::ty_def::{InvalidCause, TyId};
        use crate::analysis::ty::ty_lower::lower_hir_ty;

        let scope = self.scope();
        let assumptions =
            collect_constraints(db, GenericParamOwner::Struct(self)).instantiate_identity();
        let fields = self.fields(db);

        fields
            .data(db)
            .iter()
            .map(|field| {
                let ty = match field.type_ref.to_opt() {
                    Some(hir_ty) => lower_hir_ty(db, hir_ty, scope, assumptions),
                    None => TyId::invalid(db, InvalidCause::ParseError),
                };
                Binder::bind(self.into(), ty)
            })
            .collect()
    }

    /// Semantic ADT definition for this struct (cached via tracked query).
    pub fn as_adt(self, db: &'db dyn HirAnalysisDb) -> AdtDef<'db> {
        lower_adt(db, AdtRef::from(self))
    }

    /// Returns all inherent `impl` blocks for this struct within the same ingot.
    pub fn all_impls(self, db: &'db dyn HirAnalysisDb) -> Vec<Impl<'db>> {
        let adt = self.as_adt(db);
        self.top_mod(db)
            .ingot(db)
            .all_impls(db)
            .iter()
            .copied()
            .filter(|impl_| matches_adt(db, impl_.ty(db), adt))
            .collect()
    }

    /// Returns all `impl Trait for Struct` blocks for this struct within the same ingot.
    pub fn all_impl_traits(self, db: &'db dyn HirAnalysisDb) -> Vec<ImplTrait<'db>> {
        let adt = self.as_adt(db);
        self.top_mod(db)
            .ingot(db)
            .all_impl_traits(db)
            .iter()
            .copied()
            .filter(|impl_trait| matches_adt(db, impl_trait.ty(db), adt))
            .collect()
    }
}

#[derive(Clone, Copy, Debug)]
pub struct WhereClauseView<'db> {
    pub owner: WhereClauseOwner<'db>,
    pub id: WhereClauseId<'db>,
}

#[derive(Clone, Copy, Debug)]
pub struct WherePredicateView<'db> {
    pub clause: WhereClauseView<'db>,
    /// The predicate's position among all the clause's predicates.
    pub idx: usize,
    predicate: &'db WherePredicate<'db>,
}

impl<'db> WhereClauseOwner<'db> {
    /// Semantic where-clause view for this owner.
    pub fn clause(self, db: &'db dyn HirDb) -> WhereClauseView<'db> {
        WhereClauseView {
            owner: self,
            id: self.where_clause(db),
        }
    }
}

impl<'db> WhereClauseView<'db> {
    pub fn predicates(
        self,
        db: &'db dyn HirDb,
    ) -> impl Iterator<Item = WherePredicateView<'db>> + 'db {
        self.id
            .type_predicates(db)
            .map(move |(idx, predicate)| WherePredicateView {
                clause: self,
                idx,
                predicate,
            })
    }

    pub fn span(self) -> crate::span::params::LazyWhereClauseSpan<'db> {
        match self.owner {
            WhereClauseOwner::Func(f) => f.span().where_clause(),
            WhereClauseOwner::Struct(s) => s.span().where_clause(),
            WhereClauseOwner::Enum(e) => e.span().where_clause(),
            WhereClauseOwner::TypeFn(def) => def.span().where_clause(),
            WhereClauseOwner::Impl(i) => i.span().where_clause(),
            WhereClauseOwner::Trait(t) => t.span().where_clause(),
            WhereClauseOwner::ImplTrait(i) => i.span().where_clause(),
        }
    }
}

impl<'db> WherePredicateView<'db> {
    pub(in crate::core) fn hir_pred(self, _db: &'db dyn HirDb) -> &'db WherePredicate<'db> {
        self.predicate
    }

    fn owner_item(self) -> ItemKind<'db> {
        ItemKind::from(self.clause.owner)
    }

    /// If this predicate's subject is one of the owner's generic parameters,
    /// returns its original index (0-based within the owner).
    pub fn param_original_index(self, db: &'db dyn HirDb) -> Option<usize> {
        use crate::core::hir_def::types::TypeKind as HirTyKind;
        let hir_ty = self.hir_pred(db).ty.to_opt()?;
        let path = match hir_ty.data(db) {
            HirTyKind::Path(p) => p.to_opt()?,
            _ => return None,
        };

        let ident = path.as_ident(db)?;
        let owner = GenericParamOwner::from_item_opt(self.owner_item())?;
        let params = owner.params_list(db).data(db);
        params.iter().position(|p| match p {
            GenericParam::Type(t) => t.name.to_opt() == Some(ident),
            GenericParam::Const(c) => c.name.to_opt() == Some(ident),
        })
    }

    /// Lowered kind bound sourced from the where-clause, if present.
    pub fn kind(self, db: &'db dyn HirAnalysisDb) -> Option<Kind> {
        use crate::hir_def::Partial;
        for b in &self.hir_pred(db).bounds {
            if let TypeBound::Kind(Partial::Present(kb)) = b {
                return Some(lower_hir_kind_local(db, self.owner_item().scope(), kb));
            }
            if let TypeBound::Trait(reference) = b
                && crate::analysis::ty::ty_lower::is_core_constraint_bound(
                    db,
                    self.owner_item().scope(),
                    *reference,
                )
            {
                return Some(Kind::Constraint);
            }
        }
        None
    }

    pub fn span(self) -> crate::span::params::LazyWherePredicateSpan<'db> {
        self.clause.span().predicate(self.idx)
    }

    /// Iterate trait bounds as per-bound semantic views.
    pub fn bounds(
        self,
        db: &'db dyn HirAnalysisDb,
    ) -> impl Iterator<Item = WherePredicateBoundView<'db>> + 'db {
        let idxs: Vec<usize> = self
            .hir_pred(db)
            .bounds
            .iter()
            .enumerate()
            .filter_map(|(i, b)| match b {
                TypeBound::Trait(reference)
                    if !crate::analysis::ty::ty_lower::is_core_constraint_bound(
                        db,
                        self.owner_item().scope(),
                        *reference,
                    ) =>
                {
                    Some(i)
                }
                _ => None,
            })
            .collect();
        idxs.into_iter()
            .map(move |idx| WherePredicateBoundView { pred: self, idx })
    }

    /// True if this predicate's subject type is `Self` (within a trait).
    pub fn is_self_subject(self, db: &'db dyn HirDb) -> bool {
        self.hir_pred(db)
            .ty
            .to_opt()
            .map(|ty| ty.is_self_ty(db))
            .unwrap_or_default()
    }

    /// Lower the subject type of this where-predicate into a semantic `TyId`.
    /// Returns `None` if the HIR type is missing or invalid.
    pub fn subject_ty(self, db: &'db dyn HirAnalysisDb) -> Option<TyId<'db>> {
        let hir_ty = self.hir_pred(db).ty.to_opt()?;
        let owner_item = ItemKind::from(self.clause.owner);
        let assumptions = header_constraints_for(db, owner_item);
        Some(lower_hir_ty(db, hir_ty, owner_item.scope(), assumptions))
    }

    /// True if the lowered subject type is a const type.
    pub fn subject_is_const(self, db: &'db dyn HirAnalysisDb) -> bool {
        self.subject_ty(db).is_some_and(|t| t.is_const_ty(db))
    }

    /// True if the lowered subject type is concrete (no generic params) and not invalid.
    pub fn subject_is_concrete(self, db: &'db dyn HirAnalysisDb) -> bool {
        self.subject_ty(db)
            .is_some_and(|t| !t.has_invalid(db) && !t.has_param(db))
    }

    /// Diagnostics for all bounds (trait and kind) of this predicate.
    pub fn bound_diags(
        self,
        db: &'db dyn HirAnalysisDb,
        subject: TyId<'db>,
    ) -> Vec<TyDiagCollection<'db>> {
        let mut out = Vec::new();
        let hir = self.predicate;

        for (i, bound) in hir.bounds.iter().enumerate() {
            match bound {
                TypeBound::Trait(reference)
                    if crate::analysis::ty::ty_lower::is_core_constraint_bound(
                        db,
                        self.owner_item().scope(),
                        *reference,
                    ) =>
                {
                    let expected = Kind::Constraint;
                    if !subject.kind(db).does_match(&expected) {
                        out.push(
                            TyLowerDiag::InconsistentKindBound {
                                span: self.span().bounds().bound(i).trait_bound().into(),
                                ty: subject,
                                bound: expected,
                            }
                            .into(),
                        );
                    }
                }
                TypeBound::Trait(reference)
                    if !crate::analysis::ty::ty_lower::is_core_constraint_bound(
                        db,
                        self.owner_item().scope(),
                        *reference,
                    ) =>
                {
                    let bview = WherePredicateBoundView::new(self, i);
                    out.extend(bview.diags(db));
                }
                TypeBound::Kind(crate::hir_def::Partial::Present(kb)) => {
                    let span = self.span().bounds().bound(i).kind_bound();
                    if let Some(name) = crate::analysis::ty::ty_lower::unknown_named_kind(
                        db,
                        self.owner_item().scope(),
                        kb,
                    ) {
                        out.push(
                            TyLowerDiag::UnknownKindBound {
                                span: span.into(),
                                name,
                            }
                            .into(),
                        );
                        continue;
                    }
                    let expected = lower_hir_kind_local(db, self.owner_item().scope(), kb);
                    let actual = subject.kind(db);
                    if !actual.does_match(&expected) {
                        out.push(
                            TyLowerDiag::InconsistentKindBound {
                                span: span.into(),
                                ty: subject,
                                bound: expected,
                            }
                            .into(),
                        );
                    }
                }
                _ => {}
            }
        }
        out
    }
}

#[derive(Clone, Copy, Debug)]
pub struct WherePredicateBoundView<'db> {
    pub pred: WherePredicateView<'db>,
    pub idx: usize,
}

impl<'db> WherePredicateBoundView<'db> {
    pub fn new(pred: WherePredicateView<'db>, idx: usize) -> Self {
        Self { pred, idx }
    }
    pub fn trait_ref(self, db: &'db dyn HirDb) -> TraitRefId<'db> {
        match &self.pred.hir_pred(db).bounds[self.idx] {
            TypeBound::Trait(tr) => *tr,
            _ => unreachable!(),
        }
    }

    pub fn span(self) -> crate::span::params::LazyTypeBoundSpan<'db> {
        self.pred.span().bounds().bound(self.idx)
    }

    pub fn trait_ref_span(self) -> crate::span::params::LazyTraitRefSpan<'db> {
        self.span().trait_bound()
    }

    /// Lower this bound into a semantic trait instance using the predicate's subject type.
    pub(in crate::core) fn as_trait_inst(
        self,
        db: &'db dyn HirAnalysisDb,
    ) -> Option<TraitInstId<'db>> {
        let subject = self.pred.subject_ty(db)?;
        let owner_item = ItemKind::from(self.pred.clause.owner);
        let assumptions = header_constraints_for(db, owner_item);
        let scope = owner_item.scope();
        lower_trait_ref(
            db,
            subject,
            self.trait_ref(db),
            scope,
            assumptions,
            crate::analysis::ty::trait_resolution::constraint::enclosing_trait_self_ty(db, scope),
        )
        .ok()
    }
}

impl<'db> TypeAlias<'db> {
    /// Semantic alias target type (convenience over lower_type_alias).
    pub fn ty(self, db: &'db dyn HirAnalysisDb) -> TyId<'db> {
        let ta = lower_type_alias(db, self);
        *ta.alias_to.skip_binder()
    }

    /// Type lowering errors for the alias target type.
    pub fn ty_errors(self, db: &'db dyn HirAnalysisDb) -> Vec<TyDiagCollection<'db>> {
        let Some(hir_ty) = self.type_ref(db).to_opt() else {
            return Vec::new();
        };
        let assumptions = constraints_for(db, self.into());
        let ty = lower_hir_ty(db, hir_ty, self.scope(), assumptions);
        let premises = crate::analysis::semantic::SolverPremises::from_unpositioned_pair(
            self.scope(),
            assumptions,
        );
        if ty.has_invalid(db) {
            collect_ty_lower_errors(db, hir_ty, self.span().ty(), premises)
        } else if let Some(cause) = demanded_ground_const_cause(db, ty) {
            diag_from_invalid_cause(self.span().ty().into(), &cause)
                .into_iter()
                .collect()
        } else {
            crate::analysis::ty::ty_error::normalization_limit_diag(
                db,
                premises,
                ty,
                self.span().ty().into(),
            )
            .into_iter()
            .collect()
        }
    }

    /// Well-formedness errors for the alias target type.
    pub fn ty_wf_errors(self, db: &'db dyn HirAnalysisDb) -> Vec<TyDiagCollection<'db>> {
        let Some(hir_ty) = self.type_ref(db).to_opt() else {
            return Vec::new();
        };
        let assumptions = constraints_for(db, self.into());
        let ty = lower_hir_ty(db, hir_ty, self.scope(), assumptions);
        if let Some(diag) = check_ty_wf(db, item_solver_context(db, self.scope()), ty)
            .into_diag(self.span().ty().into())
        {
            vec![diag]
        } else {
            Vec::new()
        }
    }
}

// Trait / Impl items --------------------------------------------------------

impl<'db> Trait<'db> {
    pub fn params(self, db: &'db dyn HirAnalysisDb) -> &'db [TyId<'db>] {
        collect_generic_params(db, self.into()).params(db)
    }

    pub fn param_set(self, db: &'db dyn HirAnalysisDb) -> GenericParamTypeSet<'db> {
        collect_generic_params(db, self.into())
    }

    pub fn self_param(self, db: &'db dyn HirAnalysisDb) -> TyId<'db> {
        collect_generic_params(db, self.into())
            .trait_self(db)
            .unwrap()
    }

    pub fn original_params(self, db: &'db dyn HirAnalysisDb) -> &'db [TyId<'db>] {
        collect_generic_params(db, self.into()).explicit_params(db)
    }

    pub fn method_defs(self, db: &'db dyn HirAnalysisDb) -> IndexMap<IdentId<'db>, Func<'db>> {
        let mut methods = IndexMap::default();
        for method in self.methods(db) {
            if let Some(name) = method.name(db).to_opt() {
                methods.insert(name, method);
            }
        }
        methods
    }

    pub fn ingot(self, db: &'db dyn HirDb) -> common::ingot::Ingot<'db> {
        self.top_mod(db).ingot(db)
    }

    /// Iterate associated types as contextual views.
    pub fn assoc_types(
        self,
        db: &'db dyn HirDb,
    ) -> impl Iterator<Item = TraitAssocTypeView<'db>> + 'db {
        let len = self.types(db).len();
        (0..len).map(move |idx| TraitAssocTypeView { owner: self, idx })
    }

    /// Iterate associated consts as contextual views.
    pub fn assoc_consts(
        self,
        db: &'db dyn HirDb,
    ) -> impl Iterator<Item = TraitAssocConstView<'db>> + 'db {
        let len = self.consts(db).len();
        (0..len).map(move |idx| TraitAssocConstView { owner: self, idx })
    }

    /// Get an associated const view by name, if it exists.
    pub fn const_(
        self,
        db: &'db dyn HirDb,
        name: IdentId<'db>,
    ) -> Option<TraitAssocConstView<'db>> {
        self.assoc_consts(db).find(|c| c.name(db) == Some(name))
    }

    /// Get the index of an associated const by name, if it exists.
    pub fn const_index(self, db: &'db dyn HirDb, name: IdentId<'db>) -> Option<usize> {
        self.assoc_consts(db)
            .enumerate()
            .find_map(|(idx, c)| (c.name(db) == Some(name)).then_some(idx))
    }

    /// Get an associated const view by its index.
    pub fn const_by_index(self, idx: usize) -> TraitAssocConstView<'db> {
        TraitAssocConstView { owner: self, idx }
    }

    /// Iterate declared super-trait references as contextual views.
    pub fn super_trait_refs(
        self,
        db: &'db dyn HirDb,
    ) -> impl Iterator<Item = SuperTraitRefView<'db>> + 'db {
        let len = self.super_traits_refs(db).len();
        (0..len).map(move |idx| SuperTraitRefView { owner: self, idx })
    }

    /// Semantic super-trait bounds using the trait's own `Self` as subject.
    pub fn super_trait_bounds(
        self,
        db: &'db dyn HirAnalysisDb,
    ) -> impl Iterator<Item = TraitInstId<'db>> + 'db {
        let self_param = self.self_param(db);
        let scope = self.scope();
        let assumptions = collect_constraints(db, self.into()).instantiate_identity();

        let mut super_traits = IndexSet::new();
        for view in self.super_trait_refs(db) {
            let super_ref = view.trait_ref(db);
            if let Ok(inst) = lower_trait_ref(db, self_param, super_ref, scope, assumptions, None) {
                super_traits.insert(inst);
            }
        }

        for pred in WhereClauseOwner::Trait(self).clause(db).predicates(db) {
            if !pred.is_self_subject(db) {
                continue;
            }
            for bound in pred.bounds(db) {
                if let Some(inst) = bound.as_trait_inst(db) {
                    super_traits.insert(inst);
                }
            }
        }

        super_traits.into_iter()
    }

    pub(crate) fn super_traits(
        self,
        db: &'db dyn HirAnalysisDb,
    ) -> IndexSet<Binder<'db, TraitInstId<'db>>> {
        self.super_trait_bounds(db)
            .map(|bound| Binder::bind(self.into(), bound))
            .collect()
    }

    /// Returns all `impl Trait for Type` blocks that implement this trait
    /// within the same ingot.
    pub fn all_impl_traits(self, db: &'db dyn HirAnalysisDb) -> Vec<ImplTrait<'db>> {
        self.ingot(db)
            .all_impl_traits(db)
            .iter()
            .copied()
            .filter(|impl_trait| impl_trait.trait_def(db) == Some(self))
            .collect()
    }

    /// Returns all implementations of a specific method from this trait.
    ///
    /// Given a method name, finds all `impl Trait for Type` blocks that implement
    /// this trait and returns the corresponding method implementations.
    pub fn method_implementations(
        self,
        db: &'db dyn HirAnalysisDb,
        method_name: IdentId<'db>,
    ) -> Vec<Func<'db>> {
        self.all_impl_traits(db)
            .into_iter()
            .filter_map(|impl_trait| {
                impl_trait
                    .methods(db)
                    .find(|m| m.name(db).to_opt() == Some(method_name))
            })
            .collect()
    }

    /// Returns the method definition in this trait with the given name.
    pub fn method(self, db: &'db dyn HirDb, name: IdentId<'db>) -> Option<Func<'db>> {
        self.methods(db).find(|m| m.name(db).to_opt() == Some(name))
    }
}

// ADT recursion (semantic) --------------------------------------------------

impl<'db> AdtDef<'db> {
    /// Detects a recursive ADT cycle that is not guarded by an indirect wrapper
    /// (e.g., pointer/reference). Returns the cycle members if the ADT is part
    /// of a cycle; otherwise returns None.
    pub fn recursive_cycle(self, db: &'db dyn HirAnalysisDb) -> Option<Vec<AdtCycleMember<'db>>> {
        fn impl_check<'db>(
            db: &'db dyn HirAnalysisDb,
            root: AdtDef<'db>,
            adt: AdtDef<'db>,
            chain: &[AdtCycleMember<'db>],
        ) -> Option<Vec<AdtCycleMember<'db>>> {
            if adt == root && !chain.is_empty() {
                return Some(chain.to_vec());
            } else if chain.iter().any(|member| member.adt == adt) || adt.fields(db).is_empty() {
                return None;
            }

            let mut chain = chain.to_vec();
            for (field_idx, field) in adt.fields(db).iter().enumerate() {
                for (ty_idx, ty) in field.iter_types(db).enumerate() {
                    for field_adt_ref in collect_direct_adts(db, ty.instantiate_identity()) {
                        chain.push(AdtCycleMember {
                            adt,
                            field_idx: field_idx as u16,
                            ty_idx: ty_idx as u16,
                        });

                        if let Some(cycle) =
                            impl_check(db, root, lower_adt(db, field_adt_ref), &chain)
                        {
                            return Some(cycle);
                        }
                        chain.pop();
                    }
                }
            }
            None
        }

        impl_check(db, self, self, &[])
    }
}

/// Collect all ADTs directly appearing inside the given type without
/// traversing through indirect wrappers like pointers or references.
fn collect_direct_adts<'db>(
    db: &'db dyn HirAnalysisDb,
    ty: TyId<'db>,
) -> rustc_hash::FxHashSet<AdtRef<'db>> {
    use crate::analysis::ty::ty_def::{PrimTy, TyBase};
    use crate::analysis::ty::visitor::TyVisitable;
    use rustc_hash::FxHashSet;

    struct AdtCollector<'db> {
        db: &'db dyn HirAnalysisDb,
        adts: FxHashSet<AdtRef<'db>>,
    }
    impl<'db> TyVisitor<'db> for AdtCollector<'db> {
        fn db(&self) -> &'db dyn HirAnalysisDb {
            self.db
        }
        fn visit_app(&mut self, abs: TyId<'db>, arg: TyId<'db>) {
            let is_indirect = match abs.data(self.db) {
                TyData::TyBase(TyBase::Prim(PrimTy::Ptr)) => true,
                // Future: handle Ref when introduced.
                _ => false,
            };
            if !is_indirect {
                walk_ty(self, abs);
                walk_ty(self, arg)
            }
        }
        fn visit_adt(&mut self, adt: AdtDef<'db>) {
            self.adts.insert(adt.adt_ref(self.db));
        }
    }

    let mut collector = AdtCollector {
        db,
        adts: FxHashSet::default(),
    };
    ty.visit_with(&mut collector);
    collector.adts
}

#[derive(Clone, Copy, Debug)]
pub struct SuperTraitRefView<'db> {
    pub owner: Trait<'db>,
    pub idx: usize,
}

impl<'db> SuperTraitRefView<'db> {
    pub fn span(self) -> crate::span::params::LazyTraitRefSpan<'db> {
        self.owner.span().super_traits().super_trait(self.idx)
    }

    pub(crate) fn trait_ref(self, db: &'db dyn HirDb) -> TraitRefId<'db> {
        self.owner.super_traits_refs(db)[self.idx]
    }

    pub fn subject_self(self, db: &'db dyn HirAnalysisDb) -> TyId<'db> {
        collect_generic_params(db, self.owner.into())
            .trait_self(db)
            .unwrap()
    }

    pub fn assumptions(self, db: &'db dyn HirAnalysisDb) -> PredicateListId<'db> {
        header_constraints_for(db, self.owner.into())
    }

    /// Lower this super-trait reference to a semantic trait instance using the trait's
    /// `Self` and constraints.
    /// Semantic trait instance for this super-trait reference lowered in the owner's
    /// context. Returns an error value; does not emit diagnostics.
    pub fn trait_inst(
        self,
        db: &'db dyn HirAnalysisDb,
    ) -> Result<TraitInstId<'db>, SuperTraitLowerError> {
        use crate::analysis::ty::trait_lower::TraitRefLowerError;
        let subject = self.subject_self(db);
        let tr = self.trait_ref(db);
        let scope = self.owner.scope();
        let assumptions = self.assumptions(db);
        match crate::analysis::ty::trait_lower::lower_trait_ref(
            db,
            subject,
            tr,
            scope,
            assumptions,
            None,
        ) {
            Ok(v) => Ok(v),
            Err(TraitRefLowerError::PathResError(_)) => Err(SuperTraitLowerError::PathResolution),
            Err(TraitRefLowerError::InvalidDomain(_)) => Err(SuperTraitLowerError::InvalidDomain),
            Err(TraitRefLowerError::Cycle) => Err(SuperTraitLowerError::Cycle),
            Err(TraitRefLowerError::UnsafeLocalBoundBlanketImpl | TraitRefLowerError::Ignored) => {
                Err(SuperTraitLowerError::Ignored)
            }
        }
    }

    /// Returns a tuple of (expected_kind, actual_self) when the owner's `Self` kind
    /// does not match the super-trait's expected implementor kind. Returns None when
    /// kinds are compatible or `Self` is invalid.
    pub fn kind_mismatch_for_self(self, db: &'db dyn HirAnalysisDb) -> Option<(Kind, TyId<'db>)> {
        use crate::analysis::ty::trait_lower::{TraitRefLowerError, lower_trait_ref};
        let subject = self.subject_self(db);
        let scope = self.owner.scope();
        let assumptions = self.assumptions(db);
        let tr = self.trait_ref(db);
        let expected = match lower_trait_ref(db, subject, tr, scope, assumptions, None) {
            Ok(inst) => inst.def(db).self_param(db).kind(db).clone(),
            // If we cannot lower, defer to other diagnostics; do not emit mismatch here.
            Err(
                TraitRefLowerError::PathResError(_)
                | TraitRefLowerError::InvalidDomain(_)
                | TraitRefLowerError::Cycle
                | TraitRefLowerError::UnsafeLocalBoundBlanketImpl
                | TraitRefLowerError::Ignored,
            ) => return None,
        };
        let actual = subject;
        (!expected.does_match(actual.kind(db))).then_some((expected, actual))
    }

    // Note: callers that want an Option can map the error to None explicitly.
}

/// Semantic error for lowering a super-trait reference in its owner's context.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SuperTraitLowerError {
    PathResolution,
    InvalidDomain,
    Cycle,
    Ignored,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InherentImplAdmissibility<'db> {
    Admissible {
        ty: TyId<'db>,
    },
    NotAllowed {
        ty: TyId<'db>,
        is_nominal: bool,
    },
    InvalidTy {
        ty: TyId<'db>,
    },
    IllFormed {
        ty: TyId<'db>,
        error: WellFormedness<'db>,
    },
}

impl<'db> Impl<'db> {
    /// Semantic implementor type of this inherent impl.
    pub fn ty(self, db: &'db dyn HirAnalysisDb) -> TyId<'db> {
        let assumptions = constraints_for(db, self.into());
        self.type_ref(db).to_opt().map_or_else(
            || TyId::invalid(db, InvalidCause::ParseError),
            |hir_ty| lower_hir_ty(db, hir_ty, self.scope(), assumptions),
        )
    }

    /// Type lowering errors for the implementor type.
    pub fn ty_errors(self, db: &'db dyn HirAnalysisDb) -> Vec<TyDiagCollection<'db>> {
        let Some(hir_ty) = self.type_ref(db).to_opt() else {
            return Vec::new();
        };
        collect_ty_lower_errors(
            db,
            hir_ty,
            self.span().target_ty(),
            context_at(db, self.scope(), None).premises(db),
        )
    }

    /// Iterate associated const definitions in this inherent impl block as views.
    pub fn assoc_consts(
        self,
        db: &'db dyn HirDb,
    ) -> impl Iterator<Item = InherentAssocConstView<'db>> + 'db {
        let len = self.hir_consts(db).len();
        (0..len).map(move |idx| InherentAssocConstView { owner: self, idx })
    }

    /// Get an associated const view by name, if it exists in this impl block.
    pub fn const_(
        self,
        db: &'db dyn HirDb,
        name: IdentId<'db>,
    ) -> Option<InherentAssocConstView<'db>> {
        self.assoc_consts(db).find(|c| c.name(db) == Some(name))
    }

    /// Get the index of an associated const by name, if it exists. Mirrors
    /// `Trait::const_index` so `ScopeId::ImplConst` indices are computed in one
    /// place instead of open-coded `hir_consts().position()` scans.
    pub fn const_index(self, db: &'db dyn HirDb, name: IdentId<'db>) -> Option<usize> {
        self.assoc_consts(db)
            .enumerate()
            .find_map(|(idx, c)| (c.name(db) == Some(name)).then_some(idx))
    }

    /// Get an associated const view by its index. Mirrors `Trait::const_by_index`
    /// so `ScopeId::ImplConst` resolution routes through one helper instead of an
    /// open-coded `hir_consts()[idx]` slice access.
    pub fn const_by_index(self, idx: usize) -> InherentAssocConstView<'db> {
        InherentAssocConstView { owner: self, idx }
    }

    pub(crate) fn inherent_impl_admissibility(
        self,
        db: &'db dyn HirAnalysisDb,
    ) -> InherentImplAdmissibility<'db> {
        let ty = self.ty(db);
        if ty.has_invalid(db) {
            return InherentImplAdmissibility::InvalidTy { ty };
        }
        let ingot = self.top_mod(db).ingot(db);
        if !ty.is_inherent_impl_allowed(db, ingot) {
            let base = ty.base_ty(db);
            return InherentImplAdmissibility::NotAllowed {
                ty,
                is_nominal: !base.is_param(db),
            };
        }
        match check_ty_wf(db, item_solver_context(db, self.scope()), ty) {
            WellFormedness::WellFormed => InherentImplAdmissibility::Admissible { ty },
            error => InherentImplAdmissibility::IllFormed { ty, error },
        }
    }

    pub(crate) fn admissible_inherent_impl_ty(
        self,
        db: &'db dyn HirAnalysisDb,
    ) -> Option<TyId<'db>> {
        match self.inherent_impl_admissibility(db) {
            InherentImplAdmissibility::Admissible { ty } => Some(ty),
            InherentImplAdmissibility::NotAllowed { .. }
            | InherentImplAdmissibility::InvalidTy { .. }
            | InherentImplAdmissibility::IllFormed { .. } => None,
        }
    }

    /// Returns the pretty-printed target type name from this impl block.
    /// This is useful for documentation generation.
    /// Returns None if the type reference is missing or malformed.
    pub fn target_type_name(self, db: &dyn HirDb) -> Option<String> {
        self.type_ref(db).to_opt().map(|ty| ty.pretty_print(db))
    }
}

/// Qualified path for a scope, resolving inherent-impl consts to their target
/// type's actual path. For `ScopeId::ImplConst` this yields e.g. `a::Foo::X`
/// even when the impl is written in module `b` via `use a::Foo` — something
/// `ScopeId::pretty_path` (which only has `HirDb`) cannot do, because the
/// target type needs name resolution. Tooling (goto, LSIF monikers, SCIP) must
/// use this instead of `scope.pretty_path` for inherent-const scopes.
pub fn scope_qualified_path<'db>(
    db: &'db dyn HirAnalysisDb,
    scope: ScopeId<'db>,
) -> Option<String> {
    if let ScopeId::ImplConst(impl_, _) = scope {
        let self_ty = impl_.ty(db);
        let name = scope.name(db)?;
        let (_, args) = self_ty.decompose_ty_app(db);
        // Build the target type's path. Named types keep their module-qualified
        // path; specialized impls (`impl Foo<1>`) include their type arguments
        // so distinct definitions don't collapse to the same path; primitives
        // (`impl u256`) have no scope, so fall back to the type print.
        let type_path = match self_ty.as_scope(db).and_then(|s| s.pretty_path(db)) {
            Some(base) if args.is_empty() => base,
            Some(base) => {
                let arg_list = args
                    .iter()
                    .map(|arg| arg.pretty_print(db).to_string())
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("{base}<{arg_list}>")
            }
            None => self_ty.pretty_print(db).to_string(),
        };
        return Some(format!("{type_path}::{}", name.data(db)));
    }
    scope.pretty_path(db)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ImplTraitLowerError<'db> {
    ParseError,
    TraitRef(TraitRefLowerError<'db>),
    Conflict {
        primary: ImplTrait<'db>,
        conflict: ImplTrait<'db>,
    },
    KindMismatch {
        expected: Kind,
        actual: TyId<'db>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TraitImplAdmissibility {
    Admissible,
    ExternalTraitForExternalType,
    UnsafeLocalBoundBlanketImpl,
}

impl<'db> ImplTrait<'db> {
    /// Semantic self type of this impl-trait block.
    pub fn ty(self, db: &'db dyn HirAnalysisDb) -> TyId<'db> {
        let assumptions = constraints_for(db, self.into());
        self.type_ref(db).to_opt().map_or_else(
            || TyId::invalid(db, InvalidCause::ParseError),
            |hir_ty| lower_hir_ty(db, hir_ty, self.scope(), assumptions),
        )
    }

    /// Type lowering errors for the implementor type.
    pub fn ty_errors(self, db: &'db dyn HirAnalysisDb) -> Vec<TyDiagCollection<'db>> {
        let Some(hir_ty) = self.type_ref(db).to_opt() else {
            return Vec::new();
        };
        collect_ty_lower_errors(
            db,
            hir_ty,
            self.span().ty(),
            context_at(db, self.scope(), None).premises(db),
        )
    }

    /// Lowers this impl-trait to a semantic implementor view, performing
    /// conflict detection and kind checks.
    pub(crate) fn lowered_implementor(
        self,
        db: &'db dyn HirAnalysisDb,
    ) -> Result<ImplementorId<'db>, ImplTraitLowerError<'db>> {
        // Early return if the implementor type is syntactically missing or invalid.
        if matches!(
            self.ty(db).data(db),
            TyData::Invalid(InvalidCause::ParseError)
        ) {
            return Err(ImplTraitLowerError::ParseError);
        }
        // Note: we do NOT check ty.has_invalid(db) here universally, because
        // we want to proceed with lowering (and potentially report unrelated
        // errors) unless it's a hard parse error. Diagnostics code will check
        // validity separately.

        // Lower trait inst
        let trait_inst = match self.trait_inst_result(db) {
            Ok(inst) => inst,
            Err(err) => return Err(ImplTraitLowerError::TraitRef(err)),
        };

        // Build implementor view
        let params = self.impl_params(db);
        let types = self.assoc_type_bindings_for_trait_inst(db, trait_inst);
        let implementor =
            ImplementorId::new(db, trait_inst, params, types, ImplementorOrigin::Hir(self));

        // Conflict check
        let trait_ = implementor.trait_(db);
        for &cand_view in impls_for_trait_def(db, self.top_mod(db).ingot(db), trait_.def(db)) {
            let cand_impl_trait = cand_view.hir_impl_trait(db);
            if cand_impl_trait == self {
                continue;
            }
            if does_impl_trait_conflict(db, cand_view, implementor)
                && !Self::impl_trait_conflict_is_sealed_marker_disjoint(db, cand_impl_trait, self)
            {
                return Err(ImplTraitLowerError::Conflict {
                    primary: cand_impl_trait,
                    conflict: self,
                });
            }
        }

        // Kind check
        let expected_kind = implementor.trait_def(db).self_param(db).kind(db);

        let self_ty = self.ty(db);
        if self_ty.kind(db) != expected_kind {
            return Err(ImplTraitLowerError::KindMismatch {
                expected: expected_kind.clone(),
                actual: implementor.self_ty(db),
            });
        }

        Ok(implementor)
    }

    /// Internal helper that lowers the trait reference of this `impl trait`
    /// block to a semantic trait instance, preserving detailed error
    /// information.
    ///
    /// This is the canonical entry point for trait‑ref lowering from
    /// `impl trait` items. All callers that care about diagnostics should
    /// prefer this over re‑invoking `lower_trait_ref` directly.
    pub(crate) fn trait_inst_result(
        self,
        db: &'db dyn HirAnalysisDb,
    ) -> Result<TraitInstId<'db>, TraitRefLowerError<'db>> {
        self.trait_inst_result_in_mode(db, false)
    }

    /// Candidate signatures retain anonymous const bodies without checking
    /// them. This keeps trait-environment lookup below body type checking;
    /// normal item diagnostics still use [`Self::trait_inst_result`].
    pub(crate) fn candidate_trait_inst_result(
        self,
        db: &'db dyn HirAnalysisDb,
    ) -> Result<TraitInstId<'db>, TraitRefLowerError<'db>> {
        self.trait_inst_result_in_mode(db, true)
    }

    fn trait_inst_result_in_mode(
        self,
        db: &'db dyn HirAnalysisDb,
        defer_const_bodies: bool,
    ) -> Result<TraitInstId<'db>, TraitRefLowerError<'db>> {
        let assumptions = if defer_const_bodies {
            collect_candidate_constraints(db, self.into()).instantiate_identity()
        } else {
            constraints_for(db, self.into())
        };
        let ty = if defer_const_bodies {
            let hir_ty = self
                .type_ref(db)
                .to_opt()
                .ok_or(TraitRefLowerError::Ignored)?;
            lower_hir_ty_deferred(db, hir_ty, self.scope(), assumptions)
        } else {
            self.ty(db)
        };

        // Preserve the existing "parse error / invalid type -> early return
        // with no diags from this helper" behavior.
        if matches!(ty.data(db), TyData::Invalid(InvalidCause::ParseError)) {
            return Err(TraitRefLowerError::Ignored);
        }
        if ty.has_invalid(db) {
            return Err(TraitRefLowerError::Ignored);
        }

        // No trait-ref in source: nothing to report here.
        let Some(trait_ref) = self.trait_ref(db).to_opt() else {
            return Err(TraitRefLowerError::Ignored);
        };

        let trait_inst = if defer_const_bodies {
            lower_trait_ref_deferred(db, ty, trait_ref, self.scope(), assumptions, None)?
        } else {
            lower_trait_ref(db, ty, trait_ref, self.scope(), assumptions, None)?
        };

        match self.trait_impl_admissibility(db, ty, trait_inst, assumptions) {
            TraitImplAdmissibility::Admissible => {}
            TraitImplAdmissibility::ExternalTraitForExternalType => {
                return Err(TraitRefLowerError::Ignored);
            }
            TraitImplAdmissibility::UnsafeLocalBoundBlanketImpl => {
                return Err(TraitRefLowerError::UnsafeLocalBoundBlanketImpl);
            }
        }

        Ok(trait_inst)
    }

    fn trait_impl_admissibility(
        self,
        db: &'db dyn HirAnalysisDb,
        ty: TyId<'db>,
        trait_inst: TraitInstId<'db>,
        assumptions: PredicateListId<'db>,
    ) -> TraitImplAdmissibility {
        let impl_trait_ingot = self.top_mod(db).ingot(db);
        if Some(impl_trait_ingot) == ty.ingot(db)
            || impl_trait_ingot == trait_inst.def(db).ingot(db)
        {
            return TraitImplAdmissibility::Admissible;
        }

        if !matches!(ty.data(db), TyData::TyParam(_)) {
            return TraitImplAdmissibility::ExternalTraitForExternalType;
        }

        if assumptions.list(db).iter().any(|pred| {
            pred.self_ty(db) == ty
                && Self::is_sealed_local_marker_anchor(db, pred.def(db), impl_trait_ingot)
        }) {
            TraitImplAdmissibility::Admissible
        } else {
            TraitImplAdmissibility::UnsafeLocalBoundBlanketImpl
        }
    }

    fn is_sealed_local_marker_anchor(
        db: &'db dyn HirAnalysisDb,
        marker: Trait<'db>,
        impl_trait_ingot: common::ingot::Ingot<'db>,
    ) -> bool {
        if marker.ingot(db) != impl_trait_ingot
            || marker.scope().data(db).vis.is_pub()
            || !marker.original_params(db).is_empty()
            || marker.methods(db).next().is_some()
            || !marker.types(db).is_empty()
            || !marker.consts(db).is_empty()
            || marker.super_trait_bounds(db).next().is_some()
        {
            return false;
        }

        Self::marker_impls(db, marker)
            .all(|impl_| Self::impl_trait_targets_local_nominal_root(db, impl_, impl_trait_ingot))
    }

    /// Impls of `marker` in its own ingot. The raw trait index narrows the
    /// candidates to impls whose trait path resolves to `marker`.
    fn marker_impls(
        db: &'db dyn HirAnalysisDb,
        marker: Trait<'db>,
    ) -> impl Iterator<Item = ImplTrait<'db>> {
        collect_trait_impls(db, marker.ingot(db))
            .by_trait
            .get(&marker)
            .into_iter()
            .flatten()
            .copied()
            .filter(move |&impl_| Self::impl_trait_implements_marker(db, impl_, marker))
    }

    fn impl_trait_implements_marker(
        db: &'db dyn HirAnalysisDb,
        impl_trait: ImplTrait<'db>,
        marker: Trait<'db>,
    ) -> bool {
        let Some(trait_ref) = impl_trait.trait_ref(db).to_opt() else {
            return false;
        };
        let assumptions = constraints_for(db, impl_trait.into());
        lower_trait_ref(
            db,
            impl_trait.ty(db),
            trait_ref,
            impl_trait.scope(),
            assumptions,
            None,
        )
        .is_ok_and(|inst| inst.def(db) == marker)
    }

    fn impl_trait_conflict_is_sealed_marker_disjoint(
        db: &'db dyn HirAnalysisDb,
        a: ImplTrait<'db>,
        b: ImplTrait<'db>,
    ) -> bool {
        let Some(a_roots) = Self::sealed_marker_possible_target_roots(db, a) else {
            return false;
        };
        let Some(b_roots) = Self::sealed_marker_possible_target_roots(db, b) else {
            return false;
        };

        a_roots.is_disjoint(&b_roots)
    }

    fn sealed_marker_possible_target_roots(
        db: &'db dyn HirAnalysisDb,
        impl_trait: ImplTrait<'db>,
    ) -> Option<FxHashSet<TyBase<'db>>> {
        let markers = Self::sealed_local_marker_anchors(db, impl_trait);
        let (&first, rest) = markers.split_first()?;
        let mut roots = Self::sealed_marker_target_roots(db, first);

        for &marker in rest {
            let marker_roots = Self::sealed_marker_target_roots(db, marker);
            roots.retain(|root| marker_roots.contains(root));
        }

        Some(roots)
    }

    fn sealed_local_marker_anchors(
        db: &'db dyn HirAnalysisDb,
        impl_trait: ImplTrait<'db>,
    ) -> Vec<Trait<'db>> {
        let impl_trait_ingot = impl_trait.top_mod(db).ingot(db);
        let ty = impl_trait.ty(db);
        constraints_for(db, impl_trait.into())
            .list(db)
            .iter()
            .filter_map(|pred| {
                let marker = pred.def(db);
                (pred.self_ty(db) == ty
                    && Self::is_sealed_local_marker_anchor(db, marker, impl_trait_ingot))
                .then_some(marker)
            })
            .collect()
    }

    fn sealed_marker_target_roots(
        db: &'db dyn HirAnalysisDb,
        marker: Trait<'db>,
    ) -> FxHashSet<TyBase<'db>> {
        Self::marker_impls(db, marker)
            .filter_map(|impl_| Self::impl_trait_local_nominal_root(db, impl_))
            .collect()
    }

    fn impl_trait_targets_local_nominal_root(
        db: &'db dyn HirAnalysisDb,
        impl_trait: ImplTrait<'db>,
        impl_trait_ingot: common::ingot::Ingot<'db>,
    ) -> bool {
        Self::impl_trait_local_nominal_root(db, impl_trait).is_some_and(|root| match root {
            TyBase::Adt(adt) => adt.ingot(db) == impl_trait_ingot,
            TyBase::Contract(contract) => contract.top_mod(db).ingot(db) == impl_trait_ingot,
            TyBase::Prim(_) | TyBase::Func(_) | TyBase::TypeFn(_) => false,
        })
    }

    fn impl_trait_local_nominal_root(
        db: &'db dyn HirAnalysisDb,
        impl_trait: ImplTrait<'db>,
    ) -> Option<TyBase<'db>> {
        match impl_trait.ty(db).base_ty(db).data(db) {
            TyData::TyBase(root @ (TyBase::Adt(_) | TyBase::Contract(_))) => Some(*root),
            _ => None,
        }
    }

    /// Semantic generic parameter types for this `impl trait` block, in
    /// definition order (including `Self` when present).
    ///
    /// This is a thin wrapper over the generic-param collector used elsewhere
    /// and keeps the param‑set semantics rooted on the item.
    pub(crate) fn impl_params(self, db: &'db dyn HirAnalysisDb) -> Vec<TyId<'db>> {
        collect_generic_params(db, self.into()).params(db).to_vec()
    }

    /// Semantic associated-type bindings for this `impl trait` block, given the
    /// lowered trait instance.
    ///
    /// This mirrors the logic used in `lower_impl_trait`:
    /// - start from the explicitly provided associated types in the impl block;
    /// - then merge in defaults from the trait definition, instantiated with the
    ///   concrete generic arguments of `trait_inst` (including `Self`).
    ///
    /// Kept crate‑internal so that engine code (trait env, implementor IR) can
    /// reuse the same semantics without re‑implementing the merge.
    pub(crate) fn assoc_type_bindings_for_trait_inst(
        self,
        db: &'db dyn HirAnalysisDb,
        trait_inst: TraitInstId<'db>,
    ) -> IndexMap<IdentId<'db>, TyId<'db>> {
        self.assoc_type_bindings_for_trait_inst_in_mode(db, trait_inst, false)
    }

    pub(crate) fn candidate_assoc_type_bindings_for_trait_inst(
        self,
        db: &'db dyn HirAnalysisDb,
        trait_inst: TraitInstId<'db>,
    ) -> IndexMap<IdentId<'db>, TyId<'db>> {
        self.assoc_type_bindings_for_trait_inst_in_mode(db, trait_inst, true)
    }

    fn assoc_type_bindings_for_trait_inst_in_mode(
        self,
        db: &'db dyn HirAnalysisDb,
        trait_inst: TraitInstId<'db>,
        defer_const_bodies: bool,
    ) -> IndexMap<IdentId<'db>, TyId<'db>> {
        // Semantic associated type implementations in this impl-trait block.
        let mut types: IndexMap<_, _> = self
            .assoc_types(db)
            .filter_map(|view| {
                let ty = if defer_const_bodies {
                    view.candidate_ty(db)
                } else {
                    view.ty(db)
                };
                view.name(db).zip(ty.map(|ty| view.bind_body(db, ty)))
            })
            .collect();

        // Merge trait associated type defaults into the implementor, but evaluated in
        // the trait's own scope and then instantiated with this impl's concrete args
        // (including Self). This ensures defaults like `type Output = Self` resolve
        // to the implementor's concrete self type rather than remaining as `Self`.
        let trait_def = trait_inst.def(db);
        for view in trait_def.assoc_types(db) {
            let default = if defer_const_bodies {
                view.candidate_default_ty(db)
            } else {
                view.default_ty(db)
            };
            let (Some(name), Some(default)) = (view.name(db), default) else {
                continue;
            };

            types.entry(name).or_insert_with(|| {
                Binder::bind(trait_inst.def(db).into(), view.bind_body(db, default))
                    .instantiate(db, trait_inst.args(db))
            });
        }

        types
    }

    /// Semantic trait instance implemented by this `impl trait` block, if well-formed.
    ///
    /// This delegates to [`ImplTrait::trait_inst_result`], which preserves
    /// detailed error information for diagnostics while providing a
    /// traversal‑friendly entry point rooted on the HIR item.
    pub fn trait_inst(self, db: &'db dyn HirAnalysisDb) -> Option<TraitInstId<'db>> {
        self.trait_inst_result(db).ok()
    }

    /// Trait definition implemented by this `impl trait` block, if well-formed.
    pub fn trait_def(self, db: &'db dyn HirAnalysisDb) -> Option<Trait<'db>> {
        self.trait_inst(db).map(|inst| inst.def(db))
    }

    /// Returns the ADT (struct or enum) that this `impl Trait` block implements for,
    /// if the implementor type resolves to a concrete ADT.
    pub fn implementing_adt(self, db: &'db dyn HirAnalysisDb) -> Option<AdtRef<'db>> {
        self.ty(db).adt_ref(db)
    }

    /// Iterate associated type definitions in this impl-trait block as views.
    pub fn assoc_types(
        self,
        db: &'db dyn HirDb,
    ) -> impl Iterator<Item = ImplAssocTypeView<'db>> + 'db {
        let len = self.types(db).len();
        (0..len).map(move |idx| ImplAssocTypeView { owner: self, idx })
    }

    /// Iterate associated const definitions in this impl-trait block as views.
    pub fn assoc_consts(
        self,
        db: &'db dyn HirDb,
    ) -> impl Iterator<Item = ImplAssocConstView<'db>> + 'db {
        let len = self.consts(db).len();
        (0..len).map(move |idx| ImplAssocConstView { owner: self, idx })
    }

    /// Get an associated const view by name, if it exists in this impl-trait block.
    pub fn const_(self, db: &'db dyn HirDb, name: IdentId<'db>) -> Option<ImplAssocConstView<'db>> {
        self.assoc_consts(db).find(|c| c.name(db) == Some(name))
    }

    /// Diagnostics for implementor lowering failures.
    /// Returns `(Some(implementor), [])` on success, or `(None, diags)` on failure.
    pub(crate) fn implementor_with_errors(
        self,
        db: &'db dyn HirAnalysisDb,
    ) -> (Option<ImplementorId<'db>>, Vec<TyDiagCollection<'db>>) {
        use crate::analysis::name_resolution::{ExpectedPathKind, diagnostics::PathResDiag};
        use crate::analysis::ty::diagnostics::TraitLowerDiag;

        // First check implementor type
        let ty = self.ty(db);
        if ty.has_invalid(db) {
            let diags = self.ty_errors(db);
            if !diags.is_empty() {
                return (None, diags);
            }
            if let Some(diag) = ty.emit_diag(db, self.span().ty().into()) {
                return (None, vec![diag]);
            }
            return (None, Vec::new());
        }

        if let Some(diag) = check_ty_wf(db, item_solver_context(db, self.scope()), ty)
            .into_diag(self.span().ty().into())
        {
            return (None, vec![diag]);
        }

        match self.lowered_implementor(db) {
            Ok(implementor) => (Some(implementor), Vec::new()),
            Err(err) => {
                let mut diags = Vec::new();
                match err {
                    ImplTraitLowerError::ParseError => {}
                    ImplTraitLowerError::TraitRef(lower_err) => match lower_err {
                        TraitRefLowerError::PathResError(err) => {
                            if let Some(trait_ref) = self.trait_ref(db).to_opt() {
                                let path = trait_ref.path(db).unwrap();
                                if let Some(diag) = err.into_diag(
                                    db,
                                    path,
                                    self.span().trait_ref().path(),
                                    ExpectedPathKind::Trait,
                                ) {
                                    diags.push(diag.into());
                                }
                            }
                        }
                        TraitRefLowerError::InvalidDomain(res) => {
                            if let Some(trait_ref) = self.trait_ref(db).to_opt() {
                                diags.push(
                                    PathResDiag::ExpectedTrait(
                                        self.span().trait_ref().path().into(),
                                        trait_ref.path(db).unwrap().ident(db).unwrap(),
                                        res.kind_name(),
                                    )
                                    .into(),
                                );
                            }
                        }
                        TraitRefLowerError::Cycle => {
                            diags.push(TraitLowerDiag::CyclicTraitRef(self).into());
                        }
                        TraitRefLowerError::UnsafeLocalBoundBlanketImpl => {
                            diags.push(TraitLowerDiag::UnsafeLocalBoundBlanketImpl(self).into());
                        }
                        TraitRefLowerError::Ignored => {
                            diags.push(TraitLowerDiag::ExternalTraitForExternalType(self).into());
                        }
                    },
                    ImplTraitLowerError::Conflict { primary, conflict } => {
                        let local_impls = self.top_mod(db).ingot(db).all_impl_traits(db);
                        let primary_idx = local_impls.iter().position(|&item| item == primary);
                        let conflict_idx = local_impls.iter().position(|&item| item == conflict);
                        if !matches!(
                            (primary_idx, conflict_idx),
                            (Some(primary_idx), Some(conflict_idx)) if primary_idx > conflict_idx
                        ) {
                            diags.push(
                                TraitLowerDiag::ConflictTraitImpl {
                                    primary,
                                    conflict_with: conflict,
                                }
                                .into(),
                            );
                        }
                    }
                    ImplTraitLowerError::KindMismatch { expected, actual } => {
                        diags.push(
                            TraitConstraintDiag::TraitArgKindMismatch {
                                span: self.span().trait_ref(),
                                expected,
                                actual,
                            }
                            .into(),
                        );
                    }
                }
                (None, diags)
            }
        }
    }

    /// Returns the pretty-printed trait name from this impl trait block.
    /// This is useful for documentation generation.
    /// Returns None if the trait reference is missing or malformed.
    pub fn trait_name(self, db: &dyn HirDb) -> Option<String> {
        self.trait_ref(db).to_opt().map(|tr| tr.pretty_print(db))
    }

    /// Returns the pretty-printed target type name from this impl trait block.
    /// This is useful for documentation generation.
    /// Returns None if the type reference is missing or malformed.
    pub fn target_type_name(self, db: &dyn HirDb) -> Option<String> {
        self.type_ref(db).to_opt().map(|ty| ty.pretty_print(db))
    }
}

#[derive(Clone, Copy, Debug)]
pub struct ImplAssocTypeView<'db> {
    owner: ImplTrait<'db>,
    idx: usize,
}

impl<'db> ImplAssocTypeView<'db> {
    fn with_parameter_bounds(
        self,
        db: &'db dyn HirAnalysisDb,
        assumptions: PredicateListId<'db>,
    ) -> PredicateListId<'db> {
        if self.generic_params(db).data(db).is_empty() {
            return assumptions;
        }
        let Some(inst) = self.owner.trait_inst(db) else {
            return assumptions;
        };
        let Some(decl) = inst
            .def(db)
            .assoc_types(db)
            .find(|decl| decl.name(db) == self.name(db))
        else {
            return assumptions;
        };
        let owner = self.scope().assoc_type_owner().unwrap();
        let args: Vec<_> = (0..self.generic_params(db).data(db).len())
            .map(|idx| crate::analysis::ty::ty_lower::assoc_type_param(db, owner, idx))
            .collect();
        let Some(guards) = decl.parameter_bounds(db, inst, &args) else {
            return assumptions;
        };
        let mut predicates = assumptions.list(db).to_vec();
        predicates.extend_from_slice(guards.list(db));
        PredicateListId::new(db, predicates).extend_all_bounds(db)
    }

    pub fn scope(self) -> ScopeId<'db> {
        ScopeId::ImplTraitType(self.owner, self.idx as u16)
    }

    fn has_const_param(self, db: &'db dyn HirDb) -> bool {
        crate::hir_def::scope_graph::AssocTypeOwner::Impl(self.owner, self.idx as u16)
            .has_const_param(db)
    }

    pub(crate) fn bind_body(self, db: &'db dyn HirAnalysisDb, body: TyId<'db>) -> TyId<'db> {
        TyId::type_family(
            db,
            crate::hir_def::scope_graph::AssocTypeOwner::Impl(self.owner, self.idx as u16),
            body,
        )
    }

    pub fn generic_params(self, db: &'db dyn HirDb) -> GenericParamListId<'db> {
        self.owner.types(db)[self.idx].generic_params
    }

    fn hole_anchor(self) -> HoleAnchor<'db> {
        HoleAnchor::ImplAssocType {
            impl_trait: self.owner,
            index: self.idx as u32,
        }
    }

    pub fn name(self, db: &'db dyn HirDb) -> Option<IdentId<'db>> {
        self.owner.types(db)[self.idx].name.to_opt()
    }

    pub fn span(self) -> crate::span::item::LazyTraitTypeSpan<'db> {
        self.owner.span().associated_type(self.idx)
    }

    /// The owning impl-trait block.
    pub fn owner(self) -> ImplTrait<'db> {
        self.owner
    }

    /// Semantic type of this associated type implementation.
    pub fn ty(self, db: &'db dyn HirAnalysisDb) -> Option<TyId<'db>> {
        let hir = self.owner.types(db)[self.idx].type_ref.to_opt()?;
        if self.has_const_param(db) {
            return Some(TyId::invalid(db, InvalidCause::Other));
        }
        let assumptions = self.with_parameter_bounds(db, constraints_for(db, self.owner.into()));
        let minter = LoweringContext::new(self.hole_anchor());
        Some(lower_hir_ty_with_minter(
            db,
            hir,
            SolverPremises::from_unpositioned_pair(self.scope(), assumptions),
            &minter,
        ))
    }

    pub(crate) fn candidate_ty(self, db: &'db dyn HirAnalysisDb) -> Option<TyId<'db>> {
        let hir = self.owner.types(db)[self.idx].type_ref.to_opt()?;
        if self.has_const_param(db) {
            return Some(TyId::invalid(db, InvalidCause::Other));
        }
        let assumptions = self.with_parameter_bounds(
            db,
            collect_candidate_constraints(db, self.owner.into()).instantiate_identity(),
        );
        let minter = LoweringContext::deferred(self.hole_anchor());
        Some(lower_hir_ty_with_minter(
            db,
            hir,
            SolverPremises::from_unpositioned_pair(self.scope(), assumptions),
            &minter,
        ))
    }

    pub(crate) fn layout_root_uses(self, db: &'db dyn HirAnalysisDb) -> Vec<LayoutRootUse<'db>> {
        let Some(hir) = self.owner.types(db)[self.idx].type_ref.to_opt() else {
            return Vec::new();
        };
        if self.has_const_param(db) {
            return Vec::new();
        }
        let scope = self.scope();
        let minter = LoweringContext::new(self.hole_anchor());
        lower_layout_root_uses_in_hir_ty(
            db,
            hir,
            scope,
            self.with_parameter_bounds(db, constraints_for(db, self.owner.into())),
            &minter,
        )
    }

    /// All type-related diagnostics for this associated type.
    pub fn ty_diags(self, db: &'db dyn HirAnalysisDb) -> Vec<TyDiagCollection<'db>> {
        let Some(hir) = self.owner.types(db)[self.idx].type_ref.to_opt() else {
            return Vec::new();
        };
        if self.has_const_param(db) {
            return Vec::new();
        }

        let ty_span = self.span().ty();
        let context = context_at(db, self.scope(), None);

        let errs = collect_ty_lower_errors(db, hir, ty_span.clone(), context.premises(db));
        if !errs.is_empty() {
            return errs;
        }

        let Some(ty) = self.ty(db) else {
            return Vec::new();
        };
        if let Some(diag) = ty.emit_diag(db, ty_span.clone().into()) {
            return vec![diag];
        }
        let normalized = normalize_ty(db, ty, context.premises(db));
        if let Some(diag) = normalized.emit_diag(db, ty_span.clone().into()) {
            return vec![diag];
        }
        if let Some(diag) =
            check_ty_wf(db, context.solver_context(db), ty).into_diag(ty_span.into())
        {
            return vec![diag];
        }

        Vec::new()
    }
}

// Const / Use ---------------------------------------------------------------

impl<'db> Const<'db> {
    pub fn hir_ty(self, db: &'db dyn HirDb) -> Option<TypeId<'db>> {
        self.type_ref(db).to_opt()
    }

    // Planned semantic surface:
    // - additional const semantics/diags as needed

    /// Semantic type of this const definition.
    pub fn ty(self, db: &'db dyn HirAnalysisDb) -> TyId<'db> {
        let Some(hir_ty) = self.type_ref(db).to_opt() else {
            return TyId::invalid(db, InvalidCause::ParseError);
        };
        lower_hir_ty(db, hir_ty, self.scope(), PredicateListId::empty_list(db))
    }
}

impl<'db> Use<'db> {
    // Planned semantic surface:
    // - resolved target summary / simple diags (optional)
}

// Shared capability hints ---------------------------------------------------

impl<'db> ItemKind<'db> {
    // Planned semantic surface:
    // - semantic helpers to opt into capabilities (HasGenericParams/HasWhereClause)
}

// Note:
// Avoid adding tracked methods here. Keep tracked queries in lowering and call
// them from small, semantic helpers only. This module should remain the
// ergonomic public surface for traversal without leaking syntax.

impl<'db> GenericParamOwner<'db> {
    pub fn param_view(self, db: &'db dyn HirDb, idx: usize) -> GenericParamView<'db> {
        self.params(db)
            .nth(idx)
            .expect("failed to get the generic param")
    }

    pub fn params(self, db: &'db dyn HirDb) -> impl Iterator<Item = GenericParamView<'db>> + 'db {
        self.params_list(db)
            .data(db)
            .iter()
            .enumerate()
            .map(move |(idx, param)| GenericParamView {
                owner: self,
                param,
                idx,
            })
    }
}

impl<'db> GenericParamView<'db> {
    /// Lazy span of this generic parameter in its owner's parameter list.
    ///
    /// Exposes a context-free handle that can be resolved to a concrete
    /// source span via `SpannedHirDb`, without requiring callers to manually
    /// cross-link list spans with indices.
    pub fn span(&self) -> crate::span::params::LazyGenericParamSpan<'db> {
        self.owner.params_span().param(self.idx)
    }

    /// Lazy span atom for the parameter's name token.
    ///
    /// Returns the span for the ident of a type or const generic param,
    /// depending on the underlying parameter kind.
    pub fn name_span(&self) -> crate::span::LazySpanAtom<'db> {
        match self.param {
            GenericParam::Type(_) => self.span().into_type_param().name(),
            GenericParam::Const(_) => self.span().into_const_param().name(),
        }
    }
}

// Associated type views -----------------------------------------------------

#[derive(Clone, Copy, Debug)]
pub struct TraitAssocTypeView<'db> {
    owner: Trait<'db>,
    idx: usize,
}

impl<'db> TraitAssocTypeView<'db> {
    pub fn scope(self) -> ScopeId<'db> {
        ScopeId::TraitType(self.owner, self.idx as u16)
    }

    pub(crate) fn has_const_param(self, db: &'db dyn HirDb) -> bool {
        crate::hir_def::scope_graph::AssocTypeOwner::Trait(self.owner, self.idx as u16)
            .has_const_param(db)
    }

    pub(crate) fn bind_body(self, db: &'db dyn HirAnalysisDb, body: TyId<'db>) -> TyId<'db> {
        TyId::type_family(
            db,
            crate::hir_def::scope_graph::AssocTypeOwner::Trait(self.owner, self.idx as u16),
            body,
        )
    }

    pub fn generic_params(self, db: &'db dyn HirDb) -> GenericParamListId<'db> {
        self.decl(db).generic_params
    }

    fn decl(self, db: &'db dyn HirDb) -> &'db crate::core::hir_def::AssocTyDecl<'db> {
        &self.owner.types(db)[self.idx]
    }

    pub fn name(self, db: &'db dyn HirDb) -> Option<IdentId<'db>> {
        self.decl(db).name.to_opt()
    }

    pub fn span(self) -> crate::span::item::LazyTraitTypeSpan<'db> {
        self.owner.span().item_list().assoc_type(self.idx)
    }

    /// Raw bounds for this associated type (HIR). Prefer semantic checks where possible.
    pub(in crate::core) fn bounds_raw(self, db: &'db dyn HirDb) -> &'db [TypeBound<'db>] {
        &self.decl(db).bounds
    }

    /// Semantic default type for this associated type, lowered in the trait's
    /// scope using the trait's own constraints. Returns None if no default.
    pub fn default_ty(
        self,
        db: &'db dyn HirAnalysisDb,
    ) -> Option<crate::analysis::ty::ty_def::TyId<'db>> {
        let hir = self.decl(db).default?;
        if self.has_const_param(db) {
            return Some(TyId::invalid(db, InvalidCause::Other));
        }
        let trait_ = self.owner;
        let assumptions = self.with_parameter_bounds(db, constraints_for(db, trait_.into()));
        Some(lower_hir_ty(db, hir, self.scope(), assumptions))
    }

    pub(crate) fn default_hir_ty(self, db: &'db dyn HirDb) -> Option<crate::hir_def::TypeId<'db>> {
        self.decl(db).default
    }

    pub(crate) fn candidate_default_ty(
        self,
        db: &'db dyn HirAnalysisDb,
    ) -> Option<crate::analysis::ty::ty_def::TyId<'db>> {
        let hir = self.decl(db).default?;
        if self.has_const_param(db) {
            return Some(TyId::invalid(db, InvalidCause::Other));
        }
        let trait_ = self.owner;
        let assumptions = self.with_parameter_bounds(
            db,
            collect_candidate_constraints(db, trait_.into()).instantiate_identity(),
        );
        Some(lower_hir_ty_deferred(db, hir, self.scope(), assumptions))
    }

    pub(crate) fn layout_root_uses(self, db: &'db dyn HirAnalysisDb) -> Vec<LayoutRootUse<'db>> {
        let Some(hir) = self.decl(db).default else {
            return Vec::new();
        };
        if self.has_const_param(db) {
            return Vec::new();
        }
        let scope = self.scope();
        let assumptions = self.with_parameter_bounds(db, constraints_for(db, self.owner.into()));
        let minter = LoweringContext::new(HoleAnchor::TemplateTy {
            ty: hir,
            scope,
            assumptions,
        });
        lower_layout_root_uses_in_hir_ty(db, hir, scope, assumptions, &minter)
    }

    /// Semantic trait bounds using the trait's own `Self` as the subject.
    pub fn bounds_on_self(
        self,
        db: &'db dyn HirAnalysisDb,
    ) -> impl Iterator<Item = TraitInstId<'db>> + 'db {
        let self_ty = self.owner.self_param(db);
        self.bounds_on_subject(db, self_ty)
    }

    /// Semantic trait bounds for an explicit subject, using the owner's `Self`
    /// in the trait arguments.
    pub fn bounds_on_subject(
        self,
        db: &'db dyn HirAnalysisDb,
        subject: TyId<'db>,
    ) -> impl Iterator<Item = TraitInstId<'db>> + 'db {
        let owner_self = self.owner.self_param(db);
        AssocTypeBounds {
            base: self,
            subject,
            owner_self,
        }
        .bounds(db)
    }

    /// Semantic trait bounds for an explicit subject with an explicit owner
    /// `Self` override (e.g., impl-trait analysis).
    pub fn bounds_on_subject_with_owner(
        self,
        db: &'db dyn HirAnalysisDb,
        subject: TyId<'db>,
        owner_self: TyId<'db>,
    ) -> impl Iterator<Item = TraitInstId<'db>> + 'db {
        AssocTypeBounds {
            base: self,
            subject,
            owner_self,
        }
        .bounds(db)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct AssocTypeBoundView<'db> {
    owner: TraitAssocTypeView<'db>,
    idx: usize,
}

impl<'db> TraitAssocTypeView<'db> {
    /// `<Self as Trait<..>>::Name<P..>` over the trait's and the family's own
    /// formal parameters.
    pub(crate) fn formal_subject(self, db: &'db dyn HirAnalysisDb) -> TyId<'db> {
        let trait_inst = trait_self_predicate(db, self.owner);
        let name = self.name(db).expect("a family has a name");
        TyId::foldl(
            db,
            TyId::assoc_ty(db, trait_inst.trait_ref(db), name),
            &self.family_parameters(db),
        )
    }

    pub(crate) fn family_parameters(self, db: &'db dyn HirAnalysisDb) -> Vec<TyId<'db>> {
        let owner = self.scope().assoc_type_owner().unwrap();
        (0..self.generic_params(db).data(db).len())
            .map(|idx| crate::analysis::ty::ty_lower::assoc_type_param(db, owner, idx))
            .collect()
    }

    /// Declaration-owned requirements, instantiated for a particular enclosing
    /// trait and local family arguments. Both callers and definitions use this.
    pub(crate) fn parameter_bounds(
        self,
        db: &'db dyn HirAnalysisDb,
        inst: TraitInstId<'db>,
        args: &[TyId<'db>],
    ) -> Option<PredicateListId<'db>> {
        let parameter_count = self.generic_params(db).data(db).len();
        if inst.def(db) != self.owner || args.len() < parameter_count {
            return None;
        }
        // Further arguments apply to the constructor returned by the family.
        let args = &args[..parameter_count];
        let params = self.family_parameters(db);
        let mut requirements = Vec::new();
        for (subject, param) in params.into_iter().zip(self.generic_params(db).data(db)) {
            let GenericParam::Type(param) = param else {
                return None;
            };
            for bound in &param.bounds {
                if let TypeBound::Trait(trait_ref) = bound
                    && !crate::analysis::ty::ty_lower::is_core_constraint_bound(
                        db,
                        self.scope(),
                        *trait_ref,
                    )
                {
                    let guard = lower_trait_ref(
                        db,
                        subject,
                        *trait_ref,
                        self.scope(),
                        constraints_for(db, self.owner.into()),
                        Some(self.owner.self_param(db)),
                    )
                    .ok()?;
                    let mapping =
                        crate::analysis::ty::ty_lower::CompleteSubst::for_associated_type(
                            db,
                            self.scope().assoc_type_owner()?,
                            args.to_vec(),
                        )
                        .ok()?;
                    let guard =
                        crate::analysis::ty::subst::substitute_complete(db, guard, &mapping)
                            .ok()?;
                    requirements.push(
                        Binder::bind(self.owner.into(), guard).instantiate(db, inst.args(db)),
                    );
                }
            }
        }
        Some(PredicateListId::new(db, requirements))
    }

    pub(crate) fn with_parameter_bounds(
        self,
        db: &'db dyn HirAnalysisDb,
        assumptions: PredicateListId<'db>,
    ) -> PredicateListId<'db> {
        if self.generic_params(db).data(db).is_empty() {
            return assumptions;
        }
        let inst = TraitInstId::new(
            db,
            self.owner,
            collect_generic_params(db, self.owner.into())
                .params(db)
                .to_vec(),
            IndexMap::new(),
        );
        let Some(guards) = self.parameter_bounds(db, inst, &self.family_parameters(db)) else {
            return assumptions;
        };
        let mut predicates = assumptions.list(db).to_vec();
        predicates.extend_from_slice(guards.list(db));
        PredicateListId::new(db, predicates).extend_all_bounds(db)
    }

    /// Iterate trait bounds as per-bound semantic views.
    pub fn bounds(
        self,
        db: &'db dyn HirAnalysisDb,
    ) -> impl Iterator<Item = AssocTypeBoundView<'db>> + 'db {
        let len = self.bounds_raw(db).len();
        let idxs: Vec<usize> = (0..len)
            .filter(|&i| match self.bounds_raw(db)[i] {
                TypeBound::Trait(reference) => {
                    !crate::analysis::ty::ty_lower::is_core_constraint_bound(
                        db,
                        self.scope(),
                        reference,
                    )
                }
                _ => false,
            })
            .collect();
        idxs.into_iter()
            .map(move |idx| AssocTypeBoundView { owner: self, idx })
    }
}

impl<'db> AssocTypeBoundView<'db> {
    /// Position of this bound in the declaration's bound list.
    pub(crate) fn index(self) -> usize {
        self.idx
    }

    pub(crate) fn trait_ref(self, db: &'db dyn HirDb) -> TraitRefId<'db> {
        match self.owner.bounds_raw(db)[self.idx] {
            TypeBound::Trait(tr) => tr,
            _ => unreachable!(),
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct AssocTypeBounds<'db> {
    base: TraitAssocTypeView<'db>,
    subject: TyId<'db>,
    owner_self: TyId<'db>,
}

impl<'db> AssocTypeBounds<'db> {
    fn bounds(self, db: &'db dyn HirAnalysisDb) -> impl Iterator<Item = TraitInstId<'db>> + 'db {
        let owner_trait = self.base.owner;
        let scope = self.base.scope();
        let assumptions = constraints_for(db, owner_trait.into());
        self.base.bounds(db).filter_map(move |b| {
            b.lower_trait_inst(db, self.subject, self.owner_self, scope, assumptions)
                .ok()
        })
    }
}

impl<'db> AssocTypeBoundView<'db> {
    /// Lower this bound to a trait instance for the given subject and owner `Self`.
    pub(crate) fn lower_trait_inst(
        self,
        db: &'db dyn HirAnalysisDb,
        subject: TyId<'db>,
        owner_self: TyId<'db>,
        scope: ScopeId<'db>,
        assumptions: PredicateListId<'db>,
    ) -> Result<TraitInstId<'db>, TraitRefLowerError<'db>> {
        lower_trait_ref(
            db,
            subject,
            self.trait_ref(db),
            scope,
            assumptions,
            Some(owner_self),
        )
    }
}

#[derive(Clone, Copy, Debug)]
pub struct ImplementorAssocTypeView<'db> {
    implementor: ImplementorId<'db>,
    assoc: TraitAssocTypeView<'db>,
    impl_ty: TyId<'db>,
}

impl<'db> ImplementorAssocTypeView<'db> {
    pub(crate) fn parameter_bounds(self, db: &'db dyn HirAnalysisDb) -> PredicateListId<'db> {
        self.assoc
            .parameter_bounds(
                db,
                self.implementor.trait_(db),
                &self.assoc.family_parameters(db),
            )
            .unwrap_or_else(|| PredicateListId::empty_list(db))
    }

    pub fn name(self, db: &'db dyn HirDb) -> Option<IdentId<'db>> {
        self.assoc.name(db)
    }

    pub fn bounds(
        self,
        db: &'db dyn HirAnalysisDb,
    ) -> impl Iterator<Item = TraitInstId<'db>> + 'db {
        // Check a family pointwise under the declaration's rigid parameters.
        // Its constructor is not the subject promised to implement the bound.
        // Substitution is owner-exact: enclosing impl parameters can have the
        // same numeric indices without being family parameters.
        let subject = match self.impl_ty.data(db) {
            TyData::TypeFamily { owner, body, .. } => {
                let params = self.assoc.family_parameters(db);
                // Signature diagnostics own mismatched family parameters.
                // A malformed definition has no pointwise subject on which
                // the declaration's bounds can be checked.
                crate::analysis::ty::ty_lower::CompleteSubst::for_associated_type(
                    db, *owner, params,
                )
                .ok()
                .and_then(|subst| {
                    crate::analysis::ty::subst::substitute_complete(db, *body, &subst).ok()
                })
            }
            _ => Some(self.impl_ty),
        };
        subject.into_iter().flat_map(move |subject| {
            self.assoc
                .bounds_on_subject_with_owner(db, subject, self.implementor.self_ty(db))
        })
    }

    pub fn impl_ty(self) -> TyId<'db> {
        self.impl_ty
    }
}

impl<'db> ImplementorId<'db> {
    /// Contextual assoc-type views for this implementor, pairing each trait associated
    /// type with the implemented type (if provided).
    pub fn assoc_type_views(
        self,
        db: &'db dyn HirAnalysisDb,
    ) -> impl Iterator<Item = ImplementorAssocTypeView<'db>> + 'db {
        let trait_hir = self.trait_def(db);
        let impl_types = self.types(db);
        trait_hir.assoc_types(db).filter_map(move |assoc| {
            let name = assoc.name(db)?;
            let &impl_ty = impl_types.get(&name)?;
            Some(ImplementorAssocTypeView {
                implementor: self,
                assoc,
                impl_ty,
            })
        })
    }
}

impl<'db> TyId<'db> {
    /// Recover the declaration and enclosing arguments even after projection
    /// selected an implementation body. Captures matter even when the body's
    /// result does not mention any of these arguments.
    fn associated_family_context(
        self,
        db: &'db dyn HirAnalysisDb,
    ) -> Option<(TraitAssocTypeView<'db>, TraitInstId<'db>, &'db [TyId<'db>])> {
        use crate::hir_def::scope_graph::AssocTypeOwner;
        let (head, args) = self.decompose_ty_app(db);
        let (inst, name) = match head.data(db) {
            TyData::AssocTy(projected) => (projected.trait_.as_predicate(db), projected.name),
            TyData::TypeFamily {
                owner,
                args: captures,
                ..
            } => match *owner {
                AssocTypeOwner::Trait(trait_, idx) => (
                    TraitInstId::new(db, trait_, captures.clone(), IndexMap::new()),
                    trait_.types(db).get(idx as usize)?.name.to_opt()?,
                ),
                AssocTypeOwner::Impl(impl_, idx) => (
                    Binder::bind(impl_.into(), impl_.trait_inst(db)?).instantiate(db, captures),
                    impl_.types(db).get(idx as usize)?.name.to_opt()?,
                ),
            },
            _ => return None,
        };
        let decl = inst
            .def(db)
            .assoc_types(db)
            .find(|decl| decl.name(db) == Some(name))?;
        let params = decl.generic_params(db).data(db);
        if params.is_empty() {
            return None;
        }
        Some((decl, inst, args))
    }

    /// The family's name, if this applies a family whose parameters have
    /// bounds to fewer arguments than it has parameters.
    pub(crate) fn lacks_args_for_parameter_bounds(
        self,
        db: &'db dyn HirAnalysisDb,
    ) -> Option<IdentId<'db>> {
        let (decl, _, args) = self.associated_family_context(db)?;
        let params = decl.generic_params(db).data(db);
        let has_bounds = params.iter().any(|param| {
            matches!(param, GenericParam::Type(param) if param.bounds.iter().any(|bound| match bound {
                TypeBound::Trait(reference) =>
                    !crate::analysis::ty::ty_lower::is_core_constraint_bound(db, decl.scope(), *reference),
                _ => false,
            }))
        });
        (args.len() < params.len() && has_bounds).then(|| decl.name(db))?
    }

    /// Local declaration guards instantiated at a family application. The
    /// enclosing trait obligation is checked separately from these guards.
    pub(crate) fn family_parameter_bounds(
        self,
        db: &'db dyn HirAnalysisDb,
    ) -> Option<PredicateListId<'db>> {
        let (decl, inst, args) = self.associated_family_context(db)?;
        decl.parameter_bounds(db, inst, args)
    }

    /// Declared output rules for a saturated associated family application.
    /// These are candidates, not assumptions: consumers must establish every
    /// requirement before using a bound. Keep this separate from normalization,
    /// which can erase the projection and its formation requirements.
    pub(crate) fn family_declared_bounds(
        self,
        db: &'db dyn HirAnalysisDb,
    ) -> Option<(PredicateListId<'db>, PredicateListId<'db>)> {
        let (head, _) = self.decompose_ty_app(db);
        if !matches!(head.data(db), TyData::AssocTy(_)) {
            return None;
        }
        let (decl, inst, args) = self.associated_family_context(db)?;
        if args.len() != decl.generic_params(db).data(db).len() {
            return None;
        }
        let trait_owner = inst.def(db);
        let instantiate = |bound| {
            let mapping = crate::analysis::ty::ty_lower::CompleteSubst::for_associated_type(
                db,
                decl.scope().assoc_type_owner().expect("family owner"),
                args.to_vec(),
            )
            .expect("saturated family arguments");
            let bound = crate::analysis::ty::subst::substitute_complete(db, bound, &mapping)
                .expect("family bound substitution");
            Binder::bind(trait_owner.into(), bound).instantiate(db, inst.args(db))
        };
        let mut requirements = vec![inst];
        requirements.extend_from_slice(self.family_parameter_bounds(db)?.list(db));
        // Lower the bounds on the declaration's own formal subject, so the
        // substitutions above only ever replace formals. The actual subject
        // can mention `Self` or the family's parameters itself.
        let bounds: Vec<_> = decl
            .bounds_on_subject(db, decl.formal_subject(db))
            .map(instantiate)
            .collect();
        Some((
            PredicateListId::new(db, requirements),
            PredicateListId::new(db, bounds).extend_all_bounds(db),
        ))
    }

    /// Type-rooted entry to associated-type bounds: attach this type as subject.
    pub fn assoc_type_bounds(
        self,
        db: &'db dyn HirAnalysisDb,
        assoc: TraitAssocTypeView<'db>,
    ) -> impl Iterator<Item = TraitInstId<'db>> + 'db {
        assoc.bounds_on_subject(db, self)
    }
}

// Associated const views ----------------------------------------------------

#[derive(Clone, Copy, Debug)]
pub struct TraitAssocConstView<'db> {
    owner: Trait<'db>,
    idx: usize,
}

impl<'db> TraitAssocConstView<'db> {
    fn decl(self, db: &'db dyn HirDb) -> &'db crate::core::hir_def::AssocConstDecl<'db> {
        &self.owner.consts(db)[self.idx]
    }

    pub fn name(self, db: &'db dyn HirDb) -> Option<IdentId<'db>> {
        self.decl(db).name.to_opt()
    }

    pub fn span(self) -> crate::span::item::LazyTraitConstSpan<'db> {
        self.owner.span().item_list().assoc_const(self.idx)
    }

    /// Returns true if this associated const has a default value in the trait.
    pub fn has_default(self, db: &'db dyn HirDb) -> bool {
        self.decl(db).default.is_some()
    }

    pub fn default_body(self, db: &'db dyn HirDb) -> Option<crate::core::hir_def::Body<'db>> {
        self.decl(db).default.and_then(|body| body.to_opt())
    }

    pub fn hir_ty(self, db: &'db dyn HirDb) -> Option<TypeId<'db>> {
        self.decl(db).ty.to_opt()
    }

    /// Semantic type of this associated const, lowered in the trait's scope.
    pub fn ty(self, db: &'db dyn HirAnalysisDb) -> Option<TyId<'db>> {
        let hir = self.hir_ty(db)?;
        let trait_ = self.owner;
        let assumptions = constraints_for(db, trait_.into());
        Some(lower_hir_ty(db, hir, trait_.scope(), assumptions))
    }

    /// Semantic type of this associated const as a Binder, suitable for
    /// instantiation with trait args.
    pub fn ty_binder(self, db: &'db dyn HirAnalysisDb) -> Option<Binder<'db, TyId<'db>>> {
        self.ty(db).map(|ty| Binder::bind(self.owner.into(), ty))
    }
}

#[derive(Clone, Copy, Debug)]
pub struct ImplAssocConstView<'db> {
    owner: ImplTrait<'db>,
    idx: usize,
}

#[derive(Clone, Copy, Debug)]
pub struct InherentAssocConstView<'db> {
    owner: Impl<'db>,
    idx: usize,
}

impl<'db> InherentAssocConstView<'db> {
    fn def(self, db: &'db dyn HirDb) -> &'db crate::core::hir_def::AssocConstDef<'db> {
        &self.owner.hir_consts(db)[self.idx]
    }

    pub fn name(self, db: &'db dyn HirDb) -> Option<IdentId<'db>> {
        self.def(db).name.to_opt()
    }

    pub fn span(self) -> crate::span::item::LazyTraitConstSpan<'db> {
        self.owner.span().associated_const(self.idx)
    }

    /// Returns true if this associated const has a value defined.
    pub fn has_value(self, db: &'db dyn HirDb) -> bool {
        self.def(db).value.to_opt().is_some()
    }

    pub fn value_body(self, db: &'db dyn HirDb) -> Option<crate::core::hir_def::Body<'db>> {
        self.def(db).value.to_opt()
    }

    /// The HIR type annotation of this associated const.
    pub fn hir_ty(self, db: &'db dyn HirDb) -> Option<crate::core::hir_def::TypeId<'db>> {
        self.def(db).ty.to_opt()
    }

    /// Semantic type of this associated const, referencing the impl's own
    /// generic parameters.
    pub fn ty(self, db: &'db dyn HirAnalysisDb) -> Option<TyId<'db>> {
        let hir = self.def(db).ty.to_opt()?;
        let assumptions = constraints_for(db, self.owner.into());
        Some(lower_hir_ty(db, hir, self.owner.scope(), assumptions))
    }
}

impl<'db> ImplAssocConstView<'db> {
    fn def(self, db: &'db dyn HirDb) -> &'db crate::core::hir_def::AssocConstDef<'db> {
        &self.owner.consts(db)[self.idx]
    }

    pub fn name(self, db: &'db dyn HirDb) -> Option<IdentId<'db>> {
        self.def(db).name.to_opt()
    }

    pub fn span(self) -> crate::span::item::LazyTraitConstSpan<'db> {
        self.owner.span().associated_const(self.idx)
    }

    pub fn docs(self, db: &'db dyn HirDb) -> Option<String> {
        symbol::docs_from_attrs(db, self.def(db).attributes)
    }

    pub fn signature_with_span(self, db: &'db dyn SpannedHirDb) -> Option<SignatureWithSpan> {
        symbol::get_assoc_const_signature_with_span(db, self.span())
    }

    /// Returns true if this associated const has a value defined.
    pub fn has_value(self, db: &'db dyn HirDb) -> bool {
        self.def(db).value.to_opt().is_some()
    }

    pub fn value_body(self, db: &'db dyn HirDb) -> Option<crate::core::hir_def::Body<'db>> {
        self.def(db).value.to_opt()
    }

    pub(crate) fn body_check_policy(
        self,
        db: &'db dyn HirDb,
    ) -> crate::core::hir_def::AssocConstBodyCheckPolicy {
        self.def(db).body_check_policy
    }

    /// Semantic type of this associated const implementation.
    pub fn ty(self, db: &'db dyn HirAnalysisDb) -> Option<TyId<'db>> {
        let hir = self.def(db).ty.to_opt()?;
        let assumptions = constraints_for(db, self.owner.into());
        Some(lower_hir_ty(db, hir, self.owner.scope(), assumptions))
    }

    /// HIR type annotation of this associated const implementation.
    pub fn hir_ty(self, db: &'db dyn HirDb) -> Option<TypeId<'db>> {
        self.def(db).ty.to_opt()
    }
}

#[derive(Clone, Copy, Debug)]
pub struct VariantView<'db> {
    pub owner: Enum<'db>,
    pub idx: usize,
}

impl<'db> VariantView<'db> {
    pub fn kind(self, db: &'db dyn HirDb) -> VariantKind<'db> {
        self.owner.variants_list(db).data(db)[self.idx].kind
    }

    pub fn name(self, db: &'db dyn HirDb) -> Option<IdentId<'db>> {
        self.owner.variants_list(db).data(db)[self.idx]
            .name
            .to_opt()
    }

    pub fn span(self) -> crate::span::item::LazyVariantDefSpan<'db> {
        self.owner.span().variants().variant(self.idx)
    }

    /// Returns semantic types of this variant's fields (empty for unit variants).
    pub fn field_tys(self, db: &'db dyn HirAnalysisDb) -> Vec<Binder<'db, TyId<'db>>> {
        use crate::analysis::ty::ty_def::{InvalidCause, TyId};
        use crate::analysis::ty::ty_lower::lower_hir_ty;

        let enum_ = self.owner;
        let var = EnumVariant::new(enum_, self.idx);
        let scope = var.scope();
        let assumptions =
            collect_constraints(db, GenericParamOwner::Enum(enum_)).instantiate_identity();

        match self.kind(db) {
            VariantKind::Unit => Vec::new(),
            VariantKind::Record(_) => {
                let parent = FieldParent::Variant(var);
                let fields = parent.fields_list(db);
                fields
                    .data(db)
                    .iter()
                    .map(|field| {
                        let ty = match field.type_ref.to_opt() {
                            Some(hir_ty) => lower_hir_ty(db, hir_ty, scope, assumptions),
                            None => TyId::invalid(db, InvalidCause::ParseError),
                        };
                        Binder::bind(enum_.into(), ty)
                    })
                    .collect()
            }
            VariantKind::Tuple(tuple_id) => tuple_id
                .data(db)
                .iter()
                .map(|p| {
                    let ty = match p.to_opt() {
                        Some(hir_ty) => lower_hir_ty(db, hir_ty, scope, assumptions),
                        None => TyId::invalid(db, InvalidCause::ParseError),
                    };
                    Binder::bind(enum_.into(), ty)
                })
                .collect(),
        }
    }

    /// Semantic field-set for this variant.
    pub fn as_adt_fields(self, db: &'db dyn HirAnalysisDb) -> &'db AdtField<'db> {
        let def = lower_adt(db, AdtRef::from(self.owner));
        &def.fields(db)[self.idx]
    }

    /// Diagnostics for tuple-variant element types: star-kind and non-const checks.
    /// Returns an empty list if this is not a tuple variant.
    /// Iterates record fields (empty for non-record variants) as contextual views.
    pub fn fields(self, db: &'db dyn HirDb) -> impl Iterator<Item = FieldView<'db>> + 'db {
        let parent = FieldParent::Variant(EnumVariant::new(self.owner, self.idx));
        let len = match self.kind(db) {
            VariantKind::Record(_) => parent.fields_list(db).data(db).len(),
            _ => 0,
        };
        (0..len).map(move |idx| FieldView { parent, idx })
    }
}

// Field views --------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
pub struct FieldView<'db> {
    pub parent: FieldParent<'db>,
    pub idx: usize,
}

impl<'db> FieldView<'db> {
    pub fn name(self, db: &'db dyn HirDb) -> Option<IdentId<'db>> {
        let list = self.parent.fields_list(db);
        list.data(db)[self.idx].name.to_opt()
    }

    pub fn contract_field_id(self, db: &'db dyn HirDb) -> Option<ContractFieldId<'db>> {
        let FieldParent::Contract(contract) = self.parent else {
            return None;
        };
        self.name(db)?;
        let index = self
            .parent
            .fields(db)
            .take(self.idx + 1)
            .filter(|field| field.name(db).is_some())
            .count()
            .checked_sub(1)?;
        Some(ContractFieldId {
            contract,
            index: index as u32,
        })
    }

    /// Returns the semantic type of this field.
    pub fn ty(self, db: &'db dyn HirAnalysisDb) -> TyId<'db> {
        let (adt_field, idx) = self.as_adt_field(db);
        *adt_field.ty(db, idx).skip_binder()
    }

    /// Returns the semantic ADT field-set and index for this field.
    pub fn as_adt_field(self, db: &'db dyn HirAnalysisDb) -> (AdtField<'db>, usize) {
        (self.parent.as_adt_fields(db), self.idx)
    }

    pub fn ty_span(self) -> crate::span::DynLazySpan<'db> {
        match self.parent {
            FieldParent::Struct(s) => s.span().fields().field(self.idx).ty().into(),
            FieldParent::Contract(c) => c.span().fields().field(self.idx).ty().into(),
            FieldParent::Variant(v) => v.span().fields().field(self.idx).ty().into(),
        }
    }

    /// Returns the lazy span for this field's type in `LazyTySpan` form.
    pub fn lazy_ty_span(self) -> crate::span::types::LazyTySpan<'db> {
        match self.parent {
            FieldParent::Struct(s) => s.span().fields().field(self.idx).ty(),
            FieldParent::Contract(c) => c.span().fields().field(self.idx).ty(),
            FieldParent::Variant(v) => v.span().fields().field(self.idx).ty(),
        }
    }

    /// Returns the scope for type resolution in this field.
    pub fn scope(self) -> ScopeId<'db> {
        match self.parent {
            FieldParent::Struct(s) => s.scope(),
            FieldParent::Contract(c) => c.scope(),
            FieldParent::Variant(v) => v.enum_.scope(),
        }
    }

    /// Returns the owning item for constraint collection.
    pub fn owner_item(self) -> ItemKind<'db> {
        match self.parent {
            FieldParent::Struct(s) => ItemKind::Struct(s),
            FieldParent::Contract(c) => ItemKind::Contract(c),
            FieldParent::Variant(v) => ItemKind::Enum(v.enum_),
        }
    }

    /// All type-related diagnostics for this field.
    pub fn ty_diags(self, db: &'db dyn HirAnalysisDb) -> Vec<TyDiagCollection<'db>> {
        use crate::analysis::name_resolution::{PathRes, resolve_path};
        use crate::analysis::ty::ty_def::TyData;
        use crate::analysis::ty::ty_error::{collect_hir_ty_diags, diag_from_invalid_cause};

        let mut out = Vec::new();

        // First, surface name-resolution errors for the field's HIR type path.
        let hir_ty = self.parent.fields_list(db).data(db)[self.idx].type_ref;
        if let Some(hir_ty) = hir_ty.to_opt() {
            let errs = collect_hir_ty_diags(
                db,
                hir_ty,
                self.lazy_ty_span(),
                context_at(db, self.scope(), None).premises(db),
            );
            if !errs.is_empty() {
                return errs;
            }
        }

        let ty = self.ty(db);
        let span = self.ty_span();

        if !ty.has_star_kind(db) {
            out.push(TyLowerDiag::ExpectedStarKind(span.clone()).into());
            return out;
        }
        if ty.is_const_ty(db) {
            out.push(
                TyLowerDiag::NormalTypeExpected {
                    span: span.clone(),
                    given: ty,
                }
                .into(),
            );
            return out;
        }

        // Trait-bound well-formedness for field type.
        let owner_item = self.owner_item();
        if let Some(diag) =
            check_ty_wf(db, item_solver_context(db, owner_item.scope()), ty).into_diag(span.clone())
        {
            out.push(diag);
            return out;
        }

        // Contract-field layout rejections: a field whose storage placement
        // cannot be determined is an error rather than a silent mis-assignment.
        // At most one fires per field.
        if let Some(field) = self.contract_field_id(db)
            && let Some(error) = field
                .contract
                .storage_layout(db)
                .field_errors_for_id(field)
                .and_then(|errors| errors.first())
        {
            if let ContractLayoutError::InvalidConcreteLayoutConst { invalid } = error {
                if let Some(diag) = invalid
                    .invalid_cause(db)
                    .and_then(|cause| diag_from_invalid_cause(span.clone(), &cause))
                {
                    return vec![diag];
                }
                return vec![
                    TyLowerDiag::ContractFieldLayoutInvariant {
                        span,
                        ty,
                        issue: crate::analysis::ty::diagnostics::ContractFieldLayoutIssue::IncompleteProjection,
                    }
                    .into(),
                ];
            }
            let diag = match error {
                ContractLayoutError::ExplicitContractLayoutHole { .. } => {
                    TyLowerDiag::ContractFieldExplicitConstHole { span, ty }
                }
                ContractLayoutError::NonSlotContractLayoutHole { .. } => {
                    TyLowerDiag::ContractFieldNonSlotConstHole { span, ty }
                }
                ContractLayoutError::UnresolvedConcreteLayoutRoot { .. } => {
                    TyLowerDiag::ContractFieldConcreteLayoutRootUnresolved { span, ty }
                }
                ContractLayoutError::UnresolvedStaticSlotSpace { .. }
                | ContractLayoutError::AmbiguousStaticSlot { .. } => {
                    TyLowerDiag::StaticSlotSpaceUnresolved { span, ty }
                }
                ContractLayoutError::LayoutRootArray { array } => {
                    TyLowerDiag::ContractFieldLayoutRootArray {
                        span,
                        element: array.generic_args(db).first().copied().unwrap_or(*array),
                    }
                }
                ContractLayoutError::AmbiguousProviderLayout => {
                    TyLowerDiag::ContractFieldProviderLayoutAmbiguous { span, ty }
                }
                ContractLayoutError::UnresolvedProviderTarget => {
                    TyLowerDiag::ContractFieldProviderTargetUnresolved { span, ty }
                }
                ContractLayoutError::UnresolvedProviderSpace => {
                    TyLowerDiag::ContractFieldHandleSpaceUnresolved { span, ty }
                }
                ContractLayoutError::InvalidProviderRaw { failure } => {
                    TyLowerDiag::ContractFieldProviderRawInvalid {
                        span,
                        ty,
                        failure: *failure,
                    }
                }
                ContractLayoutError::NonRegularProviderCycle => {
                    TyLowerDiag::ContractFieldProviderCycle { span, ty }
                }
                ContractLayoutError::InvalidFieldType => return out,
                error @ (ContractLayoutError::ConflictingLayoutRootSpaces { .. }
                | ContractLayoutError::LayoutExtentOverflow
                | ContractLayoutError::IncompleteAdtLayoutProjection { .. }
                | ContractLayoutError::InconsistentConcreteArrayLength { .. }
                | ContractLayoutError::AmbiguousLayoutBindingSelector { .. }
                | ContractLayoutError::InconsistentLayoutRootType { .. }
                | ContractLayoutError::LayoutRootNeedsLanding { .. }
                | ContractLayoutError::InternalLayoutGraph) => {
                    let issue = match error {
                        ContractLayoutError::ConflictingLayoutRootSpaces { .. } => {
                            crate::analysis::ty::diagnostics::ContractFieldLayoutIssue::ConflictingRootSpaces
                        }
                        ContractLayoutError::LayoutExtentOverflow => {
                            crate::analysis::ty::diagnostics::ContractFieldLayoutIssue::ExtentOverflow
                        }
                        ContractLayoutError::IncompleteAdtLayoutProjection { .. } => {
                            crate::analysis::ty::diagnostics::ContractFieldLayoutIssue::IncompleteProjection
                        }
                        ContractLayoutError::InconsistentConcreteArrayLength { .. } => {
                            crate::analysis::ty::diagnostics::ContractFieldLayoutIssue::InconsistentArrayLength
                        }
                        ContractLayoutError::AmbiguousLayoutBindingSelector { .. } => {
                            crate::analysis::ty::diagnostics::ContractFieldLayoutIssue::AmbiguousBindingSelector
                        }
                        ContractLayoutError::InconsistentLayoutRootType { .. } => {
                            crate::analysis::ty::diagnostics::ContractFieldLayoutIssue::InconsistentRootType
                        }
                        ContractLayoutError::LayoutRootNeedsLanding { .. } => {
                            crate::analysis::ty::diagnostics::ContractFieldLayoutIssue::RootNeedsLanding
                        }
                        ContractLayoutError::InternalLayoutGraph => {
                            crate::analysis::ty::diagnostics::ContractFieldLayoutIssue::InternalGraph
                        }
                        ContractLayoutError::InvalidFieldType
                        | ContractLayoutError::InvalidConcreteLayoutConst { .. }
                        | ContractLayoutError::ExplicitContractLayoutHole { .. }
                        | ContractLayoutError::NonSlotContractLayoutHole { .. }
                        | ContractLayoutError::UnresolvedConcreteLayoutRoot { .. }
                        | ContractLayoutError::AmbiguousProviderLayout
                        | ContractLayoutError::UnresolvedProviderTarget
                        | ContractLayoutError::UnresolvedProviderSpace
                        | ContractLayoutError::InvalidProviderRaw { .. }
                        | ContractLayoutError::NonRegularProviderCycle
                        | ContractLayoutError::UnresolvedStaticSlotSpace { .. }
                        | ContractLayoutError::AmbiguousStaticSlot { .. }
                        | ContractLayoutError::LayoutRootArray { .. } => {
                            unreachable!()
                        }
                    };
                    TyLowerDiag::ContractFieldLayoutInvariant { span, ty, issue }
                }
                ContractLayoutError::InvalidConcreteLayoutConst { .. } => unreachable!(),
            };
            out.push(diag.into());
            return out;
        }

        // Const type parameter mismatch check: if field name matches a const type parameter.
        if let Some(name) = self.name(db)
            && let Ok(PathRes::Ty(t)) = resolve_path(
                db,
                PathId::from_ident(db, name),
                crate::hir_def::scope_graph::ScopeId::Field(self.parent, self.idx as u16),
                PredicateListId::empty_list(db),
                true,
            )
            && let TyData::ConstTy(const_ty) = t.data(db)
        {
            let expected = *const_ty;
            let expected_ty = expected.ty(db);
            if !expected_ty.has_invalid(db) && !ty.has_invalid(db) && ty != expected_ty {
                out.push(
                    TyLowerDiag::ConstTyMismatch {
                        span,
                        expected: expected_ty,
                        given: ty,
                    }
                    .into(),
                );
                return out;
            }
        }

        out
    }
}

impl<'db> FieldParent<'db> {
    /// Iterates fields as contextual views. For variants, only record variants have fields.
    pub fn fields(self, db: &'db dyn HirDb) -> impl Iterator<Item = FieldView<'db>> + 'db {
        let len = self.fields_list(db).data(db).len();
        (0..len).map(move |idx| FieldView { parent: self, idx })
    }

    /// Semantic field-set for this parent.
    pub fn as_adt_fields(self, db: &'db dyn HirAnalysisDb) -> AdtField<'db> {
        match self {
            FieldParent::Struct(s) => s.as_adt(db).fields(db)[0].clone(),
            FieldParent::Contract(c) => lower_contract_fields(db, c),
            FieldParent::Variant(v) => v.as_adt_fields(db).clone(),
        }
    }
}

impl<'db> EnumVariant<'db> {
    /// Semantic field-set for this variant.
    pub fn as_adt_fields(self, db: &'db dyn HirAnalysisDb) -> &'db AdtField<'db> {
        let def = lower_adt(db, AdtRef::from(self.enum_));
        &def.fields(db)[self.idx as usize]
    }
}

// Type traversal helpers ----------------------------------------------------

impl<'db> TyId<'db> {
    /// Returns the field parent for this type if it's a struct or contract.
    /// This provides access to fields via `field_parent.fields(db)`.
    pub fn field_parent(self, db: &'db dyn HirAnalysisDb) -> Option<FieldParent<'db>> {
        // Check for contract first
        if let Some(contract) = self.as_contract(db) {
            return Some(FieldParent::Contract(contract));
        }
        // Check for struct
        match self.adt_ref(db)? {
            AdtRef::Struct(s) => Some(FieldParent::Struct(s)),
            AdtRef::Enum(_) => None, // Enums don't have direct field access
        }
    }
}

#[cfg(test)]
mod item_context_tests {
    use super::*;
    use crate::test_db::{HirAnalysisTestDb, format_diagnostics};

    fn trait_named<'db>(
        db: &'db HirAnalysisTestDb,
        module: TopLevelMod<'db>,
        name: &str,
    ) -> Trait<'db> {
        module
            .children_non_nested(db)
            .find_map(|item| match item {
                ItemKind::Trait(t) if t.name(db).to_opt().is_some_and(|id| id.data(db) == name) => {
                    Some(t)
                }
                _ => None,
            })
            .expect("fixture trait")
    }

    fn premise_names<'db>(
        db: &'db HirAnalysisTestDb,
        context: crate::analysis::semantic::ImplEnv<'db>,
    ) -> Vec<String> {
        context
            .assumptions(db)
            .list(db)
            .iter()
            .map(|inst| inst.pretty_print(db, true))
            .collect()
    }

    #[test]
    fn header_context_keeps_declared_bounds_without_self_or_effect_premises() {
        let mut db = HirAnalysisTestDb::default();
        let file = db.new_stand_alone(
            "header_context_stage.fe".into(),
            "trait Leaf {}\ntrait Root<T: Leaf> {}\ntrait Provider {}\nfn factory<T: Leaf>() uses (io: Provider) {}\n",
        );
        let (module, _) = db.top_mod(file);
        let root = trait_named(&db, module, "Root");
        let factory = module
            .children_non_nested(&db)
            .find_map(|item| match item {
                ItemKind::Func(func) => Some(func),
                _ => None,
            })
            .expect("fixture function");
        for (scope, implicit) in [(root.scope(), "Root"), (factory.scope(), "Provider")] {
            let header = premise_names(&db, header_context_at(&db, scope));
            let body = premise_names(&db, context_at(&db, scope, None));
            assert!(
                header.iter().any(|name| name.contains("Leaf")),
                "declared bound: {header:?}"
            );
            assert!(
                !header.iter().any(|name| name.contains(implicit)),
                "no implicit Self or effect premise: {header:?}"
            );
            assert!(
                body.iter().any(|name| name.contains(implicit)),
                "body stage: {body:?}"
            );
        }
        assert!(format_diagnostics(&db, &db.run_on_top_mod(module)).is_empty());
    }

    /// An associated type's own parameter bounds hold at its scope. In an impl
    /// they are the trait's declared bounds with the impl's arguments, not the
    /// impl's own written bounds.
    #[test]
    fn associated_type_scopes_read_their_family_parameter_bounds() {
        let mut db = HirAnalysisTestDb::default();
        let file = db.new_stand_alone(
            "family_context.fe".into(),
            r#"
trait Allowed {}
trait Extra {}
trait Factory { type Out<T: Allowed> }
struct P {}
impl Factory for P { type Out<U> = U }
"#,
        );
        let (module, _) = db.top_mod(file);
        let factory = trait_named(&db, module, "Factory");
        let trait_scope = ScopeId::TraitType(factory, 0);
        let names = premise_names(&db, context_at(&db, trait_scope, None));
        assert!(
            names.iter().any(|name| name.contains("Allowed")),
            "trait family scope: {names:?}"
        );
        let impl_trait = module
            .children_non_nested(&db)
            .find_map(|item| match item {
                ItemKind::ImplTrait(i) => Some(i),
                _ => None,
            })
            .expect("fixture impl");
        let impl_scope = ScopeId::ImplTraitType(impl_trait, 0);
        let names = premise_names(&db, context_at(&db, impl_scope, None));
        assert!(
            names.iter().any(|name| name.contains("Allowed")),
            "impl family scope gets the trait's bound: {names:?}"
        );
        // The trait's own item scope is unchanged.
        let names = premise_names(&db, context_at(&db, factory.scope(), None));
        assert!(
            !names.iter().any(|name| name.contains("Allowed")),
            "trait scope: {names:?}"
        );
        assert!(
            format_diagnostics(&db, &db.run_on_top_mod(module)).is_empty(),
            "fixture should check cleanly"
        );
    }

    /// Family bounds naming the trait itself or another member of it are
    /// collected from the declaration without re-entering the trait context.
    #[test]
    fn self_referential_family_bounds_do_not_cycle() {
        let mut db = HirAnalysisTestDb::default();
        let file = db.new_stand_alone(
            "family_context_cycle.fe".into(),
            r#"
trait Holds<T> {}
trait Lift {
    type Item
    type Out<X: Lift>
    type Pair<Y: Holds<Self::Item>>
}
struct Base {}
impl Lift for Base {
    type Item = u256
    type Out<X> = X
    type Pair<Y> = Y
}
"#,
        );
        let (module, _) = db.top_mod(file);
        let lift = trait_named(&db, module, "Lift");
        let trait_premises = context_at(&db, lift.scope(), None)
            .assumptions(&db)
            .list(&db)
            .len();
        let out = context_at(&db, ScopeId::TraitType(lift, 1), None);
        assert!(
            out.assumptions(&db).list(&db).len() > trait_premises,
            "`Out<X: Lift>` adds its bound: {:?}",
            premise_names(&db, out)
        );
        let names = premise_names(&db, context_at(&db, ScopeId::TraitType(lift, 2), None));
        assert!(
            names.iter().any(|name| name.contains("Holds")),
            "`Pair<Y: Holds<Self::Item>>` adds its bound: {names:?}"
        );
        let diags = format_diagnostics(&db, &db.run_on_top_mod(module));
        assert!(diags.is_empty(), "{diags}");
    }

    #[test]
    fn computed_item_context_preserves_declared_bounds_without_inventing_missing_ones() {
        let mut db = HirAnalysisTestDb::default();
        let file = db.new_stand_alone(
            "computed_item_context.fe".into(),
            include_str!("../../../tests/fixtures/assumption_contexts/computed_item.fe"),
        );
        let (module, _) = db.top_mod(file);
        for (name, expected) in [("Bad", false), ("Good", true)] {
            let def = module
                .children_non_nested(&db)
                .find_map(|item| match item {
                    ItemKind::TypeFn(def)
                        if def
                            .name(&db)
                            .to_opt()
                            .is_some_and(|id| id.data(&db) == name) =>
                    {
                        Some(def)
                    }
                    _ => None,
                })
                .expect("fixture type function");
            let assumptions = param_env(&db, def.into());
            let arm = def.arms(&db)[0].ty.to_opt().expect("base arm");
            let ty = lower_hir_ty(&db, arm, def.scope(), assumptions);
            let old = TraitSolveCx::at_position(
                &db,
                crate::analysis::semantic::SolverPremises::from_unpositioned_pair(
                    def.scope(),
                    assumptions,
                ),
                crate::analysis::ty::trait_resolution::ProofProvisions::none(&db),
                context::context_authority(),
            );
            assert_eq!(
                check_ty_wf(&db, old, ty).is_wf(),
                expected,
                "existing computed-item authority: {name}"
            );
            let context = context_at(&db, def.scope(), None);
            let shared = TraitSolveCx::at_position(
                &db,
                context.premises(&db),
                crate::analysis::ty::trait_resolution::ProofProvisions::none(&db),
                self::context::context_authority(),
            );
            assert_eq!(
                check_ty_wf(&db, shared, ty).is_wf(),
                expected,
                "shared computed-item authority: {name}"
            );
        }
    }
}
