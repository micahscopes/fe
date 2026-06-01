use common::indexmap::{IndexMap, IndexSet};

use crate::{
    analysis::{
        HirAnalysisDb,
        ty::{
            constraint::{ConstraintId, ConstraintKind},
            generated::{
                GeneratedExprId, GeneratedExprKind, GeneratedImplId, GeneratedMethodBodyKind,
                GeneratedStructFieldInitListId,
            },
            method_conformance::{
                instantiate_required_method_ty_for_trait_inst, missing_required_method_names,
                required_method_arg_ty_for_trait_inst, required_trait_methods,
            },
            trait_resolution::PredicateListId,
            ty_def::TyId,
        },
    },
    core::semantic::constraints_for,
    hir_def::{Func, IdentId},
    span::DynLazySpan,
};

use super::{ElaborationCtfeContextId, reflect::reflect_struct_fields, tys_match};

pub(super) fn generated_missing_required_methods<'db>(
    db: &'db dyn HirAnalysisDb,
    generated: GeneratedImplId<'db>,
) -> Vec<IdentId<'db>> {
    missing_required_method_names(
        db,
        generated.trait_inst.def(db),
        generated.methods.list(db).iter().map(|method| method.name),
    )
}

pub(super) fn generated_invalid_required_method_bodies<'db>(
    db: &'db dyn HirAnalysisDb,
    generated: GeneratedImplId<'db>,
) -> Vec<GeneratedInvalidMethodBody<'db>> {
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
                    match generated_expr_static_ty_result(db, expr, cx) {
                        Ok(actual) if tys_match(db, actual, expected) => None,
                        Ok(actual) => Some(GeneratedInvalidMethodBody {
                            name: method.name,
                            expected,
                            actual: Some(actual),
                            span: expr.span(db).clone(),
                        }),
                        Err(span) => Some(GeneratedInvalidMethodBody {
                            name: method.name,
                            expected,
                            actual: None,
                            span,
                        }),
                    }
                }
            }
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct GeneratedInvalidMethodBody<'db> {
    pub(super) name: IdentId<'db>,
    expected: TyId<'db>,
    actual: Option<TyId<'db>>,
    pub(super) span: DynLazySpan<'db>,
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

fn generated_method_target_ty<'db>(
    db: &'db dyn HirAnalysisDb,
    cx: GeneratedMethodValidationContext<'db>,
) -> TyId<'db> {
    cx.generated.context.request(db).target(db).ty(db)
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

fn generated_expr_static_ty_result<'db>(
    db: &'db dyn HirAnalysisDb,
    expr: GeneratedExprId<'db>,
    cx: GeneratedMethodValidationContext<'db>,
) -> Result<TyId<'db>, DynLazySpan<'db>> {
    match expr.kind(db) {
        GeneratedExprKind::BoolLiteral(_) => Ok(TyId::bool(db)),
        GeneratedExprKind::BoolAnd { lhs, rhs } => {
            let lhs_ty = generated_expr_static_ty_result(db, lhs, cx)?;
            if !tys_match(db, lhs_ty, TyId::bool(db)) {
                return Err(lhs.span(db).clone());
            }
            let rhs_ty = generated_expr_static_ty_result(db, rhs, cx)?;
            if !tys_match(db, rhs_ty, TyId::bool(db)) {
                return Err(rhs.span(db).clone());
            }
            Ok(TyId::bool(db))
        }
        GeneratedExprKind::SelfRef { ty } => {
            if !cx.required_method.is_method(db) {
                return Err(expr.span(db).clone());
            }
            let self_ty = generated_method_target_ty(db, cx);
            if tys_match(db, self_ty, ty) {
                Ok(ty)
            } else {
                Err(expr.span(db).clone())
            }
        }
        GeneratedExprKind::MethodArgRef { name } => {
            generated_required_method_param_ty(db, cx, name).ok_or_else(|| expr.span(db).clone())
        }
        GeneratedExprKind::FieldGet { base, field } => {
            let base_ty = generated_expr_static_ty_result(db, base, cx)?;
            let base_ty = generated_field_access_receiver_ty(db, base_ty);
            if tys_match(db, base_ty, field.parent) {
                Ok(field.ty)
            } else {
                Err(base.span(db).clone())
            }
        }
        GeneratedExprKind::EqExpr { lhs, rhs } => {
            let lhs_ty = generated_expr_static_ty_result(db, lhs, cx)?;
            let rhs_ty = generated_expr_static_ty_result(db, rhs, cx)?;
            if tys_match(db, lhs_ty, rhs_ty) {
                Ok(TyId::bool(db))
            } else {
                Err(rhs.span(db).clone())
            }
        }
        GeneratedExprKind::DefaultCall { ty } => Ok(ty),
        GeneratedExprKind::StructInit { target, fields } => {
            generated_struct_init_ty(db, expr, target, fields, cx)
        }
    }
}

fn generated_field_access_receiver_ty<'db>(db: &'db dyn HirAnalysisDb, ty: TyId<'db>) -> TyId<'db> {
    // Generated field access is checked against the nominal reflected parent.
    // Receiver wrappers are access modes for the generated expression, not a
    // different field owner.
    if let Some(inner) = ty.as_view(db) {
        return generated_field_access_receiver_ty(db, inner);
    }
    if let Some((_, inner)) = ty.as_capability(db) {
        return generated_field_access_receiver_ty(db, inner);
    }
    ty
}

fn generated_struct_init_ty<'db>(
    db: &'db dyn HirAnalysisDb,
    expr: GeneratedExprId<'db>,
    target: TyId<'db>,
    fields: GeneratedStructFieldInitListId<'db>,
    cx: GeneratedMethodValidationContext<'db>,
) -> Result<TyId<'db>, DynLazySpan<'db>> {
    let expected_fields = reflect_struct_fields(db, target);
    let field_inits = fields.list(db);
    if expected_fields.len() != field_inits.len() {
        return Err(expr.span(db).clone());
    }

    for expected in expected_fields {
        let init = field_inits
            .iter()
            .find(|init| init.field.index == expected.index)
            .ok_or_else(|| expr.span(db).clone())?;
        if !tys_match(db, init.field.parent, target) || !tys_match(db, init.field.ty, expected.ty) {
            return Err(expr.span(db).clone());
        }
        let value_ty = generated_expr_static_ty_result(db, init.value, cx)?;
        if !tys_match(db, value_ty, expected.ty) {
            return Err(init.value.span(db).clone());
        }
    }

    Ok(target)
}

pub(super) fn generated_method_error_summary<'db>(
    db: &'db dyn HirAnalysisDb,
    missing_methods: &[IdentId<'db>],
    invalid_body_methods: &[GeneratedInvalidMethodBody<'db>],
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
    if !invalid_body_methods.is_empty() {
        parts.push(format!(
            "invalid generated body for {}",
            invalid_body_methods
                .iter()
                .map(|method| method.pretty_print(db))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    parts.join("; ")
}

impl<'db> GeneratedInvalidMethodBody<'db> {
    fn pretty_print(&self, db: &'db dyn HirAnalysisDb) -> String {
        let actual = self
            .actual
            .map(|ty| ty.pretty_print(db).to_string())
            .unwrap_or_else(|| "<unavailable>".to_string());
        format!(
            "{} (expected {}, got {})",
            self.name.data(db),
            self.expected.pretty_print(db),
            actual
        )
    }
}

pub(super) fn required_methods<'db>(
    db: &'db dyn HirAnalysisDb,
    goal: ConstraintId<'db>,
) -> IndexMap<IdentId<'db>, Func<'db>> {
    let ConstraintKind::Trait(trait_inst) = goal.kind(db) else {
        return IndexMap::new();
    };
    required_trait_methods(db, trait_inst.def(db))
}
