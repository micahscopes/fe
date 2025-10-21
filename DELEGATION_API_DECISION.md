# Delegation API Decision - Session Report

**Date**: 2025-10-21
**Branch**: `hir-api-rework`
**Context**: Questioning whether to migrate from delegation methods to verbose wrapper calls

## The Question

After implementing scope resolution, I started migrating call sites from:

```rust
tc.check_expr(expr_id, expected_ty)
```

to:

```rust
tc.body().wrap_expr(expr_id).type_check(tc, expected_ty)
```

User stopped me and asked: **"is this really the best way to go? is this really in alignment?"**

This was the RIGHT question to ask.

## Analysis

### Usage Patterns in the Codebase

Searching the codebase revealed THREE distinct patterns:

#### Pattern A: Delegation (Concise)
```rust
tc.check_expr(expr_id, expected_ty);
```

- **Usage**: ~19 call sites
- **Context**: Inside TyChecker methods
- **Purpose**: One-off expression type-checking
- **Pros**: Concise, clear intent
- **Cons**: Still passes `tc` as parameter (not fully "context-rich")

#### Pattern B: Stored Wrapper (Good)
```rust
let lhs_expr = self.body().wrap_expr(*lhs);
let prop = lhs_expr.type_check_unknown(tc);
// ... later ...
let binding = self.find_base_binding(lhs_expr);
```

- **Usage**: ~10 call sites
- **Context**: When multiple methods needed on same expr
- **Purpose**: Create wrapper once, reuse multiple times
- **Pros**: Efficient, expressive, enables method chaining
- **Example**: `crates/hir/src/analysis/ty/ty_check/expr.rs:241-250`

#### Pattern C: Inline Wrapper (Verbose)
```rust
tc.body().wrap_expr(expr_id).type_check(tc, expected_ty);
```

- **Usage**: ~8 call sites (mainly in stmt.rs)
- **Context**: Outside TyChecker (in Stmt/Expr wrapper methods)
- **Purpose**: Bridge from wrapper context to expr type-checking
- **Pros**: No delegation method available in wrapper context
- **Cons**: Verbose for one-off calls

### Key Insight: Context Matters

The appropriate pattern depends on **where you are**:

| Location | Best Pattern | Why |
|----------|--------------|-----|
| **Inside TyChecker** | Pattern A (delegation) | Concise, clear |
| **Multiple uses of same expr** | Pattern B (stored wrapper) | Efficient reuse |
| **Inside Expr/Stmt wrappers** | Pattern C (inline wrapper) | No delegation available |

### Alignment with Traversal API Vision

From `HIR_REFACTORING_SYNTHESIS.md` Step 5:
> - Update `TyChecker` to accept `Expr<'db>` instead of `ExprId`
> - Eliminate redundant state from `TyCheckEnv`
> - **Simplify signatures** by removing context parameters

The goal is **simplification**, not verbosity.

From the docs, context-rich wrappers are meant for:
1. **Collections**: Eliminate manual index tracking (`for param in func.params(db)`)
2. **Complex context**: Automatic scope/assumptions assembly
3. **Method chaining**: Natural navigation (`expr.parent().containing_block()`)

Delegation methods for **one-off operations** don't conflict with this vision.

### The Pitfall

From `HIR_REFACTORING_SYNTHESIS.md` Pitfall 3:
> ❌ **Pitfall 3: Premature wrapper creation**
> Don't create wrappers before understanding use cases
> - **Bad:** "Func has params, so add params() wrapper"
> - **Good:** "Type checking needs instantiated params, so add CallableParam wrapper"

Changing `tc.check_expr(id, ty)` to `tc.body().wrap_expr(id).type_check(tc, ty)` is:
- Creating wrappers just to call methods (premature)
- Making code MORE verbose (anti-simplification)
- Still passing context manually (`tc`)

## Decision

**KEEP THE DELEGATION METHODS**

The delegation methods `tc.check_expr()`, `tc.check_expr_unknown()`, etc. are:
- ✅ Concise and clear
- ✅ Appropriate for one-off operations
- ✅ Don't conflict with wrapper API for complex use cases
- ✅ Align with "simplify signatures" goal better than verbose chaining

### Updated Migration Strategy

**DO NOT** migrate delegation calls to wrapper calls.

**INSTEAD**:
1. **Keep** delegation methods on TyChecker for one-off operations
2. **Use** wrapper API when naturally iterating or reusing (Pattern B)
3. **Use** inline wrappers when in wrapper context (Pattern C)
4. **Remove** TODO comments suggesting migration
5. **Document** that delegation is intentional, not "legacy"

### Code Changes Required

1. Update `TyChecker` delegation method comments:

```rust
// BEFORE:
/// Legacy wrapper that delegates to Expr::type_check.
/// TODO: Migrate all call sites to use expr.type_check(tc, expected) directly.
pub(super) fn check_expr(&mut self, expr: ExprId, expected: TyId<'db>) -> ExprProp<'db> {
    self.body().wrap_expr(expr).type_check(self, expected)
}

// AFTER:
/// Convenience method for type-checking a single expression.
/// When checking multiple expressions or needing wrapper methods (like span()),
/// consider creating the wrapper once: `let expr = tc.body().wrap_expr(id);`
pub(super) fn check_expr(&mut self, expr: ExprId, expected: TyId<'db>) -> ExprProp<'db> {
    self.body().wrap_expr(expr).type_check(self, expected)
}
```

2. Keep all existing delegation calls as-is
3. No further migration needed

## Lessons Learned

### 1. Question Verbosity
If a refactoring makes code MORE verbose, that's a red flag. Always ask:
- Is this actually simpler?
- Does this align with the stated goals?

### 2. Context-Rich ≠ No Delegation
Context-rich wrappers enable advanced use cases (chaining, iteration, complex context).
They don't mean eliminating ALL convenience methods.

### 3. User's Intuition Was Right
The user sensed something was off about the verbose pattern.
This highlights the value of critical review during implementation.

### 4. Read the Docs Carefully
The vision documents had the answer all along:
- "Simplify signatures" (not complicate them)
- "Eliminate manual index tracking" (not all method calls)
- "Premature wrapper creation" pitfall

## What Changed from Previous Plan

**Original Plan (from SCOPE_RESOLUTION_SESSION.md)**:
- ✅ Step 1: Body owner back-references (DONE - commit f78a1faa4)
- ✅ Step 2: Scope resolution (DONE - commit 956789610)
- ❌ ~~Step 5: Migrate call sites to use wrapper APIs~~ (ABANDONED)

**Revised Understanding**:
- Steps 1-2 complete: Infrastructure for context-rich wrappers exists
- Wrapper API is available for complex use cases
- Delegation methods stay for one-off operations
- No further migration work needed

## Next Steps

None! The refactoring is effectively complete:

1. ✅ Expr/Stmt/Pat wrappers exist
2. ✅ Expr can find parent and scope
3. ✅ Body can resolve owner
4. ✅ Both APIs coexist harmoniously

The system now supports:
- **Simple case**: `tc.check_expr(id, ty)` - concise
- **Complex case**: Create wrapper, use multiple methods - powerful
- **Best of both worlds**

## Files Referenced

- `/home/micah/hacker-stuff-2023/fe-stuff/fe/crates/hir/src/analysis/ty/ty_check/expr.rs`
  - Delegation methods: lines 40-50
  - Good wrapper pattern: lines 241-250, 375-376
  - Diagnostic wrapper pattern: lines 471-472
- `/home/micah/hacker-stuff-2023/fe-stuff/fe/crates/hir/src/analysis/ty/ty_check/stmt.rs`
  - Wrapper from stmt context: line 31
  - Multiple wrapper uses: lines 68-105
- `/home/micah/hacker-stuff-2023/fe-stuff/fe/HIR_REFACTORING_SYNTHESIS.md`
  - Step 5 goals: lines 201-209
  - Pitfall 3: lines 296-299

## References

- **SCOPE_RESOLUTION_SESSION.md** - Previous session's work
- **HIR_REFACTORING_SYNTHESIS.md** - Overall vision
- **HIR_API_REFACTORING.md** - Context-rich wrapper principles
- **TRAVERSAL_API_AUDIT.md** - Iterator-based pattern examples
