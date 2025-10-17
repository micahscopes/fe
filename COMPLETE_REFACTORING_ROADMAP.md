# HIR Refactoring Roadmap

## Overview: Simple, Iterative Approach

Work one struct at a time, transforming collection fields to return context-rich wrapper iterators.

## Primary Focus: Context-Rich Collection Wrappers

**Goal:** Transform collection fields in `hir_def` items to return context-carrying wrappers

**See:** `HIR_ITEM_BY_ITEM_TRANSFORMATION.md` for detailed guide

**Items to transform:**
1. Struct → `Field<'db>` wrapper (~30 min)
2. Enum → `EnumVariant<'db>` iterator (wrapper exists) (~20 min)
3. Func → `FuncParam<'db>` wrapper (sketched, needs completion) (~30 min)
4. Trait → `AssocType<'db>` wrapper (~40 min)
5. ImplTrait → `AssocTypeDef<'db>` wrapper (~40 min)

**Total estimated time:** ~2-3 hours of focused work
**Commits:** One per struct (5 commits)

**Success criteria:**
- All collection fields return iterators of context-rich wrappers
- Wrappers carry parent reference and index
- Tests pass after each commit

---

## Progress Tracking

```
[ ] 1. Struct + Field wrapper
[ ] 2. Enum + EnumVariant iterator
[ ] 3. Func + FuncParam wrapper
[ ] 4. Trait + AssocType wrapper
[ ] 5. ImplTrait + AssocTypeDef wrapper
```

## Next Action

**Start here:**
1. Workshop naming/organizational heuristics (see below)
2. Pick simplest item (likely Enum - wrapper already exists)
3. Implement transformation
4. Test
5. Commit
6. Repeat!

---

## Naming and Organizational Heuristics

**Core Principle: Preserve existing names unless there's a collision, then add "Description" suffix**

### The Heuristic

1. **Data type naming:** Keep existing names (e.g., `FieldDef`, `VariantDef`, `AssocTyDecl`)
   - **If collision with wrapper:** Add `Description` suffix (e.g., `FuncParam` → `FuncParamDescription`)

2. **Field naming:** Pluralize the data type name
   - `FieldDef` → `field_defs`
   - `VariantDef` → `variant_defs`
   - `FuncParamDescription` → `param_descriptions`

3. **Wrapper naming:** Simple, direct name (the "nice" public API name)
   - Wrapper for fields: `Field`
   - Wrapper for variants: `EnumVariant`
   - Wrapper for params: `FuncParam`

4. **Iterator method naming:** Plural of wrapper type
   - `fields()` returns `Iterator<Item = Field>`
   - `variants()` returns `Iterator<Item = EnumVariant>`
   - `params()` returns `Iterator<Item = FuncParam>`

### Applied to Our 5 Items

| Collection | Data Type | Collision? | Resolution | Field Name | Wrapper | Method |
|------------|-----------|------------|------------|------------|---------|---------|
| Struct fields | `FieldDef` | ❌ No | Keep as-is | `field_defs` | `Field` | `fields()` |
| Enum variants | `VariantDef` | ❌ No | Keep as-is | `variant_defs` | `EnumVariant` | `variants()` |
| Func params | `FuncParam` | ✅ Yes | → `FuncParamDescription` | `param_descriptions` | `FuncParam` | `params()` |
| Trait assoc types | `AssocTyDecl` | ❌ No | Keep as-is | `assoc_type_decls` | `AssocType` | `assoc_types()` |
| ImplTrait assoc types | `AssocTyDef` | ⚠️ Close | Keep as-is | `assoc_type_defs` | `ImplAssocType` | `assoc_types()` |

---

## Future Directions (Gestural)

Once collection wrappers are complete, the natural next layer involves **embedding context into HIR nodes themselves**, moving from "wrappers carry context" to "nodes know their context."

**The general arc:**
1. **Upward links** - nodes know their parents (e.g., Body → Func, Expr → Body)
2. **Scope embedding** - nodes can answer "what's in scope here?" without manual threading
3. **Expression wrappers** - Expr/Stmt/Pat become context-aware like collection items
4. **Analysis simplification** - type checker uses embedded context instead of manual state

**The key insight:** Collection wrappers (parent + index) are the microcosm. The same pattern scales to expression trees, scope chains, and eventually the entire analysis layer.

See `HIR_CONTEXT_EMBEDDING_ANALYSIS.md` and `HIR_REFACTORING_SYNTHESIS.md` for exploratory thinking on these directions.

