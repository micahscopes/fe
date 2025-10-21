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

    /// The TrackedItemId of the item that owns this body (Func or Const).
    /// For anonymous bodies (e.g., in array lengths), this is None.
    /// Use `resolved_owner()` query to get the actual BodyOwner.
    owner_id: Option<TrackedItemId<'db>>,

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
    /// Resolves the owner TrackedItemId to the actual BodyOwner (Func or Const).
    /// Returns None for anonymous bodies or if resolution fails.
    #[salsa::tracked]
    pub fn resolved_owner(self, db: &'db dyn HirDb) -> Option<BodyOwner<'db>> {
        use crate::hir_def::{TrackedItemVariant};

        let owner_id = self.owner_id(db)?;
        let top_mod = self.top_mod(db);

        // Extract the variant to determine if it's a Func or Const
        // The owner_id might be Joined(Func(...), ...) or just Func(...)
        let mut variant = owner_id.variant_kind(db);

        // Unwrap Joined variants to get to the actual item
        while let TrackedItemVariant::Joined(left, _right) = variant {
            variant = *left;
        }

        match variant {
            TrackedItemVariant::Func(name) => {
                // Find the function with this name
                for func in top_mod.all_funcs(db) {
                    if func.name(db) == name {
                        return Some(BodyOwner::Func(*func));
                    }
                }
            }
            TrackedItemVariant::Const(name) => {
                // Find the const with this name
                for item in top_mod.all_items(db) {
                    if let crate::hir_def::ItemKind::Const(const_) = item {
                        if const_.name(db) == name {
                            return Some(BodyOwner::Const(*const_));
                        }
                    }
                }
            }
            _ => {}
        }

        None
    }

    /// Computes the owner of this body by searching through all functions and consts in the module.
    /// DEPRECATED: Use resolved_owner() instead once owner_id is properly wired through lowering.
    #[salsa::tracked]
    pub fn computed_owner(self, db: &'db dyn HirDb) -> Option<BodyOwner<'db>> {
        // First try the new path
        if let Some(owner) = self.resolved_owner(db) {
            return Some(owner);
        }

        // Fall back to O(n) search if owner_id wasn't set
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

/// Helper to find the parent expression of a given expression by walking the body.
fn find_expr_parent(db: &dyn HirDb, body: Body<'_>, target: ExprId) -> Option<ExprId> {
    find_expr_parent_recursive(db, body, body.expr(db), target)
}

fn find_expr_parent_recursive(db: &dyn HirDb, body: Body<'_>, current: ExprId, target: ExprId) -> Option<ExprId> {
    use crate::hir_def::expr::ExprDescription;

    let data = &body.exprs(db)[current];
    let Partial::Present(expr_data) = data else {
        return None;
    };

    // Check all child expressions
    let check_child = |child: ExprId| -> Option<ExprId> {
        if child == target {
            Some(current)
        } else {
            find_expr_parent_recursive(db, body, child, target)
        }
    };

    match expr_data {
        ExprDescription::Block(stmts) => {
            // Check expressions in statements
            for &stmt_id in stmts.iter() {
                if let Some(parent) = find_stmt_for_expr(db, body, stmt_id, target) {
                    return Some(parent);
                }
            }
            None
        }
        ExprDescription::If(cond, then_branch, else_branch) => {
            check_child(*cond)
                .or_else(|| check_child(*then_branch))
                .or_else(|| else_branch.and_then(|e| check_child(e)))
        }
        ExprDescription::Match(scrutinee, arms) => {
            check_child(*scrutinee).or_else(|| {
                if let Partial::Present(arms_vec) = arms {
                    arms_vec.iter().find_map(|arm| check_child(arm.body))
                } else {
                    None
                }
            })
        }
        ExprDescription::Call(callee, args) => {
            check_child(*callee).or_else(|| args.iter().find_map(|arg| check_child(arg.expr)))
        }
        ExprDescription::MethodCall(receiver, _, _, args) => {
            check_child(*receiver).or_else(|| args.iter().find_map(|arg| check_child(arg.expr)))
        }
        ExprDescription::Un(operand, _) => check_child(*operand),
        ExprDescription::Bin(lhs, rhs, _) => check_child(*lhs).or_else(|| check_child(*rhs)),
        ExprDescription::RecordInit(_, fields) => {
            fields.iter().find_map(|field| check_child(field.expr))
        }
        ExprDescription::Field(base, _) => check_child(*base),
        ExprDescription::Tuple(elems) => elems.iter().find_map(|&elem| check_child(elem)),
        ExprDescription::Array(elems) => elems.iter().find_map(|&elem| check_child(elem)),
        ExprDescription::ArrayRep(elem, _) => check_child(*elem),
        ExprDescription::AugAssign(lhs, rhs, _) => check_child(*lhs).or_else(|| check_child(*rhs)),
        ExprDescription::Assign(lhs, rhs) => check_child(*lhs).or_else(|| check_child(*rhs)),
        ExprDescription::Path(_) | ExprDescription::Lit(_) => None,
    }
}

fn find_stmt_for_expr(db: &dyn HirDb, body: Body<'_>, stmt_id: StmtId, target: ExprId) -> Option<ExprId> {
    use crate::hir_def::stmt::StmtDescription;

    let data = &body.stmts(db)[stmt_id];
    let Partial::Present(stmt_data) = data else {
        return None;
    };

    match stmt_data {
        StmtDescription::Let(_, _, Some(init)) => {
            find_expr_parent_recursive(db, body, *init, target)
        }
        StmtDescription::For(_, iter, body_expr) => {
            find_expr_parent_recursive(db, body, *iter, target)
                .or_else(|| find_expr_parent_recursive(db, body, *body_expr, target))
        }
        StmtDescription::While(cond, body_expr) => {
            find_expr_parent_recursive(db, body, *cond, target)
                .or_else(|| find_expr_parent_recursive(db, body, *body_expr, target))
        }
        StmtDescription::Return(Some(expr)) => find_expr_parent_recursive(db, body, *expr, target),
        StmtDescription::Expr(expr) => find_expr_parent_recursive(db, body, *expr, target),
        _ => None,
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

    /// Returns the parent expression containing this expression, if any.
    /// Returns None for the body's root expression.
    ///
    /// Note: This walks the entire body to find the parent, which is O(n) where n is the body size.
    /// For most function bodies this is acceptably fast. If profiling shows this is a bottleneck,
    /// we can cache a parent map.
    pub fn parent(self, db: &'db dyn HirDb) -> Option<Expr<'db>> {
        find_expr_parent(db, self.body, self.id).map(|parent_id| Expr::new(self.body, parent_id))
    }

    /// Returns the scope containing this expression.
    /// Walks up the parent chain to find the nearest enclosing block scope.
    pub fn scope(self, db: &'db dyn HirDb) -> ScopeId<'db> {
        use crate::hir_def::expr::ExprDescription;

        // If this expression IS a block, it creates its own scope
        if let Partial::Present(ExprDescription::Block(_)) = self.data(db) {
            return ScopeId::Block(self.body, self.id);
        }

        // Otherwise, walk up to find the parent's scope
        match self.parent(db) {
            Some(parent_expr) => parent_expr.scope(db),
            None => {
                // No parent expression - we're at the body root
                // Use the function/const's scope
                self.body.scope()
            }
        }
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
