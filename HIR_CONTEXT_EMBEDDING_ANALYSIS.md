# HIR Context Embedding: Comprehensive Analysis

## The Core Problem: Context Fragmentation

The HIR currently has **context-free nodes** that require **context-threading** through visitor state and function parameters.

### What We Have: Inert IDs + External Context

```rust
// Current: Node is just an ID
let expr: ExprId = ...;

// ALL context comes from outside
let data = expr.data(db, body);           // Need body
let span = expr.span(body);               // Need body
let ty = check_expr(&mut checker, expr);  // Need checker (has db, env, scope, assumptions)
```

### What We Want: Self-Aware Nodes

```rust
// Desired: Node carries its context
let expr: Expr<'db> = func.body(db).expr_at(position);

// Context is embedded
let data = expr.data(db);      // Body embedded
let span = expr.span(db);      // Body embedded
let ty = expr.ty(db);          // Scope + assumptions embedded
let parent = expr.parent(db);  // Hierarchy embedded
let func = expr.containing_func(db);  // Upward traversal
```

## Context That's Currently Fragmented

### 1. Location Context (Where am I?)

**Currently threaded through:**
- `TyCheckEnv.body: Body<'db>` - stored in checker
- `TyCheckEnv.expr_stack: Vec<ExprId>` - manually tracked
- `TyCheckEnv.loop_stack: Vec<StmtId>` - manually tracked

**Should be embedded in node:**
```rust
pub struct Expr<'db> {
    id: ExprId,
    body: Body<'db>,           // ← My container
    parent: Option<NodeId<'db>>,  // ← My parent (expr/stmt)
}

impl Expr<'db> {
    pub fn containing_func(self, db: &dyn HirDb) -> Option<Func<'db>> {
        self.body.owner_func(db)
    }

    pub fn parent(self, db: &dyn HirDb) -> Option<Node<'db>> {
        self.parent.map(|p| p.resolve(db, self.body))
    }

    pub fn ancestors(self, db: &dyn HirDb) -> impl Iterator<Item = Node<'db>> {
        // Can traverse upward!
    }
}
```

### 2. Scope Context (What's in scope?)

**Currently threaded through:**
- `TyCheckEnv.var_env: Vec<BlockEnv>` - scope stack
- `BlockEnv.scope: ScopeId` - current scope
- Passed explicitly to every `lower_hir_ty` call

**Should be embedded in node:**
```rust
pub struct Expr<'db> {
    // ...
    scope: ScopeId<'db>,  // ← Name resolution scope at this position
}

impl Expr<'db> {
    pub fn scope(self) -> ScopeId<'db> {
        self.scope
    }

    pub fn locals_in_scope(self, db: &dyn HirDb) -> impl Iterator<Item = LocalBinding> {
        // Can query scope directly!
        self.scope.all_bindings(db)
    }

    pub fn resolve_name(self, db: &dyn HirDb, name: IdentId) -> Option<NameRes<'db>> {
        // Uses embedded scope
        resolve_in_scope(db, name, self.scope)
    }
}
```

### 3. Type Assumptions Context (What bounds apply?)

**Currently computed on-demand:**
```rust
impl TyCheckEnv {
    pub(super) fn assumptions(&self) -> PredicateListId<'db> {
        // Recomputed every time!
        match self.hir_func() {
            Some(func) => collect_func_def_constraints(db, func.into(), true)
                .instantiate_identity()
                .extend_all_bounds(db),
            None => PredicateListId::empty_list(db),
        }
    }
}
```

**Should be cached and accessible:**
```rust
pub struct Expr<'db> {
    // ...
    // Cached at body-level or func-level
}

impl Expr<'db> {
    pub fn assumptions(self, db: &dyn HirDb) -> PredicateListId<'db> {
        // Cached in containing function
        self.containing_func(db)?.constraints(db)
    }

    pub fn lower_type(self, db: &dyn HirDb, ty: HirTyId) -> TyId<'db> {
        // Uses embedded scope + assumptions automatically!
        lower_hir_ty(db, ty, self.scope, self.assumptions(db))
    }
}
```

### 4. Instantiation Context (What are the concrete types?)

**Currently in `Callable` wrapper (good!) but still requires assembly:**
```rust
// Manual instantiation at every call site
let mut expected = expected.instantiate(db, &self.generic_args);
if let Some(inst) = self.trait_inst {
    let mut subst = AssocTySubst::new(inst);
    expected = expected.fold_with(db, &mut subst);
}
```

**Should be available from context:**
```rust
pub struct CallExpr<'db> {
    expr: Expr<'db>,           // Base expr context
    callable: Callable<'db>,   // Instantiation context
}

impl CallExpr<'db> {
    pub fn arg_types(self, db: &dyn HirDb) -> impl Iterator<Item = TyId<'db>> {
        // Automatically instantiated!
        self.callable.params(db).map(|p| p.ty(db))
    }
}
```

## The Pattern: Three-Layer Context Encoding

### Layer 1: Syntactic Position (Always Available)
Every node knows:
- Its `Body`
- Its parent node
- Its source span

```rust
pub struct Expr<'db> {
    id: ExprId,
    body: Body<'db>,
    parent: Option<NodeId<'db>>,
}

impl Expr<'db> {
    pub fn span(self, db: &dyn HirDb) -> LazyExprSpan<'db> {
        self.id.lazy_span(self.body)
    }
}
```

### Layer 2: Semantic Context (Computed Once)
Cached at appropriate granularity:
- **Per-body:** Scope tree
- **Per-function:** Type assumptions/constraints
- **Per-item:** Generic parameters

```rust
#[salsa::tracked]
pub struct Body<'db> {
    // ...existing fields...
    owner: BodyOwner<'db>,  // ← Back-pointer to Func/Const
}

#[salsa::tracked]
impl Body<'db> {
    #[salsa::tracked]
    pub fn scope_tree(self, db: &dyn HirDb) -> ScopeTree<'db> {
        // Computed once, cached
        build_scope_tree(db, self)
    }
}
```

### Layer 3: Instantiation Context (On-Demand)
Lightweight wrappers for specific contexts:
- `CallableParam` for function calls
- `AdtInstance` for struct/enum usage
- `TraitMethodCall` for method resolution

```rust
// Lightweight, not salsa-tracked
pub struct CallableParam<'db> {
    callable: Callable<'db>,
    index: usize,
    generic_ty: Binder<TyId<'db>>,
}

impl CallableParam<'db> {
    pub fn ty(self, db: &dyn HirDb) -> TyId<'db> {
        // Automatic instantiation
        let ty = self.generic_ty.instantiate(db, self.callable.generic_args());
        if let Some(inst) = self.callable.trait_inst() {
            ty.fold_with(db, &mut AssocTySubst::new(inst))
        } else {
            ty
        }
    }
}
```

## Concrete Context Threading Examples

### Example 1: Expression Type Checking (Before)

```rust
// From ty_check/expr.rs
pub(super) fn check_expr(&mut self, expr: ExprId, expected: TyId<'db>) -> ExprProp<'db> {
    // Need to get data via external body
    let Partial::Present(expr_data) = self.env.expr_data(expr) else {
        return ExprProp::invalid(self.db);
    };

    // Need to normalize with external scope + assumptions
    let expected = normalize_ty(
        self.db,
        expected,
        self.env.scope(),          // ← External context
        self.env.assumptions()     // ← External context
    );

    // Manually track expression stack
    self.env.enter_expr(expr);

    let actual = match expr_data {
        Expr::Path(path) => {
            // Need to resolve with external scope
            let res = self.resolve_path(*path, true, span);  // Uses self.env
            // ...
        }
        // ... every variant needs external context
    };

    self.env.leave_expr();
}
```

### Example 1: Expression Type Checking (After)

```rust
pub fn check_expr(db: &dyn HirDb, expr: Expr<'db>, expected: TyId<'db>) -> TyId<'db> {
    let expr_data = expr.data(db);  // Body embedded

    // Scope + assumptions embedded
    let expected = expr.normalize_ty(db, expected);

    match expr_data {
        Expr::Path(path) => {
            // Context embedded in expr
            let res = expr.resolve_path(db, *path);
            // ...
        }
        // Each variant uses expr's embedded context
    }
}
```

### Example 2: Pattern Checking (Before)

```rust
// From ty_check/pat.rs
pub(super) fn check_pat(&mut self, pat: PatId, expected: TyId<'db>) -> TyId<'db> {
    // Need external body
    let Partial::Present(pat_data) = pat.data(self.db, self.body()) else {
        return TyId::invalid(self.db, InvalidCause::ParseError);
    };

    match pat_data {
        Pat::Path(path, is_mut) => {
            // Get span via external body
            let span = pat.span(self.body()).into_path_pat();

            // Resolve via external scope + assumptions
            let res = self.resolve_path(*path, true, span.clone().path());

            // Register binding in external environment
            let binding = LocalBinding::local(pat, *is_mut);
            self.env.register_pending_binding(name, binding);
        }
    }
}
```

### Example 2: Pattern Checking (After)

```rust
pub fn check_pat(db: &dyn HirDb, pat: Pat<'db>, expected: TyId<'db>) -> TyId<'db> {
    let pat_data = pat.data(db);  // Body embedded

    match pat_data {
        Pat::Path(path, is_mut) => {
            let span = pat.span(db).into_path_pat();  // Body embedded

            // Scope embedded
            let res = pat.resolve_path(db, *path);

            // Can create binding with context
            let binding = pat.create_local_binding(db, is_mut);
        }
    }
}
```

### Example 3: Type Lowering (Before)

```rust
// From ty_lower.rs - function signature requires manual context
#[salsa::tracked]
pub fn lower_hir_ty<'db>(
    db: &'db dyn HirAnalysisDb,
    ty: HirTyId<'db>,
    scope: ScopeId<'db>,           // ← Manually passed
    assumptions: PredicateListId<'db>,  // ← Manually passed
) -> TyId<'db> {
    // Every call site must assemble scope + assumptions
}

// Call site in func_def.rs
let ty = lower_hir_ty(
    db,
    param_ty,
    func.scope(),          // ← Manual assembly
    assumptions            // ← Manual assembly
);
```

### Example 3: Type Lowering (After)

```rust
// Type reference knows its context
pub struct TypeRef<'db> {
    hir_ty: HirTyId<'db>,
    scope: ScopeId<'db>,           // ← Embedded
    assumptions: PredicateListId<'db>,  // ← Embedded
}

impl TypeRef<'db> {
    pub fn lower(self, db: &dyn HirDb) -> TyId<'db> {
        // Uses embedded context
        lower_hir_ty(db, self.hir_ty, self.scope, self.assumptions)
    }
}

// Call site
let ty = param.type_ref(db).lower(db);  // Context automatic!
```

## Proposed Node Wrapper Hierarchy

```rust
// Base wrapper: All HIR nodes carry this
pub struct Node<'db> {
    kind: NodeKind<'db>,
    body: Body<'db>,
    parent: Option<NodeId>,
}

pub enum NodeKind<'db> {
    Expr(ExprId, &'db Expr<'db>),
    Stmt(StmtId, &'db Stmt<'db>),
    Pat(PatId, &'db Pat<'db>),
}

// Specialized wrappers
pub struct Expr<'db> {
    node: Node<'db>,
    id: ExprId,
    data: &'db expr::Expr<'db>,
}

impl Expr<'db> {
    // Syntactic position
    pub fn span(self, db: &dyn HirDb) -> LazyExprSpan<'db> { ... }
    pub fn parent(self, db: &dyn HirDb) -> Option<Node<'db>> { ... }
    pub fn body(self) -> Body<'db> { self.node.body }

    // Semantic context
    pub fn scope(self, db: &dyn HirDb) -> ScopeId<'db> {
        self.body().scope_at(db, self.id)
    }

    pub fn containing_func(self, db: &dyn HirDb) -> Option<Func<'db>> {
        self.body().owner_func(db)
    }

    // Analysis queries (use embedded context)
    pub fn ty(self, db: &dyn HirDb) -> TyId<'db> {
        expr_ty(db, self)  // Query uses self's context
    }

    pub fn resolve_path(self, db: &dyn HirDb, path: PathId) -> PathRes<'db> {
        resolve_path(db, path, self.scope(db), self.assumptions(db))
    }
}

// Variant-specific wrappers
pub struct CallExpr<'db> {
    expr: Expr<'db>,
    callee: ExprId,
    args: Vec<ExprId>,
}

impl CallExpr<'db> {
    pub fn callee_expr(self, db: &dyn HirDb) -> Expr<'db> {
        self.expr.body().expr(db, self.callee)
    }

    pub fn callable(self, db: &dyn HirDb) -> Result<Callable<'db>, ...> {
        // Uses expr's context to resolve callable
        let callee_ty = self.callee_expr(db).ty(db);
        Callable::from_ty(db, callee_ty, self.expr.scope(db))
    }

    pub fn args(self, db: &dyn HirDb) -> impl Iterator<Item = Expr<'db>> {
        self.args.iter().map(|&id| self.expr.body().expr(db, id))
    }
}
```

## Implementation Strategy: Incremental Context Embedding

### Phase 1: Add Body Back-References
**Goal:** Enable `Body` to know its owner

```rust
#[salsa::tracked]
pub struct Body<'db> {
    // ... existing fields ...
    owner: BodyOwner<'db>,  // NEW
}

pub enum BodyOwner<'db> {
    Func(Func<'db>),
    Const(Const<'db>),
    Anonymous,  // For future: closures, const blocks
}

impl Body<'db> {
    pub fn owner_func(self, db: &dyn HirDb) -> Option<Func<'db>> {
        match self.owner(db) {
            BodyOwner::Func(f) => Some(f),
            _ => None,
        }
    }
}
```

### Phase 2: Build Scope Tree (Per-Body)
**Goal:** Compute scope for each expression position

```rust
#[salsa::tracked]
pub struct ScopeTree<'db> {
    body: Body<'db>,
    #[return_ref]
    expr_scopes: FxHashMap<ExprId, ScopeId<'db>>,
    #[return_ref]
    stmt_scopes: FxHashMap<StmtId, ScopeId<'db>>,
}

#[salsa::tracked]
impl Body<'db> {
    pub fn scope_tree(self, db: &dyn HirDb) -> ScopeTree<'db> {
        build_scope_tree(db, self)
    }

    pub fn expr_scope(self, db: &dyn HirDb, expr: ExprId) -> ScopeId<'db> {
        self.scope_tree(db).expr_scopes(db)[&expr]
    }
}
```

### Phase 3: Create Context-Rich Wrappers
**Goal:** Provide ergonomic API that hides context assembly

```rust
pub struct Expr<'db> {
    body: Body<'db>,
    id: ExprId,
}

impl Body<'db> {
    pub fn expr(self, db: &dyn HirDb, id: ExprId) -> Expr<'db> {
        Expr { body: self, id }
    }
}

impl Expr<'db> {
    pub fn data(self, db: &dyn HirDb) -> &'db Partial<expr::Expr<'db>> {
        &self.body.exprs(db)[self.id]
    }

    pub fn scope(self, db: &dyn HirDb) -> ScopeId<'db> {
        self.body.expr_scope(db, self.id)
    }

    pub fn span(self, db: &dyn HirDb) -> LazyExprSpan<'db> {
        self.id.lazy_span(self.body)
    }
}
```

### Phase 4: Migrate Analysis Queries
**Goal:** Update type checking to use context-rich wrappers

```rust
// Old signature
fn check_expr(&mut self, expr: ExprId, expected: TyId) -> ExprProp;

// New signature
fn check_expr(&mut self, expr: Expr<'db>, expected: TyId) -> TyId;

// Usage
let expr = body.expr(db, expr_id);
let ty = checker.check_expr(expr, expected);
```

### Phase 5: Eliminate TyCheckEnv State
**Goal:** Remove redundant state as context becomes embedded

Remove from `TyCheckEnv`:
- `expr_stack` (use `expr.parent()`)
- `scope` tracking (use `expr.scope()`)
- `body` field (embedded in wrappers)

Keep in `TyCheckEnv`:
- `var_env` for mutable type inference state
- `table: UnificationTable` (solver state)
- Result caches

## Success Criteria

✅ **No manual scope threading**
- Every `lower_hir_ty` call that currently takes `scope` parameter should use embedded context

✅ **No manual body passing**
- Every `expr.span(body)` becomes `expr.span(db)`

✅ **Upward traversal possible**
- `expr.parent(db)`, `expr.containing_func(db)` work

✅ **Composable queries**
- `expr.callee().ty(db).return_type(db)` chains work

✅ **Analysis pass simplification**
- `TyChecker` shrinks as context moves into nodes

## Critical Questions for Each Context Addition

1. **Where should it be cached?**
   - Per-node (in wrapper)
   - Per-body (salsa query)
   - Per-func (salsa query)

2. **What granularity?**
   - Every expression gets its own scope? (expensive)
   - Scopes only at block boundaries? (cheap, but less precise)

3. **What's the cost/benefit?**
   - Memory: How much context to embed?
   - Computation: Cache vs recompute?
   - Ergonomics: How much simpler does analysis become?

4. **Incremental safety?**
   - Will salsa correctly invalidate when context changes?
   - Are we creating too many fine-grained dependencies?
