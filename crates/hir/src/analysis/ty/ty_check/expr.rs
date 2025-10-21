use std::panic;

use crate::hir_def::{
    ArithBinOp, BinOp, Expr, ExprId, FieldIndex, IdentId, Partial, PatId, PathId, UnOp,
    VariantKind, ExprDescription, PatDescription,
};
use common::ingot::IngotKind;
use either::Either;

use super::{
    RecordLike, Typeable,
    env::{ExprProp, LocalBinding, TyCheckEnv},
    path::ResolvedPathInBody,
};
use crate::analysis::ty::{
    diagnostics::{BodyDiag, FuncBodyDiag},
    trait_def::TraitInstId,
    ty_check::callable::Callable,
};
use crate::analysis::ty::{trait_def::TraitDef, trait_lower::lower_trait};
use crate::analysis::{
    HirAnalysisDb, Spanned,
    name_resolution::{
        EarlyNameQueryId, ExpectedPathKind, NameDomain, NameResBucket, PathRes, QueryDirective,
        diagnostics::PathResDiag,
        is_scope_visible_from,
        method_selection::{MethodCandidate, MethodSelectionError, select_method_candidate},
        resolve_ident_to_bucket, resolve_name_res, resolve_path, resolve_query,
    },
    ty::{
        canonical::Canonicalized,
        const_ty::ConstTyId,
        normalize::normalize_ty,
        ty_check::{TyChecker, path::RecordInitChecker},
        ty_def::{InvalidCause, TyId},
    },
};

impl<'db> TyChecker<'db> {
    /// Convenience method for type-checking a single expression.
    ///
    /// For one-off type checks, this method provides a concise API. When you need to call
    /// multiple methods on the same expression (e.g., type-checking and getting span for
    /// diagnostics), consider creating the wrapper once:
    /// ```
    /// let expr = tc.body().wrap_expr(expr_id);
    /// let prop = expr.type_check(tc, expected);
    /// let span = expr.span(tc.db);
    /// ```
    pub(super) fn check_expr(&mut self, expr: ExprId, expected: TyId<'db>) -> ExprProp<'db> {
        self.body().wrap_expr(expr).type_check(self, expected)
    }

    /// Convenience method for type-checking an expression with an unknown expected type.
    ///
    /// See [`check_expr`](Self::check_expr) for guidance on when to use wrapper API directly.
    pub(super) fn check_expr_unknown(&mut self, expr: ExprId) -> ExprProp<'db> {
        self.body().wrap_expr(expr).type_check_unknown(self)
    }

    fn check_record_init_fields(&mut self, record_like: &RecordLike<'db>, expr: ExprId) {
        let hir_db = self.db;

        let Partial::Present(ExprDescription::RecordInit(_, fields)) = expr.data(hir_db, self.body()) else {
            unreachable!()
        };
        let span = expr.span(self.body()).into_record_init_expr().fields();

        let mut rec_checker = RecordInitChecker::new(self, record_like);

        for (i, field) in fields.iter().enumerate() {
            let label = field.label_eagerly(rec_checker.tc.db, rec_checker.tc.body());
            let field_span = span.clone().field(i).into();

            let expected = match rec_checker.feed_label(label, field_span) {
                Ok(ty) => ty,
                Err(diag) => {
                    rec_checker.tc.push_diag(diag);
                    TyId::invalid(rec_checker.tc.db, InvalidCause::Other)
                }
            };

            rec_checker.tc.check_expr(field.expr, expected);
        }

        if let Err(diag) = rec_checker.finalize(span.into(), false) {
            self.push_diag(diag);
        }
    }

    fn resolve_core_trait(&self, trait_path: PathId<'db>) -> Option<TraitDef<'db>> {
        let scope = self.env.scope();
        let assumptions = self.env.assumptions();
        let mut module_path = trait_path.parent(self.db)?;

        // If we are inside the core ingot, replace `core` with `ingot` in the trait path.
        let ingot = self.env.body().top_mod(self.db).ingot(self.db);
        if ingot.kind(self.db) == IngotKind::Core && trait_path.is_core_lib_path(self.db) {
            module_path = module_path.replace_root(
                IdentId::make_core(self.db),
                IdentId::make_ingot(self.db),
                self.db,
            );
        }
        let trait_name = trait_path.ident(self.db).to_opt()?;
        let Ok(PathRes::Mod(mod_scope)) =
            resolve_path(self.db, module_path, scope, assumptions, false)
        else {
            panic!("failed to resolve `{}`", module_path.pretty_print(self.db));
        };

        let bucket =
            resolve_ident_to_bucket(self.db, PathId::from_ident(self.db, trait_name), mod_scope);
        let nameres = bucket.pick(NameDomain::TYPE).as_ref().ok()?;
        Some(lower_trait(self.db, nameres.trait_()?))
    }

    fn check_ops_trait(
        &mut self,
        expr: ExprId,
        lhs_ty: TyId<'db>,
        op: &dyn TraitOps,
        rhs_expr: Option<ExprId>,
    ) -> ExprProp<'db> {
        let Some(trait_def) = self.resolve_core_trait(op.trait_path(self.db)) else {
            panic!("failed to resolve core::ops trait");
        };

        let c_lhs_ty = Canonicalized::new(self.db, lhs_ty);

        let (method, inst) = match select_method_candidate(
            self.db,
            c_lhs_ty.value,
            op.trait_method(self.db),
            self.env.scope(),
            self.env.assumptions(),
            Some(trait_def),
        ) {
            Ok(MethodCandidate::InherentMethod(_)) => unreachable!(),
            Ok(
                res @ (MethodCandidate::TraitMethod(cand)
                | MethodCandidate::NeedsConfirmation(cand)),
            ) => {
                let inst = c_lhs_ty.extract_solution(&mut self.table, cand.inst);
                if matches!(res, MethodCandidate::NeedsConfirmation(_)) {
                    self.env
                        .register_confirmation(inst, expr.span(self.body()).into());
                }

                let func_ty = cand
                    .method
                    .instantiate_with_inst(&mut self.table, lhs_ty, inst);

                if let Some(rhs_expr) = rhs_expr {
                    // Derive expected RHS type from the instantiated function type using CallableParam
                    if let Ok(callable) = Callable::new(self.db, func_ty, expr.span(self.body()).into(), Some(inst)) {
                        let expected_rhs = self.normalize_ty(callable.nth_param(self.db, 1).ty(self.db));
                        self.check_expr(rhs_expr, expected_rhs);
                    }
                }

                (func_ty, inst)
            }
            Err(MethodSelectionError::AmbiguousTraitMethod(insts)) => {
                let Some(rhs_expr) = rhs_expr else {
                    unreachable!("unary core::ops ambiguity");
                };

                let rhs = self.check_expr_unknown(rhs_expr);
                if rhs.ty.has_invalid(self.db) {
                    return ExprProp::invalid(self.db);
                }

                let method_ident = op.trait_method(self.db);
                let trait_method = trait_def.methods(self.db).get(&method_ident).unwrap();

                let mut viable: Vec<(TyId<'db>, TraitInstId<'db>, TyId<'db>)> = Vec::new();
                for inst in insts.iter().copied() {
                    let snapshot = self.table.snapshot();
                    let candidate_func_ty =
                        trait_method.instantiate_with_inst(&mut self.table, lhs_ty, inst);
                    let expected_rhs = if let Ok(callable) = Callable::new(
                        self.db,
                        candidate_func_ty,
                        expr.span(self.body()).into(),
                        Some(inst),
                    ) {
                        self.normalize_ty(callable.nth_param(self.db, 1).ty(self.db))
                    } else {
                        unreachable!("candidate func ty should be a func");
                    };
                    let unifies = self.table.unify(rhs.ty, expected_rhs).is_ok();
                    self.table.rollback_to(snapshot);
                    if unifies {
                        viable.push((candidate_func_ty, inst, expected_rhs));
                    }
                }

                match viable.len() {
                    0 => {
                        let diag = BodyDiag::ops_trait_not_implemented(
                            self.db,
                            expr.span(self.body()).into(),
                            lhs_ty,
                            op,
                        );
                        self.push_diag(diag);
                        return ExprProp::invalid(self.db);
                    }
                    1 => {
                        let (func_ty, inst, expected_rhs) = viable.pop().unwrap();
                        self.env
                            .register_confirmation(inst, expr.span(self.body()).into());
                        self.unify_ty(Typeable::Expr(rhs_expr, rhs), rhs.ty, expected_rhs);
                        (func_ty, inst)
                    }
                    _ => {
                        self.push_diag(BodyDiag::AmbiguousTraitInst {
                            primary: expr.span(self.body()).into(),
                            cands: viable.into_iter().map(|(_, inst, _)| inst).collect(),
                        });
                        return ExprProp::invalid(self.db);
                    }
                }
            }
            Err(MethodSelectionError::NotFound) => {
                let diag = BodyDiag::ops_trait_not_implemented(
                    self.db,
                    expr.span(self.body()).into(),
                    lhs_ty,
                    op,
                );
                self.push_diag(diag);
                return ExprProp::invalid(self.db);
            }
            Err(err) => {
                unreachable!("unexpected error: {err:?}");
            }
        };

        let callable = Callable::new(self.db, method, expr.span(self.body()).into(), Some(inst))
            .expect("failed to create Callable for core::ops trait method");

        let ret_ty = self.normalize_ty(callable.ret_ty(self.db));
        self.env.register_callable(expr, callable);
        ExprProp::new(ret_ty, true)
    }

    fn check_assign_lhs(&mut self, lhs: ExprId, typed_lhs: &ExprProp<'db>) {
        let lhs_expr = self.body().wrap_expr(lhs);
        if !self.is_assignable_expr(lhs_expr) {
            let diag = BodyDiag::NonAssignableExpr(lhs.span(self.body()).into());
            self.push_diag(diag);

            return;
        }

        if !typed_lhs.is_mut {
            let binding = self.find_base_binding(lhs_expr);
            let diag = match binding {
                Some(binding) => {
                    let (ident, def_span) = (
                        self.env.binding_name(binding),
                        self.env.binding_def_span(binding),
                    );

                    BodyDiag::ImmutableAssignment {
                        primary: lhs.span(self.body()).into(),
                        binding: Some((ident, def_span)),
                    }
                }

                None => BodyDiag::ImmutableAssignment {
                    primary: lhs.span(self.body()).into(),
                    binding: None,
                },
            };

            self.push_diag(diag);
        }
    }

    fn check_expr_in_new_scope(&mut self, expr: ExprId, expected: TyId<'db>) -> ExprProp<'db> {
        self.env.enter_scope(expr);
        let ty = self.check_expr(expr, expected);
        self.env.leave_scope();

        ty
    }

    /// Returns the base binding for a given expression if it exists.
    ///
    /// This function traverses the expression tree to find the base binding,
    /// which is the original variable or binding that the expression refers to.
    ///
    /// # Parameters
    ///
    /// - `expr`: The expression ID for which to find the base binding.
    ///
    /// # Returns
    ///
    /// An `Option` containing the `LocalBinding` if a base binding is found,
    /// or `None` if there is no base binding.
    fn find_base_binding(&self, expr: Expr<'db>) -> Option<LocalBinding<'db>> {
        let Partial::Present(expr_data) = expr.data(self.db) else {
            return None;
        };

        match expr_data {
            ExprDescription::Field(lhs, ..) => self.find_base_binding(self.body().wrap_expr(*lhs)),
            ExprDescription::Bin(lhs, _rhs, op) if *op == BinOp::Index => self.find_base_binding(self.body().wrap_expr(*lhs)),
            ExprDescription::Path(..) => self.env.typed_expr(expr.id())?.binding(),
            _ => None,
        }
    }

    /// Returns `true`` if the expression can be used as an left hand side of an
    /// assignment.
    /// This method doesn't take mutability into account.
    fn is_assignable_expr(&self, expr: Expr<'db>) -> bool {
        let Partial::Present(expr_data) = expr.data(self.db) else {
            return false;
        };

        match expr_data {
            ExprDescription::Path(..) | ExprDescription::Field(..) => true,
            ExprDescription::Bin(_, _, op) if *op == BinOp::Index => true,
            _ => false,
        }
    }
}

/// Type-checking methods on Expr wrappers.
/// These methods implement the traversal API where context flows through the wrappers.
impl<'db> Expr<'db> {
    pub(super) fn type_check(
        self,
        tc: &mut TyChecker<'db>,
        expected: TyId<'db>,
    ) -> ExprProp<'db> {
        let Partial::Present(expr_data) = self.data(tc.db) else {
            let typed = ExprProp::invalid(tc.db);
            tc.env.type_expr(self.id(), typed);
            return typed;
        };

        let expected = normalize_ty(tc.db, expected, tc.env.scope(), tc.env.assumptions());

        tc.env.enter_expr(self.id());
        let mut actual = match expr_data {
            ExprDescription::Lit(lit) => ExprProp::new(tc.lit_ty(lit), true),
            ExprDescription::Block(..) => self.type_check_block(tc, expected),
            ExprDescription::Un(..) => self.type_check_unary(tc),
            ExprDescription::Bin(..) => self.type_check_binary(tc),
            ExprDescription::Call(..) => self.type_check_call(tc),
            ExprDescription::Tuple(..) => self.type_check_tuple(tc, expected),
            ExprDescription::Array(..) => self.type_check_array(tc, expected),
            ExprDescription::ArrayRep(..) => self.type_check_array_rep(tc, expected),
            ExprDescription::If(..) => self.type_check_if(tc),
            ExprDescription::Match(..) => self.type_check_match(tc),
            ExprDescription::Assign(..) => self.type_check_assign(tc),
            ExprDescription::AugAssign(..) => self.type_check_aug_assign(tc),
            ExprDescription::Field(..) => self.type_check_field(tc),
            ExprDescription::Path(..) => self.type_check_path(tc),
            ExprDescription::MethodCall(..) => self.type_check_method_call(tc),
            ExprDescription::RecordInit(..) => self.type_check_record_init(tc),
        };
        tc.env.leave_expr();

        let typeable = Typeable::Expr(self.id(), actual);
        actual.ty = normalize_ty(tc.db, actual.ty, tc.env.scope(), tc.env.assumptions());
        actual.ty = tc.unify_ty(typeable, actual.ty, expected);
        actual
    }

    fn type_check_unary(self, tc: &mut TyChecker<'db>) -> ExprProp<'db> {
        let Partial::Present(expr_data) = self.data(tc.db) else {
            unreachable!()
        };
        let ExprDescription::Un(lhs, op) = expr_data else {
            unreachable!()
        };

        let lhs_expr = self.body().wrap_expr(*lhs);
        let prop = lhs_expr.type_check_unknown(tc);

        if *op == UnOp::Plus {
            // TODO: remove support for unary plus? what should it do?
            return prop;
        }
        if prop.ty.has_invalid(tc.db) {
            return ExprProp::invalid(tc.db);
        }

        if prop.ty.is_integral_var(tc.db) && matches!(op, UnOp::Plus | UnOp::Minus | UnOp::BitNot)
        {
            return prop;
        }

        let base_ty = prop.ty.base_ty(tc.db);
        if base_ty.is_ty_var(tc.db) {
            let diag = BodyDiag::TypeMustBeKnown(lhs_expr.span(tc.db).into());
            tc.push_diag(diag);
            return ExprProp::invalid(tc.db);
        }

        tc.check_ops_trait(self.id(), prop.ty, op, None)
    }

    pub(super) fn type_check_unknown(self, tc: &mut TyChecker<'db>) -> ExprProp<'db> {
        let t = tc.fresh_ty();
        self.type_check(tc, t)
    }

    fn type_check_block(self, tc: &mut TyChecker<'db>, expected: TyId<'db>) -> ExprProp<'db> {
        let Partial::Present(expr_data) = self.data(tc.db) else {
            unreachable!()
        };
        let ExprDescription::Block(stmts) = expr_data else {
            unreachable!()
        };

        if stmts.is_empty() {
            ExprProp::new(TyId::unit(tc.db), true)
        } else {
            tc.env.enter_scope(self.id());
            for &stmt in stmts[..stmts.len() - 1].iter() {
                let ty = tc.fresh_ty();
                tc.check_stmt(stmt, ty);
            }

            let last_stmt = stmts[stmts.len() - 1];
            let res = tc.check_stmt(last_stmt, expected);
            tc.env.leave_scope();
            ExprProp::new(res, true)
        }
    }

    fn type_check_binary(self, tc: &mut TyChecker<'db>) -> ExprProp<'db> {
        let Partial::Present(expr_data) = self.data(tc.db) else {
            unreachable!()
        };
        let ExprDescription::Bin(lhs_expr, rhs_expr, op) = expr_data else {
            unreachable!()
        };

        // Logical operands must be bools
        if matches!(op, BinOp::Logical(_)) {
            let bool = TyId::bool(tc.db);
            let lhs = tc.check_expr(*lhs_expr, bool);
            let rhs = tc.check_expr(*rhs_expr, bool);
            return if lhs.ty.is_bool(tc.db) && rhs.ty.is_bool(tc.db) {
                ExprProp::new(bool, true)
            } else {
                ExprProp::invalid(tc.db)
            };
        }

        let lhs = tc.check_expr_unknown(*lhs_expr);
        if lhs.ty.has_invalid(tc.db) {
            return ExprProp::invalid(tc.db);
        }

        if matches!(op, BinOp::Index) && lhs.ty.is_array(tc.db) {
            // Built-in array indexing (TODO: move to trait impl)
            let args = lhs.ty.generic_args(tc.db);
            let elem_ty = args[0];
            let index_ty = args[1].const_ty_ty(tc.db).unwrap();
            tc.check_expr(*rhs_expr, index_ty);
            return ExprProp::new(elem_ty, lhs.is_mut);
        } else if lhs.ty.is_integral_var(tc.db) {
            // Avoid 'type must be known' diagnostics when lhs is an unknown integer ty
            tc.check_expr(*rhs_expr, lhs.ty);
            return lhs;
        }

        // Fail if lhs ty is unknown
        if lhs.ty.base_ty(tc.db).is_ty_var(tc.db) {
            tc.check_expr_unknown(*rhs_expr);
            let lhs_expr_wrapped = self.body().wrap_expr(*lhs_expr);
            let diag = BodyDiag::TypeMustBeKnown(lhs_expr_wrapped.span(tc.db).into());
            tc.push_diag(diag);
            return ExprProp::invalid(tc.db);
        }

        tc.check_ops_trait(self.id(), lhs.ty, op, Some(*rhs_expr))
    }

    fn type_check_call(self, tc: &mut TyChecker<'db>) -> ExprProp<'db> {
        let Partial::Present(expr_data) = self.data(tc.db) else {
            unreachable!()
        };
        let ExprDescription::Call(callee, args) = expr_data else {
            unreachable!()
        };

        let callee_ty = tc.check_expr_unknown(*callee).ty;
        if callee_ty.has_invalid(tc.db) {
            return ExprProp::invalid(tc.db);
        }

        let callee_wrapped = self.body().wrap_expr(*callee);
        let mut callable =
            match Callable::new(tc.db, callee_ty, callee_wrapped.span(tc.db).into(), None) {
                Ok(callable) => callable,
                Err(diag) => {
                    tc.push_diag(diag);
                    return ExprProp::invalid(tc.db);
                }
            };

        let call_span = self.span(tc.db).into_call_expr();

        if let Partial::Present(ExprDescription::Path(Partial::Present(path))) =
            callee_wrapped.data(tc.db)
        {
            let idx = path.segment_index(tc.db);

            if !callable.unify_generic_args(
                tc,
                path.generic_args(tc.db),
                self.span(tc.db)
                    .into_path_expr()
                    .path()
                    .segment(idx)
                    .generic_args(),
            ) {
                return ExprProp::invalid(tc.db);
            }
        };

        callable.check_args(tc, args, call_span.args(), None);

        let ret_ty = callable.ret_ty(tc.db);
        // Normalize the return type to resolve any associated types
        let normalized_ret_ty = tc.normalize_ty(ret_ty);
        tc.env.register_callable(self.id(), callable);
        ExprProp::new(normalized_ret_ty, true)
    }

    fn type_check_tuple(self, tc: &mut TyChecker<'db>, expected: TyId<'db>) -> ExprProp<'db> {
        let Partial::Present(expr_data) = self.data(tc.db) else {
            unreachable!()
        };
        let ExprDescription::Tuple(elems) = expr_data else {
            unreachable!()
        };

        let elem_tys = match expected.decompose_ty_app(tc.db) {
            (base, args) if base.is_tuple(tc.db) && args.len() == elems.len() => args.to_vec(),
            _ => tc.fresh_tys_n(elems.len()),
        };

        for (elem, elem_ty) in elems.iter().zip(elem_tys.iter()) {
            tc.check_expr(*elem, *elem_ty);
        }

        let ty = TyId::tuple_with_elems(tc.db, &elem_tys);
        ExprProp::new(ty, true)
    }

    fn type_check_array(self, tc: &mut TyChecker<'db>, expected: TyId<'db>) -> ExprProp<'db> {
        let Partial::Present(expr_data) = self.data(tc.db) else {
            unreachable!()
        };
        let ExprDescription::Array(elems) = expr_data else {
            unreachable!()
        };

        let mut expected_elem_ty = match expected.decompose_ty_app(tc.db) {
            (base, args) if base.is_array(tc.db) => args[0],
            _ => tc.fresh_ty(),
        };

        for elem in elems {
            expected_elem_ty = tc.check_expr(*elem, expected_elem_ty).ty;
        }

        let ty = TyId::array_with_len(tc.db, expected_elem_ty, elems.len());
        ExprProp::new(ty, true)
    }

    fn type_check_array_rep(self, tc: &mut TyChecker<'db>, expected: TyId<'db>) -> ExprProp<'db> {
        let Partial::Present(expr_data) = self.data(tc.db) else {
            unreachable!()
        };
        let ExprDescription::ArrayRep(elem, len) = expr_data else {
            unreachable!()
        };

        let mut expected_elem_ty = match expected.decompose_ty_app(tc.db) {
            (base, args) if base.is_array(tc.db) => args[0],
            _ => tc.fresh_ty(),
        };

        expected_elem_ty = tc.check_expr(*elem, expected_elem_ty).ty;

        let array = TyId::array(tc.db, expected_elem_ty);
        let ty = if let Some(len_body) = len.to_opt() {
            let expected_len_ty = array
                .applicable_ty(tc.db)
                .and_then(|applicable| applicable.const_ty);

            let len_ty = ConstTyId::from_body(tc.db, len_body, expected_len_ty, None);
            let len_ty = TyId::const_ty(tc.db, len_ty);
            let array_ty = TyId::app(tc.db, array, len_ty);

            if let Some(diag) = array_ty.emit_diag(tc.db, len_body.span().into()) {
                tc.push_diag(diag);
            }

            array_ty
        } else {
            let len_ty = ConstTyId::invalid(tc.db, InvalidCause::ParseError);
            let len_ty = TyId::const_ty(tc.db, len_ty);
            TyId::app(tc.db, array, len_ty)
        };

        ExprProp::new(ty, true)
    }

    fn type_check_if(self, tc: &mut TyChecker<'db>) -> ExprProp<'db> {
        let Partial::Present(expr_data) = self.data(tc.db) else {
            unreachable!()
        };
        let ExprDescription::If(cond, then, else_) = expr_data else {
            unreachable!()
        };

        tc.check_expr(*cond, TyId::bool(tc.db));

        let if_ty = tc.fresh_ty();
        let ty = match else_ {
            Some(else_) => {
                tc.check_expr_in_new_scope(*then, if_ty);
                tc.check_expr_in_new_scope(*else_, if_ty).ty
            }

            None => {
                // If there is no else branch, the if expression itself typed as `()`
                tc.check_expr_in_new_scope(*then, if_ty);
                TyId::unit(tc.db)
            }
        };

        ExprProp::new(ty, true)
    }

    fn type_check_match(self, tc: &mut TyChecker<'db>) -> ExprProp<'db> {
        let Partial::Present(expr_data) = self.data(tc.db) else {
            unreachable!()
        };
        let ExprDescription::Match(scrutinee, arms) = expr_data else {
            unreachable!()
        };

        let scrutinee_ty = tc.fresh_ty();
        let scrutinee_ty = tc.check_expr(*scrutinee, scrutinee_ty).ty;

        let Partial::Present(arms) = arms else {
            return ExprProp::invalid(tc.db);
        };

        let mut match_ty = tc.fresh_ty();
        // Store cloned HirPat data and the original PatId for diagnostics.
        let mut hir_pats_with_ids: Vec<(&PatDescription<'db>, PatId)> = Vec::with_capacity(arms.len());

        // First loop: Type check patterns, collect HIR patterns for analysis, and type check arm bodies.
        for arm in arms.iter() {
            tc.check_pat(arm.pat, scrutinee_ty);

            let pat_data_partial = arm.pat.data(tc.db, tc.body());
            if let Partial::Present(actual_pat_data) = pat_data_partial {
                // Clone the Pat data for ownership in the vector.
                hir_pats_with_ids.push((actual_pat_data, arm.pat));
            }
            // If pat_data is Partial::Absent, check_pat should have already emitted an error.
            // We only include valid patterns in the exhaustiveness/reachability analysis.

            tc.env.enter_scope(arm.body);
            tc.env.flush_pending_bindings();
            match_ty = tc.check_expr(arm.body, match_ty).ty;
            tc.env.leave_scope();
        }

        // Collect owned HirPat data for analysis.
        let collected_hir_pats: Vec<PatDescription<'db>> = hir_pats_with_ids
            .iter()
            .map(|(p, _id)| (*p).clone())
            .collect();

        // Perform reachability analysis.
        let reachability = crate::analysis::ty::pattern_analysis::check_reachability(
            tc.db,
            &collected_hir_pats,
            tc.body(),
            tc.env.scope(),
            scrutinee_ty,
        );

        for (i, is_reachable) in reachability.iter().enumerate() {
            if !is_reachable {
                let (_current_hir_pat, current_pat_id) = &hir_pats_with_ids[i];
                let diag = BodyDiag::UnreachablePattern {
                    primary: current_pat_id.span(tc.body()).into(),
                };
                tc.push_diag(diag);
            }
        }

        // Perform exhaustiveness analysis.
        if let Err(missing_patterns) = crate::analysis::ty::pattern_analysis::check_exhaustiveness(
            tc.db,
            &collected_hir_pats,
            tc.body(),
            tc.env.scope(),
            scrutinee_ty,
        ) {
            let diag = BodyDiag::NonExhaustiveMatch {
                primary: self.span(tc.db).into(),
                scrutinee_ty,
                missing_patterns,
            };
            tc.push_diag(diag);
        }

        ExprProp::new(match_ty, true)
    }

    fn type_check_assign(self, tc: &mut TyChecker<'db>) -> ExprProp<'db> {
        let Partial::Present(expr_data) = self.data(tc.db) else {
            unreachable!()
        };
        let ExprDescription::Assign(lhs, rhs) = expr_data else {
            unreachable!()
        };

        let lhs_ty = tc.fresh_ty();
        let typed_lhs = tc.check_expr(*lhs, lhs_ty);
        tc.check_expr(*rhs, lhs_ty);

        let result_ty = TyId::unit(tc.db);

        tc.check_assign_lhs(*lhs, &typed_lhs);

        ExprProp::new(result_ty, true)
    }

    fn type_check_aug_assign(self, tc: &mut TyChecker<'db>) -> ExprProp<'db> {
        let Partial::Present(expr_data) = self.data(tc.db) else {
            unreachable!()
        };
        let ExprDescription::AugAssign(lhs, rhs, op) = expr_data else {
            unreachable!()
        };

        let unit = ExprProp::new(TyId::unit(tc.db), true);

        let typed_lhs = tc.check_expr_unknown(*lhs);
        let lhs_ty = typed_lhs.ty;
        if lhs_ty.has_invalid(tc.db) {
            return unit;
        }
        tc.check_assign_lhs(*lhs, &typed_lhs);

        // Avoid 'type must be known' diagnostics for unknown integer ty
        if lhs_ty.is_integral_var(tc.db) {
            tc.check_expr(*rhs, lhs_ty);
            return unit;
        }

        let lhs_base_ty = lhs_ty.base_ty(tc.db);
        if lhs_base_ty.is_ty_var(tc.db) {
            let lhs_wrapped = self.body().wrap_expr(*lhs);
            let diag = BodyDiag::TypeMustBeKnown(lhs_wrapped.span(tc.db).into());
            tc.push_diag(diag);
            return unit;
        }

        tc.check_ops_trait(self.id(), lhs_ty, &AugAssignOp(*op), Some(*rhs));

        // Return unit ty even if trait resolution fails
        unit
    }

    fn type_check_field(self, tc: &mut TyChecker<'db>) -> ExprProp<'db> {
        let Partial::Present(expr_data) = self.data(tc.db) else {
            unreachable!()
        };
        let ExprDescription::Field(lhs, index) = expr_data else {
            unreachable!()
        };
        let Partial::Present(field) = index else {
            return ExprProp::invalid(tc.db);
        };

        let lhs_ty = tc.fresh_ty();
        let typed_lhs = tc.check_expr(*lhs, lhs_ty);
        let lhs_ty = typed_lhs.ty;
        // let lhs_ty = normalize_ty(tc.db, lhs_ty, tc.env.scope(), tc.env.assumptions());

        let (ty_base, ty_args) = lhs_ty.decompose_ty_app(tc.db);

        if ty_base.has_invalid(tc.db) {
            return ExprProp::invalid(tc.db);
        }
        let ty_base = lhs_ty;

        if ty_base.is_ty_var(tc.db) {
            let lhs_wrapped = self.body().wrap_expr(*lhs);
            let diag = BodyDiag::TypeMustBeKnown(lhs_wrapped.span(tc.db).into());
            tc.push_diag(diag);
            return ExprProp::invalid(tc.db);
        }

        match field {
            FieldIndex::Ident(label) => {
                let record_like = RecordLike::from_ty(lhs_ty);
                if let Some(field_ty) = record_like.record_field_ty(tc.db, *label) {
                    if let Some(scope) = record_like.record_field_scope(tc.db, *label)
                        && !is_scope_visible_from(tc.db, scope, tc.env.scope())
                    {
                        // Check the visibility of the field.
                        let diag = PathResDiag::Invisible(
                            self.span(tc.db).into_field_expr().accessor().into(),
                            *label,
                            scope.name_span(tc.db),
                        );

                        tc.push_diag(diag);
                        return ExprProp::invalid(tc.db);
                    }
                    return ExprProp::new(field_ty, typed_lhs.is_mut);
                }
            }

            FieldIndex::Index(i) => {
                let arg_len = ty_args.len().into();
                if ty_base.is_tuple(tc.db) && i.data(tc.db) < &arg_len {
                    let i: usize = i.data(tc.db).try_into().unwrap();
                    let ty = ty_args[i];
                    return ExprProp::new(ty, typed_lhs.is_mut);
                }
            }
        };

        let diag = BodyDiag::AccessedFieldNotFound {
            primary: self.span(tc.db).into(),
            given_ty: lhs_ty,
            index: *field,
        };
        tc.push_diag(diag);

        ExprProp::invalid(tc.db)
    }

    fn type_check_path(self, tc: &mut TyChecker<'db>) -> ExprProp<'db> {
        let Partial::Present(expr_data) = self.data(tc.db) else {
            unreachable!()
        };
        let ExprDescription::Path(path) = expr_data else {
            unreachable!()
        };

        let Partial::Present(path) = path else {
            return ExprProp::invalid(tc.db);
        };

        let path_expr_span = self.span(tc.db).into_path_expr();
        let path_span = path_expr_span.clone().path();

        let res = if path.is_bare_ident(tc.db) {
            resolve_ident_expr(tc.db, &tc.env, *path)
        } else {
            match tc.resolve_path(*path, true, path_span.clone()) {
                Ok(r) => ResolvedPathInBody::Reso(r),
                Err(err) => {
                    let expected_kind = if matches!(tc.parent_expr(), Some(ExprDescription::Call(..))) {
                        ExpectedPathKind::Function
                    } else {
                        ExpectedPathKind::Value
                    };

                    if let Some(diag) =
                        err.into_diag(tc.db, *path, path_span.clone(), expected_kind)
                    {
                        tc.push_diag(diag)
                    }
                    ResolvedPathInBody::Invalid
                }
            }
        };

        match res {
            ResolvedPathInBody::Binding(binding) => {
                let ty = tc.env.lookup_binding_ty(binding);
                let is_mut = binding.is_mut();
                ExprProp::new_binding_ref(ty, is_mut, binding)
            }
            ResolvedPathInBody::NewBinding(ident) => {
                let diag = BodyDiag::UndefinedVariable(path_expr_span.into(), ident);
                tc.push_diag(diag);

                ExprProp::invalid(tc.db)
            }
            ResolvedPathInBody::Diag(diag) => {
                tc.push_diag(diag);
                ExprProp::invalid(tc.db)
            }
            ResolvedPathInBody::Invalid => ExprProp::invalid(tc.db),

            ResolvedPathInBody::Reso(reso) => match reso {
                PathRes::Ty(ty) | PathRes::TyAlias(_, ty) => {
                    if let Some(const_ty_ty) = ty.const_ty_ty(tc.db) {
                        ExprProp::new(tc.table.instantiate_to_term(const_ty_ty), true)
                    } else {
                        let diag = if ty.is_struct(tc.db) {
                            let record_like = RecordLike::from_ty(ty);
                            BodyDiag::unit_variant_expected(
                                tc.db,
                                path_expr_span.clone().into(),
                                record_like,
                            )
                        } else {
                            BodyDiag::NotValue {
                                primary: path_expr_span.clone().into(),
                                given: Either::Right(ty),
                            }
                        };
                        tc.push_diag(diag);

                        ExprProp::invalid(tc.db)
                    }
                }
                PathRes::Func(ty) => ExprProp::new(tc.table.instantiate_to_term(ty), true),
                PathRes::Trait(trait_) => {
                    let diag = BodyDiag::NotValue {
                        primary: path_expr_span.clone().into(),
                        given: Either::Left(trait_.def(tc.db).trait_(tc.db).into()),
                    };
                    tc.push_diag(diag);
                    ExprProp::invalid(tc.db)
                }
                PathRes::EnumVariant(variant) => {
                    let ty = match variant.kind(tc.db) {
                        VariantKind::Unit => variant.ty,
                        VariantKind::Tuple(_) => {
                            let ty = variant.constructor_func_ty(tc.db).unwrap();
                            tc.table.instantiate_to_term(ty)
                        }
                        VariantKind::Record(_) => {
                            let record_like = RecordLike::from_variant(variant);
                            let diag = BodyDiag::unit_variant_expected(
                                tc.db,
                                self.span(tc.db).into(),
                                record_like,
                            );
                            tc.push_diag(diag);

                            TyId::invalid(tc.db, InvalidCause::Other)
                        }
                    };

                    ExprProp::new(tc.table.instantiate_to_term(ty), true)
                }
                PathRes::Const(_, ty) => ExprProp::new(ty, true),
                PathRes::Method(receiver_ty, candidate) => {
                    let canonical_r_ty = Canonicalized::new(tc.db, receiver_ty);
                    let method_ty = match candidate {
                        MethodCandidate::InherentMethod(func_def) => {
                            // TODO: move this to path resolver
                            let mut method_ty = TyId::func(tc.db, func_def);
                            for &arg in receiver_ty.generic_args(tc.db) {
                                // If the method is defined in "specialized" impl block
                                // of a generic type (eg `impl Option<i32>`), then
                                // calling `TyId::app(db, method_ty, ..)` will result in
                                // `TyId::invalid`.
                                if method_ty.applicable_ty(tc.db).is_some() {
                                    method_ty = TyId::app(tc.db, method_ty, arg);
                                } else {
                                    break;
                                }
                            }
                            method_ty
                        }
                        MethodCandidate::TraitMethod(cand)
                        | MethodCandidate::NeedsConfirmation(cand) => {
                            let inst = canonical_r_ty.extract_solution(&mut tc.table, cand.inst);
                            if matches!(candidate, MethodCandidate::NeedsConfirmation(_)) {
                                tc.env
                                    .register_confirmation(inst, path_expr_span.clone().into());
                            }
                            cand.method
                                .instantiate_with_inst(&mut tc.table, receiver_ty, inst)
                        }
                    };
                    ExprProp::new(tc.table.instantiate_to_term(method_ty), true)
                }
                PathRes::Mod(_) | PathRes::FuncParam(..) => todo!(),
            },
        }
    }

    fn type_check_method_call(self, tc: &mut TyChecker<'db>) -> ExprProp<'db> {
        let Partial::Present(expr_data) = self.data(tc.db) else {
            unreachable!()
        };
        let ExprDescription::MethodCall(receiver, method_name, generic_args, args) = expr_data else {
            unreachable!()
        };
        let call_span = self.span(tc.db).into_method_call_expr();
        let Some(method_name) = method_name.to_opt() else {
            return ExprProp::invalid(tc.db);
        };

        let receiver_prop = tc.check_expr_unknown(*receiver);
        if receiver_prop.ty.has_invalid(tc.db) {
            return ExprProp::invalid(tc.db);
        }

        let canonical_r_ty = Canonicalized::new(tc.db, receiver_prop.ty);
        let candidate = match select_method_candidate(
            tc.db,
            canonical_r_ty.value,
            method_name,
            tc.env.scope(),
            tc.env.assumptions(),
            None,
        ) {
            Ok(candidate) => candidate,
            Err(err) => {
                match err {
                    MethodSelectionError::AmbiguousTraitMethod(insts) => {
                        // Defer resolution using return-type constraints
                        let ret_ty = tc.fresh_ty();
                        let typed = ExprProp::new(ret_ty, true);
                        tc.env.type_expr(self.id(), typed);
                        // Instantiate candidates with fresh inference vars so
                        // later unifications can bind their parameters.
                        let cands: Vec<_> = insts
                            .into_iter()
                            .map(|inst| {
                                tc.table.instantiate_with_fresh_vars(
                                    crate::analysis::ty::binder::Binder::bind(inst),
                                )
                            })
                            .collect();

                        tc.env.register_pending_method(super::env::PendingMethod {
                            expr: self.id(),
                            recv_ty: receiver_prop.ty,
                            method_name,
                            candidates: cands,
                            span: call_span.method_name().into(),
                        });
                        return typed;
                    }
                    _ => {
                        let receiver_wrapped = self.body().wrap_expr(*receiver);
                        let diag = body_diag_from_method_selection_err(
                            tc.db,
                            err,
                            Spanned::new(
                                canonical_r_ty.value.value,
                                receiver_wrapped.span(tc.db).into(),
                            ),
                            Spanned::new(method_name, call_span.method_name().into()),
                        );
                        tc.push_diag(diag);
                        return ExprProp::invalid(tc.db);
                    }
                }
            }
        };

        let receiver_wrapped = self.body().wrap_expr(*receiver);
        let (func_ty, trait_inst) = match candidate {
            MethodCandidate::InherentMethod(func_def) => {
                let func_ty = TyId::func(tc.db, func_def);
                (tc.table.instantiate_to_term(func_ty), None)
            }

            MethodCandidate::TraitMethod(cand) => {
                let inst = canonical_r_ty.extract_solution(&mut tc.table, cand.inst);
                let func_ty =
                    cand.method
                        .instantiate_with_inst(&mut tc.table, receiver_prop.ty, inst);
                (func_ty, Some(inst))
            }

            MethodCandidate::NeedsConfirmation(cand) => {
                let inst = canonical_r_ty.extract_solution(&mut tc.table, cand.inst);
                tc.env
                    .register_confirmation(inst, call_span.clone().into());
                let trait_method = cand.method;
                let func_ty =
                    trait_method.instantiate_with_inst(&mut tc.table, receiver_prop.ty, inst);
                (func_ty, Some(inst))
            }
        };

        let mut callable = match Callable::new(
            tc.db,
            func_ty,
            receiver_wrapped.span(tc.db).into(),
            trait_inst,
        ) {
            Ok(callable) => callable,
            Err(diag) => {
                tc.push_diag(diag);
                return ExprProp::invalid(tc.db);
            }
        };

        if !callable.unify_generic_args(tc, *generic_args, call_span.clone().generic_args()) {
            return ExprProp::invalid(tc.db);
        }

        if !callable.func_def.is_method(tc.db) {
            let diag = BodyDiag::NotAMethod {
                span: call_span,
                receiver_ty: receiver_prop.ty,
                func_name: method_name,
                func_ty,
            };
            tc.push_diag(diag);
            return ExprProp::invalid(tc.db);
        }

        callable.check_args(
            tc,
            args,
            call_span.clone().args(),
            Some((*receiver, receiver_prop)),
        );

        // Check function constraints after instantiation
        callable.check_constraints(tc, call_span.method_name().into());

        let ret_ty = callable.ret_ty(tc.db);

        // Normalize the return type to resolve any associated types
        let normalized_ret_ty = tc.normalize_ty(ret_ty);
        tc.env.register_callable(self.id(), callable);
        ExprProp::new(normalized_ret_ty, true)
    }

    fn type_check_record_init(self, tc: &mut TyChecker<'db>) -> ExprProp<'db> {
        let Partial::Present(expr_data) = self.data(tc.db) else {
            unreachable!()
        };
        let ExprDescription::RecordInit(path, ..) = expr_data else {
            unreachable!()
        };
        let span = self.span(tc.db).into_record_init_expr();

        let Partial::Present(path) = path else {
            return ExprProp::invalid(tc.db);
        };

        let Ok(reso) = tc.resolve_path(*path, true, span.clone().path()) else {
            return ExprProp::invalid(tc.db);
        };

        match reso {
            PathRes::Ty(ty) | PathRes::TyAlias(_, ty) => {
                let record_like = RecordLike::from_ty(ty);
                if record_like.is_record(tc.db) {
                    tc.check_record_init_fields(&record_like, self.id());
                    ExprProp::new(ty, true)
                } else {
                    let diag =
                        BodyDiag::record_expected(tc.db, span.path().into(), Some(record_like));
                    tc.push_diag(diag);
                    ExprProp::invalid(tc.db)
                }
            }

            PathRes::Func(ty) | PathRes::Const(_, ty) => {
                let record_like = RecordLike::from_ty(ty);
                let diag =
                    BodyDiag::record_expected(tc.db, span.path().into(), Some(record_like));
                tc.push_diag(diag);
                ExprProp::invalid(tc.db)
            }
            PathRes::Method(..) | PathRes::FuncParam(..) => {
                let diag = BodyDiag::record_expected(tc.db, span.path().into(), None);
                tc.push_diag(diag);
                ExprProp::invalid(tc.db)
            }

            PathRes::EnumVariant(variant) => {
                let ty = variant.ty;
                let record_like = RecordLike::from_variant(variant);
                if record_like.is_record(tc.db) {
                    tc.check_record_init_fields(&record_like, self.id());
                    ExprProp::new(ty, true)
                } else {
                    let diag = BodyDiag::record_expected(tc.db, span.path().into(), None);
                    tc.push_diag(diag);

                    ExprProp::invalid(tc.db)
                }
            }
            PathRes::Mod(scope) => {
                let diag = BodyDiag::NotValue {
                    primary: span.into(),
                    given: Either::Left(scope.item()),
                };
                tc.push_diag(diag);
                ExprProp::invalid(tc.db)
            }
            PathRes::Trait(trait_) => {
                let diag = BodyDiag::NotValue {
                    primary: span.into(),
                    given: Either::Left(trait_.def(tc.db).trait_(tc.db).into()),
                };
                tc.push_diag(diag);
                ExprProp::invalid(tc.db)
            }
        }
    }
}

fn body_diag_from_method_selection_err<'db>(
    db: &'db dyn HirAnalysisDb,
    err: MethodSelectionError<'db>,
    receiver: Spanned<'db, TyId<'db>>,
    method: Spanned<'db, IdentId<'db>>,
) -> FuncBodyDiag<'db> {
    match err {
        MethodSelectionError::ReceiverTypeMustBeKnown => {
            BodyDiag::TypeMustBeKnown(receiver.span).into()
        }
        MethodSelectionError::AmbiguousInherentMethod(candidates) => {
            BodyDiag::AmbiguousInherentMethodCall {
                primary: method.span,
                method_name: method.data,
                candidates,
            }
            .into()
        }

        MethodSelectionError::AmbiguousTraitMethod(traits) => BodyDiag::AmbiguousTrait {
            primary: method.span,
            method_name: method.data,
            traits,
        }
        .into(),

        MethodSelectionError::NotFound => {
            let base_ty = receiver.data.base_ty(db);
            PathResDiag::MethodNotFound {
                primary: method.span,
                method_name: method.data,
                receiver: Either::Left(base_ty),
            }
            .into()
        }

        MethodSelectionError::InvisibleInherentMethod(func) => {
            PathResDiag::Invisible(method.span, method.data, func.name_span(db).into()).into()
        }

        MethodSelectionError::InvisibleTraitMethod(traits) => BodyDiag::InvisibleAmbiguousTrait {
            primary: method.span,
            traits,
        }
        .into(),
    }
}

fn resolve_ident_expr<'db>(
    db: &'db dyn HirAnalysisDb,
    env: &TyCheckEnv<'db>,
    path: PathId<'db>,
) -> ResolvedPathInBody<'db> {
    let ident = path.ident(db).unwrap();

    let resolve_bucket = |bucket: &NameResBucket<'db>, scope| {
        let Ok(res) = bucket.pick_any(&[NameDomain::VALUE, NameDomain::TYPE]) else {
            return ResolvedPathInBody::Invalid;
        };
        let Ok(reso) = resolve_name_res(db, res, None, path, scope, env.assumptions()) else {
            return ResolvedPathInBody::Invalid;
        };
        ResolvedPathInBody::Reso(reso)
    };

    let mut current_idx = env.current_block_idx();

    loop {
        let block = env.get_block(current_idx);
        if let Some(binding) = block.lookup_var(ident) {
            return ResolvedPathInBody::Binding(binding);
        }

        let scope = block.scope;
        let directive = QueryDirective::new().disallow_lex();
        let query = EarlyNameQueryId::new(db, ident, scope, directive);
        let bucket = resolve_query(db, query);

        let resolved = resolve_bucket(bucket, scope);
        if matches!(resolved, ResolvedPathInBody::Invalid) {
            if current_idx == 0 {
                break;
            } else {
                current_idx -= 1;
            }
        } else {
            return resolved;
        }
    }

    let query = EarlyNameQueryId::new(db, ident, env.body().scope(), QueryDirective::default());
    let bucket = resolve_query(db, query);
    match resolve_bucket(bucket, env.scope()) {
        ResolvedPathInBody::Invalid => ResolvedPathInBody::NewBinding(ident),
        r => r,
    }
}

/// This traits are intended to be implemented by the operators that can work as
/// a syntax sugar for a trait method. For example, binary `+` operator
/// implements this trait to be able to work as a syntax sugar for
/// `core::ops::Add` trait method.
///
/// TODO: We need to refine this trait definition to connect core library traits
/// smoothly.
pub(crate) trait TraitOps {
    fn trait_path<'db>(&self, db: &'db dyn HirAnalysisDb) -> PathId<'db> {
        let path = core_ops_path(db);
        path.push_ident(db, self.trait_name(db))
    }

    fn trait_name<'db>(&self, db: &'db dyn HirAnalysisDb) -> IdentId<'db> {
        self.triple(db)[0]
    }

    fn trait_method<'db>(&self, db: &'db dyn HirAnalysisDb) -> IdentId<'db> {
        self.triple(db)[1]
    }

    fn op_symbol<'db>(&self, db: &'db dyn HirAnalysisDb) -> IdentId<'db> {
        self.triple(db)[2]
    }

    fn triple<'db>(&self, db: &'db dyn HirAnalysisDb) -> [IdentId<'db>; 3];
}

impl TraitOps for UnOp {
    fn triple<'db>(&self, db: &'db dyn HirAnalysisDb) -> [IdentId<'db>; 3] {
        let triple = match self {
            UnOp::Plus => ["UnaryPlus", "add", "+"],
            UnOp::Minus => ["Neg", "neg", "-"],
            UnOp::Not => ["Not", "not", "!"],
            UnOp::BitNot => ["BitNot", "bit_not", "~"],
        };

        triple.map(|s| IdentId::new(db, s.to_string()))
    }
}

impl TraitOps for BinOp {
    fn triple<'db>(&self, db: &'db dyn HirAnalysisDb) -> [IdentId<'db>; 3] {
        let triple = match self {
            BinOp::Arith(arith_op) => {
                use ArithBinOp::*;

                match arith_op {
                    Add => ["Add", "add", "+"],
                    Sub => ["Sub", "sub", "-"],
                    Mul => ["Mul", "mul", "*"],
                    Div => ["Div", "div", "/"],
                    Rem => ["Rem", "rem", "%"],
                    Pow => ["Pow", "pow", "**"],
                    LShift => ["Shl", "shl", "<<"],
                    RShift => ["Shr", "shr", ">>"],
                    BitAnd => ["BitAnd", "bitand", "&"],
                    BitOr => ["BitOr", "bitor", "|"],
                    BitXor => ["BitXor", "bitxor", "^"],
                }
            }

            BinOp::Comp(comp_op) => {
                use crate::hir_def::CompBinOp::*;

                // Comp
                match comp_op {
                    Eq => ["Eq", "eq", "=="],
                    NotEq => ["Eq", "ne", "!="],
                    Lt => ["Ord", "lt", "<"],
                    LtEq => ["Ord", "le", "<="],
                    Gt => ["Ord", "gt", ">"],
                    GtEq => ["Ord", "ge", ">="],
                }
            }

            BinOp::Logical(_) => {
                unreachable!()
            }

            BinOp::Index => ["Index", "index", "[]"],
        };

        triple.map(|s| IdentId::new(db, s.to_string()))
    }
}

#[derive(Clone, Copy, Debug)]
struct AugAssignOp(ArithBinOp);

impl TraitOps for AugAssignOp {
    fn triple<'db>(&self, db: &'db dyn HirAnalysisDb) -> [IdentId<'db>; 3] {
        use ArithBinOp::*;
        let triple = match self.0 {
            Add => ["AddAssign", "add_assign", "+="],
            Sub => ["SubAssign", "sub_assign", "-="],
            Mul => ["MulAssign", "mul_assign", "*="],
            Div => ["DivAssign", "div_assign", "/="],
            Rem => ["RemAssign", "rem_assign", "%="],
            Pow => ["PowAssign", "pow_assign", "**="],
            LShift => ["ShlAssign", "shl_assign", "<<="],
            RShift => ["ShrAssign", "shr_assign", ">>="],
            BitAnd => ["BitAndAssign", "bitand_assign", "&="],
            BitOr => ["BitOrAssign", "bitor_assign", "|="],
            BitXor => ["BitXorAssign", "bitxor_assign", "^="],
        };

        triple.map(|s| IdentId::new(db, s.to_string()))
    }
}

fn core_ops_path(db: &dyn HirAnalysisDb) -> PathId<'_> {
    let core = IdentId::new(db, "core".to_string());
    let ops_ = IdentId::new(db, "ops".to_string());
    PathId::from_ident(db, core).push_ident(db, ops_)
}
