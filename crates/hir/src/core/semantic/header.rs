//! Declared-header progress. The existing fixed point owns each prior stage.
use crate::analysis::HirAnalysisDb;
use crate::analysis::ty::{
    const_ty::{ConstBodyLowering, HoleAnchor, LoweringContext},
    trait_def::TraitInstId,
    trait_lower::{lower_trait_ref, lower_trait_ref_with_minter},
    trait_resolution::{
        PredicateListId,
        constraint::{PredicateSource, enclosing_trait_self_ty},
    },
    ty_def::TyId,
    ty_lower::{
        collect_generic_params, collect_source_generic_params, lower_hir_ty,
        lower_hir_ty_with_minter,
    },
};
use crate::hir_def::{
    GenericParam, GenericParamOwner, GenericParamView, TraitRefId, TypeBound,
    types::TypeId as HirTypeId,
};
use common::indexmap::IndexMap;
use either::Either;

pub(crate) fn collect_decl_constraint_pairs_impl<'db>(
    db: &'db dyn HirAnalysisDb,
    owner: GenericParamOwner<'db>,
    initial: &[(TraitInstId<'db>, PredicateSource<'db>)],
    const_bodies: ConstBodyLowering,
    source_params: Option<GenericParamOwner<'db>>,
) -> Vec<(TraitInstId<'db>, PredicateSource<'db>)> {
    let mut deferred: Vec<Deferred<'db>> = Vec::new();
    let owner_scope = owner.scope();

    // Generic parameter bounds
    let param_set = if source_params == Some(owner) {
        collect_source_generic_params(db, owner)
    } else {
        collect_generic_params(db, owner)
    };
    let params = owner.params(db);
    for (idx, GenericParamView { param, .. }) in params.enumerate() {
        let GenericParam::Type(hir_param) = param else {
            continue;
        };
        let Some(ty) = param_set.param_by_original_idx(db, idx) else {
            continue;
        };
        for (bound_idx, bound) in hir_param.bounds.iter().enumerate() {
            if let TypeBound::Trait(trait_ref) = bound
                && !crate::analysis::ty::ty_lower::is_core_constraint_bound(
                    db,
                    owner_scope,
                    *trait_ref,
                )
            {
                deferred.push(Deferred {
                    bound_ty: Either::Right(ty),
                    trait_ref: *trait_ref,
                    source: PredicateSource::GenericParamBound {
                        owner,
                        param_idx: idx,
                        bound_idx,
                    },
                });
            }
        }
    }

    // Where-clause predicates (directly over HIR)
    //
    // We intentionally operate on the raw where-clause HIR here rather than the
    // semantic view helpers to avoid re-entering constraint collection via
    // `constraints_for`. This keeps the fixed-point iteration semantics
    // identical to the legacy implementation while the semantic layer remains
    // the main traversal API for other callers.
    if let Some(w_owner) = owner.where_clause_owner() {
        let where_clause = w_owner.where_clause(db);
        for (pred_idx, pred) in where_clause.type_predicates(db) {
            let Some(hir_ty) = pred.ty.to_opt() else {
                continue;
            };

            // Filter out super-trait constraints on `Self`; those are handled
            // in `collect_super_traits`.
            if hir_ty.is_self_ty(db) && matches!(owner, GenericParamOwner::Trait(_)) {
                continue;
            }

            for (bound_idx, bound) in pred.bounds.iter().enumerate() {
                if let TypeBound::Trait(trait_ref) = *bound
                    && !crate::analysis::ty::ty_lower::is_core_constraint_bound(
                        db,
                        owner_scope,
                        trait_ref,
                    )
                {
                    deferred.push(Deferred {
                        bound_ty: Either::Left(hir_ty),
                        trait_ref,
                        source: PredicateSource::WherePredicateBound {
                            owner: w_owner,
                            pred_idx,
                            bound_idx,
                        },
                    });
                }
            }
        }
    }

    let mut all_predicates: IndexMap<TraitInstId<'db>, PredicateSource<'db>> =
        initial.iter().copied().collect();

    // fixed-point iteration over deferred predicates
    while !deferred.is_empty() {
        // This iteration reads the prior partial stage, never its completed output.
        let context = super::ImplEnv::new_with_selections(
            db,
            owner_scope,
            PredicateListId::new(db, all_predicates.keys().copied().collect::<Vec<_>>()),
            Vec::new(),
            Vec::new(),
        );

        let before = deferred.len();
        deferred.retain(|p| {
            match try_resolve_type_bound(db, p, context, const_bodies, source_params) {
                Some(inst) => {
                    all_predicates.entry(inst).or_insert(p.source);
                    false
                }
                None => true,
            }
        });
        if deferred.len() == before {
            break;
        }
    }

    all_predicates.into_iter().collect()
}

struct Deferred<'db> {
    bound_ty: Either<HirTypeId<'db>, TyId<'db>>,
    trait_ref: TraitRefId<'db>,
    source: PredicateSource<'db>,
}

fn try_resolve_type_bound<'db>(
    db: &'db dyn HirAnalysisDb,
    deferred: &Deferred<'db>,
    context: super::ImplEnv<'db>,
    const_bodies: ConstBodyLowering,
    source_params: Option<GenericParamOwner<'db>>,
) -> Option<TraitInstId<'db>> {
    let premises = context.premises(db);
    let scope = premises.scope();
    let assumptions = premises.assumptions();
    let ty = match deferred.bound_ty {
        Either::Left(hir_ty) => {
            let ty = match const_bodies {
                ConstBodyLowering::Eager => lower_hir_ty(db, hir_ty, scope, assumptions),
                ConstBodyLowering::Deferred => {
                    let minter = LoweringContext::deferred(HoleAnchor::TemplateTy {
                        ty: hir_ty,
                        scope,
                        assumptions,
                    })
                    .with_source_params(source_params);
                    lower_hir_ty_with_minter(db, hir_ty, premises, &minter)
                }
            };
            if ty.has_invalid(db) {
                return None;
            }
            ty
        }
        Either::Right(ty) => ty,
    };

    match const_bodies {
        ConstBodyLowering::Eager => lower_trait_ref(
            db,
            ty,
            deferred.trait_ref,
            scope,
            assumptions,
            enclosing_trait_self_ty(db, scope),
        ),
        ConstBodyLowering::Deferred => {
            let minter = LoweringContext::deferred(HoleAnchor::TemplatePath {
                path: deferred.trait_ref.path(db).to_opt()?,
                scope,
                assumptions,
            })
            .with_source_params(source_params);
            lower_trait_ref_with_minter(
                db,
                ty,
                deferred.trait_ref,
                scope,
                assumptions,
                enclosing_trait_self_ty(db, scope),
                &minter,
            )
        }
    }
    .ok()
}
