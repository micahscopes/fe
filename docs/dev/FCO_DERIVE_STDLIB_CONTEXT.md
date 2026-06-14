# FCO derive + quasi-quote: machinery vs. std-lib (context for std-lib polish)

**Snapshot as of commit `abf8a6247`. Repo is authoritative; treat this as a
hand-off brief, not a spec.**

## TL;DR

The compile-time **derive-via-provider** machinery (reflection + an impl
builder + `quote { … }` metaprogramming) is deep, landed, and tested. What is
*thin* is the **standard library**: only **three** derive providers ship. The
engine can express far more than the library currently asks of it. This is the
"built the engine, didn't flesh out std" gap.

| Layer | State |
|---|---|
| Provider execution engine (interpreter, budgets, diagnostics) | **landed** |
| Reflection API (`Reflect`/`Field`/`Variant`) | **landed** (struct + enum) |
| Impl builder API (`ImplBuilder`, ~50 ops) | **landed** |
| `quote { … }` templates (holes, open names, match-arm folding) | **landed (restricted fragment)** |
| Std derive providers | **3 only** — `Eq`, `Default`, `Eip712` |
| Std derivable traits with **no** provider | many (see gap list) |

The intrinsic types (`Derive`, `Reflect<T>`, `ImplBuilder<T>`, `Evidence<T>`,
`Field`) are **compiler intrinsics** — not declared in `ingots/`. A std-lib
author writes providers *against* them; they cannot be changed from std.

## Concrete landed syntax

### 1. Declaring a derive provider

```fe
// `impl <ProviderName>: Derive for <Trait>` — `Derive` is a marker, not a real trait.
impl StableEq: Derive for Eq {
    const fn derive<T>(ev: own Evidence<Eq<T>>) -> Evidence<Eq<T>>
        uses (
            reflect: Reflect<T>,            // reflection capability (required)
            builder: mut ImplBuilder<Eq<T>>, // impl emitter (required, must be `mut`)
        )
    {
        // ... provider body runs at compile time ...
        builder.finish()   // exactly once
        ev
    }
}
```

Recognized in `crates/hir/src/core/lower/provider.rs` (`DERIVE_MARKER`,
`REFLECT_KEY`, `IMPL_BUILDER_KEY`; `validate_provider`). Executed by
`crates/hir/src/core/lower/provider_executor.rs`.

### 2. Requesting a derive

```fe
derive Eq for Point using StableEq       // explicit provider by name
derive Eip712 for Payment using StableEip712
```

(The bare canonical form `derive Eq for Point` selects the unique canonical
provider when there is exactly one; ambiguity and not-found are diagnosed by
the `DeriveLower` pass — `ProviderNotFound` / `ProviderAmbiguous`, see
`DeriveErrorKind` in `crates/hir/src/analysis/diagnostics.rs`.)

### 3. `quote { … }` metaprogramming (the exciting part)

From `ingots/core_derives/src/lib.fe` (`StableEq`, struct arm) — this is real,
landed, tested code:

```fe
let mut body = quote { true }
for field in reflect.fields() {
    builder.require<Eq>(field.ty())
    // `${body}` splices the quote built so far; `self.${field}` is a member
    // hole filled by a Field handle; `other` is an OPEN NAME bound to the
    // emitted method's `other` parameter at emit time.
    body = quote(other) { ${body} && self.${field} == other.${field} }
}
let mut sig = builder.method("eq")
sig = builder.with_self(sig)
sig = builder.with_arg(sig, "other", builder.target_ty())
sig = builder.returns(sig, builder.ty<bool>())
builder.emit_method(sig, body)
```

Enum `match` folding (also landed, same file) builds an arm list with pattern
holes `${variant}(group)` and arm-splice holes:

```fe
let mut arms = quote { }
for variant in reflect.variants() {
    // ... build `cmp` for this variant's fields ...
    let inner = quote(other) { match other { ${variant}(rhs) => ${cmp}, _ => false } }
    arms = quote { ${arms}, ${variant}(lhs) => ${inner} }
}
body = quote { match self { ${arms} } }
```

The full worked end-to-end fixture (user-defined trait `Within`, provider
`BoundCheck`, quote-built field-wise comparison) is
`crates/fe/tests/fixtures/fe_test/quote_provider.fe` — the best single read.

## What ships today

| Provider | Trait | Where | Shape support |
|---|---|---|---|
| `StableEq` | `core::ops::Eq` | `ingots/core_derives/src/lib.fe:23` | struct + enum (incl. multi-variant `match`) |
| `StableDefault` | `core::default::Default` | `ingots/core_derives/src/lib.fe:113` | struct + enum (`#[default]` variant) |
| `StableEip712` | `std::eip712::Eip712` | `ingots/std/src/eip712.fe:248` | struct (EIP-712 type hash + member encode, nested-struct dedup) |

## Design note: `Derive` is a *bridge*, not the intended end state

The intrinsics above are the **landed implementation**, not the original
design's end state. `Derive` today is a compiler-known name-marker
(`DERIVE_MARKER` in `provider.rs`) — explicitly a bridge. From the obligations
docs review (`/workspace/fe-obligations-docs-review-2026-06-09.md:144`), as a
recorded design decision:

> `Derive` as compiler-known trait, **bridged until Constraint kinds exist** —
> HONORED in spirit.

The intended end state is for a trait to be a *kinded type constructor* — `Eq :
* -> Constraint` (a type → a proposition/constraint) — at which point `Derive`
becomes a real trait/constraint over such trait-constructors rather than a
hard-coded marker, and `derive Eq for Point using P` is an ordinary
constrained program, not a bespoke lowering path.

That is **not expressible today**: the kind system is
(`crates/hir/src/analysis/ty/ty_def.rs:1278`)

```rust
pub enum Kind { Star, Abs(Box<(Kind, Kind)>), Any }   // *, (k -> k), invalid
```

— there is **no `Constraint` kind**. So `Eq<T>` in `Evidence<Eq<T>>` /
`ImplBuilder<Eq<T>>` is recognized specially by the provider machinery, not as
a general `(* -> Constraint)` application. HKT exists at the *type* level
(`* -> *` via `Abs`) but constraints are not first-class kinds.

**Graduating the bridge** (architect-scoped, post-M5, HKT-family): add a
`Constraint` kind to `Kind`, give traits the kind `* -> Constraint` (n-ary as
`* -> … -> Constraint`), and re-express `Derive`/`Evidence`/`require<…>` as
ordinary kinded constructs. This is a real kind-system extension, not a std-lib
change — but it is the reason the current surface looks "special-cased": it is,
deliberately, until the kind system can carry it. Until then, **adding std
providers does not require touching this** — the bridge fully supports new
providers today.

## The std-lib gap (derivable traits with no provider)

These traits already exist in `ingots/core` / `ingots/std` and are natural
derive targets, but **no provider authors them**. Prioritized by value:

**High value / clearly structural**
- `core::clone::Clone` (`core/clone.fe:3`, `fn clone(self) -> Self`) — field-wise clone; the canonical "missing derive".
- `core::ops::Ord` (`core/ops.fe:126`) — lexicographic field/variant compare. Pairs with the shipped `Eq`.
- ABI serialization family (`core/abi.fe`): `Encode<A>` (227), `Decode<A>` (191), `AbiSize` (173), `AbiSpan` (182) — derive ABI encode/decode for structs/enums. Highest *practical* value for contract authors; today every encodable type is hand-written.

**Event / error / message surfaces**
- `core::error::ErrorVariant` (`error.fe:9`), `core::message::MsgVariant` (`message.fe:7`), `core::abi::EventAbiEncode` (`abi.fe:234`) — these are exactly the `#[event]`/`#[error]` shapes; note the **existing** guard that `#[derive]` cannot combine with `#[event]`/`#[error]` (derive error code 5), so the design question is whether event/error encoding becomes a *derive* or stays attribute-driven.
- `core::bytes::AsBytes` (`bytes.fe:3`).

**SSZ** — `std/src/evm/ssz.fe` likely wants a derive analogous to `Eip712`.

**Not derive targets** (for the record): `Copy` is a no-method marker
(`marker.fe:10`); `Functor`/`Applicative`/`Monad` (`functional.fe`) are HKT
abstractions, not structural derives; the arithmetic `ops` traits are
semantic, not mechanical.

> Note: the HKT fixtures (`hkt_bound.fe`, `functor.fe`, `result_applicative_ap.fe`)
> exercise the *kind system*, not derive. There is **no** fixture that derives
> an HKT-kinded trait — likely correct (HKT traits aren't structurally
> derivable), but worth an explicit decision.

## Limitations the architect must design around

The `quote` fragment is deliberately small (`provider_executor.rs`):
- Quote bodies allow only: literals (`true`/`false`/string), `&&`, `==`,
  `self` + declared open names, **non-generic** method calls, `match`, and
  `${…}` holes. **No integer literals, no other operators, no nested quotes,
  no generic method calls, no `if`/blocks.** (For richer expressions, use the
  `builder.*` expression API instead of quote, or build fragments in `let`s and
  splice.)
- Reflection iteration is `for`-only: `reflect.fields()` / `reflect.variants()`
  / `variant.fields()` cannot be bound to a `let`.
- Reflect exposes name + struct/enum kind + fields/variants + `variant.is_default()`;
  **no** discriminant values, attributes, or visibility.
- `#[derive]` on a target with **const** generic params is unsupported (type
  generics are fine) — derive error code 1.
- Provider runs are budgeted (100k eval steps, 10k builder commands).
- The `builder.*` API is broad where quote is narrow: it has `match_expr`/
  `with_arm`, tuple/struct/variant initializers, `trait_call`/`static_call`/
  `call`, `keccak`, `concat`/`str`/`str_ty`, `trait_const`/`trait_assoc_ty`,
  etc. New providers needing arithmetic or richer bodies use these, not quote.

Full method-by-method inventory with file:line is in the exploration notes
(Reflect API, ImplBuilder API, quote holes, 14 derive diagnostics, budgets) —
ask if you want it inlined here.

## Authoring a new provider (template)

```fe
// In ingots/core_derives/src/lib.fe (core traits) or a std module (std traits).
use core::clone::Clone

impl StableClone: Derive for Clone {
    const fn derive<T>(ev: own Evidence<Clone<T>>) -> Evidence<Clone<T>>
        uses (reflect: Reflect<T>, builder: mut ImplBuilder<Clone<T>>)
    {
        if reflect.is_struct() {
            let mut init = builder.struct_init()
            for field in reflect.fields() {
                builder.require<Clone>(field.ty())
                // self.field.clone()  via the builder expression API
                let cloned = builder.call(
                    builder.field_get(builder.self_ref(), field),
                    "clone",
                )
                init = builder.with_field(init, field, cloned)
            }
            let mut sig = builder.method("clone")
            sig = builder.with_self(sig)
            sig = builder.returns(sig, builder.self_ty())
            builder.emit_method(sig, init)
        }
        if reflect.is_enum() { /* match self { V(x) => V(x.clone()), .. } */ }
        builder.finish()
        ev
    }
}
```

(Illustrative — enum arm omitted; mirror `StableEq`'s enum `match` fold.)

## Suggested first slice for the architect

1. `StableClone` (`Clone`) — pure structural, mirrors `StableDefault`; smallest
   high-value win, exercises struct + enum.
2. `StableOrd` (`Ord`) — pairs with shipped `Eq`; enum ordering exercises the
   match fold.
3. ABI `Encode`/`Decode`/`AbiSize` derive — the practically important one;
   larger, needs the builder expression API and `AbiSize` arithmetic, so it
   also pressure-tests whether the quote/builder fragment is rich enough (a
   real "does std need more engine?" signal).

Each new provider wants a `fe_test` fixture next to the existing ones
(`crates/fe/tests/fixtures/fe_test/derived_*.fe`), ideally with a hand-written
twin oracle the way `derived_eip712.fe` does.
