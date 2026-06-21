# Tier A constraint aliases via marker-trait + blanket-impl

> **HISTORICAL/REFERENCE (works-today technique) → `FCO_MAP.md` / SSOT `FCO_THE_SLIDE_2026-06-19.md`.** Documents the safe "Tier A" constraint-alias face that already works in Fe with zero compiler change. Kept as a dated reference; not part of the live build spine.

**Status:** Verified working in Fe today, zero compiler change. (2026-06-16)

A "constraint alias" bundles several trait obligations under one name. This
documents the **safe Tier A face** of first-class-obligations: getting an alias
with the marker-trait + blanket-impl pattern that the compiler *already*
supports, instead of the parked heavy `constraint Name<T> = C1 + C2` `ItemKind`
(a ~45-file change). The whole point of Tier A is that the alias **expands to
its concrete member obligations before the solver runs** — so the solver never
sees a variable-headed alias goal. That is exactly what supertrait elaboration
on a marker trait does.

## The pattern

```fe
trait ValueLike: Eq + Default {}                       // members = supertrait list
impl<T> ValueLike for T where T: Eq + Default {}       // blanket impl = "alias"
```

`where T: ValueLike` is one written bound that elaborates to `T: Eq` and
`T: Default` at the use site. The blanket impl makes it an *alias* rather than an
opt-in marker: authors never write `impl ValueLike for MyType`; satisfying the
members is sufficient (and the blanket impl supplies the evidence).

## Which real traits, and why

Bundled two **real prelude traits**, both derivable with plain `#[derive(...)]`:

- `core::ops::Eq` — declared `pub trait Eq<T = Self>` in `ingots/core/src/ops.fe:119`
- `core::default::Default` — declared `pub trait Default` in `ingots/core/src/default.fe:3`

They compose naturally into **"value-like"**: a type you can compare for
equality (`Eq`) and construct a canonical default of (`Default`). Both are single
type/Self-parameter traits, so they slot cleanly into a supertrait list.

> The effort has also floated `Encode`/`Decode`/`AbiSize`. Those were rejected
> for the *honest* fixture because they are not single-Self-parameter: in
> `ingots/core/src/abi.fe` they are `Encode<A: Abi>` / `Decode<A: Abi>`, with
> `AbiSize` as their supertrait. They would force an ABI type parameter into the
> alias and obscure the point. `Clone` was also rejected: `core` only provides it
> via a `derive Clone ... using StableClone` provider, not a plain derive. `Eq`
> + `Default` is the cleanest *honest* bundle. The blanket-impl mechanism is
> identical regardless of which members are chosen.

The exact blanket-impl shape used here is already load-bearing in core itself —
`impl<T> WordValue for T where T: IntWord` at `ingots/core/src/num.fe:681` and
`impl<T> Clone for T where T: Copy` at `ingots/core/src/clone.fe:7` — so this is
not a novel construct, just applied as an alias.

## Positive fixture (executes)

`crates/fe/tests/fixtures/fe_test/constraint_alias_marker_trait.fe`

The alias-bounded function uses BOTH members through the single bound — proving
the supertrait obligations elaborate into scope:

```fe
fn is_default<T>(_ x: T) -> bool where T: ValueLike {
    x.eq(T::default())          // .eq from Eq member, T::default() from Default member
}
```

`#[derive(Eq, Default)] struct Point { x: u256, y: u256 }` satisfies the whole
bundle, so `Point: ValueLike` holds by the blanket impl and the calls type-check
and RUN. Test output:

```
PASS  [0.0192s] alias_admits_satisfying_type_point
PASS  [0.0002s] alias_admits_satisfying_type_pair
PASS  [0.0002s] alias_eq_member_through_bound
PASS  [0.0002s] alias_default_member_through_bound

test result: ok. 4 passed; 0 failed
```

Through the official harness:

```
test test_fe_test__constraint_alias_marker_trait ... ok
```

## Negative fixture (diagnostic quality)

`crates/uitest/fixtures/ty_check/constraint_alias_marker_trait_missing_member.fe`

`OnlyEq` implements `Eq` but NOT `Default`. `Both` satisfies everything (so the
fixture is anti-vacuous: exactly one error, proving the alias admits a
satisfying type and rejects only the one missing a member). Calling the
alias-bounded function on `OnlyEq` produces this **exact** diagnostic (verbatim,
ANSI stripped — also the snapshot in `*.snap`):

```
error[6-0003]: trait bound is not satisfied
   ┌─ constraint_alias_marker_trait_missing_member.fe:55:5
   │
44 │ fn needs_value_like<T>(_ x: T) where T: ValueLike {
   │                                         --------- required by this bound on `needs_value_like`
   ·
55 │     needs_value_like(OnlyEq {})
   │     ^^^^^^^^^^^^^^^^
   │     │
   │     `OnlyEq` doesn't implement `ValueLike`
   │     trait bound `OnlyEq: Default` is not satisfied
```

This is the key quality result. The error:

1. Names the alias bound: `` `OnlyEq` doesn't implement `ValueLike` ``.
2. **Names the specific missing MEMBER obligation:**
   `` trait bound `OnlyEq: Default` is not satisfied `` — not an alias-internal
   or blanket-impl-internal error, and it correctly singles out `Default` while
   recognizing that `Eq` is satisfied.
3. Points at both the call site (line 55) and the bound declaration (line 44).

A symmetric probe (a type implementing `Default` but missing `Eq`) yields
`` trait bound `OnlyDefault: Eq<OnlyDefault>` is not satisfied `` — so the member
naming is real, not hard-coded to one trait.

## Does it work cleanly today? Yes. What breaks?

**Nothing breaks. The pattern works cleanly with zero compiler change.** The
positive case executes, the negative case gives a precise member-naming
diagnostic. Coherence behaves correctly too: an explicit `impl ValueLike for Foo`
*in addition to* the blanket impl is rejected with a clean
`error[5-0001]: conflicting trait implementations`. That is the *desired*
behavior — with the alias defined by a blanket impl, "satisfying the members" IS
the impl, and there is nothing to override. There was no overlap problem with
core's own blanket impls; the alias blanket impl coexists fine.

The only honest caveat is ergonomic, not a breakage: defining an alias is two
lines (the marker trait + the blanket impl) and both must list the members, so
the member set is written twice. That is the cost the heavy `constraint` ItemKind
would remove.

## Tier A marker-trait vs. the parked heavy `constraint` ItemKind

- **Marker-trait + blanket-impl (this, Tier A):** zero compiler change, works
  today, good diagnostics; alias is closed (the blanket impl forbids per-type
  overrides) and the member list is duplicated across the `trait`/`impl` lines.
  Worth it now, and for any alias whose members are fixed and few.
- **`constraint Name<T> = C1 + C2` ItemKind (parked, heavy):** ~45-file compiler
  change; gives first-class alias syntax (members written once), but must still
  expand-before-solve to preserve the no-variable-headed-goal invariant — i.e. it
  buys *syntax*, not new solving power, over this pattern.
- **When each wins:** ship Tier A now for real aliases (it is the sanctioned safe
  move and the durable test below proves it); only invest in the ItemKind once
  alias definitions are numerous/churny enough that the two-line duplication and
  closed-ness become real friction.

## Reproduce

```
cargo test -p fe --test cli_output constraint_alias_marker_trait
cargo test -p fe-uitest --test ty_check constraint_alias_marker_trait_missing_member
```
