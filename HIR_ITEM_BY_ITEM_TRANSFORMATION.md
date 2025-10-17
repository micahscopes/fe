# HIR Item-by-Item Transformation Guide

## The Goal: Systematic Field → Method Conversion

**Current state:** `hir_def` items have **public attribute-like fields**
**Target state:** Items have **private fields** with **context-injecting accessor methods**

### The Two Transformation Patterns

#### Pattern A: Simple Encapsulation
For scalar/simple fields - just make private, use salsa-generated accessor

```rust
// Before
#[salsa::tracked]
pub struct Func<'db> {
    pub name: Partial<IdentId<'db>>,  // ← public field
}

// After
#[salsa::tracked]
pub struct Func<'db> {
    pub(crate) name: Partial<IdentId<'db>>,  // ← private field
}

// Access changes from:
let name = func.name;

// To:
let name = func.name(db);  // ← salsa-generated method
```

#### Pattern B: Context-Rich Wrapper
For collection fields - make private, return iterator of context-carrying wrappers

```rust
// Before
#[salsa::tracked]
pub struct Enum<'db> {
    pub variants: VariantDefListId<'db>,  // ← public field, raw ID
}

// Usage: manual index tracking
let variant_list = enum_.variants;
for i in 0..variant_list.data(db).len() {
    let variant_def = &variant_list.data(db)[i];
    let name = variant_def.name;  // ← no parent context
}

// After
#[salsa::tracked]
pub struct Enum<'db> {
    pub(crate) variant_list: VariantDefListId<'db>,  // ← private, renamed
}

impl Enum<'db> {
    pub fn variants(self, db: &dyn HirDb) -> impl Iterator<Item = EnumVariant<'db>> {
        let count = self.variant_list(db).data(db).len();
        (0..count).map(move |i| EnumVariant::new(self, i))
    }
}

// Usage: context-rich iteration
for variant in enum_.variants(db) {
    let name = variant.name(db);  // ← has parent context!
    let parent_enum = variant.enum_;  // ← can navigate upward
}
```

## Complete Transformation Checklist

### Group 1: Leaves (No dependencies on other items)

Transform these first - they don't reference other items being transformed.

#### ✅ 1. TopLevelMod
- **Pattern:** A (simple)
- **Public fields to hide:** `name`
- **Dependencies:** None
- **Estimated effort:** 5 min

**Transformation:**
```rust
// Before
#[salsa::tracked]
pub struct TopLevelMod<'db> {
    pub name: IdentId<'db>,
    pub(crate) file: File,
}

// After
#[salsa::tracked]
pub struct TopLevelMod<'db> {
    pub(crate) name: IdentId<'db>,  // ← changed
    pub(crate) file: File,
}

// Update all call sites:
// top_mod.name → top_mod.name(db)
```

**Commit:** `refactor(hir): privatize TopLevelMod fields, use accessor methods`

---

#### ✅ 2. Const
- **Pattern:** A (simple)
- **Public fields to hide:** `name`, `attributes`, `ty`, `body`, `vis`, `top_mod`
- **Dependencies:** TopLevelMod (already done), Body
- **Estimated effort:** 10 min

**Transformation:**
```rust
// Before
#[salsa::tracked]
pub struct Const<'db> {
    pub name: Partial<IdentId<'db>>,
    pub attributes: AttrListId<'db>,
    pub ty: Partial<TypeId<'db>>,
    pub body: Partial<Body<'db>>,
    pub vis: Visibility,
    pub top_mod: TopLevelMod<'db>,
}

// After
#[salsa::tracked]
pub struct Const<'db> {
    pub(crate) name: Partial<IdentId<'db>>,
    pub(crate) attributes: AttrListId<'db>,
    pub(crate) ty: Partial<TypeId<'db>>,
    pub(crate) body: Partial<Body<'db>>,
    pub(crate) vis: Visibility,
    pub(crate) top_mod: TopLevelMod<'db>,
}

// All access becomes: const_.name(db), const_.body(db), etc.
```

**Commit:** `refactor(hir): privatize Const fields, use accessor methods`

---

#### ✅ 3. Use
- **Pattern:** A (simple)
- **Public fields to hide:** `path`, `alias`, `vis`, `top_mod`
- **Dependencies:** TopLevelMod
- **Estimated effort:** 10 min

**Commit:** `refactor(hir): privatize Use fields, use accessor methods`

---

#### ✅ 4. TypeAlias
- **Pattern:** A (simple)
- **Public fields to hide:** `name`, `attributes`, `vis`, `generic_params`, `ty`, `top_mod`
- **Dependencies:** TopLevelMod
- **Estimated effort:** 10 min

**Commit:** `refactor(hir): privatize TypeAlias fields, use accessor methods`

---

#### ✅ 5. Mod
- **Pattern:** A (simple)
- **Public fields to hide:** `name`, `attributes`, `vis`, `top_mod`
- **Dependencies:** TopLevelMod
- **Estimated effort:** 10 min

**Commit:** `refactor(hir): privatize Mod fields, use accessor methods`

---

### Group 2: Collections (Need context-rich wrappers)

These have collection fields that should return iterators of wrappers.

#### ✅ 6. Struct (and Contract)
- **Pattern:** A for simple fields, B for `fields`
- **Public fields to hide:** `name`, `attributes`, `vis`, `generic_params`, `where_clause`, `top_mod`
- **Wrapper needed:** `Field<'db>` (wraps FieldDef with parent context)
- **Dependencies:** TopLevelMod, FieldDefListId
- **Estimated effort:** 30 min

**Transformation:**
```rust
// Before
#[salsa::tracked]
pub struct Struct<'db> {
    pub name: Partial<IdentId<'db>>,
    pub attributes: AttrListId<'db>,
    pub vis: Visibility,
    pub generic_params: GenericParamListId<'db>,
    pub where_clause: WhereClauseId<'db>,
    pub fields: FieldDefListId<'db>,  // ← needs wrapper!
    pub top_mod: TopLevelMod<'db>,
}

// After
#[salsa::tracked]
pub struct Struct<'db> {
    pub(crate) name: Partial<IdentId<'db>>,
    pub(crate) attributes: AttrListId<'db>,
    pub(crate) vis: Visibility,
    pub(crate) generic_params: GenericParamListId<'db>,
    pub(crate) where_clause: WhereClauseId<'db>,
    pub(crate) field_list: FieldDefListId<'db>,  // ← renamed + private
    pub(crate) top_mod: TopLevelMod<'db>,
}

impl Struct<'db> {
    pub fn fields(self, db: &dyn HirDb) -> impl Iterator<Item = Field<'db>> + '_ {
        let count = self.field_list(db).data(db).len();
        (0..count).map(move |i| Field::new(FieldParent::Struct(self), i))
    }
}

// NEW: Context-rich wrapper
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Field<'db> {
    parent: FieldParent<'db>,
    index: usize,
}

impl Field<'db> {
    fn new(parent: FieldParent<'db>, index: usize) -> Self {
        Self { parent, index }
    }

    pub fn parent(self) -> FieldParent<'db> {
        self.parent
    }

    pub fn index(self) -> usize {
        self.index
    }

    pub fn def(self, db: &dyn HirDb) -> &FieldDef<'db> {
        &self.parent.fields(db).data(db)[self.index]
    }

    pub fn name(self, db: &dyn HirDb) -> Partial<IdentId<'db>> {
        self.def(db).name
    }

    pub fn ty(self, db: &dyn HirDb) -> Partial<TypeId<'db>> {
        self.def(db).ty
    }

    pub fn vis(self, db: &dyn HirDb) -> Visibility {
        self.def(db).vis
    }

    pub fn span(self, db: &dyn HirDb) -> DynLazySpan<'db> {
        self.parent.field_name_span(self.index)
    }
}
```

**Commit:** `refactor(hir): add context-rich Field wrapper for Struct/Contract`

---

#### ✅ 7. Enum
- **Pattern:** A for simple fields, B for `variants`
- **Wrapper needed:** `EnumVariant<'db>` (**already exists!** Just need to update API)
- **Dependencies:** TopLevelMod, VariantDefListId
- **Estimated effort:** 20 min

**Transformation:**
```rust
// Before
#[salsa::tracked]
pub struct Enum<'db> {
    pub name: Partial<IdentId<'db>>,
    pub attributes: AttrListId<'db>,
    pub vis: Visibility,
    pub generic_params: GenericParamListId<'db>,
    pub where_clause: WhereClauseId<'db>,
    pub variants: VariantDefListId<'db>,  // ← needs wrapper!
    pub top_mod: TopLevelMod<'db>,
}

// After
#[salsa::tracked]
pub struct Enum<'db> {
    pub(crate) name: Partial<IdentId<'db>>,
    pub(crate) attributes: AttrListId<'db>,
    pub(crate) vis: Visibility,
    pub(crate) generic_params: GenericParamListId<'db>,
    pub(crate) where_clause: WhereClauseId<'db>,
    pub(crate) variant_list: VariantDefListId<'db>,  // ← renamed + private
    pub(crate) top_mod: TopLevelMod<'db>,
}

impl Enum<'db> {
    pub fn variants(self, db: &dyn HirDb) -> impl Iterator<Item = EnumVariant<'db>> + '_ {
        let count = self.variant_list(db).data(db).len();
        (0..count).map(move |i| EnumVariant::new(self, i))
    }
}

// EnumVariant already exists and is already context-rich! ✅
// Just update call sites to use the iterator API
```

**Commit:** `refactor(hir): add variants() iterator for Enum, privatize fields`

---

#### ✅ 8. Func
- **Pattern:** A for simple fields, B for `param_descriptions`
- **Wrapper needed:** `FuncParam<'db>` (**already sketched!** Need to implement)
- **Dependencies:** TopLevelMod, Body, FuncParamListId
- **Estimated effort:** 30 min

**Transformation:**
```rust
// Before
#[salsa::tracked]
pub struct Func<'db> {
    pub name: Partial<IdentId<'db>>,
    pub attributes: AttrListId<'db>,
    pub generic_params: GenericParamListId<'db>,
    pub where_clause: WhereClauseId<'db>,
    param_descriptions: Partial<FuncParamListId<'db>>,  // ← already private! ✅
    pub ret_ty: Option<TypeId<'db>>,
    pub modifier: ItemModifier,
    pub body: Option<Body<'db>>,
    pub is_extern: bool,
    pub top_mod: TopLevelMod<'db>,
}

impl Func<'db> {
    pub fn params(self, db: &dyn HirDb) -> Vec<FuncParam> {}  // ← empty, needs impl!
}

// After
#[salsa::tracked]
pub struct Func<'db> {
    pub(crate) name: Partial<IdentId<'db>>,
    pub(crate) attributes: AttrListId<'db>,
    pub(crate) generic_params: GenericParamListId<'db>,
    pub(crate) where_clause: WhereClauseId<'db>,
    param_descriptions: Partial<FuncParamListId<'db>>,
    pub(crate) ret_ty: Option<TypeId<'db>>,
    pub(crate) modifier: ItemModifier,
    pub(crate) body: Option<Body<'db>>,
    pub(crate) is_extern: bool,
    pub(crate) top_mod: TopLevelMod<'db>,
}

impl Func<'db> {
    pub fn params(self, db: &dyn HirDb) -> impl Iterator<Item = FuncParam<'db>> + '_ {
        let Some(param_list) = self.param_descriptions(db).to_opt() else {
            return itertools::Either::Left(std::iter::empty());
        };

        let iter = param_list.data(db)
            .iter()
            .enumerate()
            .map(move |(idx, desc)| FuncParam {
                parent: self,
                index: idx as u16,
                desc: *desc,
            });

        itertools::Either::Right(iter)
    }
}

// FuncParam already defined, add impl
impl FuncParam<'db> {
    pub fn parent(self) -> Func<'db> {
        self.parent
    }

    pub fn index(self) -> usize {
        self.index as usize
    }

    pub fn name(self, db: &dyn HirDb) -> Partial<FuncParamName<'db>> {
        self.desc.name
    }

    pub fn label(self, db: &dyn HirDb) -> Option<FuncParamName<'db>> {
        self.desc.label
    }

    pub fn ty_hir(self, db: &dyn HirDb) -> Partial<TypeId<'db>> {
        self.desc.ty
    }

    pub fn is_mut(self, db: &dyn HirDb) -> bool {
        self.desc.is_mut
    }

    pub fn span(self, db: &dyn HirDb) -> LazyFuncParamSpan<'db> {
        self.parent.span().params().param(self.index)
    }
}
```

**Commit:** `refactor(hir): implement FuncParam wrapper, privatize Func fields`

---

#### ✅ 9. Trait
- **Pattern:** A for simple fields, B for `types` and `super_traits`
- **Wrapper needed:** `AssocType<'db>` (new)
- **Dependencies:** TopLevelMod, TraitRefId, AssocTyDecl
- **Estimated effort:** 40 min

**Transformation:**
```rust
// Before
#[salsa::tracked]
pub struct Trait<'db> {
    pub name: Partial<IdentId<'db>>,
    pub attributes: AttrListId<'db>,
    pub vis: Visibility,
    pub generic_params: GenericParamListId<'db>,
    #[return_ref]
    pub super_traits: Vec<TraitRefId<'db>>,  // ← could return iterator
    pub where_clause: WhereClauseId<'db>,
    #[return_ref]
    pub types: Vec<AssocTyDecl<'db>>,  // ← needs wrapper!
    pub top_mod: TopLevelMod<'db>,
}

// After
#[salsa::tracked]
pub struct Trait<'db> {
    pub(crate) name: Partial<IdentId<'db>>,
    pub(crate) attributes: AttrListId<'db>,
    pub(crate) vis: Visibility,
    pub(crate) generic_params: GenericParamListId<'db>,
    #[return_ref]
    pub(crate) super_trait_refs: Vec<TraitRefId<'db>>,  // ← renamed
    pub(crate) where_clause: WhereClauseId<'db>,
    #[return_ref]
    pub(crate) assoc_type_decls: Vec<AssocTyDecl<'db>>,  // ← renamed
    pub(crate) top_mod: TopLevelMod<'db>,
}

impl Trait<'db> {
    pub fn super_traits(self, db: &dyn HirDb) -> &[TraitRefId<'db>] {
        self.super_trait_refs(db)  // For now, just delegate
    }

    pub fn assoc_types(self, db: &dyn HirDb) -> impl Iterator<Item = AssocType<'db>> + '_ {
        let count = self.assoc_type_decls(db).len();
        (0..count).map(move |i| AssocType {
            parent: self,
            index: i,
        })
    }
}

// NEW: Context-rich wrapper
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AssocType<'db> {
    parent: Trait<'db>,
    index: usize,
}

impl AssocType<'db> {
    pub fn parent(self) -> Trait<'db> {
        self.parent
    }

    pub fn index(self) -> usize {
        self.index
    }

    pub fn decl(self, db: &dyn HirDb) -> &AssocTyDecl<'db> {
        &self.parent.assoc_type_decls(db)[self.index]
    }

    pub fn name(self, db: &dyn HirDb) -> Partial<IdentId<'db>> {
        self.decl(db).name
    }

    pub fn bounds(self, db: &dyn HirDb) -> &[TypeBound<'db>] {
        &self.decl(db).bounds
    }

    pub fn default(self, db: &dyn HirDb) -> Option<TypeId<'db>> {
        self.decl(db).default
    }
}
```

**Commit:** `refactor(hir): add AssocType wrapper for Trait, privatize fields`

---

#### ✅ 10. ImplTrait
- **Pattern:** Similar to Trait
- **Wrapper needed:** `AssocTypeDef<'db>` (new)
- **Dependencies:** TopLevelMod, TraitRefId
- **Estimated effort:** 40 min

**Commit:** `refactor(hir): add AssocTypeDef wrapper for ImplTrait, privatize fields`

---

#### ✅ 11. Impl
- **Pattern:** A (simple, but has child items via scope graph)
- **Public fields to hide:** `ty`, `attributes`, `generic_params`, `where_clause`, `top_mod`
- **Dependencies:** TopLevelMod
- **Estimated effort:** 15 min
- **Note:** Child functions accessed via `impl.funcs(db)` which uses scope graph, not a field

**Commit:** `refactor(hir): privatize Impl fields, use accessor methods`

---

## Testing Strategy for Each Transformation

For each item transformation:

1. **Make fields private** (add `pub(crate)`)
2. **Run `cargo check`** - see what breaks
3. **Fix call sites** systematically:
   - Use ripgrep: `rg "struct_name\.\w+" --type rust`
   - Update to method calls: `.field` → `.field(db)`
4. **For wrapper fields:**
   - Implement wrapper struct with methods
   - Update iteration patterns to use new API
5. **Run tests:** `cargo test -p fe-hir`
6. **Commit** with message format: `refactor(hir): <what changed>`

## Progress Tracking Template

```markdown
## Transformation Progress

- [x] 1. TopLevelMod (Pattern A)
- [x] 2. Const (Pattern A)
- [ ] 3. Use (Pattern A)
- [ ] 4. TypeAlias (Pattern A)
- [ ] 5. Mod (Pattern A)
- [ ] 6. Struct + Field wrapper (Pattern B)
- [ ] 7. Enum + variants iterator (Pattern B)
- [ ] 8. Func + FuncParam impl (Pattern B)
- [ ] 9. Trait + AssocType wrapper (Pattern B)
- [ ] 10. ImplTrait + AssocTypeDef wrapper (Pattern B)
- [ ] 11. Impl (Pattern A)
```

## Validation: Is Transformation Complete?

For each struct, check:
- ✅ No public fields except `id` (auto-generated by salsa)
- ✅ All collection fields have iterator methods returning wrappers
- ✅ All wrapper structs have parent reference and methods
- ✅ Tests pass: `cargo test -p fe-hir`
- ✅ No clippy warnings about public fields

## Next Steps After hir_def Items

Once all `hir_def/item.rs` structs are transformed:

1. **Transform other hir_def modules:**
   - `params.rs` (GenericParam wrappers)
   - `expr.rs` / `stmt.rs` / `pat.rs` (Node wrappers with Body context)
   - `path.rs`, `types.rs`, etc.

2. **Add context embedding:**
   - Body owner back-references
   - Scope tree construction
   - Expr/Stmt/Pat wrappers with embedded context

3. **Migrate analysis layer:**
   - Update to use new iterator APIs
   - Remove redundant context threading
   - Simplify TyChecker

## Key Principles

1. **One struct per commit** - easy to review, easy to revert
2. **Test after each** - don't batch transformations
3. **Follow dependency order** - leaves first, then collections
4. **Pattern A before B** - simple changes build confidence
5. **Document wrappers** - explain what context they carry and why
