use thin_vec::ThinVec;

use super::{
    assoc_const::AssocConstUse,
    binder::Binder,
    const_expr::ConstExpr,
    const_ty::{
        ConstCanonEnv, ConstCanonMode, ConstTyData, ConstTyId, canonicalize_ty_for_mode,
        const_ty_from_assoc_const_use, normalize_const_tys_for_comparison,
    },
    diagnostics::{ImplDiag, TyDiagCollection},
    effects::{
        CanonicalEffectIdentity, EffectKeyCanonMode, EffectKeyKind,
        canonical_effect_identity_for_binding, place_effect_provider_param_index_map,
        rows::row_is_empty,
    },
    fold::{TyFoldable, TyFolder},
    normalize::{normalize_from_assumptions, normalize_ty},
    trait_def::TraitInstId,
    trait_lower::lower_impl_trait,
    trait_resolution::{
        GoalSatisfiability, PredicateListId, TraitSolveCx,
        constraint::collect_func_def_constraints, is_goal_satisfiable,
    },
    ty_check::{BodyOwner, ConstRef, check_anon_const_body},
    ty_def::{BorrowKind, InvalidCause, TyData, TyId},
    ty_lower::{CompleteSubst, ParamDomainId, ParamSchemaId, PartialSubst},
    unify::UnificationTable,
};
use crate::analysis::HirAnalysisDb;
use crate::hir_def::{CallableDef, Expr, Func, ItemKind, Partial, PathKind, scope_graph::ScopeId};
use common::indexmap::IndexMap;

/// Compares the implementation method with the trait method to ensure they
/// match.
///
/// This function performs the following checks:
///
/// 1. Unsafety.
/// 2. Number of generic parameters.
/// 3. Kinds of generic parameters.
/// 4. Arity (number of arguments).
/// 5. Argument labels.
/// 6. Argument types and return type.
/// 7. Method constraints.
///
/// If any of these checks fail, the function will record the appropriate
/// diagnostics.
///
/// # Arguments
///
/// * `db` - Reference to the database implementing the `HirAnalysisDb` trait.
/// * `impl_m` - The implementation method to compare.
/// * `trait_m` - The trait method to compare against.
/// * `implementor` - The implementor that contains associated type bindings.
/// * `sink` - A mutable reference to a vector where diagnostic messages will be
///   collected.
///
/// A method without a mode can be a `mut?` trait method's body at one of
/// its modes, `body_mode` (the `ref` body unless it is the `mut` one of a
/// pair); a `mut?` method implements it at both.
pub(super) fn compare_impl_method<'db>(
    db: &'db dyn HirAnalysisDb,
    impl_m: CallableDef<'db>,
    trait_m: CallableDef<'db>,
    body_mode: BorrowKind,
    sink: &mut Vec<TyDiagCollection<'db>>,
) {
    if let (CallableDef::Func(impl_func), CallableDef::Func(trait_func)) = (impl_m, trait_m)
        && impl_func.is_unsafe(db) != trait_func.is_unsafe(db)
    {
        sink.push(ImplDiag::MethodUnsafeMismatch { trait_m, impl_m }.into());
    }

    if !compare_generic_param_num(db, impl_m, trait_m, sink) {
        return;
    }

    if !compare_generic_param_kind(db, impl_m, trait_m, sink) {
        return;
    }

    if !compare_arity(db, impl_m, trait_m, sink) {
        return;
    }

    let both = [Some(BorrowKind::Ref), Some(BorrowKind::Mut)];
    let single = [Some(body_mode)];
    let modes: &[Option<BorrowKind>] = match (trait_m.modes(db), impl_m.modes(db)) {
        ([None], [None]) => &[None],
        ([None], _) => {
            sink.push(ImplDiag::MethodMaybeMutMismatch { trait_m, impl_m }.into());
            return;
        }
        (_, [None]) => &single,
        _ => &both,
    };

    // Compare the argument labels, argument types, and return type of the impl
    // method with the trait method.
    let mut err = !compare_arg_label(db, impl_m, trait_m, sink);

    // The effects pair at each mode, where a `mut?` effect has its mode.
    let CallableDef::Func(body) = impl_m else {
        return;
    };
    let substs: Result<Vec<_>, _> = modes
        .iter()
        .map(|&trait_mode| {
            method_correspondence(db, body, trait_mode).map(|correspondence| {
                let subst = correspondence.trait_to_body.residualize(db);
                (correspondence, subst)
            })
        })
        .collect();
    let Ok(substs) = substs else {
        // Structural generic errors above retain their specific diagnostics.
        // A malformed effect/domain correspondence cannot be used as a map.
        sink.push(
            ImplDiag::MethodEffectMismatch {
                trait_m,
                impl_m,
                mode: None,
            }
            .into(),
        );
        return;
    };
    let unpaired: Vec<_> = substs
        .iter()
        .filter(|(correspondence, _)| !correspondence.effects_complete)
        .map(|(correspondence, _)| correspondence.trait_mode)
        .collect();
    if matches!(
        (impl_m, trait_m),
        (CallableDef::Func(_), CallableDef::Func(_))
    ) && !unpaired.is_empty()
    {
        // A `mut?` trait method's effects differ at one of its modes alone.
        let mode = match unpaired.as_slice() {
            [mode] => *mode,
            _ => None,
        };
        sink.push(
            ImplDiag::MethodEffectMismatch {
                trait_m,
                impl_m,
                mode,
            }
            .into(),
        );
        err = true;
    }
    for (correspondence, param_subst) in &substs {
        err |= !compare_ty(
            db,
            (impl_m, correspondence.body_mode),
            (trait_m, correspondence.trait_mode),
            param_subst,
            correspondence.trait_inst,
            sink,
        );
    }
    if err {
        return;
    }

    for (correspondence, param_subst) in &substs {
        compare_constraints(
            db,
            (impl_m, correspondence.body_mode),
            (trait_m, correspondence.trait_mode),
            correspondence.trait_inst,
            param_subst,
            sink,
        );
    }
}

/// Checks if the number of generic parameters of the implemented method is the
/// same as the number of generic parameters of the trait method.
/// Returns `false` if the comparison fails.
fn compare_generic_param_num<'db>(
    db: &'db dyn HirAnalysisDb,
    impl_m: CallableDef<'db>,
    trait_m: CallableDef<'db>,
    sink: &mut Vec<TyDiagCollection<'db>>,
) -> bool {
    let impl_params = impl_m.explicit_params(db);
    let trait_params = trait_m.explicit_params(db);

    if impl_params.len() == trait_params.len() {
        true
    } else {
        sink.push(ImplDiag::MethodTypeParamNumMismatch { trait_m, impl_m }.into());
        false
    }
}

/// Checks if the generic parameter kinds are the same.
/// Returns `false` if the comparison fails.
fn compare_generic_param_kind<'db>(
    db: &'db dyn HirAnalysisDb,
    impl_m: CallableDef<'db>,
    trait_m: CallableDef<'db>,
    sink: &mut Vec<TyDiagCollection<'db>>,
) -> bool {
    let mut err = false;
    for (idx, (&trait_m_param, &impl_m_param)) in trait_m
        .explicit_params(db)
        .iter()
        .zip(impl_m.explicit_params(db))
        .enumerate()
    {
        let trait_m_kind = trait_m_param.kind(db);
        let impl_m_kind = impl_m_param.kind(db);

        if !trait_m_kind.does_match(impl_m_kind) {
            sink.push(
                ImplDiag::MethodTypeParamKindMismatch {
                    trait_m,
                    impl_m,
                    param_idx: idx,
                }
                .into(),
            );
            err = true;
        }
    }

    !err
}

/// Checks if the arity of the implemented method is the same as the arity of
/// the trait method.
/// Returns `false` if the comparison fails.
fn compare_arity<'db>(
    db: &'db dyn HirAnalysisDb,
    impl_m: CallableDef<'db>,
    trait_m: CallableDef<'db>,
    sink: &mut Vec<TyDiagCollection<'db>>,
) -> bool {
    let impl_m_arity = impl_m.arg_tys(db).len();
    let trait_m_arity = trait_m.arg_tys(db).len();

    // Checks if the arity are the same.
    if impl_m_arity == trait_m_arity {
        true
    } else {
        sink.push(ImplDiag::MethodArgNumMismatch { impl_m, trait_m }.into());
        false
    }
}

/// Checks if the argument labels of the implemented method are the same as the
/// argument labels of the trait method.
/// Returns `false` if the comparison fails.
fn compare_arg_label<'db>(
    db: &'db dyn HirAnalysisDb,
    impl_m: CallableDef<'db>,
    trait_m: CallableDef<'db>,
    sink: &mut Vec<TyDiagCollection<'db>>,
) -> bool {
    let mut err = false;
    let len = impl_m.arg_tys(db).len().min(trait_m.arg_tys(db).len());
    for idx in 0..len {
        let Some(expected_label) = trait_m.param_label_or_name(db, idx) else {
            continue;
        };
        let Some(method_label) = impl_m.param_label_or_name(db, idx) else {
            continue;
        };
        if expected_label != method_label {
            sink.push(
                ImplDiag::MethodArgLabelMismatch {
                    trait_m,
                    impl_m,
                    param_idx: idx,
                }
                .into(),
            );
            err = true;
        }
    }

    !err
}

/// Checks if the argument types and return type of the implemented method,
/// used at its mode, are the same as the argument types and return type of
/// the trait method at its mode.
/// Returns `false` if the comparison fails.
fn compare_ty<'db>(
    db: &'db dyn HirAnalysisDb,
    (impl_m, impl_mode): (CallableDef<'db>, Option<BorrowKind>),
    (trait_m, trait_mode): (CallableDef<'db>, Option<BorrowKind>),
    param_subst: &CompleteSubst<'db>,
    trait_inst: TraitInstId<'db>,
    sink: &mut Vec<TyDiagCollection<'db>>,
) -> bool {
    let mut err = false;
    let impl_m_arg_tys = impl_m.arg_tys(db);
    let trait_m_arg_tys = trait_m.arg_tys(db);

    let evidence = PredicateListId::new(db, vec![trait_inst]);
    let impl_assumptions = collect_func_def_constraints(db, impl_m, true).instantiate_identity();
    let trait_assumptions = instantiate_trait_method_template(
        db,
        collect_func_def_constraints(db, trait_m, true),
        param_subst,
    );
    let compare_assumptions = impl_assumptions
        .merge(db, trait_assumptions)
        .extend_all_bounds(db);
    let trait_scope = impl_m.scope();

    for (idx, (trait_m_ty, impl_m_ty)) in trait_m_arg_tys
        .iter()
        .zip(impl_m_arg_tys.iter())
        .enumerate()
    {
        // 1) Instantiate trait method's type params into the impl's generics
        let trait_m_ty = instantiate_trait_method_template(db, *trait_m_ty, param_subst);
        if trait_m_ty.has_invalid(db) {
            continue;
        }
        let impl_m_ty = impl_m_ty.instantiate_identity();

        // 2) Substitute associated types using the provided trait instance.
        let trait_m_ty_substituted =
            normalize_from_assumptions(db, trait_m_ty, trait_scope, evidence);

        // 3) Normalize both types to resolve any further nested associated types.
        let trait_m_ty_normalized =
            normalize_ty(db, trait_m_ty_substituted, trait_scope, compare_assumptions);
        let impl_m_ty_normalized = normalize_ty(db, impl_m_ty, trait_scope, compare_assumptions);
        let trait_m_ty_normalized = normalize_compare_assoc_consts(
            db,
            trait_m_ty_normalized,
            trait_scope,
            compare_assumptions,
            trait_inst,
            true,
        );
        let impl_m_ty_normalized = normalize_compare_assoc_consts(
            db,
            impl_m_ty_normalized,
            trait_scope,
            compare_assumptions,
            trait_inst,
            false,
        );
        // 4) Compare for equality, including the parameter's mode, or its
        // parts' modes for a product.
        let same_parts = match (
            trait_m.param_product(db, idx, trait_mode),
            impl_m.param_product(db, idx, impl_mode),
        ) {
            (None, None) => true,
            (Some(trait_p), Some(impl_p)) => trait_p.skip_binder().same_modes(impl_p.skip_binder()),
            _ => false,
        };
        if !impl_m_ty.has_invalid(db)
            && (trait_m_ty_normalized != impl_m_ty_normalized
                || trait_m.param_mode(db, idx, trait_mode) != impl_m.param_mode(db, idx, impl_mode)
                || !same_parts)
        {
            sink.push(
                ImplDiag::MethodArgTyMismatch {
                    trait_m,
                    impl_m,
                    trait_m_ty: trait_m_ty_normalized,
                    impl_m_ty: impl_m_ty_normalized,
                    param_idx: idx,
                    trait_mode,
                    impl_mode,
                }
                .into(),
            );
            err = true;
        }
    }

    let impl_m_ret_ty = impl_m.ret_ty(db).instantiate_identity();
    let trait_m_ret_ty = instantiate_trait_method_template(db, trait_m.ret_ty(db), param_subst);

    // Substitute and normalize the return type as well.
    let trait_m_ret_ty_substituted =
        normalize_from_assumptions(db, trait_m_ret_ty, trait_scope, evidence);
    let trait_m_ret_ty_normalized = normalize_ty(
        db,
        trait_m_ret_ty_substituted,
        trait_scope,
        compare_assumptions,
    );
    let impl_m_ret_ty_normalized =
        normalize_ty(db, impl_m_ret_ty, trait_scope, compare_assumptions);
    let trait_m_ret_ty_normalized = normalize_compare_assoc_consts(
        db,
        trait_m_ret_ty_normalized,
        trait_scope,
        compare_assumptions,
        trait_inst,
        true,
    );
    let impl_m_ret_ty_normalized = normalize_compare_assoc_consts(
        db,
        impl_m_ret_ty_normalized,
        trait_scope,
        compare_assumptions,
        trait_inst,
        false,
    );
    // A projection's return modes are part of its signature.
    let same_modes = match (
        impl_m.ret_shape(db, impl_mode),
        trait_m.ret_shape(db, trait_mode),
    ) {
        (Some(impl_shape), Some(trait_shape)) => impl_shape
            .skip_binder()
            .same_modes(trait_shape.skip_binder()),
        (impl_shape, trait_shape) => impl_shape.is_none() && trait_shape.is_none(),
    };
    if !impl_m_ret_ty.has_invalid(db)
        && !trait_m_ret_ty.has_invalid(db)
        && (trait_m_ret_ty_normalized != impl_m_ret_ty_normalized || !same_modes)
    {
        sink.push(
            ImplDiag::MethodRetTyMismatch {
                trait_m,
                impl_m,
                trait_ty: trait_m_ret_ty_normalized,
                impl_ty: impl_m_ret_ty_normalized,
                trait_mode,
                impl_mode,
            }
            .into(),
        );

        err = true;
    }

    !err
}

fn insert_param_mapping<'db>(
    db: &'db dyn HirAnalysisDb,
    out: &mut PartialSubst<'db>,
    from: TyId<'db>,
    to: TyId<'db>,
) -> Result<(), MethodCorrespondenceError> {
    let key = out
        .domain()
        .schema(db)
        .original_key(db, from)
        .ok_or(MethodCorrespondenceError::ParameterDomain)?;
    out.bind(db, key, to)
        .map_err(|_| MethodCorrespondenceError::ParameterDomain)
}

/// The trait method's params as the impl method's, the pairs of their
/// effects at the methods' modes, and whether every effect of the trait
/// method has its pair and every ordinary body effect has a nominal source.
fn trait_to_impl_param_subst<'db>(
    db: &'db dyn HirAnalysisDb,
    (impl_m, impl_mode): (CallableDef<'db>, Option<BorrowKind>),
    (trait_m, trait_mode): (CallableDef<'db>, Option<BorrowKind>),
    trait_inst: TraitInstId<'db>,
) -> Result<(PartialSubst<'db>, Vec<(usize, usize)>, bool), MethodCorrespondenceError> {
    let schema = ParamSchemaId::callable(db, trait_m);
    let mut out = PartialSubst::new(db, ParamDomainId::full(db, schema));

    // Map inherited trait params (trait Self + trait generics) using the trait method's
    // lowered param prefix. These are the TyIds that actually occur inside the method's
    // signature and constraints.
    let trait_inst_args = trait_inst.args(db);
    let trait_m_params = trait_m.params(db);
    let trait_def_params = trait_inst.def(db).params(db);
    if trait_m_params.len() < trait_inst_args.len()
        || trait_def_params.len() < trait_inst_args.len()
    {
        return Err(MethodCorrespondenceError::ParameterDomain);
    }
    for (idx, &arg) in trait_inst_args.iter().enumerate() {
        if let Some(&trait_param) = trait_m_params.get(idx) {
            insert_param_mapping(db, &mut out, trait_param, arg)?;
        }
        if let Some(&trait_param) = trait_def_params.get(idx) {
            insert_param_mapping(db, &mut out, trait_param, arg)?;
        }
    }

    // Map explicit method generics by identity.
    for (&trait_param, &impl_param) in trait_m
        .explicit_params(db)
        .iter()
        .zip(impl_m.explicit_params(db).iter())
    {
        insert_param_mapping(db, &mut out, trait_param, impl_param)?;
    }

    let (effect_pairs, effects_paired) = map_effect_provider_params_by_identity(
        db,
        &mut out,
        (impl_m, impl_mode),
        (trait_m, trait_mode),
        trait_inst,
    )?;

    Ok((out, effect_pairs, effects_paired))
}

/// One declaration correspondence, before any caller arguments or proofs are supplied.
#[derive(Debug, Clone, PartialEq, Eq, salsa::Update)]
pub(crate) struct MethodCorrespondence<'db> {
    pub nominal: Func<'db>,
    pub trait_inst: TraitInstId<'db>,
    pub trait_mode: Option<BorrowKind>,
    pub body_mode: Option<BorrowKind>,
    pub trait_to_body: PartialSubst<'db>,
    pub body_to_trait_slots: Vec<Option<usize>>,
    /// None for a trait default body, whose effects keep their own identity.
    pub trait_effect_to_body: Option<Vec<(usize, usize)>>,
    pub effects_complete: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, salsa::Update)]
pub(crate) enum MethodCorrespondenceError {
    NotTraitMethod,
    InvalidImplementation,
    MissingTraitMethod,
    WrongBodyForMode,
    GenericArity,
    GenericKind,
    ParameterDomain,
}

/// The requested *trait* mode selects the contract even when its implementation
/// is an ordinary function (with body mode None). Derive the other inputs here,
/// so conformance, call elaboration and proof transport cannot choose them apart.
/// This query normalizes declaration effect keys; it never checks conformance
/// or constructs a semantic instance itself.
#[salsa::tracked]
pub(crate) fn method_correspondence<'db>(
    db: &'db dyn HirAnalysisDb,
    body: Func<'db>,
    requested_trait_mode: Option<BorrowKind>,
) -> Result<MethodCorrespondence<'db>, MethodCorrespondenceError> {
    let (nominal, trait_inst, is_default) = match body.scope().parent_item(db) {
        Some(ItemKind::ImplTrait(owner)) => {
            let implementor = lower_impl_trait(db, owner)
                .ok_or(MethodCorrespondenceError::InvalidImplementation)?;
            let name = body
                .name(db)
                .to_opt()
                .ok_or(MethodCorrespondenceError::MissingTraitMethod)?;
            let nominal = implementor
                .trait_def(db)
                .method_defs(db)
                .get(&name)
                .copied()
                .ok_or(MethodCorrespondenceError::MissingTraitMethod)?;
            if implementor.method_at(db, name, requested_trait_mode) != Some(body) {
                return Err(MethodCorrespondenceError::WrongBodyForMode);
            }
            (nominal, implementor.trait_inst(db), false)
        }
        Some(ItemKind::Trait(owner)) if body.body(db).is_some() => (
            body,
            TraitInstId::new_simple(db, owner, owner.params(db).to_vec()),
            true,
        ),
        _ => return Err(MethodCorrespondenceError::NotTraitMethod),
    };
    let trait_mode = BodyOwner::func_at(db, nominal, requested_trait_mode).mode();
    if !nominal.modes(db).contains(&trait_mode)
        || (!nominal.is_maybe_mut(db) && body.is_maybe_mut(db))
    {
        return Err(MethodCorrespondenceError::WrongBodyForMode);
    }
    let body_mode = BodyOwner::func_at(db, body, requested_trait_mode).mode();
    let impl_m = CallableDef::Func(body);
    let trait_m = CallableDef::Func(nominal);
    if impl_m.explicit_params(db).len() != trait_m.explicit_params(db).len() {
        return Err(MethodCorrespondenceError::GenericArity);
    }
    if impl_m
        .explicit_params(db)
        .iter()
        .zip(trait_m.explicit_params(db))
        .any(|(&body, &nominal)| !nominal.kind(db).does_match(body.kind(db)))
    {
        return Err(MethodCorrespondenceError::GenericKind);
    }
    let (mapping, effect_pairs, effects_complete) = if is_default {
        let schema = ParamSchemaId::callable(db, trait_m);
        let mut mapping = PartialSubst::new(db, ParamDomainId::full(db, schema));
        for &param in trait_m.params(db) {
            insert_param_mapping(db, &mut mapping, param, param)?;
        }
        (mapping, Vec::new(), true)
    } else {
        trait_to_impl_param_subst(db, (impl_m, body_mode), (trait_m, trait_mode), trait_inst)?
    };
    let impl_params = impl_m.params(db);
    let mut body_to_trait = vec![None; impl_params.len()];
    for (trait_slot, &trait_param) in trait_m.params(db).iter().enumerate() {
        let Some(key) = mapping.domain().schema(db).original_key(db, trait_param) else {
            continue;
        };
        let Some(body_param) = mapping.get(db, key) else {
            continue;
        };
        if let Some(body_slot) = impl_params.iter().position(|param| *param == body_param) {
            body_to_trait[body_slot] = Some(trait_slot);
        }
    }
    Ok(MethodCorrespondence {
        nominal,
        trait_inst,
        trait_mode,
        body_mode,
        trait_to_body: mapping,
        body_to_trait_slots: body_to_trait,
        trait_effect_to_body: (!is_default).then_some(effect_pairs),
        effects_complete,
    })
}

#[derive(Clone, Copy)]
struct EffectProviderEntry<'db> {
    effect_idx: usize,
    provider_param: Option<TyId<'db>>,
    identity: CanonicalEffectIdentity<'db>,
}

fn map_effect_provider_params_by_identity<'db>(
    db: &'db dyn HirAnalysisDb,
    out: &mut PartialSubst<'db>,
    (impl_m, impl_mode): (CallableDef<'db>, Option<BorrowKind>),
    (trait_m, trait_mode): (CallableDef<'db>, Option<BorrowKind>),
    trait_inst: TraitInstId<'db>,
) -> Result<(Vec<(usize, usize)>, bool), MethodCorrespondenceError> {
    let assumptions = collect_func_def_constraints(db, impl_m, true).instantiate_identity();
    // An impl method splices the impl's own rows into its effects: a trait
    // method's entry naming one pairs with the splice, not by identity. A row
    // known to be empty needs no entry at all.
    let impl_func = match impl_m {
        CallableDef::Func(func) => Some(func),
        CallableDef::VariantCtor(_) | CallableDef::StructCtor(_) => None,
    };
    let trait_entries: Vec<_> = collect_effect_provider_entries(
        db,
        (trait_m, trait_mode),
        Some(out),
        trait_inst,
        impl_m.scope(),
        assumptions,
    )
    .into_iter()
    .filter(|entry| {
        !entry.identity.key_row.is_some_and(|row| {
            row_is_empty(db, row, impl_m.scope(), assumptions)
                || row.inst.def(db) == trait_inst.def(db)
                    && row
                        .name(db)
                        .zip(impl_func)
                        .is_some_and(|(name, func)| func.row_splice(db, name).is_some())
        })
    })
    .collect();
    let impl_entries: Vec<_> = collect_effect_provider_entries(
        db,
        (impl_m, impl_mode),
        None,
        trait_inst,
        impl_m.scope(),
        assumptions,
    )
    .into_iter()
    .filter(|entry| {
        !impl_func.is_some_and(|func| func.effect_from_row(db, entry.effect_idx))
            && !entry
                .identity
                .key_row
                .is_some_and(|row| row_is_empty(db, row, impl_m.scope(), assumptions))
    })
    .collect();
    let mut used_impl_entries = vec![false; impl_entries.len()];
    let mut effect_pairs = Vec::new();
    let trait_effects = trait_entries.len();

    for trait_entry in trait_entries {
        let Some((impl_idx, impl_entry)) =
            impl_entries.iter().enumerate().find(|(idx, impl_entry)| {
                !used_impl_entries[*idx]
                    && effect_identity_matches(db, trait_entry.identity, impl_entry.identity)
            })
        else {
            continue;
        };
        used_impl_entries[impl_idx] = true;
        effect_pairs.push((trait_entry.effect_idx, impl_entry.effect_idx));
        if let (Some(trait_param), Some(impl_param)) =
            (trait_entry.provider_param, impl_entry.provider_param)
        {
            insert_param_mapping(db, out, trait_param, impl_param)?;
        }
    }
    // Entailing a capability predicate does not supply the provider value.
    // Only row-spliced/empty effects above may lack a nominal provider role.
    let paired = effect_pairs.len() == trait_effects && used_impl_entries.iter().all(|used| *used);
    Ok((effect_pairs, paired))
}

fn collect_effect_provider_entries<'db>(
    db: &'db dyn HirAnalysisDb,
    (method, mode): (CallableDef<'db>, Option<BorrowKind>),
    param_subst: Option<&PartialSubst<'db>>,
    trait_inst: TraitInstId<'db>,
    scope: ScopeId<'db>,
    assumptions: PredicateListId<'db>,
) -> Vec<EffectProviderEntry<'db>> {
    let CallableDef::Func(func) = method else {
        return Vec::new();
    };
    let param_subst = param_subst.map(|subst| subst.residualize(db));

    let provider_param_map = place_effect_provider_param_index_map(db, func);
    let params = method.params(db);

    func.effect_requirements(db, mode)
        .iter()
        .map(|binding| {
            let effect_idx = binding.binding_idx as usize;
            let provider_param = provider_param_map
                .get(binding.binding_idx as usize)
                .copied()
                .flatten()
                .and_then(|provider_param_idx| params.get(provider_param_idx).copied());
            let mut binding = binding.clone();
            if let Some(subst) = &param_subst {
                binding.key = match binding.key {
                    crate::core::semantic::EffectRequirementKey::Type(key_ty) => {
                        crate::core::semantic::EffectRequirementKey::Type(
                            instantiate_trait_method_template(
                                db,
                                Binder::bind(method.generic_owner(), key_ty),
                                subst,
                            ),
                        )
                    }
                    crate::core::semantic::EffectRequirementKey::Trait(key_trait) => {
                        crate::core::semantic::EffectRequirementKey::Trait(
                            instantiate_trait_method_template(
                                db,
                                Binder::bind(method.generic_owner(), key_trait),
                                subst,
                            ),
                        )
                    }
                    crate::core::semantic::EffectRequirementKey::Row(row) => {
                        crate::core::semantic::EffectRequirementKey::Row(
                            instantiate_trait_method_template(
                                db,
                                Binder::bind(method.generic_owner(), row),
                                subst,
                            ),
                        )
                    }
                    crate::core::semantic::EffectRequirementKey::Other => {
                        crate::core::semantic::EffectRequirementKey::Other
                    }
                };
            }
            let rebase_same_trait_uses = param_subst.is_some();
            let mut identity = canonical_effect_identity_for_binding(
                db,
                &binding,
                scope,
                assumptions,
                Some(trait_inst),
                EffectKeyCanonMode::Compare,
            );
            identity.key_ty = identity.key_ty.map(|ty| {
                normalize_compare_assoc_consts(
                    db,
                    normalize_ty(db, ty, scope, assumptions),
                    scope,
                    assumptions,
                    trait_inst,
                    rebase_same_trait_uses,
                )
            });
            identity.key_trait = identity.key_trait.map(|inst| {
                normalize_predicate_for_comparison(
                    db,
                    inst,
                    scope,
                    assumptions,
                    trait_inst,
                    rebase_same_trait_uses,
                )
            });

            EffectProviderEntry {
                effect_idx,
                provider_param,
                identity,
            }
        })
        .collect()
}

pub(crate) fn trait_effect_key_matches_with<'db>(
    db: &'db dyn HirAnalysisDb,
    expected: TraitInstId<'db>,
    actual: TraitInstId<'db>,
    mut cmp: impl FnMut(TyId<'db>, TyId<'db>) -> bool,
) -> bool {
    if expected.def(db) != actual.def(db) || expected.args(db).len() != actual.args(db).len() {
        return false;
    }

    let expected_assoc = expected.assoc_type_bindings(db);
    let actual_assoc = actual.assoc_type_bindings(db);
    if expected_assoc.len() != actual_assoc.len() {
        return false;
    }

    for (&expected_arg, &actual_arg) in expected
        .args(db)
        .iter()
        .skip(1)
        .zip(actual.args(db).iter().skip(1))
    {
        if !cmp(expected_arg, actual_arg) {
            return false;
        }
    }

    for (name, &expected_ty) in expected_assoc {
        let Some(&actual_ty) = actual_assoc.get(name) else {
            return false;
        };
        if !cmp(expected_ty, actual_ty) {
            return false;
        }
    }

    true
}

fn effect_identity_matches<'db>(
    db: &'db dyn HirAnalysisDb,
    trait_identity: CanonicalEffectIdentity<'db>,
    impl_identity: CanonicalEffectIdentity<'db>,
) -> bool {
    if trait_identity.key_kind != impl_identity.key_kind
        || trait_identity.is_mut != impl_identity.is_mut
    {
        return false;
    }

    match trait_identity.key_kind {
        EffectKeyKind::Type => match (trait_identity.key_ty, impl_identity.key_ty) {
            (Some(trait_key_ty), Some(impl_key_ty)) => {
                effect_identity_tys_match(db, trait_key_ty, impl_key_ty)
            }
            _ => false,
        },
        EffectKeyKind::Trait => match (trait_identity.key_trait, impl_identity.key_trait) {
            (Some(trait_key_trait), Some(impl_key_trait)) => {
                trait_effect_key_matches_with(db, trait_key_trait, impl_key_trait, |lhs, rhs| {
                    effect_identity_tys_match(db, lhs, rhs)
                })
            }
            _ => false,
        },
        EffectKeyKind::Row => match (trait_identity.key_row, impl_identity.key_row) {
            (Some(trait_row), Some(impl_row)) => {
                trait_row.row == impl_row.row
                    && trait_effect_key_matches_with(
                        db,
                        trait_row.inst,
                        impl_row.inst,
                        |lhs, rhs| effect_identity_tys_match(db, lhs, rhs),
                    )
            }
            _ => false,
        },
        EffectKeyKind::Other => trait_identity.key_syntax == impl_identity.key_syntax,
    }
}

fn effect_identity_tys_match<'db>(
    db: &'db dyn HirAnalysisDb,
    expected: TyId<'db>,
    actual: TyId<'db>,
) -> bool {
    UnificationTable::new(db).unify(expected, actual).is_ok()
}

fn instantiate_trait_method_template<'db, T>(
    db: &'db dyn HirAnalysisDb,
    binder: Binder<'db, T>,
    param_subst: &CompleteSubst<'db>,
) -> T
where
    T: TyFoldable<'db>,
{
    binder
        .instantiate_subst(db, param_subst)
        .expect("trait method comparison uses the declared parameter domain")
}

pub(crate) fn normalize_predicate_for_comparison<'db>(
    db: &'db dyn HirAnalysisDb,
    pred: TraitInstId<'db>,
    scope: ScopeId<'db>,
    assumptions: PredicateListId<'db>,
    trait_inst: TraitInstId<'db>,
    rebase_same_trait_uses: bool,
) -> TraitInstId<'db> {
    let args: Vec<_> = pred
        .args(db)
        .iter()
        .copied()
        .map(|ty| {
            let ty = normalize_ty(db, ty, scope, assumptions);
            normalize_compare_assoc_consts(
                db,
                ty,
                scope,
                assumptions,
                trait_inst,
                rebase_same_trait_uses,
            )
        })
        .collect();
    let mut assoc_type_bindings: Vec<_> = pred
        .assoc_type_bindings(db)
        .iter()
        .map(|(name, &ty)| {
            let ty = normalize_ty(db, ty, scope, assumptions);
            (
                *name,
                normalize_compare_assoc_consts(
                    db,
                    ty,
                    scope,
                    assumptions,
                    trait_inst,
                    rebase_same_trait_uses,
                ),
            )
        })
        .collect();
    assoc_type_bindings.sort_by(|(lhs, _), (rhs, _)| lhs.data(db).cmp(rhs.data(db)));
    TraitInstId::new(
        db,
        pred.def(db),
        args,
        assoc_type_bindings.into_iter().collect::<IndexMap<_, _>>(),
        pred.mode(db),
    )
}

fn normalize_predicates_for_comparison<'db>(
    db: &'db dyn HirAnalysisDb,
    preds: PredicateListId<'db>,
    scope: ScopeId<'db>,
    assumptions: PredicateListId<'db>,
    trait_inst: TraitInstId<'db>,
    rebase_same_trait_uses: bool,
) -> PredicateListId<'db> {
    PredicateListId::new(
        db,
        preds
            .list(db)
            .iter()
            .copied()
            .map(|pred| {
                normalize_predicate_for_comparison(
                    db,
                    pred,
                    scope,
                    assumptions,
                    trait_inst,
                    rebase_same_trait_uses,
                )
            })
            .collect::<Vec<_>>(),
    )
}

pub(crate) fn normalize_compare_assoc_consts<'db>(
    db: &'db dyn HirAnalysisDb,
    ty: TyId<'db>,
    scope: crate::hir_def::scope_graph::ScopeId<'db>,
    assumptions: super::trait_resolution::PredicateListId<'db>,
    trait_inst: TraitInstId<'db>,
    rebase_same_trait_uses: bool,
) -> TyId<'db> {
    fn is_identity_trait_inst<'db>(db: &'db dyn HirAnalysisDb, inst: TraitInstId<'db>) -> bool {
        let trait_def = inst.def(db);
        inst.assoc_type_bindings(db).is_empty() && inst.args(db) == trait_def.params(db)
    }

    let ty = normalize_const_tys_for_comparison(db, ty);

    struct CompareAssocConstNormalizer<'db> {
        scope: crate::hir_def::scope_graph::ScopeId<'db>,
        assumptions: super::trait_resolution::PredicateListId<'db>,
        trait_inst: TraitInstId<'db>,
        rebase_same_trait_uses: bool,
    }

    impl<'db> CompareAssocConstNormalizer<'db> {
        fn canonicalize_assoc_use(
            &self,
            db: &'db dyn HirAnalysisDb,
            assoc: AssocConstUse<'db>,
        ) -> AssocConstUse<'db> {
            let assoc = assoc.with_env(self.scope, self.assumptions);
            if self.rebase_same_trait_uses
                && assoc.inst().def(db) == self.trait_inst.def(db)
                && is_identity_trait_inst(db, assoc.inst())
            {
                assoc.with_inst(self.trait_inst)
            } else {
                assoc
            }
        }

        fn normalize_assoc_const_ty(
            &mut self,
            db: &'db dyn HirAnalysisDb,
            assoc: AssocConstUse<'db>,
            expected_ty: TyId<'db>,
        ) -> Option<TyId<'db>> {
            let assoc = self.canonicalize_assoc_use(db, assoc);
            let const_ty = const_ty_from_assoc_const_use(db, assoc)?;
            let evaluated = const_ty.evaluate(db, Some(expected_ty));
            if matches!(
                evaluated.ty(db).invalid_cause(db),
                Some(InvalidCause::ConstEvalUnsupported { .. })
            ) {
                return None;
            }

            Some(self.fold_ty(db, TyId::new(db, TyData::ConstTy(evaluated))))
        }

        fn normalize_unevaluated_assoc_const(
            &mut self,
            db: &'db dyn HirAnalysisDb,
            body: crate::hir_def::Body<'db>,
            expected_ty: TyId<'db>,
            capture: &super::const_ty::ConstCaptureEnv<'db>,
        ) -> Option<TyId<'db>> {
            let (diags, typed_body) = check_anon_const_body(db, body, expected_ty);
            if !diags.is_empty() {
                return None;
            }

            let typed_body = capture.specialize(db, typed_body.clone());
            let ConstRef::TraitConst { use_: assoc, .. } =
                typed_body.expr_const_ref(body.expr(db))?
            else {
                return None;
            };

            self.normalize_assoc_const_ty(db, assoc, expected_ty)
        }

        fn normalize_unevaluated_const_fn_call(
            &mut self,
            db: &'db dyn HirAnalysisDb,
            const_ty: ConstTyId<'db>,
            expected_ty: TyId<'db>,
        ) -> Option<TyId<'db>> {
            let evaluated = const_ty.evaluate(db, Some(expected_ty));
            if evaluated == const_ty || evaluated.ty(db).invalid_cause(db).is_some() {
                return None;
            }
            if !matches!(
                evaluated.data(db),
                ConstTyData::Value(..) | ConstTyData::Description(..) | ConstTyData::Abstract(..)
            ) {
                return None;
            }
            Some(self.fold_ty(db, TyId::new(db, TyData::ConstTy(evaluated))))
        }
    }

    impl<'db> TyFolder<'db> for CompareAssocConstNormalizer<'db> {
        fn fold_ty(&mut self, db: &'db dyn HirAnalysisDb, ty: TyId<'db>) -> TyId<'db> {
            let ty = ty.super_fold_with(db, self);
            let TyData::ConstTy(const_ty) = ty.data(db) else {
                return ty;
            };

            match const_ty.data(db) {
                ConstTyData::Abstract(expr, expected_ty) => {
                    let ConstExpr::TraitConst(assoc) = expr.data(db) else {
                        return ty;
                    };
                    let assoc = self.canonicalize_assoc_use(db, *assoc);
                    if let Some(ty) = self.normalize_assoc_const_ty(db, assoc, *expected_ty) {
                        return ty;
                    }
                    let expr =
                        super::const_expr::ConstExprId::new(db, ConstExpr::TraitConst(assoc));
                    TyId::new(
                        db,
                        TyData::ConstTy(super::const_ty::ConstTyId::new(
                            db,
                            ConstTyData::Abstract(expr, *expected_ty),
                        )),
                    )
                }
                ConstTyData::UnEvaluated {
                    body,
                    ty: expected_ty,
                    const_def,
                    capture,
                    ..
                } => {
                    if const_def.is_some() {
                        return ty;
                    }
                    let Some(expected_ty) = *expected_ty else {
                        return ty;
                    };
                    let expr = body.expr(db);
                    let Partial::Present(Expr::Path(path)) = expr.data(db, *body) else {
                        return self
                            .normalize_unevaluated_const_fn_call(db, *const_ty, expected_ty)
                            .unwrap_or(ty);
                    };
                    let Some(path) = path.to_opt() else {
                        return ty;
                    };
                    let Some(parent) = path.parent(db) else {
                        return ty;
                    };
                    let PathKind::Ident {
                        generic_args: path_generic_args,
                        ..
                    } = path.kind(db)
                    else {
                        return ty;
                    };
                    if !parent.is_self_ty(db) || !path_generic_args.is_empty(db) {
                        return self
                            .normalize_unevaluated_const_fn_call(db, *const_ty, expected_ty)
                            .unwrap_or(ty);
                    }

                    self.normalize_unevaluated_assoc_const(db, *body, expected_ty, capture)
                        .unwrap_or(ty)
                }
                _ => ty,
            }
        }
    }

    let mut folder = CompareAssocConstNormalizer {
        scope,
        assumptions,
        trait_inst,
        rebase_same_trait_uses,
    };
    let ty = canonicalize_ty_for_mode(
        db,
        ty.fold_with(db, &mut folder),
        ConstCanonEnv::new(scope, assumptions, None),
        ConstCanonMode::Identity,
    );
    ty.fold_with(db, &mut folder)
}

/// Checks if the method constraints are stricter than the trait constraints.
/// This check is performed by checking if the `impl_method` constraints are
/// satisfied under the assumptions that is obtained from the `expected_method`
/// constraints.
/// Returns `false` if the comparison fails.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ConstraintEntailment {
    Proven,
    Disproven,
    Incomplete,
}

fn classify_constraint_entailment(result: GoalSatisfiability<'_>) -> ConstraintEntailment {
    match result {
        GoalSatisfiability::Satisfied(_) | GoalSatisfiability::ContainsInvalid => {
            ConstraintEntailment::Proven
        }
        // Method comparison asks only whether a proof exists, so one admitted
        // answer is sufficient even if uniqueness or saturation is unknown.
        GoalSatisfiability::NeedsConfirmation { solutions, .. } if !solutions.is_empty() => {
            ConstraintEntailment::Proven
        }
        GoalSatisfiability::NeedsConfirmation { .. } => ConstraintEntailment::Incomplete,
        GoalSatisfiability::UnSat(_) => ConstraintEntailment::Disproven,
    }
}

fn compare_constraints<'db>(
    db: &'db dyn HirAnalysisDb,
    (impl_m, impl_mode): (CallableDef<'db>, Option<BorrowKind>),
    (trait_m, trait_mode): (CallableDef<'db>, Option<BorrowKind>),
    trait_inst: TraitInstId<'db>,
    param_subst: &CompleteSubst<'db>,
    sink: &mut Vec<TyDiagCollection<'db>>,
) -> bool {
    let impl_m_constraints = collect_func_def_constraints(db, impl_m, false)
        .instantiate_identity()
        .at_mode(db, impl_mode);
    let trait_m_constraints = instantiate_trait_method_template(
        db,
        collect_func_def_constraints(db, trait_m, false),
        param_subst,
    )
    .at_mode(db, trait_mode);
    let compare_assumptions = impl_m_constraints
        .merge(db, trait_m_constraints)
        .extend_all_bounds(db);
    let impl_m_constraints = normalize_predicates_for_comparison(
        db,
        impl_m_constraints,
        trait_m.scope(),
        compare_assumptions,
        trait_inst,
        false,
    );
    let trait_m_constraints = normalize_predicates_for_comparison(
        db,
        trait_m_constraints,
        trait_m.scope(),
        compare_assumptions,
        trait_inst,
        true,
    );
    // A row's components bound their providers as the row's implementation
    // requires; the trait method's row stands for them.
    let row_providers: Vec<TyId<'db>> = match impl_m {
        CallableDef::Func(func) => place_effect_provider_param_index_map(db, func)
            .iter()
            .enumerate()
            .filter(|(effect, _)| func.effect_from_row(db, *effect))
            .filter_map(|(_, param)| impl_m.params(db).get((*param)?).copied())
            .collect(),
        CallableDef::VariantCtor(_) | CallableDef::StructCtor(_) => Vec::new(),
    };
    let mut unsatisfied_goals = ThinVec::new();
    for &goal in impl_m_constraints.list(db) {
        if row_providers.contains(&goal.self_ty(db)) {
            continue;
        }
        // Source-level concrete assumptions are rejected, so the solver does
        // not consult assumptions for concrete goals. Method comparison can
        // legitimately produce a concrete goal after substituting the owning
        // impl, and an identical declared trait bound entails itself without
        // requiring trait selection.
        if trait_m_constraints.list(db).contains(&goal) {
            continue;
        }
        let entailment = classify_constraint_entailment(is_goal_satisfiable(
            db,
            TraitSolveCx::new(db, trait_m.scope()).with_assumptions(trait_m_constraints),
            goal,
        ));
        match entailment {
            ConstraintEntailment::Proven => {}
            // An incomplete search cannot establish that the trait method
            // entails the impl method's bound. Diagnose it conservatively
            // instead of accepting a potentially stricter implementation.
            ConstraintEntailment::Disproven | ConstraintEntailment::Incomplete => {
                unsatisfied_goals.push(goal);
            }
        }
    }

    if unsatisfied_goals.is_empty() {
        true
    } else {
        sink.push(
            ImplDiag::MethodStricterBound {
                span: impl_m.name_span(),
                stricter_bounds: unsatisfied_goals,
            }
            .into(),
        );
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{hir_def::IdentId, test_db::HirAnalysisTestDb};

    #[test]
    fn declaration_correspondence_preserves_reordered_effect_provider_roles() {
        let mut db = HirAnalysisTestDb::default();
        let file = db.new_stand_alone(
            "method_correspondence_effects.fe".into(),
            r#"
struct First {}
struct Second {}
struct Item {}
trait Run {
    fn run(self) -> u256 uses (a: First, b: Second)
}
impl Run for Item {
    fn run(self) -> u256 uses (b: Second, a: First) { 1 }
}
"#,
        );
        let (module, _) = db.top_mod(file);
        db.assert_no_diags(module);
        let owner = module.all_impl_traits(&db)[0];
        let body = owner.methods(&db).next().unwrap();
        let descriptor = method_correspondence(&db, body, None).unwrap();
        assert!(descriptor.effects_complete);
        assert_eq!(descriptor.trait_effect_to_body, Some(vec![(0, 1), (1, 0)]));
        let implementor = lower_impl_trait(&db, owner).unwrap();
        let instance = super::super::trait_def::ResolvedImplInstance::new(
            implementor,
            implementor,
            implementor.trait_inst(&db),
        );
        let selected = instance.method(&db, body.name(&db).unwrap(), None).unwrap();
        assert_eq!(
            selected.effect_pairs(),
            descriptor.trait_effect_to_body.as_deref()
        );
        let nominal_args = CallableDef::Func(descriptor.nominal).params(&db);
        let completed = selected.complete_body_args(&db, nominal_args).unwrap();
        let body_params = CallableDef::Func(body).params(&db);
        for (body_slot, trait_slot) in descriptor.body_to_trait_slots.iter().enumerate() {
            if let Some(trait_slot) = trait_slot {
                assert_eq!(completed.values()[body_slot], nominal_args[*trait_slot]);
            }
        }
        assert_eq!(completed.values().len(), body_params.len());
    }

    #[test]
    fn declaration_correspondence_distinguishes_split_trait_modes() {
        let mut db = HirAnalysisTestDb::default();
        let file = db.new_stand_alone(
            "method_correspondence_modes.fe".into(),
            r#"
struct Store { value: u256 }
struct Item { value: u256 }
trait Slot {
    fn slot(mut? self) -> mut? u256 uses (store: mut? Store)
}
impl Slot for Item {
    fn slot(self) -> ref u256 uses (store: Store) { ref self.value }
    fn slot(mut self) -> mut u256 uses (store: mut Store) { mut self.value }
}
"#,
        );
        let (module, _) = db.top_mod(file);
        db.assert_no_diags(module);
        let owner = module.all_impl_traits(&db)[0];
        let implementor = lower_impl_trait(&db, owner).unwrap();
        let name = IdentId::new(&db, "slot");
        let read = implementor
            .method_at(&db, name, Some(BorrowKind::Ref))
            .unwrap();
        let write = implementor
            .method_at(&db, name, Some(BorrowKind::Mut))
            .unwrap();
        assert_ne!(read, write);
        for (body, mode) in [(read, BorrowKind::Ref), (write, BorrowKind::Mut)] {
            let descriptor = method_correspondence(&db, body, Some(mode)).unwrap();
            assert_eq!(descriptor.trait_mode, Some(mode));
            assert_eq!(descriptor.body_mode, None);
            assert!(descriptor.effects_complete);
            assert_eq!(descriptor.trait_effect_to_body, Some(vec![(0, 0)]));
        }
        assert_eq!(
            method_correspondence(&db, read, Some(BorrowKind::Mut)),
            Err(MethodCorrespondenceError::WrongBodyForMode)
        );
        assert_eq!(
            method_correspondence(&db, write, Some(BorrowKind::Ref)),
            Err(MethodCorrespondenceError::WrongBodyForMode)
        );
    }

    #[test]
    fn declaration_correspondence_keeps_default_and_generic_method_domains() {
        let mut db = HirAnalysisTestDb::default();
        let file = db.new_stand_alone(
            "method_correspondence_generic.fe".into(),
            r#"
struct Item {}
trait Map<A> {
    fn convert<T>(_ value: T, _ arg: A) -> T
    fn fallback<T>(_ value: T) -> T { value }
}
impl Map<u256> for Item {
    fn convert<U>(_ value: U, _ arg: u256) -> U { value }
}
"#,
        );
        let (module, _) = db.top_mod(file);
        db.assert_no_diags(module);
        let owner = module.all_impl_traits(&db)[0];
        let body = owner.methods(&db).next().unwrap();
        let descriptor = method_correspondence(&db, body, None).unwrap();
        let nominal = CallableDef::Func(descriptor.nominal);
        let body = CallableDef::Func(body);
        let mapped = descriptor.trait_to_body.residualize(&db);
        let nominal_own = nominal.explicit_params(&db)[0];
        let key = mapped
            .domain()
            .schema(&db)
            .original_key(&db, nominal_own)
            .unwrap();
        assert_eq!(mapped.get(&db, key), Some(body.explicit_params(&db)[0]));
        let default =
            descriptor.trait_inst.def(&db).method_defs(&db)[&IdentId::new(&db, "fallback")];
        let identity = method_correspondence(&db, default, None).unwrap();
        assert_eq!(identity.nominal, default);
        assert_eq!(identity.trait_effect_to_body, None);
        assert_eq!(
            identity.body_to_trait_slots,
            (0..CallableDef::Func(default).params(&db).len())
                .map(Some)
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn declaration_correspondence_rejects_an_unprovided_blanket_capability() {
        let mut db = HirAnalysisTestDb::default();
        let file = db.new_stand_alone(
            "method_correspondence_extra_provider.fe".into(),
            r#"
trait Cap {}
impl<T> Cap for T {}
struct Item {}
trait Contract { fn run() }
impl Contract for Item { fn run() uses (extra: Cap) {} }
"#,
        );
        let (module, _) = db.top_mod(file);
        let owner = module
            .all_impl_traits(&db)
            .iter()
            .copied()
            .find(|owner| {
                owner.methods(&db).any(|func| {
                    func.name(&db)
                        .to_opt()
                        .is_some_and(|name| name.data(&db) == "run")
                })
            })
            .unwrap();
        let body = owner.methods(&db).next().unwrap();
        let descriptor = method_correspondence(&db, body, None).unwrap();
        assert!(
            !descriptor.effects_complete,
            "a blanket predicate cannot supply a provider value"
        );
        assert!(
            !lower_impl_trait(&db, owner)
                .unwrap()
                .diags_method_conformance(&db)
                .is_empty()
        );
    }

    #[test]
    fn declaration_correspondence_reports_malformed_generic_arity() {
        let mut db = HirAnalysisTestDb::default();
        let file = db.new_stand_alone(
            "method_correspondence_invalid.fe".into(),
            r#"
struct Item {}
trait Map { fn convert<T>(_ value: T) -> T }
impl Map for Item { fn convert(_ value: u256) -> u256 { value } }
fn unrelated() {}
"#,
        );
        let (module, _) = db.top_mod(file);
        let body = module.all_impl_traits(&db)[0].methods(&db).next().unwrap();
        assert_eq!(
            method_correspondence(&db, body, None),
            Err(MethodCorrespondenceError::GenericArity)
        );
        let unrelated = module
            .children_non_nested(&db)
            .find_map(|item| match item {
                ItemKind::Func(func) => Some(func),
                _ => None,
            })
            .unwrap();
        assert_eq!(
            method_correspondence(&db, unrelated, None),
            Err(MethodCorrespondenceError::NotTraitMethod)
        );
        // The public checker retains the source-specific diagnostic instead of panicking.
        let implementor = lower_impl_trait(&db, module.all_impl_traits(&db)[0]).unwrap();
        assert!(!implementor.diags_method_conformance(&db).is_empty());
    }
}
