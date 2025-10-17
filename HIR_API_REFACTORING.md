# HIR API Refactoring: Archaeological Analysis & Design

## Executive Summary

After deep analysis of the existing codebase, we've discovered that the HIR already has a **two-layer prototype/instance architecture**. The refactoring goal is NOT to mechanically convert fields to methods, but to:

1. **Expose the instance layer as the primary public API**
2. **Make context assembly automatic** instead of manual
3. **Eliminate repetitive index-based queries** with context-rich wrappers

## Critical Insight: The Current Two-Layer Architecture

### Layer 1: Prototypes (`hir_def`)
**What it stores:** Syntax-level, deduplicated HIR representations
- `#[salsa::tracked]` or `#[salsa::interned]`
- Stores "the shape" of definitions as written in source
- Examples:
  - `Func<'db>` with `FuncParamListId` → list of `FuncParam` (syntactic types)
  - `Struct<'db>` with `FieldDefListId` → list of `FieldDef` (syntactic types)
  - `Enum<'db>` with `VariantDefListId` → list of `VariantDef`

### Layer 2: Instances (`analysis::ty`)
**What it computes:** Type-resolved, context-aware analysis views
- `#[salsa::tracked]` wrappers that cache expensive computations
- Stores resolved `analysis::ty::TyId`, not `hir_def::TypeId`
- Examples:
  - `FuncDef<'db>` wraps `Func`, caches `arg_tys: Vec<Binder<TyId>>`
  - `AdtDef<'db>` wraps `Struct/Enum/Contract`, contains `AdtField` wrappers
  - `Callable<'db>` wraps `FuncDef` + instantiation context (generic args, trait inst)

## The Problem: Manual Context Assembly

The analysis layer currently **manually tracks context** in three painful ways:

### Problem 1: Index Gymnastics
Everywhere params/fields are accessed, indices are manually tracked:

```rust
// In callable.rs:195-199
for (i, (given, expected)) in args
    .into_iter()
    .zip(self.func_def.arg_tys(db).iter())  // ← Manual zip
    .enumerate()  // ← Manual index tracking
{
    if let Some(expected_label) = self.func_def.param_label(db, i) // ← Pass index
```

Every query needs the index passed separately: `param_label(db, i)`, `param_span(db, i)`

### Problem 2: Multi-Stage Generic Instantiation
At call sites, instantiation is a manual multi-step process:

```rust
// In callable.rs:213-217
let mut expected = expected.instantiate(db, &self.generic_args); // Step 1
if let Some(inst) = self.trait_inst {  // Step 2
    let mut subst = AssocTySubst::new(inst);
    expected = expected.fold_with(db, &mut subst); // Step 3
}
```

This pattern repeats everywhere types are instantiated.

### Problem 3: Scope/Assumptions Threading
When lowering types, scope and trait constraints must be manually assembled:

```rust
// In func_def.rs
lower_hir_ty(db, ty, func.scope(), assumptions) // ← Manual context gathering
```

## What Analysis Actually Needs: The Use Cases

### Use Case 1: Type Checking Call Arguments
**Current pattern:**
```rust
// Get expected type by index
let expected = self.func_def.arg_tys(db)[i];
// Manually instantiate
let expected = expected.instantiate(db, &self.generic_args);
// Manually fold for trait methods
if let Some(inst) = self.trait_inst {
    expected = expected.fold_with(db, &mut AssocTySubst::new(inst));
}
// Finally unify
tc.equate_ty(given_ty, expected, span);
```

**Desired API:**
```rust
for (arg, param) in call_args.zip(callable.params(db)) {
    let expected_ty = param.ty(db); // Automatically instantiated!
    tc.equate_ty(arg.ty, expected_ty, arg.span);
}
```

### Use Case 2: Argument Label Checking
**Current pattern:**
```rust
for (i, given_arg) in args.enumerate() {
    if let Some(expected) = self.func_def.param_label(db, i) { // Index!
        if expected != given_arg.label {
            emit_error(self.func_def.param_span(db, i)); // Index again!
        }
    }
}
```

**Desired API:**
```rust
for (arg, param) in args.zip(callable.params(db)) {
    if let Some(expected) = param.label(db) {
        if expected != arg.label {
            emit_error(param.span(db)); // No index needed!
        }
    }
}
```

### Use Case 3: LSP Hover/Goto-Definition
**Current pattern:**
```rust
// Find parameter by index somehow
let idx = /* complex position calculation */;
let name = func_def.param_label_or_name(db, idx);
let ty = func_def.arg_tys(db)[idx];
let span = func_def.param_span(db, idx);
```

**Desired API:**
```rust
let param = func.param_at_position(db, cursor_position);
let name = param.name(db);
let ty = param.ty(db);
let span = param.span(db);
```

## The Existing Good Pattern: `AdtField`

**Key insight:** `AdtField` already demonstrates the right pattern!

```rust
#[derive(Debug, Clone, PartialEq, Eq, Hash, salsa::Update)]
pub struct AdtField<'db> {
    tys: Vec<Partial<HirTyId<'db>>>,  // Syntactic types
    scope: ScopeId<'db>,               // Context for resolution!
}

impl AdtField<'db> {
    pub fn ty(&self, db: &'db dyn HirAnalysisDb, i: usize) -> Binder<TyId<'db>> {
        // Uses stored context to resolve on-demand
        lower_hir_ty(db, self.tys[i], self.scope, assumptions)
    }
}
```

**Why this works:**
- Lightweight wrapper (not salsa-tracked itself)
- Carries resolution context (`scope`)
- Defers expensive computation until needed (`ty()`)
- Hides the complexity of `lower_hir_ty` + assumptions

## Design Principles for Context-Rich Wrappers

### Principle 1: Two-Stage Instantiation
1. **Prototype → Instance (cached)**: `lower_func` creates `FuncDef` with resolved generic types
2. **Instance → Contextual (on-demand)**: Wrappers like `CallableParam` carry instantiation context

```rust
// Stage 1: Cached in FuncDef
#[salsa::tracked]
pub struct FuncDef<'db> {
    arg_tys: Vec<Binder<TyId<'db>>>, // Generic types
}

// Stage 2: On-demand instantiation
pub struct CallableParam<'db> {
    callable: Callable<'db>,  // Has generic_args + trait_inst
    index: usize,
    generic_ty: Binder<TyId<'db>>, // From FuncDef
}

impl CallableParam<'db> {
    pub fn ty(&self, db: &dyn HirAnalysisDb) -> TyId<'db> {
        // Automatically applies instantiation!
        let ty = self.generic_ty.instantiate(db, self.callable.generic_args());
        if let Some(inst) = self.callable.trait_inst() {
            ty.fold_with(db, &mut AssocTySubst::new(inst))
        } else {
            ty
        }
    }
}
```

### Principle 2: Iterator-Based Access (Not Index-Based)
Replace index-based queries with iterators that yield context-rich wrappers:

```rust
// ❌ Old: Index-based
for i in 0..func_def.arg_tys(db).len() {
    let ty = func_def.arg_tys(db)[i];
    let label = func_def.param_label(db, i);
    let span = func_def.param_span(db, i);
}

// ✅ New: Iterator-based
for param in callable.params(db) {
    let ty = param.ty(db);     // Automatically instantiated
    let label = param.label(db);
    let span = param.span(db);
}
```

### Principle 3: Context Carries Upward
Wrappers carry parent references, enabling upward traversal:

```rust
pub struct CallableParam<'db> {
    callable: Callable<'db>,  // Parent context
    index: usize,
}

impl CallableParam<'db> {
    pub fn parent_func(&self) -> FuncDef<'db> {
        self.callable.func_def
    }

    pub fn scope(&self, db: &dyn HirAnalysisDb) -> ScopeId<'db> {
        self.parent_func().scope(db)
    }
}
```

## Prototype vs Instance Mapping

### Functions

| Level | Type | What's Stored | When Created |
|-------|------|---------------|--------------|
| Prototype | `hir_def::Func` | Syntactic param types (`hir_def::TypeId`) | During lowering from AST |
| Instance (cached) | `analysis::FuncDef` | Resolved generic param types (`analysis::TyId`) | Via `lower_func` query |
| Contextual (on-demand) | `Callable` | Instantiation context (generic args, trait inst) | At call sites |
| **NEW** Wrapper | `CallableParam` | Parent + index + resolved type | Iterator from `Callable::params()` |

### ADTs (Structs/Enums)

| Level | Type | What's Stored | When Created |
|-------|------|---------------|--------------|
| Prototype | `hir_def::Struct` | Syntactic field types | During lowering from AST |
| Instance (cached) | `analysis::AdtDef` | `AdtField` wrappers with scope | Via `lower_adt` query |
| Contextual (on-demand) | `AdtField` | Syntactic types + scope context | Created by `lower_adt` |
| **NEW** Wrapper | `AdtInstance` | Instantiated ADT with concrete type args | At usage sites |

## Revised Implementation Strategy

### Phase 1: Extend Existing Instance Layer
Don't start with `hir_def` - **start with the analysis layer that already works!**

1. **Add `Callable::params()` iterator**
   - Returns `impl Iterator<Item = CallableParam>`
   - `CallableParam` wraps `(Callable, usize, Binder<TyId>)`
   - Replaces manual index tracking in `check_args`

2. **Add `AdtInstance` wrapper**
   - Like `Callable` but for ADTs
   - Carries ADT + generic args
   - `fields()` returns instantiated field types

### Phase 2: Surface Analysis APIs in `hir_def`
Once the analysis layer has clean APIs, expose them from `hir_def`:

3. **Add convenience methods to `hir_def::Func`**
   ```rust
   impl Func<'db> {
       pub fn def(self, db: &dyn HirDb) -> FuncDef<'db> {
           lower_func(db, self)
       }

       // Convenience: delegates to FuncDef
       pub fn params_analyzed(self, db: &dyn HirDb) -> impl Iterator<...> {
           self.def(db).params(db)
       }
   }
   ```

4. **Similar for `Struct`, `Enum`, etc.**

### Phase 3: Progressive Migration
5. **Update analysis consumers** to use new iterator APIs
6. **Deprecate index-based queries** (or make them private)
7. **Update LSP** to use context-rich wrappers

## Concrete Next Steps

### Step 1: Add `CallableParam` Wrapper
File: `crates/hir/src/analysis/ty/ty_check/callable.rs`

```rust
pub struct CallableParam<'db> {
    callable: &'db Callable<'db>,
    index: usize,
    generic_ty: Binder<TyId<'db>>,
}

impl<'db> CallableParam<'db> {
    pub fn ty(&self, db: &'db dyn HirAnalysisDb) -> TyId<'db> {
        let mut ty = self.generic_ty.instantiate(db, self.callable.generic_args());
        if let Some(inst) = self.callable.trait_inst() {
            ty = ty.fold_with(db, &mut AssocTySubst::new(inst));
        }
        ty
    }

    pub fn label(&self, db: &'db dyn HirAnalysisDb) -> Option<IdentId<'db>> {
        self.callable.func_def.param_label(db, self.index)
    }

    pub fn span(&self, db: &'db dyn HirAnalysisDb) -> DynLazySpan<'db> {
        self.callable.func_def.param_span(db, self.index)
    }

    pub fn index(&self) -> usize {
        self.index
    }
}

impl<'db> Callable<'db> {
    pub fn params(&'db self, db: &'db dyn HirAnalysisDb) -> impl Iterator<Item = CallableParam<'db>> + 'db {
        self.func_def.arg_tys(db)
            .iter()
            .enumerate()
            .map(|(i, &ty)| CallableParam {
                callable: self,
                index: i,
                generic_ty: ty,
            })
    }
}
```

### Step 2: Refactor `check_args` to Use Iterator
```rust
// Before: Manual index tracking
for (i, (given, expected)) in args.zip(self.func_def.arg_tys(db)).enumerate() {
    let expected_label = self.func_def.param_label(db, i);
    let mut expected_ty = expected.instantiate(db, &self.generic_args);
    if let Some(inst) = self.trait_inst {
        expected_ty = expected_ty.fold_with(db, &mut AssocTySubst::new(inst));
    }
    // ...
}

// After: Context-rich iterator
for (given_arg, param) in args.into_iter().zip(self.params(db)) {
    if let Some(expected_label) = param.label(db)
        && expected_label != given_arg.label
    {
        emit_error(param.span(db)); // No index needed!
    }

    let expected_ty = param.ty(db); // Automatically instantiated!
    tc.equate_ty(given_arg.ty, expected_ty, given_arg.span);
}
```

### Step 3: Test & Commit
```bash
cargo test -p fe-hir
git commit -m "refactor(hir/analysis): add CallableParam context-rich wrapper"
```

## Success Criteria

By the end, the analysis layer should:
- ✅ No manual index tracking in hot paths
- ✅ No manual generic instantiation at call sites
- ✅ Automatic context assembly via wrappers
- ✅ Iterator-based APIs for params, fields, variants
- ✅ Clear prototype → instance → contextual flow

## Critical Questions to Keep Asking

For each new wrapper:
1. **What analysis query needs this?** (Not "this field exists")
2. **What context must it carry?** (Parent? Index? Scope? Generic args?)
3. **Is computation cached or on-demand?** (salsa-tracked vs lightweight)
4. **Does this eliminate manual context assembly?** (The whole point!)

## Appendix: Gemini's Key Findings

### Current Access Patterns
- **Prototype access:** `func.params(db)` → `FuncParamListId` → `data(db)` → `Vec<FuncParam>`
- **Instance creation:** `lower_func(db, func)` → caches resolved `arg_tys: Vec<Binder<TyId>>`
- **Metadata access:** Delegates back to prototype (names, spans not cached)

### What's Cached vs On-Demand
- **Cached (in FuncDef):** Lowered parameter types (expensive)
- **On-demand:** Names, labels, spans (cheap lookups)
- **Pattern:** Cache expensive type resolution, delegate cheap metadata

### Context Assembly Pain Points
1. Index must be tracked separately from types
2. Generic instantiation is 2-3 step manual process
3. Scope + assumptions must be threaded through every `lower_hir_ty` call

### The AdtField Template
`AdtField` already implements the right pattern:
- Lightweight wrapper (not salsa-tracked)
- Carries context (scope)
- Defers computation (`ty()` calls `lower_hir_ty` with context)
- Used by `AdtDef` which IS salsa-tracked

**We should replicate this pattern everywhere!**
