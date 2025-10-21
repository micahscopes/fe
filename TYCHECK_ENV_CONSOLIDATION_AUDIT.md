# TyCheckEnv Consolidation Audit

**Date**: 2025-10-21
**Branch**: `hir-api-rework`
**Context**: After implementing `Expr::parent()` and `Expr::scope()`, checking if TyCheckEnv has redundant manual tracking

## Question

Now that `Expr<'db>` wrappers have `parent(db)` and `scope(db)` methods, is the manual tracking in `TyCheckEnv` redundant?

## Current Manual Tracking in TyCheckEnv

### 1. Expression Stack (`expr_stack: Vec<ExprId>`)

**Purpose**: Track parent expressions during traversal

**Methods**:
```rust
pub(super) fn enter_expr(&mut self, expr: ExprId) {
    self.expr_stack.push(expr);
}

pub(super) fn leave_expr(&mut self) {
    self.expr_stack.pop();
}

pub(super) fn parent_expr(&self) -> Option<ExprId> {
    self.expr_stack.iter().nth_back(1).copied()
}
```

**Usage**:
- `enter_expr`: Called at start of `Expr::type_check` (line 349)
- `leave_expr`: Called at end of `Expr::type_check` (line 368)
- `parent_expr()`: Used in 2 places
  - `expr.rs:878` - Check if parent is Call for diagnostics
  - `mod.rs:110` - Get parent expr ID

**Could be replaced with**: `expr.parent(db)`

### 2. Scope Stack (`var_env: Vec<BlockEnv<'db>>`)

**Purpose**: Track current scope for name resolution and type normalization

**Methods**:
```rust
pub(super) fn enter_scope(&mut self, block: ExprId) {
    let new_scope = match block.data(self.db, self.body) {
        Partial::Present(ExprDescription::Block(_)) => ScopeId::Block(self.body, block),
        _ => self.scope(),
    };
    let var_env = BlockEnv::new(new_scope, self.var_env.len());
    self.var_env.push(var_env);
}

pub(super) fn leave_scope(&mut self) {
    self.var_env.pop().unwrap();
}

pub(super) fn scope(&self) -> ScopeId<'db> {
    self.var_env.last().unwrap().scope
}
```

**Note**: `enter_scope` logic duplicates `Expr::scope()` logic!

**Usage**:
- `enter_scope`: Called 5 times when entering blocks
- `leave_scope`: Called 5 times when leaving blocks
- `scope()`: Used 16 times for:
  - Name resolution
  - Type normalization
  - Visibility checking
  - Method selection

**Could potentially be replaced with**: `expr.scope(db)` in some cases

## Key Observations

### Observation 1: Duplicated Logic

`TyCheckEnv::enter_scope()` at line 163-171 does:
```rust
let new_scope = match block.data(self.db, self.body) {
    Partial::Present(ExprDescription::Block(_)) => ScopeId::Block(self.body, block),
    _ => self.scope(),
};
```

`Expr::scope()` at `body.rs:349-365` does:
```rust
if let Partial::Present(ExprDescription::Block(_)) = self.data(db) {
    return ScopeId::Block(self.body, self.id);
}
match self.parent(db) {
    Some(parent_expr) => parent_expr.scope(db),
    None => self.body.scope()
}
```

**This is the SAME logic** - checking if block, creating Block scope.

### Observation 2: expr_stack vs Expr::parent()

The `expr_stack` maintains a manual parent chain by pushing/popping during traversal.

`Expr::parent()` computes the parent on-demand by walking the body tree.

**Trade-off**:
- `expr_stack`: O(1) lookup, requires push/pop discipline
- `Expr::parent()`: O(n) lookup, no manual maintenance

### Observation 3: var_env has OTHER purposes

`var_env` is not ONLY for scope tracking. It also:
- Tracks local variable bindings (`register_var`, `lookup_var`)
- Tracks block indices for shadowing detection
- Stores pending variables

So we CAN'T just remove `var_env` entirely.

### Observation 4: scope() is called frequently

`env.scope()` is used 16 times in type-checking code for:
- `normalize_ty(db, ty, scope, assumptions)` - Type normalization
- `resolve_bucket(bucket, scope)` - Name resolution
- `is_scope_visible_from(db, field_scope, current_scope)` - Visibility

Replacing all these with `expr.scope(db)` would add O(n) calls. With 16 calls during type-checking, this could be 16 * n operations.

## Analysis: Should We Consolidate?

### Option A: Keep Manual Tracking (Status Quo)

**Pros**:
- Fast: O(1) scope and parent lookups
- Battle-tested: Current code works
- No performance risk

**Cons**:
- Duplicated logic (enter_scope vs Expr::scope)
- Manual push/pop discipline required
- Two sources of truth

### Option B: Replace expr_stack with Expr::parent()

**Pros**:
- Eliminates manual push/pop
- One source of truth
- Simpler mental model

**Cons**:
- Only 2 uses of `parent_expr()` - low impact
- Would still need enter_expr/leave_expr for OTHER reasons (e.g., detecting recursion)

**Impact**: Very small - only 2 call sites would change

### Option C: Replace var_env scope tracking with Expr::scope()

**Pros**:
- Eliminates duplicated scope logic
- One source of truth for scopes

**Cons**:
- 16 calls to `env.scope()` during type-checking
- Replacing with `expr.scope(db)` = 16 * O(n) operations
- But: We still NEED var_env for variable bindings
- So we'd need to refactor var_env to separate concerns

**Complexity**: High - requires careful refactoring

### Option D: Hybrid Approach

**Keep manual tracking for hot paths**, but:
1. Remove duplicated scope logic - make `enter_scope` call `Expr::scope()`
2. Replace `parent_expr()` with `Expr::parent()` in the 2 call sites
3. Add assertion that manual scope matches `expr.scope(db)` in debug builds

**Pros**:
- Eliminates duplication without performance cost
- Validates consistency
- Gradual migration path

**Cons**:
- Still two tracking mechanisms

## Recommendations

### Short-term: Option D (Hybrid)

1. **Refactor `enter_scope` to use `Expr::scope()`**:
   ```rust
   pub(super) fn enter_scope(&mut self, block: ExprId) {
       let new_scope = self.body.wrap_expr(block).scope(self.db);
       let var_env = BlockEnv::new(new_scope, self.var_env.len());
       self.var_env.push(var_env);
   }
   ```
   This eliminates the duplicated scope logic.

2. **Replace `parent_expr()` calls with `Expr::parent()`**:
   - Only 2 call sites
   - Low risk, clear improvement

3. **Add debug assertions**:
   ```rust
   #[cfg(debug_assertions)]
   debug_assert_eq!(self.env.scope(), expr.scope(self.db));
   ```
   Validates that manual tracking matches wrapper API.

### Long-term: Consider Option C

If profiling shows that:
- Type-checking is not performance-critical, OR
- The O(n) overhead is negligible in practice

Then we could:
1. Separate `var_env` into two concerns:
   - `scope_stack: Vec<ScopeId>` for scopes
   - `var_env: Vec<VarBindings>` for variables
2. Replace `scope_stack` with `expr.scope(db)` calls
3. Keep `var_env` for actual variable tracking

But this requires profiling data to justify the change.

## Measuring Success

### Quantitative Metrics

Before/after for Option D:

| Metric | Before | After |
|--------|--------|-------|
| Duplicated scope logic | Yes (2 implementations) | No (1 implementation) |
| parent_expr() uses | 2 (manual) | 0 (uses Expr::parent) |
| Lines in TyCheckEnv | ~320 | ~310 |
| Performance | Baseline | Same (no hot path changes) |

### Next Steps

1. Implement Option D (Hybrid approach)
2. Run benchmarks to measure impact
3. Add profiling to measure `expr.scope()` call frequency
4. Based on data, decide if Option C is worth pursuing

## Open Questions

1. **Are there OTHER uses of expr_stack besides parent tracking?**
   - Need to audit all enter_expr/leave_expr calls

2. **How often is scope() called during type-checking?**
   - 16 static call sites, but how many dynamic calls?
   - Could be 16 * (number of expressions) = thousands

3. **What's the actual body size distribution?**
   - If most functions are <100 expressions, O(n) is fine
   - If some are >1000, might be problematic

4. **Is there a third option: cache parent map on first use?**
   - Lazy caching: Build map only if parent() called
   - Best of both worlds?

## References

- `TyCheckEnv` definition: `crates/hir/src/analysis/ty/ty_check/env.rs:36-50`
- `enter_scope` implementation: `env.rs:163-171`
- `Expr::scope` implementation: `crates/hir/src/hir_def/body.rs:349-365`
- `Expr::parent` implementation: `body.rs:337-344`
- `parent_expr()` uses: `expr.rs:878`, `mod.rs:110`
- `scope()` uses: 16 total across type-checking code
