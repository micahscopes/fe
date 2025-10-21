use parser::ast;

use super::FileLowerCtxt;
use crate::{
    hir_def::{
        Body, BodyKind, BodySourceMap, ExprId, NodeStore, Partial, PatId, StmtId,
        TrackedItemId, TrackedItemVariant,
        ExprDescription, StmtDescription, PatDescription,
        Expr,
    },
    span::HirOrigin,
};

impl<'db> Body<'db> {
    pub(super) fn lower_ast(f_ctxt: &mut FileLowerCtxt<'db>, ast: ast::Expr, owner_id: TrackedItemId<'db>) -> Self {
        let id = f_ctxt.joined_id(TrackedItemVariant::FuncBody);
        let mut ctxt = BodyCtxt::new(f_ctxt, id);
        let body_expr = Expr::lower_ast(&mut ctxt, ast.clone());
        ctxt.build(&ast, body_expr, BodyKind::FuncBody, Some(owner_id))
    }

    pub(super) fn lower_ast_nameless(f_ctxt: &mut FileLowerCtxt<'db>, ast: ast::Expr) -> Self {
        let id = f_ctxt.joined_id(TrackedItemVariant::NamelessBody);
        let mut ctxt = BodyCtxt::new(f_ctxt, id);
        let body_expr = Expr::lower_ast(&mut ctxt, ast.clone());
        ctxt.build(&ast, body_expr, BodyKind::Anonymous, None)
    }
}

pub(super) struct BodyCtxt<'ctxt, 'db> {
    pub(super) f_ctxt: &'ctxt mut FileLowerCtxt<'db>,
    pub(super) id: TrackedItemId<'db>,

    pub(super) stmts: NodeStore<StmtId, Partial<StmtDescription<'db>>>,
    pub(super) exprs: NodeStore<ExprId, Partial<ExprDescription<'db>>>,
    pub(super) pats: NodeStore<PatId, Partial<PatDescription<'db>>>,
    pub(super) source_map: BodySourceMap,
}

impl<'ctxt, 'db> BodyCtxt<'ctxt, 'db> {
    pub(super) fn push_expr(&mut self, expr: ExprDescription<'db>, origin: HirOrigin<ast::Expr>) -> ExprId {
        let expr_id = self.exprs.push(Partial::Present(expr));
        self.source_map.expr_map.insert(expr_id, origin);

        expr_id
    }

    pub(super) fn push_invalid_expr(&mut self, origin: HirOrigin<ast::Expr>) -> ExprId {
        let expr_id = self.exprs.push(Partial::Absent);
        self.source_map.expr_map.insert(expr_id, origin);

        expr_id
    }

    pub(super) fn push_missing_expr(&mut self) -> ExprId {
        let expr_id = self.exprs.push(Partial::Absent);
        self.source_map.expr_map.insert(expr_id, HirOrigin::None);
        expr_id
    }

    pub(super) fn push_stmt(&mut self, stmt: StmtDescription<'db>, origin: HirOrigin<ast::Stmt>) -> StmtId {
        let stmt_id = self.stmts.push(Partial::Present(stmt));
        self.source_map.stmt_map.insert(stmt_id, origin);

        stmt_id
    }

    pub(super) fn push_pat(&mut self, pat: PatDescription<'db>, origin: HirOrigin<ast::Pat>) -> PatId {
        let pat_id = self.pats.push(Partial::Present(pat));
        self.source_map.pat_map.insert(pat_id, origin);
        pat_id
    }

    pub(super) fn push_missing_pat(&mut self) -> PatId {
        let pat_id = self.pats.push(Partial::Absent);
        self.source_map.pat_map.insert(pat_id, HirOrigin::None);
        pat_id
    }

    fn new(f_ctxt: &'ctxt mut FileLowerCtxt<'db>, id: TrackedItemId<'db>) -> Self {
        f_ctxt.enter_body_scope(id);
        Self {
            f_ctxt,
            id,
            stmts: NodeStore::new(),
            exprs: NodeStore::new(),
            pats: NodeStore::new(),
            source_map: BodySourceMap::default(),
        }
    }

    fn build(self, ast: &ast::Expr, body_expr: ExprId, body_kind: BodyKind, owner_id: Option<TrackedItemId<'db>>) -> Body<'db> {
        let origin = HirOrigin::raw(ast);
        let body = Body::new(
            self.f_ctxt.db(),
            self.id,
            body_expr,
            body_kind,
            owner_id,
            self.stmts,
            self.exprs,
            self.pats,
            self.f_ctxt.top_mod(),
            self.source_map,
            origin,
        );

        self.f_ctxt.leave_item_scope(body);
        body
    }
}
