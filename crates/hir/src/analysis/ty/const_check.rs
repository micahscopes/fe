use crate::analysis::HirAnalysisDb;
use crate::analysis::semantic::instance::recorded_method_instance;
use crate::analysis::ty::diagnostics::{BodyDiag, FuncBodyDiag};
use crate::analysis::ty::trait_resolution::Selection;
use crate::analysis::ty::ty_check::{
    Callable, EffectArgLayoutView, EffectParamSite, EffectPassMode, TraitChoiceId, TypedBody,
};
use crate::hir_def::{
    Body, CallableDef, Cond, CondId, Expr, ExprId, Func, Partial, Pat, Stmt, StmtId,
};
use crate::semantic::{EffectEnvView, EffectRequirementKey};
use crate::span::DynLazySpan;

/// Const transport admits immutable trait dictionaries, not storage providers.
pub(crate) fn const_effects_supported(db: &dyn HirAnalysisDb, func: Func<'_>) -> bool {
    if !func.has_effects(db) {
        return true;
    }
    if func.is_extern(db) {
        return false;
    }
    let requirements = EffectEnvView::new(EffectParamSite::Func(func)).requirements(db);
    requirements.len() == func.effects(db).data(db).len()
        && requirements.iter().all(|requirement| {
            !requirement.is_mut && matches!(requirement.key, EffectRequirementKey::Trait(_))
        })
}

pub(crate) fn check_const_fn_body<'db>(
    db: &'db dyn HirAnalysisDb,
    func: Func<'db>,
    typed_body: &TypedBody<'db>,
) -> Vec<FuncBodyDiag<'db>> {
    let Some(body) = func.body(db) else {
        return Vec::new();
    };

    let mut diags = check_const_body_expressions(db, body, typed_body);
    if !const_effects_supported(db, func) {
        diags.insert(
            0,
            BodyDiag::ConstFnEffectsNotAllowed(func.span().effects().into()).into(),
        );
    }
    diags
}

/// Shared const-language checks for function bodies and declaration predicates.
pub(crate) fn check_const_body_expressions<'db>(
    db: &'db dyn HirAnalysisDb,
    body: Body<'db>,
    typed_body: &TypedBody<'db>,
) -> Vec<FuncBodyDiag<'db>> {
    let mut checker = ConstFnChecker {
        db,
        body,
        typed_body,
        diags: Vec::new(),
    };
    checker.check_expr(body.expr(db));
    checker.diags
}

struct ConstFnChecker<'db, 'a> {
    db: &'db dyn HirAnalysisDb,
    body: Body<'db>,
    typed_body: &'a TypedBody<'db>,
    diags: Vec<FuncBodyDiag<'db>>,
}

impl<'db> ConstFnChecker<'db, '_> {
    fn push(&mut self, diag: BodyDiag<'db>) {
        self.diags.push(diag.into());
    }

    /// Reports a callee that a `const fn` cannot call, and returns whether it
    /// reported one.
    fn check_callable(
        &mut self,
        position: ExprId,
        primary: DynLazySpan<'db>,
        callable: &Callable<'db>,
        opened: Option<(ExprId, super::trait_def::TraitInstId<'db>)>,
    ) -> bool {
        let choice = self.body.decision_at(self.typed_body, primary.clone());
        let Some(callee) = self.callable_func(position, callable, choice, opened.is_some()) else {
            return false;
        };

        if let Some((_, goal)) = opened
            && (callee.params(self.db).next().is_some()
                || callee.has_effects(self.db)
                || callee.is_unsafe(self.db)
                || super::ty_lower::param_schema(
                    self.db,
                    callee.into(),
                    super::ty_lower::ParamBasis::Full,
                )
                .keys(self.db)
                .len()
                    != goal.args(self.db).len()
                || !crate::hir_def::WhereClauseOwner::Func(callee)
                    .clause(self.db)
                    .id
                    .const_predicates(self.db)
                    .is_empty())
        {
            self.push(BodyDiag::ObligationOpening {
                primary,
                reason: "evidence dispatch currently requires a safe nullary const method without effects, method generics or const predicates",
            });
            return true;
        }

        let diag = if !callee.is_const(self.db) {
            BodyDiag::ConstFnNonConstCall {
                primary,
                callee: callable.callable_def(),
            }
        } else if !const_effects_supported(self.db, callee) {
            BodyDiag::ConstFnEffectfulCall {
                primary,
                callee: callable.callable_def(),
            }
        } else {
            return false;
        };
        self.push(diag);
        true
    }

    fn callable_func(
        &self,
        position: ExprId,
        callable: &Callable<'db>,
        choice: Option<TraitChoiceId<'db>>,
        opened: bool,
    ) -> Option<Func<'db>> {
        let CallableDef::Func(func) = callable.callable_def() else {
            return None;
        };
        // An assumption authorizes the trait declaration, rather than a
        // concrete implementation body. Check its const contract directly.
        if callable.trait_inst().is_some()
            && choice.is_some_and(|answer| {
                matches!(
                    answer.data(self.db).answer_origin(self.db),
                    super::trait_def::ImplementorOrigin::Assumption,
                )
            })
        {
            return Some(func);
        }
        if !opened
            && callable.trait_inst().is_some()
            && let Some(name) = func.name(self.db).to_opt()
            && let Selection::Unique(method) = recorded_method_instance(
                self.db,
                crate::semantic::context_at(
                    self.db,
                    self.body.scope(),
                    Some((self.typed_body, position)),
                )
                .solver_context(self.db),
                choice?,
                name,
            )
            .ok()?
            && let Some(impl_func) = method.body()
        {
            return Some(impl_func);
        }
        Some(func)
    }

    fn check_call_target(&mut self, expr: ExprId) {
        let Some(callable) = self.typed_body.callable_expr(expr) else {
            return;
        };
        // One diagnostic per call: the providers a call passes are checked
        // only when the callee itself may be called.
        // This diagnostic walk also sees rejected trait calls. Only accepted
        // calls have a dispatch record; inference owns the missing-impl error.
        let opened = self
            .body
            .decision_at(self.typed_body, expr.span(self.body).into())
            .and_then(|_| crate::semantic::opened_call_at(self.db, self.typed_body, expr));
        if self.check_callable(expr, expr.span(self.body).into(), callable, opened) {
            return;
        }
        if self.typed_body.call_effect_args(expr).is_some_and(|args| {
            args.iter().any(|arg| {
                arg.required_mut
                    || arg.key_kind != super::effects::EffectKeyKind::Trait
                    || arg.pass_mode != EffectPassMode::ByValue
                    || arg.layout_view != EffectArgLayoutView::Direct
                    || arg.provider.is_some_and(|space| {
                        space != crate::analysis::ty::provider::ProviderAddressSpace::Memory
                    })
                    || arg.provider_target_ty.is_some()
            })
        }) {
            self.push(BodyDiag::ConstFnEffectfulCall {
                primary: expr.span(self.body).into(),
                callee: callable.callable_def(),
            });
        }
    }

    fn check_stmt(&mut self, stmt: StmtId) {
        let Partial::Present(stmt_data) = stmt.data(self.db, self.body) else {
            return;
        };

        match stmt_data {
            Stmt::Let(pat, _ty, init) => {
                self.check_let_pat(*pat);
                if let Some(init) = init {
                    self.check_expr(*init);
                }
            }
            Stmt::For(pat, iter, body, _) => {
                self.check_let_pat(*pat);
                self.check_expr(*iter);
                if let Some(seq) = self.typed_body.for_loop_seq(stmt) {
                    let span: DynLazySpan<'db> = stmt.span(self.body).into();
                    // The loop's sequence calls are made where its iterator is.
                    self.check_callable(*iter, span.clone(), &seq.len_callable, None);
                    self.check_callable(*iter, span, &seq.get_callable, None);
                }
                self.check_expr(*body);
            }
            Stmt::While(cond, body) => {
                self.check_cond(*cond);
                self.check_expr(*body);
            }
            Stmt::Continue | Stmt::Break => {}
            Stmt::Return(expr) => {
                if let Some(expr) = expr {
                    self.check_expr(*expr);
                }
            }
            Stmt::Expr(expr) => self.check_expr(*expr),
        }
    }

    fn check_let_pat(&mut self, pat: crate::hir_def::PatId) {
        let Partial::Present(pat_data) = pat.data(self.db, self.body) else {
            return;
        };

        match pat_data {
            Pat::Hole(_) | Pat::WildCard | Pat::Rest => {}
            Pat::Lit(_) | Pat::Path(_, _) => {}
            Pat::Tuple(elems) | Pat::PathTuple(_, elems) => {
                elems.iter().for_each(|elem| self.check_let_pat(*elem));
            }
            Pat::Record(_, fields) => fields
                .iter()
                .for_each(|field| self.check_let_pat(field.pat)),
            Pat::Or(lhs, rhs) => {
                self.check_let_pat(*lhs);
                self.check_let_pat(*rhs);
            }
        }
    }

    fn check_expr(&mut self, expr: ExprId) {
        let Partial::Present(expr_data) = expr.data(self.db, self.body) else {
            return;
        };

        match expr_data {
            Expr::Lit(
                crate::hir_def::LitKind::Int(_)
                | crate::hir_def::LitKind::Bool(_)
                | crate::hir_def::LitKind::String(_),
            )
            | Expr::Path(_) => {}

            Expr::Block(stmts) => stmts.iter().for_each(|stmt| self.check_stmt(*stmt)),

            Expr::Bin(lhs, rhs, _) => {
                self.check_expr(*lhs);
                self.check_expr(*rhs);
                self.check_call_target(expr);
            }

            Expr::Un(inner, _) => {
                self.check_expr(*inner);
                self.check_call_target(expr);
            }

            Expr::Field(inner, _) | Expr::ArrayRep(inner, _) | Expr::Cast(inner, _) => {
                self.check_expr(*inner);
            }

            Expr::If(cond, then, else_) => {
                self.check_cond(*cond);
                self.check_expr(*then);
                if let Some(else_) = else_ {
                    self.check_expr(*else_);
                }
            }
            Expr::Call(_callee, args) => {
                args.iter().for_each(|arg| self.check_expr(arg.expr));
                self.check_call_target(expr);
            }
            Expr::UnsupportedMacroCall => {}
            // Splices run when the quote is built; the template is not run.
            Expr::Quote(quote) => quote
                .holes
                .iter()
                .for_each(|hole| self.check_expr(hole.expr)),
            Expr::Hole(_) => {}
            Expr::Assert(args) => args.iter().for_each(|arg| self.check_expr(arg.expr)),
            Expr::MethodCall(receiver, _name, _generic_args, args) => {
                self.check_expr(*receiver);
                args.iter().for_each(|arg| self.check_expr(arg.expr));
                self.check_call_target(expr);
            }
            Expr::Match(scrutinee, arms) => {
                self.check_expr(*scrutinee);
                if let Some(arms) = arms.clone().to_opt() {
                    arms.iter().for_each(|arm| {
                        if let Some((pat, body)) = arm.parts() {
                            self.check_match_pat(pat);
                            self.check_expr(body);
                        }
                    });
                }
            }
            Expr::Assign(lhs, rhs) | Expr::AugAssign(lhs, rhs, _) => {
                self.check_expr(*lhs);
                self.check_expr(*rhs);
                self.check_call_target(expr);
            }
            Expr::With(bindings, body) => {
                for binding in bindings {
                    self.check_expr(binding.value);
                }
                self.check_expr(*body);
            }
            Expr::RecordInit(_path, fields) => {
                fields.iter().for_each(|field| self.check_expr(field.expr));
            }

            Expr::Tuple(elems) | Expr::Array(elems) => {
                elems.iter().for_each(|elem| self.check_expr(*elem));
            }
        }
    }

    fn check_cond(&mut self, cond: CondId) {
        let Partial::Present(cond_data) = cond.data(self.db, self.body) else {
            return;
        };

        match cond_data {
            Cond::Expr(expr) => self.check_expr(*expr),
            Cond::Let(pat, value) => {
                self.check_let_pat(*pat);
                self.check_expr(*value);
            }
            Cond::Bin(lhs, rhs, _) => {
                self.check_cond(*lhs);
                self.check_cond(*rhs);
            }
        }
    }

    fn check_match_pat(&mut self, pat: crate::hir_def::PatId) {
        let Partial::Present(pat_data) = pat.data(self.db, self.body) else {
            return;
        };

        match pat_data {
            Pat::Hole(_) | Pat::WildCard | Pat::Rest => {}
            Pat::Lit(_) | Pat::Path(_, _) => {}
            Pat::Tuple(elems) | Pat::PathTuple(_, elems) => {
                elems.iter().for_each(|elem| self.check_match_pat(*elem));
            }
            Pat::Record(_, fields) => fields
                .iter()
                .for_each(|field| self.check_match_pat(field.pat)),
            Pat::Or(lhs, rhs) => {
                self.check_match_pat(*lhs);
                self.check_match_pat(*rhs);
            }
        }
    }
}
