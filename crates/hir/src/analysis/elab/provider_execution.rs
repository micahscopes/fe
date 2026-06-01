use crate::{
    analysis::{
        HirAnalysisDb,
        name_resolution::{PathRes, resolve_path},
        ty::{
            constraint::{
                ConstraintApplicationId, ConstraintHeadId, ConstraintHeadKind, ConstraintId,
                ConstraintKind,
            },
            derive_provider::DeriveProviderId,
            generated::{
                GeneratedExprId, GeneratedExprKind, GeneratedStructFieldInit,
                GeneratedStructFieldInitListId,
            },
            trait_def::TraitInstId,
            trait_resolution::PredicateListId,
            ty_def::{Kind, TyId},
        },
    },
    hir_def::{
        Body, Expr, GenericArg, GenericArgListId, IdentId, LitKind, Partial, Pat, Stmt,
        Trait as TraitDef, TypeKind,
    },
    span::DynLazySpan,
};

use super::{
    BuilderError, CapabilityEnv, ElaborationCtfeContextId, ImplBuilderSession,
    ProviderFailureReason, ProviderOutputId, ProviderOutputStatus, ReflectedField,
    ReflectedVariant, RequirementOrigin,
    generated_method::trait_methods_for_goal,
    reflect::{reflect_enum_variants, reflect_struct_fields, reflect_variant_fields},
};

#[salsa::tracked]
pub(super) fn provider_output_for_context<'db>(
    db: &'db dyn HirAnalysisDb,
    context: ElaborationCtfeContextId<'db>,
) -> ProviderOutputId<'db> {
    let request = context.request(db);
    let provider = context.provider(db);
    let goal = request.goal(db);
    let env = CapabilityEnv::from_context(db, context);
    if !env.has_impl_builder(db, goal) {
        return ProviderOutputId::new(
            db,
            request,
            provider,
            context,
            failed_status(
                ProviderFailureReason::MissingBuilderCapability,
                provider.func(db).span().effects().into(),
            ),
        );
    }

    let status = execute_provider_body(db, context, env);
    ProviderOutputId::new(db, request, provider, context, status)
}

struct ProviderExecutionError<'db> {
    reason: ProviderFailureReason,
    span: DynLazySpan<'db>,
}

fn failed_status<'db>(
    reason: ProviderFailureReason,
    span: DynLazySpan<'db>,
) -> ProviderOutputStatus<'db> {
    ProviderOutputStatus::Failed { reason, span }
}

fn failed_execution<'db>(
    reason: ProviderFailureReason,
    span: DynLazySpan<'db>,
) -> ProviderExecutionError<'db> {
    ProviderExecutionError { reason, span }
}

#[derive(Clone, Copy)]
enum ElabValue<'db> {
    Field(ReflectedField<'db>),
    Variant(ReflectedVariant<'db>),

    /// Internal provider-CTFE type witness produced by reflection operations
    /// such as `field.ty()`.
    ///
    /// This is not a public runtime value. If type witnesses become part of
    /// Fe's surface language, they should be modeled as explicit
    /// compile-time-only values instead of exposing raw `TyId`.
    TypeWitness(TyId<'db>),
    GeneratedExpr(GeneratedExprId<'db>),
}

#[derive(Clone, Copy)]
enum BuilderRequirementHead<'db> {
    ConcreteTrait(TraitDef<'db>),
    GenericConstraint(TyId<'db>),
}

impl<'db> BuilderRequirementHead<'db> {
    fn apply(self, db: &'db dyn HirAnalysisDb, arg: TyId<'db>) -> ConstraintId<'db> {
        match self {
            Self::ConcreteTrait(trait_) => {
                ConstraintId::from_trait(db, TraitInstId::new_simple(db, trait_, vec![arg]))
            }
            Self::GenericConstraint(head_ty) => {
                let head = ConstraintHeadId::new(db, ConstraintHeadKind::GenericParam(head_ty));
                let application = ConstraintApplicationId::new(db, head, vec![arg]);
                ConstraintId::new(db, ConstraintKind::ConstraintApplication(application))
            }
        }
    }
}

struct ProviderBodyExecutor<'db> {
    db: &'db dyn HirAnalysisDb,
    context: ElaborationCtfeContextId<'db>,
    env: CapabilityEnv<'db>,
    builder: ImplBuilderSession<'db>,
    builder_names: Vec<IdentId<'db>>,
    reflect_names: Vec<IdentId<'db>>,
    field_bindings: Vec<(IdentId<'db>, ReflectedField<'db>)>,
    variant_bindings: Vec<(IdentId<'db>, ReflectedVariant<'db>)>,
    value_bindings: Vec<(IdentId<'db>, ElabValue<'db>)>,
}

impl<'db> ProviderBodyExecutor<'db> {
    fn new(
        db: &'db dyn HirAnalysisDb,
        context: ElaborationCtfeContextId<'db>,
        env: CapabilityEnv<'db>,
    ) -> Self {
        let provider = context.provider(db);
        Self {
            db,
            context,
            env,
            builder: ImplBuilderSession::new(db, context),
            builder_names: provider_impl_builder_effect_names(db, provider),
            reflect_names: provider_reflect_effect_names(db, provider),
            field_bindings: Vec::new(),
            variant_bindings: Vec::new(),
            value_bindings: Vec::new(),
        }
    }

    fn execute_body(mut self) -> ProviderOutputStatus<'db> {
        let request = self.context.request(self.db);
        let provider = self.context.provider(self.db);
        if self
            .builder
            .emit_impl(self.db, request.goal(self.db))
            .is_err()
        {
            return failed_status(
                ProviderFailureReason::InvalidBuilderState,
                provider.func(self.db).span().name().into(),
            );
        }

        let Some(body) = provider.func(self.db).body(self.db) else {
            return failed_status(
                ProviderFailureReason::MissingFinish,
                provider.func(self.db).span().name().into(),
            );
        };

        match self.execute_expr(body, body.expr(self.db)) {
            Ok(()) => match self.builder.into_commands(self.db) {
                Ok(commands) => ProviderOutputStatus::Succeeded { commands },
                Err(BuilderError::NotFinished) => failed_status(
                    ProviderFailureReason::MissingFinish,
                    body.expr(self.db).span(body).into(),
                ),
                Err(_) => failed_status(
                    ProviderFailureReason::InvalidBuilderState,
                    body.expr(self.db).span(body).into(),
                ),
            },
            Err(err) => failed_status(err.reason, err.span),
        }
    }

    fn execute_stmt(
        &mut self,
        body: Body<'db>,
        stmt: crate::hir_def::StmtId,
    ) -> Result<(), ProviderExecutionError<'db>> {
        let Partial::Present(stmt_data) = stmt.data(self.db, body) else {
            return Ok(());
        };
        match stmt_data {
            Stmt::Let(_, _, init) => {
                if let Some(init) = init {
                    if let Stmt::Let(pat, _, _) = stmt_data
                        && let Some(binding) = simple_pat_binding_name(self.db, body, *pat)
                        && let Some(value) = self.eval_expr_value(body, *init)
                    {
                        self.value_bindings.push((binding, value));
                        return Ok(());
                    }
                    self.execute_expr(body, *init)?;
                }
            }
            Stmt::For(pat, iterable, loop_body, _) => {
                let Some(binding) = simple_pat_binding_name(self.db, body, *pat) else {
                    return Err(failed_execution(
                        ProviderFailureReason::UnsupportedProviderBody,
                        stmt.span(body).into(),
                    ));
                };
                match self.provider_iterable(body, *iterable)? {
                    Some(ProviderIterable::Fields(fields)) => {
                        for field in fields {
                            self.field_bindings.push((binding, field));
                            self.execute_expr(body, *loop_body)?;
                            self.field_bindings.pop();
                        }
                    }
                    Some(ProviderIterable::Variants(variants)) => {
                        for variant in variants {
                            self.variant_bindings.push((binding, variant));
                            self.execute_expr(body, *loop_body)?;
                            self.variant_bindings.pop();
                        }
                    }
                    None => {
                        return Err(failed_execution(
                            ProviderFailureReason::UnsupportedProviderBody,
                            (*iterable).span(body).into(),
                        ));
                    }
                }
            }
            Stmt::While(_, _) => {
                return Err(failed_execution(
                    ProviderFailureReason::UnsupportedProviderBody,
                    stmt.span(body).into(),
                ));
            }
            Stmt::Return(expr) => {
                if let Some(expr) = expr {
                    self.execute_expr(body, *expr)?;
                }
            }
            Stmt::Expr(expr) => self.execute_expr(body, *expr)?,
            Stmt::Continue | Stmt::Break => {
                return Err(failed_execution(
                    ProviderFailureReason::UnsupportedProviderBody,
                    stmt.span(body).into(),
                ));
            }
        }
        Ok(())
    }

    fn execute_expr(
        &mut self,
        body: Body<'db>,
        expr: crate::hir_def::ExprId,
    ) -> Result<(), ProviderExecutionError<'db>> {
        let Partial::Present(expr_data) = expr.data(self.db, body) else {
            return Ok(());
        };

        match expr_data {
            Expr::Block(stmts) => {
                let old_value_bindings = self.value_bindings.len();
                let result = (|| {
                    for &stmt in stmts {
                        self.execute_stmt(body, stmt)?;
                    }
                    Ok(())
                })();
                self.value_bindings.truncate(old_value_bindings);
                result?;
            }
            Expr::Assign(lhs, rhs) => {
                let Some(value) = self.eval_expr_value(body, *rhs) else {
                    return Err(failed_execution(
                        ProviderFailureReason::UnsupportedProviderBody,
                        (*rhs).span(body).into(),
                    ));
                };
                if !self.assign_value_binding(body, *lhs, value) {
                    return Err(failed_execution(
                        ProviderFailureReason::UnsupportedProviderBody,
                        (*lhs).span(body).into(),
                    ));
                }
            }
            Expr::Call(_, _) => {
                return Err(failed_execution(
                    ProviderFailureReason::UnsupportedProviderBody,
                    expr.span(body).into(),
                ));
            }
            Expr::MethodCall(receiver, method, generic_args, args) => {
                if !self.execute_method_call(body, expr, *receiver, *method, *generic_args, args)? {
                    if self.eval_expr_value(body, expr).is_none() {
                        return Err(failed_execution(
                            ProviderFailureReason::UnsupportedProviderBody,
                            expr.span(body).into(),
                        ));
                    }
                }
            }
            Expr::Bin(_, _, _)
            | Expr::AugAssign(_, _, _)
            | Expr::Un(_, _)
            | Expr::Cast(_, _)
            | Expr::Field(_, _)
            | Expr::Tuple(_)
            | Expr::Array(_)
            | Expr::ArrayRep(_, _)
            | Expr::If(_, _, _)
            | Expr::Match(_, _)
            | Expr::RecordInit(_, _)
            | Expr::With(_, _) => {
                return Err(failed_execution(
                    ProviderFailureReason::UnsupportedProviderBody,
                    expr.span(body).into(),
                ));
            }
            Expr::Lit(_) | Expr::Path(_) => {}
        }
        Ok(())
    }

    fn assign_value_binding(
        &mut self,
        body: Body<'db>,
        lhs: crate::hir_def::ExprId,
        value: ElabValue<'db>,
    ) -> bool {
        let Some(name) = simple_expr_path_ident(self.db, body, lhs) else {
            return false;
        };
        if let Some((_, binding)) = self
            .value_bindings
            .iter_mut()
            .rev()
            .find(|(candidate, _)| *candidate == name)
        {
            *binding = value;
            true
        } else {
            false
        }
    }

    fn execute_method_call(
        &mut self,
        body: Body<'db>,
        expr: crate::hir_def::ExprId,
        receiver: crate::hir_def::ExprId,
        method: Partial<IdentId<'db>>,
        generic_args: GenericArgListId<'db>,
        args: &[crate::hir_def::CallArg<'db>],
    ) -> Result<bool, ProviderExecutionError<'db>> {
        if !expr_is_path_named_any(self.db, body, receiver, &self.builder_names) {
            return Ok(false);
        }
        let Some(method) = method.to_opt() else {
            return Err(failed_execution(
                ProviderFailureReason::UnsupportedProviderBody,
                receiver.span(body).into(),
            ));
        };
        match method.data(self.db).as_str() {
            BUILDER_REQUIRE_METHOD => {
                let [arg] = args else {
                    return Err(failed_execution(
                        ProviderFailureReason::UnsupportedProviderBody,
                        receiver.span(body).into(),
                    ));
                };
                self.execute_require(body, generic_args, arg.expr)?;
                Ok(true)
            }
            BUILDER_FINISH_METHOD => {
                if !args.is_empty() {
                    return Err(failed_execution(
                        ProviderFailureReason::UnsupportedProviderBody,
                        receiver.span(body).into(),
                    ));
                }
                self.builder
                    .finish_explicit(expr.span(body).into())
                    .map_err(|err| match err {
                        BuilderError::AlreadyFinished => failed_execution(
                            ProviderFailureReason::DuplicateFinish,
                            receiver.span(body).into(),
                        ),
                        _ => failed_execution(
                            ProviderFailureReason::InvalidBuilderState,
                            receiver.span(body).into(),
                        ),
                    })?;
                Ok(true)
            }
            BUILDER_BOOL_METHOD
            | BUILDER_AND_METHOD
            | BUILDER_SELF_REF_METHOD
            | BUILDER_ARG_REF_METHOD
            | BUILDER_FIELD_GET_METHOD
            | BUILDER_EQ_METHOD
            | BUILDER_DEFAULT_METHOD
            | BUILDER_STRUCT_INIT_METHOD
            | BUILDER_WITH_FIELD_METHOD => Ok(false),
            BUILDER_EMIT_METHOD => {
                let (method_name, method_name_span, expr_arg) = match args {
                    [name_arg, expr_arg] => {
                        let Some(method_name) = self.string_literal_ident_arg(body, name_arg.expr)
                        else {
                            return Err(failed_execution(
                                ProviderFailureReason::InvalidGeneratedMethodName,
                                name_arg.expr.span(body).into(),
                            ));
                        };
                        (method_name, name_arg.expr.span(body).into(), expr_arg.expr)
                    }
                    _ => {
                        return Err(failed_execution(
                            ProviderFailureReason::UnsupportedProviderBody,
                            receiver.span(body).into(),
                        ));
                    }
                };
                self.execute_emit_method(body, method_name, method_name_span, expr_arg)?;
                Ok(true)
            }
            _ => Err(failed_execution(
                ProviderFailureReason::UnsupportedProviderBody,
                receiver.span(body).into(),
            )),
        }
    }

    fn execute_require(
        &mut self,
        body: Body<'db>,
        generic_args: GenericArgListId<'db>,
        constraint_arg: crate::hir_def::ExprId,
    ) -> Result<(), ProviderExecutionError<'db>> {
        let Some(head) = self.resolve_requirement_head_generic_arg(generic_args) else {
            return Err(failed_execution(
                ProviderFailureReason::InvalidBuilderRequirement,
                constraint_arg.span(body).into(),
            ));
        };
        let Some(ElabValue::TypeWitness(arg_ty)) = self.eval_expr_value(body, constraint_arg)
        else {
            return Err(failed_execution(
                ProviderFailureReason::InvalidBuilderRequirement,
                constraint_arg.span(body).into(),
            ));
        };

        let constraint = head.apply(self.db, arg_ty);
        let origin = self
            .requirement_origin_for_expr(body, constraint_arg)
            .unwrap_or(RequirementOrigin::ProviderCode);
        self.builder
            .require_with_origin(constraint, origin)
            .map_err(|err| match err {
                BuilderError::AlreadyFinished => failed_execution(
                    ProviderFailureReason::CommandAfterFinish,
                    constraint_arg.span(body).into(),
                ),
                _ => failed_execution(
                    ProviderFailureReason::InvalidBuilderState,
                    constraint_arg.span(body).into(),
                ),
            })
    }

    fn execute_emit_method(
        &mut self,
        body: Body<'db>,
        method_name: IdentId<'db>,
        method_name_span: DynLazySpan<'db>,
        expr_arg: crate::hir_def::ExprId,
    ) -> Result<(), ProviderExecutionError<'db>> {
        let trait_methods =
            trait_methods_for_goal(self.db, self.context.request(self.db).goal(self.db));
        if !trait_methods.contains_key(&method_name) {
            return Err(failed_execution(
                ProviderFailureReason::InvalidGeneratedMethodName,
                method_name_span,
            ));
        }
        let Some(ElabValue::GeneratedExpr(expr)) = self.eval_expr_value(body, expr_arg) else {
            return Err(failed_execution(
                ProviderFailureReason::InvalidGeneratedMethodBody,
                expr_arg.span(body).into(),
            ));
        };
        self.builder
            .emit_method_expr(
                method_name,
                method_name_span,
                expr,
                expr_arg.span(body).into(),
            )
            .map_err(|err| match err {
                BuilderError::AlreadyFinished => failed_execution(
                    ProviderFailureReason::CommandAfterFinish,
                    expr_arg.span(body).into(),
                ),
                _ => failed_execution(
                    ProviderFailureReason::InvalidBuilderState,
                    expr_arg.span(body).into(),
                ),
            })
    }

    fn provider_iterable(
        &self,
        body: Body<'db>,
        iterable: crate::hir_def::ExprId,
    ) -> Result<Option<ProviderIterable<'db>>, ProviderExecutionError<'db>> {
        if let Some(fields) = self.reflect_fields_iterable(body, iterable)? {
            return Ok(Some(ProviderIterable::Fields(fields)));
        }
        if let Some(variants) = self.reflect_variants_iterable(body, iterable)? {
            return Ok(Some(ProviderIterable::Variants(variants)));
        }
        if let Some(fields) = self.variant_fields_iterable(body, iterable)? {
            return Ok(Some(ProviderIterable::Fields(fields)));
        }
        Ok(None)
    }

    fn reflect_fields_iterable(
        &self,
        body: Body<'db>,
        iterable: crate::hir_def::ExprId,
    ) -> Result<Option<Vec<ReflectedField<'db>>>, ProviderExecutionError<'db>> {
        let Partial::Present(Expr::MethodCall(receiver, method, _, args)) =
            iterable.data(self.db, body)
        else {
            return Ok(None);
        };
        if method
            .to_opt()
            .is_none_or(|method| method.data(self.db) != REFLECT_FIELDS_METHOD)
        {
            return Ok(None);
        }
        if !args.is_empty() {
            return Ok(None);
        }

        let target_ty = self.context.request(self.db).target(self.db).ty(self.db);
        if !expr_is_path_named_any(self.db, body, *receiver, &self.reflect_names) {
            if expr_is_path_named(self.db, body, *receiver, "reflect") {
                return Err(failed_execution(
                    ProviderFailureReason::MissingReflectCapability,
                    iterable.span(body).into(),
                ));
            }
            return Ok(None);
        }
        if !self.env.has_reflect_target(self.db, target_ty) {
            return Err(failed_execution(
                ProviderFailureReason::MissingReflectCapability,
                iterable.span(body).into(),
            ));
        }
        Ok(Some(reflect_struct_fields(self.db, target_ty)))
    }

    fn reflect_variants_iterable(
        &self,
        body: Body<'db>,
        iterable: crate::hir_def::ExprId,
    ) -> Result<Option<Vec<ReflectedVariant<'db>>>, ProviderExecutionError<'db>> {
        let Partial::Present(Expr::MethodCall(receiver, method, _, args)) =
            iterable.data(self.db, body)
        else {
            return Ok(None);
        };
        if method
            .to_opt()
            .is_none_or(|method| method.data(self.db) != REFLECT_VARIANTS_METHOD)
        {
            return Ok(None);
        }
        if !args.is_empty() {
            return Ok(None);
        }

        let target_ty = self.context.request(self.db).target(self.db).ty(self.db);
        if !expr_is_path_named_any(self.db, body, *receiver, &self.reflect_names) {
            if expr_is_path_named(self.db, body, *receiver, "reflect") {
                return Err(failed_execution(
                    ProviderFailureReason::MissingReflectCapability,
                    iterable.span(body).into(),
                ));
            }
            return Ok(None);
        }
        if !self.env.has_reflect_target(self.db, target_ty) {
            return Err(failed_execution(
                ProviderFailureReason::MissingReflectCapability,
                iterable.span(body).into(),
            ));
        }
        Ok(Some(reflect_enum_variants(self.db, target_ty)))
    }

    fn variant_fields_iterable(
        &self,
        body: Body<'db>,
        iterable: crate::hir_def::ExprId,
    ) -> Result<Option<Vec<ReflectedField<'db>>>, ProviderExecutionError<'db>> {
        let Partial::Present(Expr::MethodCall(receiver, method, _, args)) =
            iterable.data(self.db, body)
        else {
            return Ok(None);
        };
        if method
            .to_opt()
            .is_none_or(|method| method.data(self.db) != VARIANT_FIELDS_METHOD)
        {
            return Ok(None);
        }
        if !args.is_empty() {
            return Ok(None);
        }
        let Some(variant) = self.variant_value_for_expr(body, *receiver) else {
            return Ok(None);
        };
        Ok(Some(reflect_variant_fields(self.db, variant)))
    }

    fn field_value_for_expr(
        &self,
        body: Body<'db>,
        expr: crate::hir_def::ExprId,
    ) -> Option<ReflectedField<'db>> {
        match self.eval_expr_value(body, expr)? {
            ElabValue::Field(field) => Some(field),
            ElabValue::Variant(_) | ElabValue::TypeWitness(_) | ElabValue::GeneratedExpr(_) => None,
        }
    }

    fn variant_value_for_expr(
        &self,
        body: Body<'db>,
        expr: crate::hir_def::ExprId,
    ) -> Option<ReflectedVariant<'db>> {
        match self.eval_expr_value(body, expr)? {
            ElabValue::Variant(variant) => Some(variant),
            ElabValue::Field(_) | ElabValue::TypeWitness(_) | ElabValue::GeneratedExpr(_) => None,
        }
    }

    fn eval_expr_value(
        &self,
        body: Body<'db>,
        expr: crate::hir_def::ExprId,
    ) -> Option<ElabValue<'db>> {
        if let Some(field) = self.field_bindings.iter().rev().find_map(|(name, field)| {
            expr_is_path_named_any(self.db, body, expr, &[*name]).then_some(*field)
        }) {
            return Some(ElabValue::Field(field));
        }
        if let Some(variant) = self
            .variant_bindings
            .iter()
            .rev()
            .find_map(|(name, variant)| {
                expr_is_path_named_any(self.db, body, expr, &[*name]).then_some(*variant)
            })
        {
            return Some(ElabValue::Variant(variant));
        }
        if let Some(value) = self.value_bindings.iter().rev().find_map(|(name, value)| {
            expr_is_path_named_any(self.db, body, expr, &[*name]).then_some(*value)
        }) {
            return Some(value);
        }

        let Partial::Present(Expr::MethodCall(receiver, method, _, args)) =
            expr.data(self.db, body)
        else {
            return None;
        };
        let method = method.to_opt()?;
        match method.data(self.db).as_str() {
            BUILDER_BOOL_METHOD
                if expr_is_path_named_any(self.db, body, *receiver, &self.builder_names) =>
            {
                let [arg] = args.as_slice() else {
                    return None;
                };
                let Partial::Present(Expr::Lit(LitKind::Bool(value))) =
                    arg.expr.data(self.db, body)
                else {
                    return None;
                };
                Some(self.generated_expr_value(body, expr, GeneratedExprKind::BoolLiteral(*value)))
            }
            BUILDER_AND_METHOD
                if expr_is_path_named_any(self.db, body, *receiver, &self.builder_names) =>
            {
                let [lhs_arg, rhs_arg] = args.as_slice() else {
                    return None;
                };
                let ElabValue::GeneratedExpr(lhs) = self.eval_expr_value(body, lhs_arg.expr)?
                else {
                    return None;
                };
                let ElabValue::GeneratedExpr(rhs) = self.eval_expr_value(body, rhs_arg.expr)?
                else {
                    return None;
                };
                Some(self.generated_expr_value(body, expr, GeneratedExprKind::BoolAnd { lhs, rhs }))
            }
            BUILDER_SELF_REF_METHOD
                if expr_is_path_named_any(self.db, body, *receiver, &self.builder_names) =>
            {
                if !args.is_empty() {
                    return None;
                }
                Some(self.generated_expr_value(
                    body,
                    expr,
                    GeneratedExprKind::SelfRef {
                        ty: self.context.request(self.db).target(self.db).ty(self.db),
                    },
                ))
            }
            BUILDER_ARG_REF_METHOD
                if expr_is_path_named_any(self.db, body, *receiver, &self.builder_names) =>
            {
                let [arg] = args.as_slice() else {
                    return None;
                };
                let name = self.string_literal_ident_arg(body, arg.expr)?;
                Some(self.generated_expr_value(
                    body,
                    expr,
                    GeneratedExprKind::MethodArgRef { name },
                ))
            }
            BUILDER_FIELD_GET_METHOD
                if expr_is_path_named_any(self.db, body, *receiver, &self.builder_names) =>
            {
                let [base_arg, field_arg] = args.as_slice() else {
                    return None;
                };
                let ElabValue::GeneratedExpr(base) = self.eval_expr_value(body, base_arg.expr)?
                else {
                    return None;
                };
                let ElabValue::Field(field) = self.eval_expr_value(body, field_arg.expr)? else {
                    return None;
                };
                Some(self.generated_expr_value(
                    body,
                    expr,
                    GeneratedExprKind::FieldGet { base, field },
                ))
            }
            BUILDER_EQ_METHOD
                if expr_is_path_named_any(self.db, body, *receiver, &self.builder_names) =>
            {
                let [lhs_arg, rhs_arg] = args.as_slice() else {
                    return None;
                };
                let ElabValue::GeneratedExpr(lhs) = self.eval_expr_value(body, lhs_arg.expr)?
                else {
                    return None;
                };
                let ElabValue::GeneratedExpr(rhs) = self.eval_expr_value(body, rhs_arg.expr)?
                else {
                    return None;
                };
                Some(self.generated_expr_value(body, expr, GeneratedExprKind::EqExpr { lhs, rhs }))
            }
            BUILDER_DEFAULT_METHOD
                if expr_is_path_named_any(self.db, body, *receiver, &self.builder_names) =>
            {
                let [ty_arg] = args.as_slice() else {
                    return None;
                };
                let ElabValue::TypeWitness(ty) = self.eval_expr_value(body, ty_arg.expr)? else {
                    return None;
                };
                Some(self.generated_expr_value(body, expr, GeneratedExprKind::DefaultCall { ty }))
            }
            BUILDER_STRUCT_INIT_METHOD
                if expr_is_path_named_any(self.db, body, *receiver, &self.builder_names) =>
            {
                if !args.is_empty() {
                    return None;
                }
                let target = self.context.request(self.db).target(self.db).ty(self.db);
                Some(self.generated_expr_value(
                    body,
                    expr,
                    GeneratedExprKind::StructInit {
                        target,
                        fields: GeneratedStructFieldInitListId::new(self.db, Vec::new()),
                    },
                ))
            }
            BUILDER_WITH_FIELD_METHOD
                if expr_is_path_named_any(self.db, body, *receiver, &self.builder_names) =>
            {
                let [init_arg, field_arg, value_arg] = args.as_slice() else {
                    return None;
                };
                let ElabValue::GeneratedExpr(init) = self.eval_expr_value(body, init_arg.expr)?
                else {
                    return None;
                };
                let GeneratedExprKind::StructInit { target, fields } = init.kind(self.db) else {
                    return None;
                };
                let ElabValue::Field(field) = self.eval_expr_value(body, field_arg.expr)? else {
                    return None;
                };
                let ElabValue::GeneratedExpr(value) = self.eval_expr_value(body, value_arg.expr)?
                else {
                    return None;
                };
                let mut field_inits = fields.list(self.db).to_vec();
                field_inits.push(GeneratedStructFieldInit { field, value });
                Some(self.generated_expr_value(
                    body,
                    expr,
                    GeneratedExprKind::StructInit {
                        target,
                        fields: GeneratedStructFieldInitListId::new(self.db, field_inits),
                    },
                ))
            }
            FIELD_TY_METHOD => {
                if !args.is_empty() {
                    return None;
                }
                let ElabValue::Field(field) = self.eval_expr_value(body, *receiver)? else {
                    return None;
                };
                Some(ElabValue::TypeWitness(field.ty))
            }
            _ => None,
        }
    }

    fn generated_expr_value(
        &self,
        body: Body<'db>,
        expr: crate::hir_def::ExprId,
        kind: GeneratedExprKind<'db>,
    ) -> ElabValue<'db> {
        let span: DynLazySpan<'db> = expr.span(body).into();
        ElabValue::GeneratedExpr(GeneratedExprId::new(self.db, kind, span))
    }

    fn requirement_origin_for_expr(
        &self,
        body: Body<'db>,
        expr: crate::hir_def::ExprId,
    ) -> Option<RequirementOrigin<'db>> {
        let Partial::Present(Expr::MethodCall(receiver, method, _, args)) =
            expr.data(self.db, body)
        else {
            return None;
        };
        if !args.is_empty()
            || method
                .to_opt()
                .is_none_or(|method| method.data(self.db) != FIELD_TY_METHOD)
        {
            return None;
        }
        self.field_value_for_expr(body, *receiver)
            .map(RequirementOrigin::ReflectedField)
    }

    fn resolve_requirement_head_generic_arg(
        &self,
        generic_args: GenericArgListId<'db>,
    ) -> Option<BuilderRequirementHead<'db>> {
        let [GenericArg::Type(type_arg)] = generic_args.data(self.db).as_slice() else {
            return None;
        };
        let hir_ty = type_arg.ty.to_opt()?;
        let TypeKind::Path(path) = hir_ty.data(self.db) else {
            return None;
        };
        let path = path.to_opt()?;
        let assumptions = PredicateListId::empty_list(self.db);
        let scope = self.context.provider(self.db).func(self.db).scope();
        match resolve_path(self.db, path, scope, assumptions, false).ok()? {
            PathRes::Trait(inst) => Some(BuilderRequirementHead::ConcreteTrait(inst.def(self.db))),
            PathRes::Ty(ty) if is_unary_constraint_constructor_kind(&ty.kind(self.db)) => {
                Some(BuilderRequirementHead::GenericConstraint(ty))
            }
            _ => None,
        }
    }

    fn string_literal_ident_arg(
        &self,
        body: Body<'db>,
        expr: crate::hir_def::ExprId,
    ) -> Option<IdentId<'db>> {
        let Partial::Present(Expr::Lit(LitKind::String(value))) = expr.data(self.db, body) else {
            return None;
        };
        Some(IdentId::new(self.db, value.data(self.db).to_string()))
    }
}

fn is_unary_constraint_constructor_kind(kind: &Kind) -> bool {
    match kind {
        Kind::Abs(inner) => {
            inner.0.does_match(&Kind::Star) && inner.1.does_match(&Kind::Constraint)
        }
        Kind::Any => true,
        _ => false,
    }
}

fn execute_provider_body<'db>(
    db: &'db dyn HirAnalysisDb,
    context: ElaborationCtfeContextId<'db>,
    env: CapabilityEnv<'db>,
) -> ProviderOutputStatus<'db> {
    ProviderBodyExecutor::new(db, context, env).execute_body()
}

fn provider_impl_builder_effect_names<'db>(
    db: &'db dyn HirAnalysisDb,
    provider: DeriveProviderId<'db>,
) -> Vec<IdentId<'db>> {
    provider
        .func(db)
        .effect_params(db)
        .filter(|param| param.is_mut(db))
        .filter_map(|param| {
            let name = param.name(db)?;
            let key_path = param.key_path(db)?;
            key_path
                .ident(db)
                .to_opt()
                .is_some_and(|ident| ident.data(db) == "ImplBuilder")
                .then_some(name)
        })
        .collect()
}

fn provider_reflect_effect_names<'db>(
    db: &'db dyn HirAnalysisDb,
    provider: DeriveProviderId<'db>,
) -> Vec<IdentId<'db>> {
    provider
        .func(db)
        .effect_params(db)
        .filter_map(|param| {
            let name = param.name(db)?;
            let key_path = param.key_path(db)?;
            key_path
                .ident(db)
                .to_opt()
                .is_some_and(|ident| ident.data(db) == "Reflect")
                .then_some(name)
        })
        .collect()
}

fn expr_is_path_named_any<'db>(
    db: &'db dyn HirAnalysisDb,
    body: Body<'db>,
    expr: crate::hir_def::ExprId,
    names: &[IdentId<'db>],
) -> bool {
    simple_expr_path_ident(db, body, expr).is_some_and(|ident| names.contains(&ident))
}

fn expr_is_path_named<'db>(
    db: &'db dyn HirAnalysisDb,
    body: Body<'db>,
    expr: crate::hir_def::ExprId,
    name: &str,
) -> bool {
    simple_expr_path_ident(db, body, expr).is_some_and(|ident| ident.data(db) == name)
}

fn simple_expr_path_ident<'db>(
    db: &'db dyn HirAnalysisDb,
    body: Body<'db>,
    expr: crate::hir_def::ExprId,
) -> Option<IdentId<'db>> {
    let Partial::Present(Expr::Path(path)) = expr.data(db, body) else {
        return None;
    };
    let Partial::Present(path) = path else {
        return None;
    };
    path.as_ident(db)
}

fn simple_pat_binding_name<'db>(
    db: &'db dyn HirAnalysisDb,
    body: Body<'db>,
    pat: crate::hir_def::PatId,
) -> Option<IdentId<'db>> {
    let Partial::Present(Pat::Path(Partial::Present(path), _)) = pat.data(db, body) else {
        return None;
    };
    path.as_ident(db)
}

const BUILDER_REQUIRE_METHOD: &str = "require";
const BUILDER_FINISH_METHOD: &str = "finish";
const BUILDER_BOOL_METHOD: &str = "bool";
const BUILDER_AND_METHOD: &str = "and";
const BUILDER_SELF_REF_METHOD: &str = "self_ref";
const BUILDER_ARG_REF_METHOD: &str = "arg_ref";
const BUILDER_FIELD_GET_METHOD: &str = "field_get";
const BUILDER_EQ_METHOD: &str = "eq";
const BUILDER_DEFAULT_METHOD: &str = "default";
const BUILDER_STRUCT_INIT_METHOD: &str = "struct_init";
const BUILDER_WITH_FIELD_METHOD: &str = "with_field";
const BUILDER_EMIT_METHOD: &str = "emit_method";
const REFLECT_FIELDS_METHOD: &str = "fields";
const REFLECT_VARIANTS_METHOD: &str = "variants";
const VARIANT_FIELDS_METHOD: &str = "fields";
const FIELD_TY_METHOD: &str = "ty";

enum ProviderIterable<'db> {
    Fields(Vec<ReflectedField<'db>>),
    Variants(Vec<ReflectedVariant<'db>>),
}
