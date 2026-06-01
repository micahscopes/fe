use common::indexmap::{IndexMap, IndexSet};

use crate::{
    analysis::{
        HirAnalysisDb,
        ty::{
            binder::Binder,
            trait_def::TraitInstId,
            trait_resolution::PredicateListId,
            ty_def::{TyData, TyId},
            ty_lower::ConstDefaultCompletion,
        },
    },
    hir_def::{Func, IdentId, Trait},
};

pub(crate) fn required_trait_methods<'db>(
    db: &'db dyn HirAnalysisDb,
    trait_: Trait<'db>,
) -> IndexMap<IdentId<'db>, Func<'db>> {
    trait_methods(db, trait_)
        .into_iter()
        .filter(|(_, method)| method.body(db).is_none())
        .collect()
}

pub(crate) fn trait_methods<'db>(
    db: &'db dyn HirAnalysisDb,
    trait_: Trait<'db>,
) -> IndexMap<IdentId<'db>, Func<'db>> {
    trait_.method_defs(db).into_iter().collect()
}

pub(crate) fn missing_required_method_names<'db>(
    db: &'db dyn HirAnalysisDb,
    trait_: Trait<'db>,
    provided: impl IntoIterator<Item = IdentId<'db>>,
) -> Vec<IdentId<'db>> {
    let provided = provided.into_iter().collect::<IndexSet<_>>();
    required_trait_methods(db, trait_)
        .keys()
        .copied()
        .filter(|name| !provided.contains(name))
        .collect()
}

pub(crate) fn required_method_arg_ty_for_trait_inst<'db>(
    db: &'db dyn HirAnalysisDb,
    trait_inst: TraitInstId<'db>,
    required_method: Func<'db>,
    name: IdentId<'db>,
    assumptions: PredicateListId<'db>,
) -> Option<TyId<'db>> {
    let method = required_method.as_callable(db)?;
    let arg_tys = method.arg_tys(db);
    let trait_args = trait_inst_args_with_defaults(db, trait_inst, assumptions);

    for (idx, param) in required_method.params(db).enumerate() {
        if param.is_self_param(db) {
            continue;
        }
        if param.name(db).is_some_and(|param_name| param_name == name) {
            let ty = arg_tys.get(idx).copied()?;
            let ty = ty.instantiate_identity();
            if ty_is_named_self(db, ty)
                && let Some(&target_ty) = trait_args.first()
            {
                return Some(target_ty);
            }
            return Some(instantiate_required_method_ty_for_trait_inst(
                db,
                trait_inst,
                required_method,
                ty,
                assumptions,
            ));
        }
    }

    None
}

pub(crate) fn instantiate_required_method_ty_for_trait_inst<'db>(
    db: &'db dyn HirAnalysisDb,
    trait_inst: TraitInstId<'db>,
    required_method: Func<'db>,
    ty: TyId<'db>,
    assumptions: PredicateListId<'db>,
) -> TyId<'db> {
    let trait_args = trait_inst_args_with_defaults(db, trait_inst, assumptions);
    if ty_is_named_self(db, ty)
        && let Some(&target_ty) = trait_args.first()
    {
        return target_ty;
    }

    let mut mappings = Vec::new();
    let Some(method) = required_method.as_callable(db) else {
        return ty;
    };
    if let Some(self_ty) = required_method.expected_self_ty(db)
        && let Some(&target_ty) = trait_args.first()
    {
        mappings.push((self_ty, target_ty));
    }
    for (idx, &arg) in trait_args.iter().enumerate() {
        if let Some(&method_param) = method.params(db).get(idx) {
            mappings.push((method_param, arg));
        }
        if let Some(&trait_param) = trait_inst.def(db).params(db).get(idx) {
            mappings.push((trait_param, arg));
        }
    }
    Binder::bind(ty).instantiate_with(db, |ty| {
        mappings
            .iter()
            .find_map(|(param, arg)| (*param == ty).then_some(*arg))
            .unwrap_or(ty)
    })
}

fn trait_inst_args_with_defaults<'db>(
    db: &'db dyn HirAnalysisDb,
    trait_inst: TraitInstId<'db>,
    assumptions: PredicateListId<'db>,
) -> Vec<TyId<'db>> {
    let args = trait_inst.args(db);
    if args.len() >= trait_inst.def(db).params(db).len() {
        return args.to_vec();
    }
    let Some((&self_ty, provided_explicit)) = args.split_first() else {
        return Vec::new();
    };
    let completed = trait_inst.def(db).param_set(db).complete_explicit_args(
        db,
        Some(self_ty),
        provided_explicit,
        assumptions,
        ConstDefaultCompletion::evaluate(None),
    );
    let mut full_args = Vec::with_capacity(1 + completed.len());
    full_args.push(self_ty);
    full_args.extend(completed);
    full_args
}

fn ty_is_named_self<'db>(db: &'db dyn HirAnalysisDb, ty: TyId<'db>) -> bool {
    if let Some(inner) = ty.as_view(db) {
        return ty_is_named_self(db, inner);
    }
    if let Some((_, inner)) = ty.as_capability(db) {
        return ty_is_named_self(db, inner);
    }
    matches!(
        ty.base_ty(db).data(db),
        TyData::TyParam(param) if param.name.is_self_ty(db)
    )
}
