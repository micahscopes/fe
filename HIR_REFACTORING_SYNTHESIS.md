# HIR API Refactoring: Final Synthesis

## The Real Problem (Now Crystal Clear)

You're trying to **invert the analysis architecture** from:
- **Context flows THROUGH analysis passes** (imperative, visitor-based)

To:
- **Context is EMBEDDED IN HIR nodes** (declarative, traversal-based)

This is NOT about:
- ❌ Making fields private
- ❌ Adding getter methods
- ❌ Creating two layers (they already exist!)

This IS about:
- ✅ Encoding context in traversable structures
- ✅ Eliminating manual context threading
- ✅ Making HIR nodes self-aware
- ✅ Enabling compositional analysis queries

## The Three-Dimensional Context Problem

Context is currently fragmented across THREE dimensions:

### Dimension 1: Where am I? (Structural Position)
- **Currently:** Manually tracked in `TyCheckEnv.expr_stack`
- **Should be:** `expr.parent(db)`, `expr.containing_func(db)`

### Dimension 2: What's in scope? (Semantic Context)
- **Currently:** Computed via `env.scope()`, passed as parameter everywhere
- **Should be:** `expr.scope(db)`, cached in scope tree

### Dimension 3: What are the types? (Instantiation Context)
- **Currently:** Manual multi-stage `instantiate() → fold_with()` at every use
- **Should be:** `param.ty(db)` automatically instantiated

## The Pattern We Found: Three-Layer Encoding

### Layer 1: Prototypes (Salsa-Cached Definitions)
**What exists:** `hir_def::Func`, `hir_def::Struct`, etc.
**What's stored:** Syntactic structure as written in source

### Layer 2: Instances (Salsa-Cached Analysis)
**What exists:** `analysis::FuncDef`, `analysis::AdtDef`
**What's stored:** Type-resolved, but still generic

### Layer 3: Contextual (Lightweight Wrappers)
**What needs to be added:** Context-carrying wrappers
**What they store:** Parent refs, indices, instantiation context

**Critical insight:** Layer 3 already exists for `AdtField`, `Callable` - we need to **systematize and extend this pattern**!

## Examples of What "Context-Rich Traversal" Means

### Example 1: From Call Site to Parameter Types

**Current (manual context assembly):**
```rust
// In ty_check/callable.rs
for (i, (given, expected)) in args.zip(func_def.arg_tys(db)).enumerate() {
    let scope = self.env.scope();  // Manual
    let assumptions = self.env.assumptions();  // Manual

    let mut expected_ty = expected.instantiate(db, &self.generic_args);  // Manual
    if let Some(inst) = self.trait_inst {  // Manual
        expected_ty = expected_ty.fold_with(db, &mut AssocTySubst::new(inst));
    }

    let label = func_def.param_label(db, i);  // Manual index
    let span = func_def.param_span(db, i);    // Manual index
}
```

**Desired (traversal):**
```rust
for (arg, param) in call.args(db).zip(call.params(db)) {
    let expected_ty = param.ty(db);  // ← Automatic instantiation!
    let label = param.label(db);     // ← No index needed!
    let span = param.span(db);       // ← Context embedded!
}
```

### Example 2: From Expression to Its Type

**Current (context threading):**
```rust
fn check_expr(&mut self, expr: ExprId, expected: TyId) -> ExprProp {
    let data = self.env.expr_data(expr);  // Need env for body
    let scope = self.env.scope();         // Need env for scope
    let assumptions = self.env.assumptions();  // Recomputed every time!

    let normalized = normalize_ty(db, expected, scope, assumptions);  // Manual
}
```

**Desired (traversal):**
```rust
fn check_expr(db: &dyn HirDb, expr: Expr<'db>, expected: TyId) -> TyId {
    let data = expr.data(db);            // Body embedded
    let normalized = expr.normalize_ty(db, expected);  // Scope + assumptions embedded
}
```

### Example 3: From Type Reference to Resolved Type

**Current (explicit parameters):**
```rust
// Every call site must provide context
lower_hir_ty(
    db,
    param_ty,
    func.scope(),           // Where does scope come from?
    collect_constraints()   // How do we get the right constraints?
)
```

**Desired (embedded context):**
```rust
// Type reference knows its definition context
param.type_ref(db).lower(db)  // Scope + constraints embedded!
```

## The Implementation Path Forward

### Step 1: Add Owner Back-References to Body ✅ Start Here!

**Why first:** Enables upward traversal (expr → body → func)

```rust
#[salsa::tracked]
pub struct Body<'db> {
    // ... existing fields ...
    owner: BodyOwner<'db>,  // NEW: who owns this body?
}

pub enum BodyOwner<'db> {
    Func(Func<'db>),
    Const(Const<'db>),
}
```

**Validation:** Can we answer "what function am I in" from any expression?

### Step 2: Build Per-Body Scope Tree

**Why second:** Enables `expr.scope(db)` without threading

```rust
#[salsa::tracked]
impl Body<'db> {
    pub fn scope_tree(self, db: &dyn HirDb) -> ScopeTree<'db> {
        // Walk the body once, assign scope to each expr/stmt
    }
}
```

**Validation:** Can we eliminate all `scope` parameters from analysis calls?

### Step 3: Create Expr/Stmt/Pat Wrappers

**Why third:** Provides ergonomic API with embedded context

```rust
pub struct Expr<'db> {
    body: Body<'db>,
    id: ExprId,
}

impl Expr<'db> {
    pub fn data(self, db: &dyn HirDb) -> &'db expr::Expr<'db>;
    pub fn scope(self, db: &dyn HirDb) -> ScopeId<'db>;
    pub fn span(self, db: &dyn HirDb) -> LazyExprSpan<'db>;
    pub fn parent(self, db: &dyn HirDb) -> Option<Node<'db>>;
    pub fn containing_func(self, db: &dyn HirDb) -> Option<Func<'db>>;
}
```

**Validation:** Can we chain queries naturally? `expr.parent(db).containing_block(db)`

### Step 4: Extend Callable-Style Wrappers

**Why fourth:** Complete the instantiation context layer

```rust
pub struct CallableParam<'db> {
    callable: Callable<'db>,  // Parent context
    index: usize,
    generic_ty: Binder<TyId<'db>>,
}

impl CallableParam<'db> {
    pub fn ty(self, db: &dyn HirDb) -> TyId<'db> {
        // Automatic instantiation + trait substitution!
    }
}
```

**Validation:** Can we eliminate manual `instantiate()` + `fold_with()` calls?

### Step 5: Refactor Analysis to Use Wrappers

**Why last:** Migration step, proves the design works

- Update `TyChecker` to accept `Expr<'db>` instead of `ExprId`
- Eliminate redundant state from `TyCheckEnv`
- Simplify signatures by removing context parameters

**Validation:** Is analysis code simpler? Are there fewer parameters?

## Measuring Success

### Quantitative Metrics

Before/after comparison:

| Metric | Before | After (Goal) |
|--------|--------|--------------|
| Average params per analysis function | 4-5 | 2-3 |
| Lines in `TyCheckEnv` | ~200 | ~100 |
| Manual `scope` parameter passes | ~50 | 0 |
| Manual index tracking loops | ~30 | 0 |

### Qualitative Checks

✅ **Can you chain queries?**
```rust
expr.parent(db).containing_block(db).locals(db)
```

✅ **Is context automatic?**
```rust
// Not this:
lower_hir_ty(db, ty, scope, assumptions)

// But this:
type_ref.lower(db)
```

✅ **Is analysis simpler?**
```rust
// Not this:
for (i, arg) in args.enumerate() {
    let ty = func.arg_tys(db)[i];
    let label = func.param_label(db, i);
    let span = func.param_span(db, i);
}

// But this:
for param in func.params(db) {
    let ty = param.ty(db);
    let label = param.label(db);
    let span = param.span(db);
}
```

## Critical Design Questions

For each context addition, ask:

### 1. What context is needed?
- Location (body, parent)
- Scope (name resolution)
- Assumptions (trait constraints)
- Instantiation (generic args)

### 2. Where should it be cached?
- **Per-node:** For frequently accessed, cheap data (parent ref)
- **Per-body:** For expensive, shared data (scope tree)
- **Per-func:** For definition-level data (constraints)
- **Not at all:** For rare queries or tiny data

### 3. What's the tradeoff?
- **Memory:** More embedded context = larger wrappers
- **Computation:** Cache vs recompute
- **Complexity:** Salsa dependency graph size
- **Ergonomics:** How much simpler does it make analysis?

### 4. Is it incrementally safe?
- Will salsa invalidate correctly?
- Are we creating too many fine-grained dependencies?
- Is the query structure still maintainable?

## Common Pitfalls to Avoid

### ❌ Pitfall 1: Over-caching
Don't embed everything "just in case"
- **Bad:** Every expr carries its full type, even if unchecked
- **Good:** Type is a query, computed on demand

### ❌ Pitfall 2: Breaking salsa boundaries
Don't store salsa-tracked data in non-tracked structs
- **Bad:** `struct Expr { func: Func<'db> }` (wrapper holds tracked type)
- **Good:** `struct Expr { body: Body<'db> }` then `impl { fn func(self) -> Func }`

### ❌ Pitfall 3: Premature wrapper creation
Don't create wrappers before understanding use cases
- **Bad:** "Func has params, so add params() wrapper"
- **Good:** "Type checking needs instantiated params, so add CallableParam wrapper"

### ❌ Pitfall 4: Ignoring existing patterns
Don't invent new patterns when good ones exist
- **Bad:** Inventing a new way to handle generic instantiation
- **Good:** Following the `AdtField` pattern (scope + on-demand lowering)

## Next Concrete Actions

1. **Add `Body::owner` field**
   - Modify lowering to set owner when creating bodies
   - Add `Body::owner_func()` accessor
   - Test: Can we navigate from expr → func?

2. **Prototype scope tree construction**
   - Write `build_scope_tree(db, body)` function
   - Cache in salsa query
   - Test: Does scope match current TyCheckEnv behavior?

3. **Create `Expr<'db>` wrapper (minimal)**
   - Just `{ body, id }` initially
   - Add `data()`, `scope()`, `span()` methods
   - Test: Can existing code use it?

4. **Refactor one small analysis function**
   - Pick simple function (e.g., literal checking)
   - Convert from `ExprId` to `Expr<'db>`
   - Measure: Is it actually simpler?

5. **Review and iterate**
   - Does the pattern feel right?
   - What's still awkward?
   - What context is still manually threaded?

## The Vision: Fully Traversable HIR

Ultimate goal state:

```rust
// Find a function call
let call_expr = func.body(db)
    .exprs(db)
    .find(|e| e.is_call(db))
    .unwrap();

// Navigate to callee
let callee = call_expr.callee(db);
let callee_func = callee.as_func(db)?;

// Check parameter types
for (arg, param) in call_expr.args(db).zip(callee_func.params(db)) {
    let arg_ty = arg.ty(db);      // Inferred type
    let param_ty = param.ty(db);  // Expected type (instantiated!)
    assert_eq!(arg_ty, param_ty);
}

// Navigate upward
let containing_func = call_expr.containing_func(db);
let module = containing_func.module(db);
let ingot = module.ingot(db);

// All context is embedded, all traversal is natural!
```

This is the "context-rich traversal API" you're building toward.
