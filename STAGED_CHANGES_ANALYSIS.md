# Analysis of Staged Changes: The FuncParam Sketch

## What the Sketch Shows

The staged changes represent a **partial transformation** of `Func` to use the context-rich wrapper pattern:

```diff
// Field renamed and made private
- pub params: Partial<FuncParamListId<'db>>,
+ param_descriptions: Partial<FuncParamListId<'db>>,

// New public method (empty)
+ pub fn params(self, db: &dyn HirDb) -> Vec<FuncParam> {}

// New wrapper struct
+ pub struct FuncParam<'db> {
+     parent: Func<'db>,
+     index: u16,
+     desc: FuncParamDescription<'db>,
+ }
```

## Critical Insight: Name Collision

The sketch reveals an important issue:

### There Are TWO Different `FuncParam` Types!

1. **`hir_def::params::FuncParam`** (exists in params.rs:136)
   - This is the **prototype/description**
   - Has fields: `is_mut`, `label`, `name`, `ty`, `self_ty_fallback`
   - This is what's stored in `FuncParamListId`
   - Already has methods like `label_eagerly()`, `is_self_param()`

2. **`hir_def::item::FuncParam`** (sketched in staged changes)
   - This is the **context-rich wrapper**
   - Has fields: `parent`, `index`, `desc`
   - Should provide contextual access to analysis queries
   - The `desc` field is of type `FuncParamDescription<'db>`

### Wait - What's `FuncParamDescription`?

Looking at the import:
```rust
use crate::hir_def::{FuncParamDescription, TraitRefId};
```

But I don't see `FuncParamDescription` defined anywhere! This must be a **type alias** that should be created:

```rust
// In params.rs
pub type FuncParamDescription<'db> = FuncParam<'db>;
```

Or the wrapper should directly use the existing `FuncParam` type.

## What's Missing from the Sketch

### 1. Implementation of `params()` Method

Currently empty:
```rust
pub fn params(self, db: &dyn HirDb) -> Vec<FuncParam> {}
```

Should be:
```rust
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
            desc: *desc,  // FuncParam is Copy
        });

    itertools::Either::Right(iter)
}
```

### 2. Implementation Block for `FuncParam` Wrapper

The wrapper struct is defined but has **no methods**:

```rust
impl<'db> FuncParam<'db> {
    // Structural accessors
    pub fn parent(self) -> Func<'db> {
        self.parent
    }

    pub fn index(self) -> usize {
        self.index as usize
    }

    // Delegate to description
    pub fn is_mut(self) -> bool {
        self.desc.is_mut
    }

    pub fn label(self) -> Option<FuncParamName<'db>> {
        self.desc.label
    }

    pub fn name(self) -> Partial<FuncParamName<'db>> {
        self.desc.name
    }

    pub fn ty_hir(self) -> Partial<TypeId<'db>> {
        self.desc.ty
    }

    pub fn is_self_param(self, db: &dyn HirDb) -> bool {
        self.desc.is_self_param(db)
    }

    pub fn label_eagerly(self) -> Option<IdentId<'db>> {
        self.desc.label_eagerly()
    }

    // Context-rich additions
    pub fn span(self, db: &dyn HirDb) -> LazyFuncParamSpan<'db> {
        self.parent.span().params().param(self.index as usize)
    }

    pub fn containing_func(self) -> Func<'db> {
        self.parent
    }

    // TODO: Type analysis queries
    // pub fn ty_analyzed(self, db: &dyn HirAnalysisDb) -> TyId<'db> {
    //     // Use parent + index to query analysis layer
    //     func_param_ty(db, self.parent, self.index)
    // }
}
```

### 3. Fix Broken Call Sites

The existing code has calls like:
```rust
pub fn param_label(self, db: &'db dyn HirDb, idx: usize) -> Option<IdentId<'db>> {
    self.params(db).to_opt()?.data(db).get(idx)?.label_eagerly()
}
```

This won't work anymore because:
- `self.params(db)` now returns an iterator of wrappers, not `Partial<FuncParamListId>`
- Need to either keep the old accessor or update call sites

**Two options:**

**Option A:** Keep both accessors
```rust
// Keep private accessor for internal use
fn param_descriptions(self, db: &dyn HirDb) -> Partial<FuncParamListId<'db>> {
    // salsa-generated
}

// New public API
pub fn params(self, db: &dyn HirDb) -> impl Iterator<Item = FuncParam<'db>> {
    // wrapper iterator
}

// Update existing methods to use param_descriptions
pub fn param_label(self, db: &'db dyn HirDb, idx: usize) -> Option<IdentId<'db>> {
    self.param_descriptions(db).to_opt()?.data(db).get(idx)?.label_eagerly()
}
```

**Option B:** Rewrite helpers to use new API
```rust
pub fn param_label(self, db: &'db dyn HirDb, idx: usize) -> Option<IdentId<'db>> {
    self.params(db).nth(idx)?.label_eagerly()
}
```

### 4. Naming Conflict Resolution

Need to decide on names to avoid collision:

**Option A:** Keep both, rename wrapper
```rust
// In params.rs - the description/prototype
pub struct FuncParamDef<'db> { ... }  // Rename from FuncParam

// In item.rs - the context-rich wrapper
pub struct FuncParam<'db> {
    parent: Func<'db>,
    index: u16,
    desc: FuncParamDef<'db>,  // Reference to def
}
```

**Option B:** Use type alias
```rust
// In params.rs
pub struct FuncParamData<'db> { ... }  // Rename
pub type FuncParamDescription<'db> = FuncParamData<'db>;  // Alias

// In item.rs
pub struct FuncParam<'db> {
    parent: Func<'db>,
    index: u16,
    desc: FuncParamDescription<'db>,
}
```

**Option C:** Module namespacing
```rust
// Keep FuncParam name in both places, use qualified imports
use crate::hir_def::params::FuncParam as FuncParamDef;

pub struct FuncParam<'db> {
    parent: Func<'db>,
    index: u16,
    desc: FuncParamDef<'db>,
}
```

## What the Sketch Gets Right

1. ✅ **Field renamed and privatized** - `params` → `param_descriptions`
2. ✅ **Wrapper carries context** - `parent: Func<'db>` and `index: u16`
3. ✅ **Wrapper references description** - `desc` field
4. ✅ **Public method returns wrappers** - `params()` method (though empty)

## What Needs to Be Added

1. ❌ **Implement `params()` method** - return iterator of wrappers
2. ❌ **Add `impl` block for wrapper** - accessor methods
3. ❌ **Fix name collision** - `FuncParam` exists in two places
4. ❌ **Update call sites** - existing helpers like `param_label()`
5. ❌ **Add `FuncParamDescription` type** - currently imported but doesn't exist

## Broader Pattern This Demonstrates

This sketch shows the **Pattern B transformation** for any collection field:

### Generic Pattern for Collection Wrappers

```rust
// Step 1: Rename and privatize collection field
#[salsa::tracked]
pub struct Parent<'db> {
    // OLD: pub children: ChildListId<'db>,
    child_list: ChildListId<'db>,  // Renamed + private
}

// Step 2: Create context-rich wrapper
pub struct Child<'db> {
    parent: Parent<'db>,
    index: usize,
    desc: ChildDescription<'db>,  // Data from the list
}

// Step 3: Implement wrapper methods
impl<'db> Child<'db> {
    // Structural
    pub fn parent(self) -> Parent<'db> { self.parent }
    pub fn index(self) -> usize { self.index }

    // Data delegation
    pub fn name(self) -> IdentId<'db> { self.desc.name }

    // Context-rich additions
    pub fn span(self, db: &dyn HirDb) -> LazySpan<'db> {
        self.parent.span().children().child(self.index)
    }
}

// Step 4: Add iterator method on parent
impl<'db> Parent<'db> {
    pub fn children(self, db: &dyn HirDb) -> impl Iterator<Item = Child<'db>> + '_ {
        let count = self.child_list(db).data(db).len();
        (0..count).map(move |i| Child {
            parent: self,
            index: i,
            desc: self.child_list(db).data(db)[i],
        })
    }
}
```

## Connection to Broader Refactoring Goals

This `FuncParam` sketch is a **microcosm** of the entire refactoring:

1. **Prototype** (`FuncParamData` in params.rs) = syntactic structure
2. **Wrapper** (`FuncParam` in item.rs) = context-carrying accessor
3. **Iteration** (`.params(db)`) = traversal API
4. **Context** (`parent` + `index`) = enables upward navigation and analysis queries

Once this pattern works for `Func.params()`, it can be replicated for:
- `Struct.fields()` → `Field<'db>`
- `Enum.variants()` → `EnumVariant<'db>` (already exists!)
- `Trait.assoc_types()` → `AssocType<'db>`
- `Impl.methods()` → `Method<'db>`

## Recommended Completion Steps

### Step 1: Resolve Naming (5 min)
Choose naming strategy (recommend Option A: rename def to `FuncParamDef`)

### Step 2: Implement `params()` (10 min)
Fill in the empty method body to return iterator

### Step 3: Add Wrapper Methods (20 min)
Implement the `impl FuncParam<'db>` block

### Step 4: Fix Call Sites (15 min)
Update existing `param_label()`, `param_label_or_name()` methods

### Step 5: Test (10 min)
Run `cargo test -p fe-hir`, fix any issues

### Total: ~1 hour to complete the sketch

## Questions for Consideration

1. **Return type:** `Vec<FuncParam>` or `impl Iterator`?
   - Sketch says `Vec`, but iterator is more flexible
   - Analysis: Iterator is better (lazy, no allocation)

2. **Name collision:** How to handle two `FuncParam` types?
   - Analysis: Rename the description type to `FuncParamDef`

3. **Analysis queries:** Where do they live?
   - Analysis: Add `ty_analyzed()` method on wrapper that queries analysis layer
   - This is the **bridge** to the analysis layer we discussed!

4. **Field visibility:** Should `desc` field be public or private?
   - Analysis: Private, force users to use accessor methods

## Next Actions

1. Complete the `FuncParam` implementation following the pattern above
2. Test it works
3. Use as template for other collection wrappers
4. Document the pattern for future transformations
