# HIR Refactoring Audit - 2025

**Date**: 2025-10-17
**Status**: Post Phase 1 & 2 completion
**Auditor**: Gemini (via Claude Code orchestration)

## Context

We've completed two major refactoring phases:

1. **Phase 1**: Collection wrapper migrations (5 types: StructField, EnumVariant, FuncParam, TraitAssocType, ImplTraitAssocType)
2. **Phase 2**: `CallableParam` wrapper for automatic type instantiation

This audit identifies next opportunities following the established patterns.

---

## Path B: More Instantiation Wrappers

### High Priority

#### 1. Method Comparison in Trait Implementations

**Location**: `crates/hir/src/analysis/ty/method_cmp.rs:218-258`

**Current Pattern**:
```rust
let mut substituter = AssocTySubst::new(trait_inst);
let trait_m_ty = trait_m_ty.instantiate(db, map_to_impl);
let impl_m_ty = impl_m_ty.instantiate_identity();
let trait_m_ty_substituted = trait_m_ty.fold_with(db, &mut substituter);
```

**Problem**:
- Pattern duplicated for both arg types (lines 222-231) and return types (lines 254-258)
- Called for EVERY method in EVERY trait impl
- Manual instantiation + substitution logic exposed

**Suggested Wrapper**: `MethodComparisonParam` or extend `FuncDef` with method context

**Impact**: HIGH - Hot path in trait impl checking

---

#### 2. Operator Trait Methods (core::ops)

**Location**: `crates/hir/src/analysis/ty/ty_check/expr.rs:990-1001, 1027-1031`

**Current Pattern**:
```rust
let func_ty = cand.method.instantiate_with_inst(&mut self.table, lhs_ty, inst);
let (base, gen_args) = func_ty.decompose_ty_app(self.db);
let mut expected_rhs = func_def.arg_tys(self.db)[1].instantiate(self.db, gen_args);
let mut subst = AssocTySubst::new(inst);
expected_rhs = self.normalize_ty(expected_rhs.fold_with(self.db, &mut subst));
```

**Problem**:
- Duplicated in ambiguous resolution path (lines 1027-1031)
- Used in EVERY binary operation with trait methods
- Manual array indexing (`[1]`) fragile

**Suggested Solution**: Extend `Callable` with `.nth_param(idx)` returning `CallableParam`

**Impact**: HIGH - Every `+`, `-`, `*`, `/`, etc. uses this pattern

---

### Medium Priority

#### 3. Generic Field Access with Instantiation

**Location**: `crates/hir/src/analysis/ty/def_analysis.rs:1092`

**Current Pattern**:
```rust
for field_adt_ref in ty.instantiate_identity().collect_direct_adts(db) {
    // recursive ADT checking
}
```

**Suggested Wrapper**: `InstantiatedField` - similar to `CallableParam` but for struct/enum fields

**Impact**: MEDIUM - Used in recursive type checking

---

#### 4. Scope + Assumptions Threading

**Locations**: 96 occurrences across 20+ files

**Current Pattern**:
```rust
lower_generic_arg_list(db, args, tc.env.scope(), tc.env.assumptions())
normalize_ty(db, ty, self.env.scope(), self.env.assumptions())
```

**Problem**: Always passed together, never separately

**Suggested Wrapper**:
```rust
pub struct TypeContext<'db> {
    scope: ScopeId<'db>,
    assumptions: PredicateListId<'db>,
}
```

**Impact**: MEDIUM - High occurrence (96 call sites), straightforward refactor

---

### Low Priority

#### 5. DRY Refactoring in Callable

**Location**: `crates/hir/src/analysis/ty/ty_check/callable.rs:150-169`

**Problem**: Three methods duplicate the same instantiation logic:
- `ret_ty()` (lines 150-157)
- `ty()` (lines 160-169)
- `CallableParam::ty()` (lines 50-57)

**Solution**: Extract helper method
```rust
impl Callable {
    fn apply_instantiation(&self, db: &'db dyn HirAnalysisDb, ty: TyId<'db>) -> TyId<'db> {
        let mut ty = ty.instantiate(db, &self.generic_args);
        if let Some(inst) = self.trait_inst {
            let mut subst = AssocTySubst::new(inst);
            ty = ty.fold_with(db, &mut subst);
        }
        ty
    }
}
```

**Impact**: LOW - Already well-contained, just needs DRY

---

## Path C1: Body Back-References

### Current State

**Body Creation**: `crates/hir/src/lower/body.rs:92-106`

```rust
fn build(self, ast: &ast::Expr, body_expr: ExprId, body_kind: BodyKind) -> Body<'db> {
    let body = Body::new(
        self.f_ctxt.db(),
        self.id,
        body_expr,
        body_kind,
        None,  // TODO: Pass owner once we have it
        self.stmts,
        self.exprs,
        // ...
    );
```

### Infrastructure Already Exists!

✅ `Body` has `owner: Option<BodyOwner<'db>>` field (line 39)
✅ `BodyOwner` enum exists with `Func(Func<'db>)` and `Const(Const<'db>)` variants
❌ Owner is currently always `None` at creation time
❌ No connection from `BodyCtxt` to owning `Func`/`Const`

### Current Workaround Performance Issue

**Problem**: `Body::computed_owner()` (lines 122-146) does O(n) search:

```rust
for func in top_mod.all_funcs(db) {
    if let Some(func_body) = func.body(db) {
        if func_body == self {
            return Some(BodyOwner::Func(*func));
        }
    }
}
```

**Impact**:
- Called 17+ times in ty_check module alone
- With 100s of functions in large module, this is expensive
- Every `expr.containing_func(db)` call triggers this

### Blockers

#### 1. Chicken-and-Egg Problem

**Current flow**:
```
Lower AST → Create Body → Create Func (with body ref)
```

**Need**:
```
Lower AST → Create Func stub → Create Body (with func ref) → Finalize Func
```

This is a Salsa tracked struct initialization ordering issue.

#### 2. Owner Info Not Available at Body Creation

- `BodyCtxt` knows its `TrackedItemId` (knows it's a FuncBody)
- But doesn't have reference to actual `Func<'db>` or `Const<'db>`
- These are created AFTER the body is lowered

### Recommendation

**Status**: Ready to start with preparation

**Steps Required**:
1. Refactor item lowering to create "stub" items before body lowering
2. Thread owner through `BodyCtxt::new()` and `build()`
3. Remove `computed_owner` entirely once all callers use `.owner` field

**Benefits**:
- Eliminates O(n) module scans
- Enables O(1) `expr.containing_func(db)` lookup
- Infrastructure already present, just needs wiring

**Estimated Effort**: 2-3 hours

---

## Quick Wins

### 1. Extract `Callable::apply_instantiation` Helper

**Effort**: 15 minutes
**File**: `callable.rs`
**Lines affected**: 50-57, 150-157, 160-169

**Pattern**:
```rust
impl Callable {
    fn apply_instantiation(&self, db: &'db dyn HirAnalysisDb, ty: TyId<'db>) -> TyId<'db> {
        let mut ty = ty.instantiate(db, &self.generic_args);
        if let Some(inst) = self.trait_inst {
            let mut subst = AssocTySubst::new(inst);
            ty = ty.fold_with(db, &mut subst);
        }
        ty
    }
}
```

---

### 2. Add `Callable::nth_param(idx)` Accessor

**Effort**: 10 minutes
**File**: `callable.rs`
**Usage**: Replace operator trait patterns in `expr.rs:990-1001, 1027-1031`

**Pattern**:
```rust
impl Callable {
    pub fn nth_param(&self, db: &'db dyn HirAnalysisDb, idx: usize) -> CallableParam<'db> {
        CallableParam {
            callable: self.clone(),
            index: idx,
            generic_ty: self.func_def.arg_tys(db)[idx],
        }
    }
}
```

**Benefit**: Eliminates manual array indexing in operator overload checking

---

### 3. Remove Manual Enumerate + Index Patterns

**Effort**: 20 minutes
**File**: `callable.rs:106-109`

**Current**:
```rust
for (i, hir_arg) in call_args.iter().enumerate() {
    let arg = CallArg::from_hir_arg(tc, hir_arg, span.clone().arg(i));
}
```

**Improvement**: Already using iterator properly elsewhere (line 111). Could refactor to build args in one pass.

---

### 4. Create `TypeContext` Wrapper

**Effort**: 1-2 hours (mechanical)
**Files affected**: ~20 files, 96 call sites

**Pattern**:
```rust
pub struct TypeContext<'db> {
    scope: ScopeId<'db>,
    assumptions: PredicateListId<'db>,
}

impl TypeContext<'db> {
    pub fn lower_ty(&self, db: &'db dyn HirAnalysisDb, ty: HirTyId<'db>) -> TyId<'db> {
        lower_hir_ty(db, ty, self.scope, self.assumptions)
    }
}
```

**Benefit**: Thread single context object instead of two args everywhere

---

## Recommended Priority Order

1. **Quick Wins 1-3** (total ~45 min): Low risk, immediate DRY benefits
2. **Path B #2** (Operator `nth_param`): Unblocks frequent binary op pattern
3. **Path B #1** (Method comparison wrapper): High-frequency refactoring
4. **Path C1** (Body owner back-ref): Moderate complexity, high performance impact
5. **Path B #4** (TypeContext wrapper): Large but mechanical refactor

---

## Summary

The codebase is in excellent shape for continued refactoring. The patterns established in Phase 1 (collection wrappers) and Phase 2 (CallableParam) are working well and can be extended naturally to these new areas.

**Key Insights**:
- Manual instantiation + fold_with patterns appear in multiple hot paths
- Body owner infrastructure exists but is unused (easy to activate)
- Scope + assumptions are always passed together (easy to bundle)
- The refactoring patterns are consistent and repeatable

**Next Session Goals**:
- Knock out quick wins 1-3 (~1 hour)
- Start Path B #2 or Path C1 depending on interest
