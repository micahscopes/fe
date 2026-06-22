# FCO Fix SURFACE: `FixImpl<G>` from a prelude `fix`, consumed by `impl .. with f`

**Date:** 2026-06-22 (design wizard pass, Micah-directed) · **Status:** DESIGN, surface only.
Refines the SURFACE of `FCO_FIX_UNIVERSAL_IMPL_DESIGN_2026-06-21.md`; that doc's GUTS are fixed
(use-once authority, the money floor, the §1.1 dial, gates EXISTENCE not USE, the cliff law). No emdashes.

A "fix" is permission to establish the ONE allowed impl of a one-of-a-kind goal (storage layout, ABI
encoding: two versions mean two parts of the program disagree about the bytes). Default-ALLOW: granted for
free for almost everything; a small hold-back set (today `goal_is_canonical`) has to ask.

## Decisions
1. **Type name: `FixImpl<G>`.** Not `FixProvider` (the provider is what hands out a fix, not the fix
   itself; would collide with `Derive<P> for P` provider vocabulary and `RootProvider`). Not bare `Fix`
   for the surface noun. `FixImpl` reads as "a fix for one impl," which is its arity: one establishment.
   `G` is the saturated CONCRETE goal, the trait applied to its type (e.g. `StorageLayout<Ledger>`), kind
   `Constraint`, same shape as `Evidence<Eq<T>>` / `ImplBuilder<Eq<T>>`. So `FixImpl : Constraint -> *`.
   The type parameter is load-bearing: it binds the permission to ONE (trait, type) so the move-checker and
   the establish-gate can match "the fix you hold" to "the impl you establish." A goal-less `Fix` could be
   replayed against any impl; `FixImpl<StorageLayout<Ledger>>` authorizes only `impl StorageLayout for Ledger`.

2. **Minting: an ordinary prelude `const fn`,** added to the prelude as a plain `pub use` (like `panic` /
   `assert`):
   ```
   pub const fn fix<G: Constraint>() -> FixImpl<G>
       uses (grant: MintRight<G>)
   ```
   The gate is NOT in the function body; it is the `uses (grant: MintRight<G>)` obligation (the same
   effect-clause providers use to demand `Reflect<T>` / `mut ImplBuilder<G>`). Whether it discharges is the
   whole policy:
   - **Unlocked goal:** `MintRight<G>` resolves against a default-allow blanket provider, ambiently. Free.
   - **Locked goal:** the blanket is EXCLUDED for the hold-back set, so there is no ambient grant; the
     obligation is satisfiable only where a `MintRight<G>` was threaded in non-ambiently (from root,
     delegated to the deployed-contract root). Calling `fix` without it is an ordinary unsatisfied-obligation
     error. The policy ("which goals are locked") is an ordinary blanket provider `GrantsMint<G>` with an
     exclusion set, i.e. `goal_is_canonical` re-expressed as a PROVISION, not a hardcoded branch.

3. **Consumption: a trailing `with f` clause on the impl,** sibling to `as Name` (same parser slot, between
   the for-type and the where-clause; `with` is already a keyword):
   ```
   impl StorageLayout for Ledger with the_fix { .. }   // the_fix: own FixImpl<StorageLayout<Ledger>>
   ```
   The establish-gate consumes it by move at this single site. Use-once falls out of affine `own`: a second
   `impl .. with the_fix` for the same goal is a use-after-move, so a second establishment cannot type-check.
   That IS the money floor "second establishment impossible by construction," with no counter and no global
   table to keep consistent.

## Default-free: what each case writes
- **Normal impl (unlocked goal):** `impl Trait for Type { .. }`, nothing extra. The compiler grants and
  consumes the fix implicitly; the user never sees `fix` or `FixImpl`. Byte-for-byte today.
- **Locked impl:** `impl Trait for Type with the_fix { .. }`, plus obtaining `the_fix = fix()` somewhere it
  holds the grant. One extra clause naming a value you were granted.

## The one piece of non-ordinary machinery (named, minimized)
`FixImpl<G>` and `MintRight<G>` are unforgeable compile-time capabilities recognized by RESOLVED IDENTITY
(`core::derive::FixImpl`, `core::derive::MintRight`): private field, no constructor, not-in-prelude-by-bare-name,
exactly like the four types already in `derive.fe`. `fix` is the single authorized SOURCE of a `FixImpl`,
the way a `derive` fn is the authorized source of `Evidence`. That is the only magic: the existing capability
pattern with two more recognized names. Value-gates-compile-time-impl is the doc's existing phase story:
`fix` is a `const fn` in the compile phase, `with f` is checked in that same phase, the move-checker sees the
`own FixImpl<G>` consumed there.

## Risks / open (for when T3 builds this)
1. **HIGHEST: ambient-capture exclusion must share ONE predicate with `goal_is_canonical`.** The blanket
   `GrantsMint<G>` must be excluded for held-back goals via the SAME source of truth as the money floor; if
   the blanket accidentally covers a canonical goal, the floor is void. `MintRight` must thread non-ambiently
   for locked goals (a `uses`-demand on a barrier-excluded frame); `snapshot_provisions` currently DROPS
   barriers, so this is new walk logic, not a wire-up. This is D4 (ambient-capture) wearing a provider hat.
2. **Where a locked `MintRight<G>` originates is the real substance (D8 / keystone-adjacent).** Surface is
   honest only once a grant flows from `RootProvider` and is delegated to the deployed-contract root.
   Prereq unchanged: `ProvisionEnv` must retain the originating `ScopeId` (GAP 1).
3. **Keep `FixImpl` an affine VALUE at the consume site (D2),** out of `as_capability` / provider-binding
   lowering, or the affine floor turns off for it. Add a double-consume fixture (consume twice, expect
   use-after-move) after wiring.
4. **Recognizer set grows by two** (`FixImpl`, `MintRight`) in `CoreDeriveItem` / `scope_is_core_derive_item`.
   Recommendation: RENAME the inert `Fix<T>` in `derive.fe` to `FixImpl` (it is currently dead-code-allowed,
   so free) and add `MintRight` as the sibling.
5. **Generic-context restriction (D6):** `with f` allowed only at monomorphic/root impl sites in v1
   (matches the cascade's direct-call-only observability).
6. **Naming bikeshed:** `fix` / `MintRight` are placeholders; "fix" may not be final. None block the guts.

## Pitch (Sean/Yoshi)
Ordinary language, not magic: a prelude `const fn` (`fix`), an ordinary capability obligation
(`uses (grant: MintRight<G>)`), one extra trailing impl clause (`with f`) in the slot `as Name` already
occupies. No new keyword, no new declaration form. Gates EXISTENCE not USE (a phase distinction). Money-floor
safety by construction (affine move, no counter). Dogfoods the provider system (the hold-back policy is a
swappable provider, not a compiler table). The common case is unchanged.

Runnable plain sketch: `crates/fe/tests/fixtures/fe_test/showcase_metaprogramming.fe` ("Coming soon" section).
