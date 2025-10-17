# HIR Item-by-Item Transformation Guide

## The Goal: Context-Rich Collection Wrappers

**Current state:** Collection fields return raw list IDs requiring manual index tracking
**Target state:** Collection fields return iterators of context-carrying wrappers

### Understanding Salsa Accessors

**Important:** Salsa tracked structs automatically generate accessor methods for ALL fields.
Both `pub` and `pub(crate)` fields become methods accessed like `x.foo(db)`.

This means most simple fields are already fine as-is. The transformation focuses on **collections**.

### The Transformation Pattern: Context-Rich Wrappers

For collection fields - rename field, add public method returning iterator of context-carrying wrappers

```rust
// Before
#[salsa::tracked]
pub struct Enum<'db> {
    pub variants: VariantDefListId<'db>,  // ← returns raw list ID
}

// Usage: manual index tracking
let variant_list = enum_.variants(db);
for i in 0..variant_list.data(db).len() {
    let variant_def = &variant_list.data(db)[i];
    let name = variant_def.name;  // ← no parent context
}

// After
#[salsa::tracked]
pub struct Enum<'db> {
    variant_defs: VariantDefListId<'db>,  // ← renamed (salsa generates private accessor)
}

impl Enum<'db> {
    pub fn variants(self, db: &dyn HirDb) -> impl Iterator<Item = EnumVariant<'db>> + '_ {
        let count = self.variant_defs(db).data(db).len();
        (0..count).map(move |i| EnumVariant::new(self, i))
    }
}

// Usage: context-rich iteration
for variant in enum_.variants(db) {
    let name = variant.name(db);      // ← has parent context!
    let parent_enum = variant.parent(); // ← can navigate upward
}
```

## Items Requiring Transformation

Focus on structs with collection fields that need context-rich wrappers:

### 1. Struct (and Contract)
- **Collection field:** `fields: FieldDefListId<'db>`
- **Wrapper needed:** `Field<'db>` (wraps FieldDef with parent context)
- **Estimated effort:** 30 min

**Transformation:**
```rust
// Before
#[salsa::tracked]
pub struct Struct<'db> {
    // ... other fields ...
    pub fields: FieldDefListId<'db>,  // ← returns raw list ID
}

// After
#[salsa::tracked]
pub struct Struct<'db> {
    // ... other fields ...
    field_defs: FieldDefListId<'db>,  // ← renamed (salsa generates private accessor)
}

impl Struct<'db> {
    pub fn fields(self, db: &dyn HirDb) -> impl Iterator<Item = Field<'db>> + '_ {
        let count = self.field_defs(db).data(db).len();
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

### 2. Enum
- **Collection field:** `variants: VariantDefListId<'db>`
- **Wrapper:** `EnumVariant<'db>` (**already exists!** Just need to add iterator API)
- **Estimated effort:** 20 min

**Transformation:**
```rust
// Before
#[salsa::tracked]
pub struct Enum<'db> {
    // ... other fields ...
    pub variants: VariantDefListId<'db>,  // ← returns raw list ID
}

// After
#[salsa::tracked]
pub struct Enum<'db> {
    // ... other fields ...
    variant_defs: VariantDefListId<'db>,  // ← renamed (salsa generates private accessor)
}

impl Enum<'db> {
    pub fn variants(self, db: &dyn HirDb) -> impl Iterator<Item = EnumVariant<'db>> + '_ {
        let count = self.variant_defs(db).data(db).len();
        (0..count).map(move |i| EnumVariant::new(self, i))
    }
}

// EnumVariant already exists and is already context-rich! ✅
// Just update call sites to use the iterator API
```

**Commit:** `refactor(hir): add variants() iterator for Enum`

---

### 3. Func
- **Collection field:** `params: Partial<FuncParamListId<'db>>` (note: already renamed to `param_descriptions` in staged changes)
- **Wrapper needed:** `FuncParam<'db>` (**already sketched!** Need to implement)
- **Estimated effort:** 30 min

**Current state (staged changes):**
```rust
#[salsa::tracked]
pub struct Func<'db> {
    // ... other fields ...
    param_descriptions: Partial<FuncParamListId<'db>>,  // ← already renamed!
}

impl Func<'db> {
    pub fn params(self, db: &dyn HirDb) -> Vec<FuncParam> {}  // ← empty, needs impl!
}

pub struct FuncParam<'db> {
    parent: Func<'db>,
    index: u16,
    desc: FuncParamDescription<'db>,  // ← type doesn't exist yet!
}
```

**Needs completion:**
- Implement the empty `params()` method body
- Add impl block for `FuncParam` wrapper with accessor methods
- Resolve naming (what is `FuncParamDescription`?)
- Update broken call sites like `param_label()`

See STAGED_CHANGES_ANALYSIS.md for detailed analysis.

**Commit:** `refactor(hir): implement FuncParam wrapper and params() iterator`

---

### 4. Trait
- **Collection field:** `types: Vec<AssocTyDecl<'db>>`
- **Wrapper needed:** `AssocType<'db>` (new)
- **Estimated effort:** 40 min

**Transformation:**
```rust
// Before
#[salsa::tracked]
pub struct Trait<'db> {
    // ... other fields ...
    #[return_ref]
    pub types: Vec<AssocTyDecl<'db>>,  // ← returns raw vec
}

// After
#[salsa::tracked]
pub struct Trait<'db> {
    // ... other fields ...
    #[return_ref]
    assoc_type_decls: Vec<AssocTyDecl<'db>>,  // ← renamed (salsa generates private accessor)
}

impl Trait<'db> {
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

**Commit:** `refactor(hir): add AssocType wrapper for Trait`

---

### 5. ImplTrait
- **Collection field:** `types: Vec<...>` (similar to Trait)
- **Wrapper needed:** `AssocTypeDef<'db>` (new)
- **Estimated effort:** 40 min

**Commit:** `refactor(hir): add AssocTypeDef wrapper for ImplTrait`

---

## Testing Strategy

For each transformation:

1. **Rename collection field** (e.g., `fields` → `field_defs`)
2. **Add public iterator method** returning context-rich wrappers
3. **Implement wrapper struct** with parent reference and accessor methods
4. **Run `cargo check`** - see what breaks
5. **Fix call sites** to use new iterator API
6. **Run tests:** `cargo test -p fe-hir`
7. **Commit**

## Progress Tracking

```markdown
[ ] 1. Struct + Field wrapper
[ ] 2. Enum + EnumVariant iterator (wrapper already exists)
[ ] 3. Func + FuncParam wrapper (already sketched, needs completion)
[ ] 4. Trait + AssocType wrapper
[ ] 5. ImplTrait + AssocTypeDef wrapper
```

## Key Principles

1. **One struct per commit** - easy to review, easy to revert
2. **Test after each** - don't batch transformations
3. **Start simple** - Enum is easiest (wrapper already exists)
4. **Workshop naming** - decide naming conventions before implementing
