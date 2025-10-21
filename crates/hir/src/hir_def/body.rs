// This is necessary because `salsa::tracked` structs generates a
// constructor
// that may take many arguments depending on the number of fields in the struct.
#![allow(clippy::too_many_arguments)]

use std::hash::Hash;

use common::indexmap::IndexMap;
use cranelift_entity::{EntityRef, PrimaryMap, SecondaryMap};
use parser::ast::{self, prelude::*};
use rustc_hash::FxHashMap;
use salsa::Update;

use super::{
    Const, ExprId, Func, Partial, PatId, StmtId, TopLevelMod, TrackedItemId,
    expr::ExprDescription, stmt::StmtDescription, pat::PatDescription,
    scope_graph::ScopeId,
};
use crate::{
    HirDb,
    span::{HirOrigin, item::LazyBodySpan},
    visitor::prelude::*,
};

#[salsa::tracked]
#[derive(Debug)]
pub struct Body<'db> {
    //    #[id]
    id: TrackedItemId<'db>,

    /// The expression that evaluates to the value of the body.
    /// In case of a function body, this is always be the block expression.
    pub expr: ExprId,

    pub body_kind: BodyKind,

    /// The item that owns this body (Func or Const).
    /// For anonymous bodies (e.g., in array lengths), this is None.
    pub owner: Option<BodyOwner<'db>>,

    #[return_ref]
    pub stmts: NodeStore<StmtId, Partial<StmtDescription<'db>>>,
    #[return_ref]
    pub exprs: NodeStore<ExprId, Partial<ExprDescription<'db>>>,
    #[return_ref]
    pub pats: NodeStore<PatId, Partial<PatDescription<'db>>>,
    pub top_mod: TopLevelMod<'db>,

    #[return_ref]
    pub(crate) source_map: BodySourceMap,
    #[return_ref]
    pub(crate) origin: HirOrigin<ast::Expr>,
}

impl<'db> Body<'db> {
    pub fn span(self) -> LazyBodySpan<'db> {
        LazyBodySpan::new(self)
    }

    pub fn scope(self) -> ScopeId<'db> {
        ScopeId::from_item(self.into())
    }

    /// Returns the function that owns this body, if any.
    pub fn owner_func(self, db: &'db dyn HirDb) -> Option<Func<'db>> {
        self.computed_owner(db).and_then(|owner| match owner {
            BodyOwner::Func(func) => Some(func),
            _ => None,
        })
    }

    /// Returns the const that owns this body, if any.
    pub fn owner_const(self, db: &'db dyn HirDb) -> Option<Const<'db>> {
        self.computed_owner(db).and_then(|owner| match owner {
            BodyOwner::Const(const_) => Some(const_),
            _ => None,
        })
    }

    /// Creates a context-rich wrapper for an expression in this body.
    pub fn wrap_expr(self, id: ExprId) -> Expr<'db> {
        Expr::new(self, id)
    }

    /// Creates a context-rich wrapper for a statement in this body.
    pub fn wrap_stmt(self, id: StmtId) -> Stmt<'db> {
        Stmt::new(self, id)
    }

    /// Creates a context-rich wrapper for a pattern in this body.
    pub fn wrap_pat(self, id: PatId) -> Pat<'db> {
        Pat::new(self, id)
    }

    #[doc(hidden)]
    /// Returns the order of the blocks in the body in lexical order.
    /// e.g.,
    /// ```fe
    /// fn foo() { // 0
    ///     ...
    ///     { // 1
    ///         ...
    ///         { // 2
    ///             ...
    ///         }
    ///     }
    /// }
    ///
    ///
    /// Currently, this is only used for testing.
    /// When it turns out to be generally useful, we need to consider to let
    /// salsa track this method.
    pub fn iter_block(self, db: &dyn HirDb) -> FxHashMap<ExprId, usize> {
        BlockOrderCalculator::new(db, self).calculate()
    }
}

#[salsa::tracked]
impl<'db> Body<'db> {
    /// Computes the owner of this body by searching through all functions and consts in the module.
    #[salsa::tracked]
    pub fn computed_owner(self, db: &'db dyn HirDb) -> Option<BodyOwner<'db>> {
        // Search through all funcs and consts in the top module
        let top_mod = self.top_mod(db);

        // Check all functions
        for func in top_mod.all_funcs(db) {
            if let Some(func_body) = func.body(db) {
                if func_body == self {
                    return Some(BodyOwner::Func(*func));
                }
            }
        }

        // Check all consts
        for item in top_mod.all_items(db) {
            if let crate::hir_def::ItemKind::Const(const_) = item {
                if let Some(const_body) = const_.body(db).to_opt() {
                    if const_body == self {
                        return Some(BodyOwner::Const(*const_));
                    }
                }
            }
        }

        None
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BodyKind {
    FuncBody,
    Anonymous,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, salsa::Update)]
pub enum BodyOwner<'db> {
    Func(Func<'db>),
    Const(Const<'db>),
}

/// Context-rich wrapper for expressions that carries the body context.
/// This enables navigation from expr → body → func without manual threading.
///
/// Note: This is the public-facing wrapper type. The underlying expression
/// data enum is `crate::hir_def::expr::ExprDescription`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Expr<'db> {
    body: Body<'db>,
    id: ExprId,
}

impl<'db> Expr<'db> {
    pub fn new(body: Body<'db>, id: ExprId) -> Self {
        Self { body, id }
    }

    /// Returns the body containing this expression.
    pub fn body(self) -> Body<'db> {
        self.body
    }

    /// Returns the expression ID.
    pub fn id(self) -> ExprId {
        self.id
    }

    /// Returns the expression data.
    ///
    /// Note: The return type uses the `ExprDescription` enum from `crate::hir_def::expr`,
    /// not this wrapper struct.
    pub fn data(self, db: &'db dyn HirDb) -> &'db Partial<crate::hir_def::expr::ExprDescription<'db>> {
        &self.body.exprs(db)[self.id]
    }

    /// Returns the scope containing this expression.
    /// For now, delegates to the body's scope. In the future, this could
    /// resolve to the nearest enclosing block scope.
    pub fn scope(self, _db: &'db dyn HirDb) -> ScopeId<'db> {
        // TODO: Look up expr's specific scope from a scope tree
        self.body.scope()
    }

    /// Returns the function containing this expression, if any.
    pub fn containing_func(self, db: &'db dyn HirDb) -> Option<Func<'db>> {
        self.body.owner_func(db)
    }

    /// Returns the const containing this expression, if any.
    pub fn containing_const(self, db: &'db dyn HirDb) -> Option<Const<'db>> {
        self.body.owner_const(db)
    }

    /// Returns the lazy span for this expression.
    pub fn span(self, _db: &'db dyn HirDb) -> crate::span::expr::LazyExprSpan<'db> {
        self.id.span(self.body)
    }
}

/// Context-rich wrapper for statements that carries the body context.
///
/// Note: This is the public-facing wrapper type. The underlying statement
/// data enum is `crate::hir_def::stmt::StmtDescription`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Stmt<'db> {
    body: Body<'db>,
    id: StmtId,
}

impl<'db> Stmt<'db> {
    pub fn new(body: Body<'db>, id: StmtId) -> Self {
        Self { body, id }
    }

    pub fn body(self) -> Body<'db> {
        self.body
    }

    pub fn id(self) -> StmtId {
        self.id
    }

    pub fn data(self, db: &'db dyn HirDb) -> &'db Partial<crate::hir_def::stmt::StmtDescription<'db>> {
        &self.body.stmts(db)[self.id]
    }

    pub fn scope(self, _db: &'db dyn HirDb) -> ScopeId<'db> {
        // TODO: Look up stmt's specific scope from a scope tree
        self.body.scope()
    }

    pub fn containing_func(self, db: &'db dyn HirDb) -> Option<Func<'db>> {
        self.body.owner_func(db)
    }
}

/// Context-rich wrapper for patterns that carries the body context.
///
/// Note: This is the public-facing wrapper type. The underlying pattern
/// data enum is `crate::hir_def::pat::PatDescription`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Pat<'db> {
    body: Body<'db>,
    id: PatId,
}

impl<'db> Pat<'db> {
    pub fn new(body: Body<'db>, id: PatId) -> Self {
        Self { body, id }
    }

    pub fn body(self) -> Body<'db> {
        self.body
    }

    pub fn id(self) -> PatId {
        self.id
    }

    pub fn data(self, db: &'db dyn HirDb) -> &'db Partial<crate::hir_def::pat::PatDescription<'db>> {
        &self.body.pats(db)[self.id]
    }

    pub fn scope(self, _db: &'db dyn HirDb) -> ScopeId<'db> {
        // TODO: Look up pat's specific scope from a scope tree
        self.body.scope()
    }

    pub fn containing_func(self, db: &'db dyn HirDb) -> Option<Func<'db>> {
        self.body.owner_func(db)
    }
}

#[derive(Debug, Hash, Clone)]
pub struct NodeStore<K, V>(PrimaryMap<K, V>)
where
    K: EntityRef;

impl<K, V> NodeStore<K, V>
where
    K: EntityRef,
{
    pub fn new() -> Self {
        Self(PrimaryMap::new())
    }
}
impl<K, V> Default for NodeStore<K, V>
where
    K: EntityRef,
{
    fn default() -> Self {
        Self::new()
    }
}

impl<K, V> std::ops::Deref for NodeStore<K, V>
where
    K: EntityRef,
{
    type Target = PrimaryMap<K, V>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<K, V> std::ops::DerefMut for NodeStore<K, V>
where
    K: EntityRef,
{
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl<K, V> std::ops::Index<K> for NodeStore<K, V>
where
    K: EntityRef,
{
    type Output = V;

    fn index(&self, k: K) -> &V {
        &self.0[k]
    }
}

unsafe impl<K, V> Update for NodeStore<K, V>
where
    K: EntityRef + Update,
    V: Update,
{
    unsafe fn maybe_update(old_ptr: *mut Self, new_val: Self) -> bool {
        unsafe {
            let old_val = &mut *old_ptr;
            if old_val.len() != new_val.len() {
                *old_val = new_val;
                return true;
            }

            let mut changed = false;
            for (k, v) in new_val.0.into_iter() {
                let old_val = &mut old_val[k];
                changed |= Update::maybe_update(old_val, v);
            }

            changed
        }
    }
}

/// Mutable indexing into an `PrimaryMap`.
impl<K, V> std::ops::IndexMut<K> for NodeStore<K, V>
where
    K: EntityRef,
{
    fn index_mut(&mut self, k: K) -> &mut V {
        &mut self.0[k]
    }
}

pub trait SourceAst: AstNode + Clone + Hash + PartialEq + Eq {}
impl<T> SourceAst for T where T: AstNode + Clone + Hash + PartialEq + Eq {}

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct BodySourceMap {
    pub stmt_map: SourceNodeMap<ast::Stmt, StmtId>,
    pub expr_map: SourceNodeMap<ast::Expr, ExprId>,
    pub pat_map: SourceNodeMap<ast::Pat, PatId>,
}

#[allow(clippy::derived_hash_with_manual_eq)]
#[derive(Clone, Debug, Hash)]
pub struct SourceNodeMap<Ast, Node>
where
    Ast: SourceAst,
    Node: EntityRef,
{
    pub node_to_source: SecondaryMap<Node, HirOrigin<Ast>>,
    pub source_to_node: IndexMap<HirOrigin<Ast>, Node>,
}

impl<Ast, Node> SourceNodeMap<Ast, Node>
where
    Ast: SourceAst,
    Node: EntityRef,
{
    pub(crate) fn insert(&mut self, node: Node, ast: HirOrigin<Ast>) {
        self.node_to_source[node] = ast.clone();
        self.source_to_node.insert(ast, node);
    }

    pub(crate) fn node_to_source(&self, node: Node) -> &HirOrigin<Ast> {
        &self.node_to_source[node]
    }
}

impl<Ast, Node> PartialEq for SourceNodeMap<Ast, Node>
where
    Ast: SourceAst,
    Node: EntityRef,
{
    fn eq(&self, other: &Self) -> bool {
        self.node_to_source == other.node_to_source
    }
}

impl<Ast, Node> Eq for SourceNodeMap<Ast, Node>
where
    Ast: SourceAst,
    Node: EntityRef,
{
}

impl<Ast, Node> Default for SourceNodeMap<Ast, Node>
where
    Ast: SourceAst,
    Node: EntityRef,
{
    fn default() -> Self {
        Self {
            source_to_node: IndexMap::default(),
            node_to_source: SecondaryMap::new(),
        }
    }
}

struct BlockOrderCalculator<'db> {
    db: &'db dyn HirDb,
    order: FxHashMap<ExprId, usize>,
    body: Body<'db>,
    fresh_number: usize,
}

impl<'db> Visitor<'db> for BlockOrderCalculator<'db> {
    fn visit_expr(
        &mut self,
        ctxt: &mut crate::visitor::VisitorCtxt<'db, crate::span::expr::LazyExprSpan<'db>>,
        expr: ExprId,
        expr_data: &ExprDescription<'db>,
    ) {
        if ctxt.body() == self.body && matches!(expr_data, ExprDescription::Block(..)) {
            self.order.insert(expr, self.fresh_number);
            self.fresh_number += 1;
        }

        walk_expr(self, ctxt, expr)
    }
}

impl<'db> BlockOrderCalculator<'db> {
    fn new(db: &'db dyn HirDb, body: Body<'db>) -> Self {
        Self {
            db,
            order: FxHashMap::default(),
            body,
            fresh_number: 0,
        }
    }

    fn calculate(mut self) -> FxHashMap<ExprId, usize> {
        let expr = self.body.expr(self.db);
        let Partial::Present(expr_data) = expr.data(self.db, self.body) else {
            return self.order;
        };

        let mut ctxt = VisitorCtxt::with_expr(self.db, self.body.scope(), self.body, expr);
        self.visit_expr(&mut ctxt, expr, expr_data);
        self.order
    }
}
