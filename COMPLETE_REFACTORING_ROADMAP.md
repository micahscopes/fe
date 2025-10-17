# Complete HIR Refactoring Roadmap

## Overview: Three Parallel Tracks

This refactoring has three interconnected tracks that can proceed somewhat in parallel:

```
Track 1: hir_def Item Transformation (field → method)
   ↓
Track 2: Context Embedding (Body owners, scope trees, wrappers)
   ↓
Track 3: Analysis Layer Migration (use new APIs)
```

## Track 1: Item-by-Item Field Privatization

**Goal:** Transform all `hir_def` structs from public fields to accessor methods

**See:** `HIR_ITEM_BY_ITEM_TRANSFORMATION.md` for detailed guide

**Order:**
1. ✅ TopLevelMod (5 min)
2. ✅ Const (10 min)
3. ✅ Use (10 min)
4. ✅ TypeAlias (10 min)
5. ✅ Mod (10 min)
6. ✅ Impl (15 min)
7. ✅ Struct + Field wrapper (30 min)
8. ✅ Enum + variants() (20 min)
9. ✅ Func + FuncParam impl (30 min)
10. ✅ Trait + AssocType wrapper (40 min)
11. ✅ ImplTrait + AssocTypeDef wrapper (40 min)

**Total estimated time:** ~3-4 hours of focused work
**Commits:** One per struct (11 commits)

**Success criteria:**
- No public fields in hir_def items (except salsa-generated)
- All collection fields return iterators of wrappers
- Tests pass after each commit

---

## Track 2: Context Embedding

**Goal:** Make HIR nodes self-aware with embedded context

**See:** `HIR_CONTEXT_EMBEDDING_ANALYSIS.md` for detailed analysis

### Phase 2.1: Body Owner Back-References

**Estimated time:** 1-2 hours

**Changes:**
```rust
#[salsa::tracked]
pub struct Body<'db> {
    // ... existing fields ...
    owner: BodyOwner<'db>,  // NEW
}

pub enum BodyOwner<'db> {
    Func(Func<'db>),
    Const(Const<'db>),
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

**Where to update:**
- `crates/hir/src/lower/body.rs` - set owner when creating Body
- `crates/hir/src/hir_def/body.rs` - add field and methods

**Validation:**
- Can navigate: `body.owner_func(db)` works
- Test: Create body, check owner is set correctly

**Commit:** `feat(hir): add Body owner back-references for upward traversal`

---

### Phase 2.2: Scope Tree Construction

**Estimated time:** 2-3 hours

**Changes:**
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
fn build_scope_tree<'db>(db: &'db dyn HirDb, body: Body<'db>) -> ScopeTree<'db> {
    // Walk body, assign scopes
}

impl Body<'db> {
    pub fn scope_tree(self, db: &dyn HirDb) -> ScopeTree<'db> {
        build_scope_tree(db, self)
    }

    pub fn expr_scope(self, db: &dyn HirDb, expr: ExprId) -> ScopeId<'db> {
        self.scope_tree(db).expr_scopes(db).get(&expr).copied()
            .unwrap_or_else(|| /* parent scope */)
    }
}
```

**Where to add:**
- New file: `crates/hir/src/scope_tree.rs`
- Update: `crates/hir/src/hir_def/body.rs`

**Validation:**
- Compare scope_tree results with current TyCheckEnv scope tracking
- All expressions have a scope
- Nested blocks have nested scopes

**Commit:** `feat(hir): add per-body scope tree for context embedding`

---

### Phase 2.3: Expr/Stmt/Pat Wrappers

**Estimated time:** 3-4 hours

**Changes:**
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

    pub fn containing_func(self, db: &dyn HirDb) -> Option<Func<'db>> {
        self.body.owner_func(db)
    }

    // TODO: parent navigation, type queries, etc.
}
```

**Where to add:**
- New file: `crates/hir/src/hir_def/expr_wrapper.rs`
- Similar for: `stmt_wrapper.rs`, `pat_wrapper.rs`

**Validation:**
- Can create wrapper: `body.expr(db, id)`
- Can access data: `expr.data(db)`
- Can get scope: `expr.scope(db)`
- Can navigate up: `expr.containing_func(db)`

**Commit:** `feat(hir): add context-carrying Expr/Stmt/Pat wrappers`

---

## Track 3: Analysis Layer Migration

**Goal:** Update analysis to use new context-rich APIs

**See:** `HIR_REFACTORING_SYNTHESIS.md` for vision

### Phase 3.1: Update Callable to Use Iterators

**Estimated time:** 1-2 hours

**Changes:**
Update `crates/hir/src/analysis/ty/ty_check/callable.rs`:

```rust
// Add CallableParam wrapper (if not done in Track 1)
pub struct CallableParam<'db> {
    callable: &'db Callable<'db>,
    index: usize,
    generic_ty: Binder<TyId<'db>>,
}

impl CallableParam<'db> {
    pub fn ty(self, db: &dyn HirDb) -> TyId<'db> {
        // Auto-instantiation!
        let mut ty = self.generic_ty.instantiate(db, self.callable.generic_args());
        if let Some(inst) = self.callable.trait_inst() {
            ty = ty.fold_with(db, &mut AssocTySubst::new(inst));
        }
        ty
    }

    pub fn label(self, db: &dyn HirDb) -> Option<IdentId<'db>> {
        self.callable.func_def.param_label(db, self.index)
    }

    pub fn span(self, db: &dyn HirDb) -> DynLazySpan<'db> {
        self.callable.func_def.param_span(db, self.index)
    }
}

impl Callable<'db> {
    pub fn params(&self, db: &dyn HirDb) -> impl Iterator<Item = CallableParam<'db>> + '_ {
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

// Update check_args to use iterator
impl Callable<'db> {
    pub(super) fn check_args(&self, tc: &mut TyChecker<'db>, ...) {
        // Before: manual enumerate + index tracking
        // After: zip with params iterator
        for (given_arg, param) in args.into_iter().zip(self.params(db)) {
            if let Some(expected_label) = param.label(db) {
                // No index needed!
            }
            let expected_ty = param.ty(db);  // Auto-instantiated!
            tc.equate_ty(given_arg.ty, expected_ty, given_arg.span);
        }
    }
}
```

**Validation:**
- No more manual `enumerate()` in check_args
- No more manual instantiation loops
- Tests pass

**Commit:** `refactor(hir/analysis): use CallableParam iterator in type checking`

---

### Phase 3.2: Simplify TyChecker

**Estimated time:** 2-3 hours

**Changes:**
Remove redundant state from `TyCheckEnv` as context becomes embedded:

```rust
pub(super) struct TyCheckEnv<'db> {
    db: &'db dyn HirAnalysisDb,
    body: Body<'db>,

    // Keep these (mutable inference state):
    deferred: Vec<DeferredTask<'db>>,
    var_env: Vec<BlockEnv<'db>>,
    pending_vars: FxHashMap<IdentId<'db>, LocalBinding<'db>>,

    // REMOVE these (now in node wrappers):
    // loop_stack: Vec<StmtId>,      ← use expr.parent() chain
    // expr_stack: Vec<ExprId>,      ← use expr.parent() chain
}
```

**Update check_expr signatures:**
```rust
// Before
pub(super) fn check_expr(&mut self, expr: ExprId, expected: TyId) -> ExprProp;

// After
pub(super) fn check_expr(&mut self, expr: Expr<'db>, expected: TyId) -> TyId;
```

**Validation:**
- Fewer fields in TyCheckEnv
- Simpler function signatures (fewer parameters)
- Same type checking behavior

**Commit:** `refactor(hir/analysis): simplify TyChecker using context-rich wrappers`

---

## Execution Timeline

### Week 1: Foundation
- **Day 1-2:** Track 1, items 1-6 (simple encapsulation)
- **Day 3:** Track 1, items 7-8 (Struct, Enum with wrappers)
- **Day 4-5:** Track 1, items 9-11 (Trait, ImplTrait, Impl)

**Milestone:** All hir_def items use accessor methods

---

### Week 2: Context Embedding
- **Day 1:** Track 2, Phase 2.1 (Body owners)
- **Day 2-3:** Track 2, Phase 2.2 (Scope tree)
- **Day 4-5:** Track 2, Phase 2.3 (Expr/Stmt/Pat wrappers)

**Milestone:** HIR nodes carry their own context

---

### Week 3: Analysis Migration
- **Day 1-2:** Track 3, Phase 3.1 (Callable iterators)
- **Day 3-4:** Track 3, Phase 3.2 (TyChecker simplification)
- **Day 5:** Testing, documentation, cleanup

**Milestone:** Analysis layer uses context-rich APIs

---

## Progress Tracking

### Track 1: Item Transformation
```
[ ] 1. TopLevelMod
[ ] 2. Const
[ ] 3. Use
[ ] 4. TypeAlias
[ ] 5. Mod
[ ] 6. Impl
[ ] 7. Struct + Field wrapper
[ ] 8. Enum + variants()
[ ] 9. Func + FuncParam impl
[ ] 10. Trait + AssocType wrapper
[ ] 11. ImplTrait + AssocTypeDef wrapper
```

### Track 2: Context Embedding
```
[ ] 2.1. Body owner back-references
[ ] 2.2. Scope tree construction
[ ] 2.3. Expr/Stmt/Pat wrappers
```

### Track 3: Analysis Migration
```
[ ] 3.1. Callable iterator usage
[ ] 3.2. TyChecker simplification
```

## Success Metrics

### Quantitative
- **Before:** ~200 lines in TyCheckEnv
- **After:** ~100 lines (50% reduction)

- **Before:** Average 4-5 params per analysis function
- **After:** Average 2-3 params

- **Before:** ~50 manual `scope` parameter passes
- **After:** 0 (all embedded)

### Qualitative
✅ Can chain queries: `expr.parent(db).containing_block(db)`
✅ No manual index tracking in analysis
✅ No manual generic instantiation loops
✅ Analysis code is simpler and more readable

## Risk Mitigation

### Risk 1: Breaking Changes
**Mitigation:** One commit per change, easy to revert

### Risk 2: Performance Regression
**Mitigation:** Benchmark before/after, salsa should handle it

### Risk 3: Scope Creep
**Mitigation:** Stick to roadmap, resist adding features

### Risk 4: Merge Conflicts
**Mitigation:** Small, focused commits, regular rebasing

## Communication Plan

**Commit messages format:**
```
refactor(hir): <what changed>

<why it changed>
<impact on users>

Part of HIR context-embedding refactoring
See: HIR_REFACTORING_ROADMAP.md
```

**Documentation updates:**
- Update architecture docs after each track completes
- Add examples of new API usage
- Deprecation notices for old patterns

## Next Action

**Start here:**
1. Read `HIR_ITEM_BY_ITEM_TRANSFORMATION.md`
2. Pick first item: TopLevelMod
3. Make fields private
4. Fix call sites
5. Test
6. Commit
7. Repeat!

**Questions to ask yourself before starting each item:**
1. What context does this item need to provide?
2. Are there collection fields that need wrappers?
3. What's the dependency order?
4. How will I validate the transformation worked?
