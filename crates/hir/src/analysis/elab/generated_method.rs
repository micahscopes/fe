use common::indexmap::{IndexMap, IndexSet};

use crate::{
    analysis::{
        HirAnalysisDb,
        ty::{
            binder::Binder,
            constraint::{ConstraintId, ConstraintKind},
            generated::{
                GeneratedExprId, GeneratedExprKind, GeneratedImplId, GeneratedMethodBodyKind,
                GeneratedStructFieldInitListId,
            },
            trait_def::TraitInstId,
            trait_resolution::PredicateListId,
            ty_def::{TyData, TyId},
            ty_lower::ConstDefaultCompletion,
        },
    },
    core::semantic::constraints_for,
    hir_def::{Func, IdentId},
};

use super::{ElaborationCtfeContextId, reflect::reflect_struct_fields, tys_match};

pub(super) fn generated_missing_required_methods<'db>(
    db: &'db dyn HirAnalysisDb,
    generated: GeneratedImplId<'db>,
) -> Vec<IdentId<'db>> {
    let provided = generated
        .methods
        .list(db)
        .iter()
        .map(|method| method.name)
        .collect::<IndexSet<_>>();
    required_method_names(db, ConstraintId::from_trait(db, generated.trait_inst))
        .into_iter()
        .filter(|name| !provided.contains(name))
        .collect()
}

pub(super) fn generated_unsupported_required_methods<'db>(
    db: &'db dyn HirAnalysisDb,
    generated: GeneratedImplId<'db>,
) -> Vec<IdentId<'db>> {
    let required = required_methods(db, ConstraintId::from_trait(db, generated.trait_inst));
    generated
        .methods
        .list(db)
        .iter()
        .filter_map(|method| {
            let required_method = required.get(&method.name)?;
            match method.body {
                GeneratedMethodBodyKind::Expr(expr) => {
                    let cx = GeneratedMethodValidationContext {
                        generated,
                        required_method: *required_method,
                    };
                    let expected = generated_required_method_return_ty(db, cx);
                    (!generated_expr_ty_matches(db, expr, expected, cx)).then_some(method.name)
                }
            }
        })
        .collect()
}

#[derive(Clone, Copy)]
struct GeneratedMethodValidationContext<'db> {
    generated: GeneratedImplId<'db>,
    required_method: Func<'db>,
}

fn generated_required_method_return_ty<'db>(
    db: &'db dyn HirAnalysisDb,
    cx: GeneratedMethodValidationContext<'db>,
) -> TyId<'db> {
    instantiate_required_method_ty(db, cx, cx.required_method.return_ty(db))
}

fn generated_required_method_param_ty<'db>(
    db: &'db dyn HirAnalysisDb,
    cx: GeneratedMethodValidationContext<'db>,
    name: IdentId<'db>,
) -> Option<TyId<'db>> {
    required_method_arg_ty_for_trait_inst(
        db,
        cx.generated.trait_inst,
        cx.required_method,
        name,
        generated_method_default_assumptions(db, cx.generated.context),
    )
}

pub(super) fn required_method_arg_ty_for_trait_inst<'db>(
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

fn generated_method_target_ty<'db>(
    db: &'db dyn HirAnalysisDb,
    cx: GeneratedMethodValidationContext<'db>,
) -> TyId<'db> {
    cx.generated.context.request(db).target(db).ty(db)
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

fn instantiate_required_method_ty<'db>(
    db: &'db dyn HirAnalysisDb,
    cx: GeneratedMethodValidationContext<'db>,
    ty: TyId<'db>,
) -> TyId<'db> {
    instantiate_required_method_ty_for_trait_inst(
        db,
        cx.generated.trait_inst,
        cx.required_method,
        ty,
        generated_method_default_assumptions(db, cx.generated.context),
    )
}

fn instantiate_required_method_ty_for_trait_inst<'db>(
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
    let method = required_method.as_callable(db).unwrap();
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

pub(super) fn generated_method_default_assumptions<'db>(
    db: &'db dyn HirAnalysisDb,
    context: ElaborationCtfeContextId<'db>,
) -> PredicateListId<'db> {
    let mut assumptions = IndexSet::new();

    // Defaulted trait arguments for generated methods are completed in the
    // provider/request environment, not in an empty world. Trait-owned projection
    // defaults still need a cycle-safe ParamEnv path before we include the required
    // method's own assumptions here.
    assumptions.extend(
        context
            .provider(db)
            .func(db)
            .assumptions(db)
            .list(db)
            .iter()
            .copied(),
    );
    assumptions.extend(
        constraints_for(db, context.request(db).target(db).item())
            .list(db)
            .iter()
            .copied(),
    );

    PredicateListId::new(db, assumptions.into_iter().collect::<Vec<_>>())
}

fn generated_expr_ty_matches<'db>(
    db: &'db dyn HirAnalysisDb,
    expr: GeneratedExprId<'db>,
    expected: TyId<'db>,
    cx: GeneratedMethodValidationContext<'db>,
) -> bool {
    generated_expr_static_ty(db, expr, cx).is_some_and(|ty| tys_match(db, ty, expected))
}

fn generated_expr_static_ty<'db>(
    db: &'db dyn HirAnalysisDb,
    expr: GeneratedExprId<'db>,
    cx: GeneratedMethodValidationContext<'db>,
) -> Option<TyId<'db>> {
    match expr.kind(db) {
        GeneratedExprKind::BoolLiteral(_) => Some(TyId::bool(db)),
        GeneratedExprKind::BoolAnd { lhs, rhs } => {
            if generated_expr_ty_matches(db, lhs, TyId::bool(db), cx)
                && generated_expr_ty_matches(db, rhs, TyId::bool(db), cx)
            {
                Some(TyId::bool(db))
            } else {
                None
            }
        }
        GeneratedExprKind::SelfRef { ty } => {
            if !cx.required_method.is_method(db) {
                return None;
            }
            let self_ty = generated_method_target_ty(db, cx);
            tys_match(db, self_ty, ty).then_some(ty)
        }
        GeneratedExprKind::MethodArgRef { name, ty } => {
            let arg_ty = generated_required_method_param_ty(db, cx, name)?;
            tys_match(db, arg_ty, ty).then_some(ty)
        }
        GeneratedExprKind::FieldGet { base, field } => {
            let base_ty = generated_expr_static_ty(db, base, cx)?;
            let base_ty = field_access_base_ty(db, base_ty);
            tys_match(db, base_ty, field.parent).then_some(field.ty)
        }
        GeneratedExprKind::EqExpr { lhs, rhs } => {
            let lhs_ty = generated_expr_static_ty(db, lhs, cx)?;
            let rhs_ty = generated_expr_static_ty(db, rhs, cx)?;
            tys_match(db, lhs_ty, rhs_ty).then_some(TyId::bool(db))
        }
        GeneratedExprKind::DefaultCall { ty } => Some(ty),
        GeneratedExprKind::StructInit { target, fields } => {
            generated_struct_init_ty(db, target, fields, cx)
        }
    }
}

fn field_access_base_ty<'db>(db: &'db dyn HirAnalysisDb, ty: TyId<'db>) -> TyId<'db> {
    if let Some(inner) = ty.as_view(db) {
        return field_access_base_ty(db, inner);
    }
    if let Some((_, inner)) = ty.as_capability(db) {
        return field_access_base_ty(db, inner);
    }
    ty
}

fn generated_struct_init_ty<'db>(
    db: &'db dyn HirAnalysisDb,
    target: TyId<'db>,
    fields: GeneratedStructFieldInitListId<'db>,
    cx: GeneratedMethodValidationContext<'db>,
) -> Option<TyId<'db>> {
    let expected_fields = reflect_struct_fields(db, target);
    let field_inits = fields.list(db);
    if expected_fields.len() != field_inits.len() {
        return None;
    }

    for expected in expected_fields {
        let init = field_inits
            .iter()
            .find(|init| init.field.index == expected.index)?;
        if !tys_match(db, init.field.parent, target) || !tys_match(db, init.field.ty, expected.ty) {
            return None;
        }
        let value_ty = generated_expr_static_ty(db, init.value, cx)?;
        if !tys_match(db, value_ty, expected.ty) {
            return None;
        }
    }

    Some(target)
}

pub(super) fn generated_method_error_summary<'db>(
    db: &'db dyn HirAnalysisDb,
    missing_methods: &[IdentId<'db>],
    unsupported_methods: &[IdentId<'db>],
) -> String {
    let mut parts = Vec::new();
    if !missing_methods.is_empty() {
        parts.push(format!(
            "missing {}",
            missing_methods
                .iter()
                .map(|name| name.data(db).to_string())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    if !unsupported_methods.is_empty() {
        parts.push(format!(
            "unsupported {}",
            unsupported_methods
                .iter()
                .map(|name| name.data(db).to_string())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    parts.join("; ")
}

pub(super) fn required_method_names<'db>(
    db: &'db dyn HirAnalysisDb,
    goal: ConstraintId<'db>,
) -> Vec<IdentId<'db>> {
    required_methods(db, goal).keys().copied().collect()
}

pub(super) fn required_methods<'db>(
    db: &'db dyn HirAnalysisDb,
    goal: ConstraintId<'db>,
) -> IndexMap<IdentId<'db>, Func<'db>> {
    let ConstraintKind::Trait(trait_inst) = goal.kind(db) else {
        return IndexMap::new();
    };
    trait_inst
        .def(db)
        .method_defs(db)
        .into_iter()
        .filter(|(_, method)| method.body(db).is_none())
        .collect()
}
