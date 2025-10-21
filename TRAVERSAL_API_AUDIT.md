# Traversal API Refactoring - Current Status Audit

**Date**: 2025-10-21
**Context**: Understanding what's complete vs what remains for the traversal API refactoring

**Note**: AdtInstance/AdtFieldInstance wrapper work stashed in git stash with message:
"WIP: AdtInstance/AdtFieldInstance wrappers - debatable value for manual field access patterns"
- The wrappers are useful for iterating over ALL fields with automatic instantiation
- But applying them to name-based single-field lookups makes code MORE verbose, not less
- Keeping in stash for potential future use in appropriate contexts

## What "Traversal API" Means

Based on HIR_API_REFACTORING.md and HIR_REFACTORING_SYNTHESIS.md:

- **NOT** about visitor pattern type-checking traversal
- **IS** about creating **context-rich wrappers** that eliminate:
  - Manual index tracking (`enumerate()`, array indexing)
  - Manual context assembly (scope, assumptions threading)
  - Manual generic instantiation (`instantiate() → fold_with()`)

## Pattern: Context-Rich Wrappers

```rust
// ❌ OLD: Manual index tracking + instantiation
for (i, ty) in func_def.arg_tys(db).iter().enumerate() {
    let label = func_def.param_label(db, i);  // Manual index
    let instantiated = ty.instantiate(db, &generic_args);  // Manual
    if let Some(inst) = trait_inst {
        instantiated = instantiated.fold_with(db, &mut AssocTySubst::new(inst));
    }
}

// ✅ NEW: Context-rich iterator
for param in callable.params(db) {
    let ty = param.ty(db);  // Automatic instantiation!
    let label = param.label(db);  // No index needed!
}
```

## ✅ What's Complete

### 1. Expression/Statement/Pattern Wrappers
- ✅ `Expr<'db>` wrapper with `.type_check(tc, expected)` 
- ✅ `Stmt<'db>` wrapper with `.type_check(tc, expected)`
- ✅ `Pat<'db>` wrapper with `.type_check(tc, expected)`
- ✅ All carry `body` context, can access data via `self.body()`

### 2. CallableParam Wrapper (DONE!)
- ✅ `Callable::params()` returns `impl Iterator<Item = CallableParam>`
- ✅ `CallableParam::ty()` automatically applies generic instantiation + trait substitution
- ✅ `CallableParam::label()` and `.span()` eliminate index tracking
- ✅ **Already used in `check_args` (line 263)**

### 3. DRY Instantiation Helper (DONE!)
- ✅ `Callable::apply_instantiation()` helper method
- ✅ Used by `CallableParam::ty()`, `Callable::ret_ty()`
- ✅ Eliminates 13 lines of duplication

## 🔄 What Remains

### Enumerate Patterns Analysis

Found 10 `.enumerate()` patterns. Status:

| File | Line | Purpose | Status |
|------|------|---------|--------|
| callable.rs:150 | Creating CallableParam wrappers | ✅ OK - encapsulated |
| callable.rs:211 | Span tracking in unify_generic_args | ⚠️ Could use wrapper |
| callable.rs:258 | Creating CallArg with spans | ✅ OK - needs index for spans |
| env.rs:75 | Registering params in env | 🔴 **Should use CallableParam** |
| expr.rs:62 | Record init fields | 🔴 **Needs RecordField wrapper** |
| expr.rs:692 | Match arm reachability | ⚠️ Analyze - may need MatchArm wrapper |
| pat.rs:35 | Record pattern fields | 🔴 **Needs RecordFieldPat wrapper** |
| pat.rs:73 | Tuple rest pattern | ⚠️ Internal helper - probably OK |
| pat.rs:167 | Tuple pattern checking | ⚠️ Internal helper - probably OK |
| pat.rs:392 | PathTuple variant fields | ⚠️ Internal helper - probably OK |

### Missing Context-Rich Wrappers

#### 1. **RecordField Wrapper** (HIGH PRIORITY)
Used for:
- Record initialization (`expr.rs:62`)
- Record pattern matching (`pat.rs:35`)

Should provide:
- `.ty(db)` - automatically instantiated
- `.label(db)` - field name
- `.span(db)` - field span
- No manual index tracking

#### 2. **MatchArm Wrapper** (MEDIUM PRIORITY)
Used for:
- Match exhaustiveness (`expr.rs:692`)
- Could provide `.pat(db)`, `.guard(db)`, `.body(db)`

#### 3. **TyContext Wrapper** (LOW PRIORITY)
Bundle scope + assumptions that are always passed together:
```rust
struct TyContext { scope: ScopeId, assumptions: PredicateListId }
```
Found in 96+ call sites across codebase.

### Manual Instantiation Patterns

Found 3 patterns after DRY refactor:

| File | Line | Context | Status |
|------|------|---------|--------|
| callable.rs:128 | Inside `apply_instantiation` helper | ✅ OK - is the helper |
| callable.rs:330 | Constraint instantiation | 🔴 **Could use helper** |
| pat.rs:407 | Variant field instantiation | 🔴 **Could use helper or wrapper** |

## Recommended Priority

### Phase 1: Complete Existing Pattern (Highest Impact)
1. ✅ **DONE**: CallableParam wrapper
2. ✅ **DONE**: apply_instantiation helper
3. 🔄 **TODO**: Migrate `env.rs:75` to use `callable.params()`
4. 🔄 **TODO**: Extend instantiation helper to constraint checking

### Phase 2: Record Field Wrappers (High Impact)
5. Create `RecordField<'db>` wrapper for record init
6. Create `RecordFieldPat<'db>` wrapper for record patterns
7. Migrate record init/pattern checking to use wrappers

### Phase 3: Polish & Optimization (Medium Impact)
8. Add `MatchArm<'db>` wrapper if beneficial
9. Create `TyContext` bundle for scope+assumptions (96 call sites)
10. Document the wrapper pattern for future refactoring

## Success Criteria

By completion, we should have:
- ✅ No manual index tracking for params/fields/arms
- ✅ No manual generic instantiation at call sites  
- ✅ Automatic context assembly via wrappers
- ✅ Iterator-based APIs everywhere
- ✅ Clear prototype → instance → contextual flow

## Next Steps

1. **Immediate**: Migrate `env.rs:75` to use `callable.params()`
2. **Short-term**: Create RecordField wrappers
3. **Medium-term**: Evaluate MatchArm and TyContext wrappers
