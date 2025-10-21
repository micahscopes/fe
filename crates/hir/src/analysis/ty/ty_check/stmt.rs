use crate::hir_def::{IdentId, Partial, StmtId, StmtDescription, Stmt};

use super::TyChecker;
use crate::analysis::ty::{
    diagnostics::BodyDiag,
    fold::TyFoldable,
    ty_def::{InvalidCause, TyId},
};

impl<'db> TyChecker<'db> {
    /// Legacy wrapper that delegates to Stmt::type_check.
    /// TODO: Migrate all call sites to use stmt.type_check(tc, expected) directly.
    pub(super) fn check_stmt(&mut self, stmt: StmtId, expected: TyId<'db>) -> TyId<'db> {
        self.body().wrap_stmt(stmt).type_check(self, expected)
    }
}

impl<'db> Stmt<'db> {
    pub(super) fn type_check(self, tc: &mut TyChecker<'db>, expected: TyId<'db>) -> TyId<'db> {
        let Partial::Present(stmt_data) = self.data(tc.db) else {
            return TyId::invalid(tc.db, InvalidCause::ParseError);
        };

        match stmt_data {
            StmtDescription::Let(..) => self.type_check_let(tc),
            StmtDescription::For(..) => self.type_check_for(tc),
            StmtDescription::While(..) => self.type_check_while(tc),
            StmtDescription::Continue => self.type_check_continue(tc),
            StmtDescription::Break => self.type_check_break(tc),
            StmtDescription::Return(..) => self.type_check_return(tc),
            StmtDescription::Expr(expr) => self.body().wrap_expr(*expr).type_check(tc, expected).ty,
        }
    }

    fn type_check_let(self, tc: &mut TyChecker<'db>) -> TyId<'db> {
        let Partial::Present(stmt_data) = self.data(tc.db) else {
            unreachable!()
        };
        let StmtDescription::Let(pat, ascription, expr) = stmt_data else {
            unreachable!()
        };

        let span = self.id().span(self.body()).into_let_stmt();

        let ascription = match ascription {
            Some(ty) => tc.lower_ty(*ty, span.ty(), true),
            None => tc.fresh_ty(),
        };

        if let Some(expr) = expr {
            self.body().wrap_expr(*expr).type_check(tc, ascription);
        }

        tc.check_pat(*pat, ascription);
        tc.env.flush_pending_bindings();
        TyId::unit(tc.db)
    }

    fn type_check_for(self, tc: &mut TyChecker<'db>) -> TyId<'db> {
        let Partial::Present(stmt_data) = self.data(tc.db) else {
            unreachable!()
        };
        let StmtDescription::For(pat, expr, body) = stmt_data else {
            unreachable!()
        };

        let expr_ty = tc.fresh_ty();
        let typed_expr = self.body()
            .wrap_expr(*expr)
            .type_check(tc, expr_ty)
            .fold_with(tc.db, &mut tc.table);
        let expr_ty = typed_expr.ty;

        let (base, arg) = expr_ty.decompose_ty_app(tc.db);
        // TODO: We can generalize this by just checking the `expr_ty` implements
        // `Iterator` trait when `std::iter::Iterator` is implemented.
        let elem_ty = if base.is_array(tc.db) {
            arg[0]
        } else if base.has_invalid(tc.db) {
            TyId::invalid(tc.db, InvalidCause::Other)
        } else if base.is_ty_var(tc.db) {
            let expr_wrapped = self.body().wrap_expr(*expr);
            let diag = BodyDiag::TypeMustBeKnown(expr_wrapped.span(tc.db).into());
            tc.push_diag(diag);
            TyId::invalid(tc.db, InvalidCause::Other)
        } else {
            let expr_wrapped = self.body().wrap_expr(*expr);
            let diag = BodyDiag::TraitNotImplemented {
                primary: expr_wrapped.span(tc.db).into(),
                ty: expr_ty.pretty_print(tc.db).to_string(),
                trait_name: IdentId::new(tc.db, "Iterator".to_string()),
            };
            tc.push_diag(diag);

            TyId::invalid(tc.db, InvalidCause::Other)
        };

        tc.check_pat(*pat, elem_ty);

        tc.env.enter_loop(self.id());
        tc.env.enter_scope(*body);
        tc.env.flush_pending_bindings();

        let body_ty = tc.fresh_ty();
        self.body().wrap_expr(*body).type_check(tc, body_ty);

        tc.env.leave_scope();
        tc.env.leave_loop();

        TyId::unit(tc.db)
    }

    fn type_check_while(self, tc: &mut TyChecker<'db>) -> TyId<'db> {
        let Partial::Present(stmt_data) = self.data(tc.db) else {
            unreachable!()
        };
        let StmtDescription::While(cond, body) = stmt_data else {
            unreachable!()
        };

        self.body().wrap_expr(*cond).type_check(tc, TyId::bool(tc.db));

        tc.env.enter_loop(self.id());
        self.body().wrap_expr(*body).type_check(tc, TyId::unit(tc.db));
        tc.env.leave_loop();

        TyId::unit(tc.db)
    }

    fn type_check_continue(self, tc: &mut TyChecker<'db>) -> TyId<'db> {
        let Partial::Present(stmt_data) = self.data(tc.db) else {
            unreachable!()
        };
        assert!(matches!(stmt_data, StmtDescription::Continue));

        if tc.env.current_loop().is_none() {
            let span = self.id().span(self.body());
            let diag = BodyDiag::LoopControlOutsideOfLoop {
                primary: span.into(),
                is_break: false,
            };
            tc.push_diag(diag);
        }

        TyId::never(tc.db)
    }

    fn type_check_break(self, tc: &mut TyChecker<'db>) -> TyId<'db> {
        let Partial::Present(stmt_data) = self.data(tc.db) else {
            unreachable!()
        };
        assert!(matches!(stmt_data, StmtDescription::Break));

        if tc.env.current_loop().is_none() {
            let span = self.id().span(self.body());
            let diag = BodyDiag::LoopControlOutsideOfLoop {
                primary: span.into(),
                is_break: true,
            };
            tc.push_diag(diag);
        }

        TyId::never(tc.db)
    }

    fn type_check_return(self, tc: &mut TyChecker<'db>) -> TyId<'db> {
        let Partial::Present(stmt_data) = self.data(tc.db) else {
            unreachable!()
        };
        let StmtDescription::Return(expr) = stmt_data else {
            unreachable!()
        };

        let returned_ty = if let Some(expr) = expr {
            let returned_ty = tc.fresh_ty();
            self.body().wrap_expr(*expr).type_check(tc, returned_ty);
            returned_ty.fold_with(tc.db, &mut tc.table)
        } else {
            TyId::unit(tc.db)
        };

        if tc.table.unify(returned_ty, tc.expected).is_err() {
            let func = tc.env.func();
            let span = self.id().span(self.body());
            let diag = BodyDiag::ReturnedTypeMismatch {
                primary: span.into(),
                actual: returned_ty,
                expected: tc.expected,
                func: func.map(|f| f.hir_func_def(tc.db).unwrap()),
            };

            tc.push_diag(diag);
        }

        TyId::never(tc.db)
    }
}
