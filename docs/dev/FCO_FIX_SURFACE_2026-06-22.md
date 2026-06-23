# FCO anchor surface: `Anchor<G>` from a prelude `anchor()`, consumed by `impl .. with a`

**Date:** 2026-06-22 (design wizard pass, Micah-directed) · **Status:** DESIGN, surface only.
Refines the SURFACE of `FCO_FIX_UNIVERSAL_IMPL_DESIGN_2026-06-21.md`; that doc's GUTS are fixed (use-once
authority, the money floor, the dial, gates EXISTENCE not USE, the cliff law). No emdashes.

(Filename keeps the "FIX" program lineage. The chosen USER-FACING word is **anchor**, as in "anchor in
place / fixed point," NOT "repair." It supersedes the earlier `FixImpl` draft naming.)

A goal is "one-of-a-kind" when there must be exactly one impl, ever (storage layout, ABI encoding: two
versions mean two parts of the program disagree about the bytes). One impl gets ANCHORED in place and no
other can be written. Default-ALLOW: granted for free for almost everything; a small hold-back set (today
`goal_is_canonical`) has to ask.

## Decisions
1. **Type name: `Anchor<G>`** (chosen 2026-06-22; ranked over fix/lock/seal/admit/claim). "anchor" means
   hold-in-place, has no PL/proof collision (unlike `lock` = mutex, `admit` = Coq abandon-proof, `seal` =
   sealed traits, `fix` = repair and the Y-combinator), and reads clean as type, verb, and error.
   `G` is the saturated CONCRETE goal, the trait applied to its type (e.g. `StorageLayout<Ledger>`), kind
   `Constraint`, same shape as `Evidence<Eq<T>>` / `ImplBuilder<Eq<T>>`. So `Anchor : Constraint -> *`.
   The type parameter is load-bearing: it binds the permission to ONE (trait, type) so the move-checker and
   the establish-gate can match "the anchor you hold" to "the impl you establish." A goal-less anchor could
   be replayed against any impl; `Anchor<StorageLayout<Ledger>>` authorizes only `impl StorageLayout for Ledger`.

2. **Minting: an ordinary prelude `const fn`,** added to the prelude as a plain `pub use` (like `panic` /
   `assert`):
   ```
   pub const fn anchor<G: Constraint>() -> Anchor<G>
       uses (grant: AdmitAnchor<G>)
   ```
   The gate is NOT in the body; it is the `uses (grant: AdmitAnchor<G>)` obligation (the same effect-clause
   providers use to demand `Reflect<T>` / `mut ImplBuilder<G>`). Whether it discharges is the whole policy:
   - **Ordinary goal:** `AdmitAnchor<G>` resolves against a default-allow blanket provider, ambiently. Free.
   - **One-of-a-kind goal:** the blanket is EXCLUDED for the hold-back set, so there is no ambient grant; the
     obligation is satisfiable only where an `AdmitAnchor<G>` was threaded in non-ambiently (from root,
     delegated to the deployed-contract root). Calling `anchor()` without it is an ordinary
     unsatisfied-obligation error. The policy ("which goals are one-of-a-kind") is an ordinary blanket
     provider `GrantsAnchor<G>` with an exclusion set: `goal_is_canonical` re-expressed as a PROVISION, not a
     hardcoded branch.

3. **Consumption: a trailing `with a` clause on the impl,** sibling to `as Name` (same parser slot, between
   the for-type and the where-clause; `with` is already a keyword):
   ```
   impl StorageLayout for Ledger with a { .. }   // a: own Anchor<StorageLayout<Ledger>>
   ```
   The establish-gate consumes it by move at this single site. Use-once falls out of affine `own`: a second
   `impl .. with a` for the same goal is a use-after-move, so a second establishment cannot type-check. That
   IS the money floor "second establishment impossible by construction," with no counter and no global table.

## Default-free: what each case writes
- **Normal impl (ordinary goal):** `impl Trait for Type { .. }`, nothing extra. The compiler grants and
  consumes the anchor implicitly; the user never sees `anchor()` or `Anchor`. Byte-for-byte today.
- **One-of-a-kind impl:** `impl Trait for Type with a { .. }`, plus obtaining `a = anchor()` somewhere it
  holds the grant. One extra clause naming a value you were granted.
- **The guardrail (emulated error):** writing `impl StorageLayout for Ledger { .. }` with no anchor reports
  "StorageLayout for Ledger may have only one impl, and this one is not anchored. mint an anchor with
  `anchor()` where you hold the authority, then add `with a`."

## The one piece of non-ordinary machinery (named, minimized)
`Anchor<G>` and `AdmitAnchor<G>` are unforgeable compile-time capabilities recognized by RESOLVED IDENTITY
(`core::derive::Anchor`, `core::derive::AdmitAnchor`): private field, no constructor, not-in-prelude-by-bare-
name, exactly like the four types already in `derive.fe`. `anchor()` is the single authorized SOURCE of an
`Anchor`, the way a `derive` fn is the authorized source of `Evidence`. That is the only magic: the existing
capability pattern with two more recognized names. Value-gates-compile-time-impl is the existing phase story:
`anchor()` is a `const fn` in the compile phase, `with a` is checked in that same phase, the move-checker sees
the `own Anchor<G>` consumed there.

## Risks / open (for when T3 builds this)
1. **HIGHEST: the ambient-capture exclusion must share ONE predicate with `goal_is_canonical`.** The blanket
   `GrantsAnchor<G>` must be excluded for held-back goals via the SAME source of truth as the money floor; if
   the blanket accidentally covers a canonical goal, the floor is void. `AdmitAnchor` must thread
   non-ambiently for one-of-a-kind goals (a `uses`-demand on a barrier-excluded frame); `snapshot_provisions`
   currently DROPS barriers, so this is new walk logic, not a wire-up. This is D4 wearing a provider hat.
2. **Where a one-of-a-kind `AdmitAnchor<G>` originates is the real substance (D8 / keystone-adjacent).**
   Surface is honest only once a grant flows from `RootProvider` and is delegated to the deployed-contract
   root. Prereq unchanged: `ProvisionEnv` must retain the originating `ScopeId` (GAP 1).
3. **Keep `Anchor` an affine VALUE at the consume site (D2),** out of `as_capability` / provider-binding
   lowering, or the affine floor turns off for it. Add a double-consume fixture after wiring.
4. **Recognizer set grows by two** (`Anchor`, `AdmitAnchor`) in `CoreDeriveItem` / `scope_is_core_derive_item`.
   Recommendation: RENAME the inert `Fix<T>` in `derive.fe` to `Anchor` (it is dead-code-allowed, so free)
   and add `AdmitAnchor` as the sibling.
5. **Generic-context restriction (D6):** `with a` allowed only at monomorphic/root impl sites in v1.
6. **Naming:** the user-facing word "anchor" is chosen; `AdmitAnchor` / `GrantsAnchor` (the authority layer)
   are working names and can still move. None block the guts.

## Pitch (Sean/Yoshi)
Ordinary language, not magic: a prelude `const fn` (`anchor`), an ordinary capability obligation
(`uses (grant: AdmitAnchor<G>)`), one extra trailing impl clause (`with a`) in the slot `as Name` already
occupies. No new keyword, no new declaration form. Gates EXISTENCE not USE (a phase distinction). Money-floor
safety by construction (affine move, no counter). Dogfoods the provider system (the hold-back policy is a
swappable provider, not a compiler table). The common case is unchanged.

Runnable plain sketch: `crates/fe/tests/fixtures/fe_test/showcase_metaprogramming.fe` ("Coming soon" section).

## Build plan (spike-corrected 2026-06-22, read-only feasibility pass)
The Risks section above was written from the design's guesses. A read-only spike measured them against the
code and downgraded all three (per `reverify-inherited-blockers`):
- **GAP-1 (ProvisionEnv scope): already done.** `ProvisionEnv` (`trait_resolution/mod.rs:172`) carries `scope`
  end-to-end (~40 `for_scope` sites pass a real scope); it is just not yet READ by resolution (the solve keys
  on `origin_ingot`; `scope` is deliberately out of the salsa key). Real remaining work = a rung-3.4 scope read
  OUTSIDE the tracked solve, only for the held-back mint referent (inc5). MEDIUM -> LOW.
- **Shared SSOT for the exclusion: trivial.** `goal_is_canonical` (`trait_def.rs:893`) has ONE production
  caller (the establish-gate `mod.rs:3992`) and no second list. `GrantsAnchor` exclusion = `!goal_is_canonical`.
  An inert recognition seam is already planted next to the floor (`mod.rs:4011`, `_fix_capability_present`).
  HIGHEST -> LOW.
- **Non-ambient threading: reuse, not invent.** The barrier-dropping `snapshot_provisions`
  (`effect_env.rs:269`) feeds only the always-empty evidence-snapshot seam. The LIVE capability walk
  `resolve_effect_query` -> `lookup_precise` (`effect_env.rs:162-226`) already respects barriers
  (`BlockedByBarrier`). `AdmitAnchor<G>` rides that. Work = a seeding policy + root mint, not new walk logic.
- **Affine floor confirmed:** `as_capability` (`ty_def.rs:289`) matches only Borrow/View ctors, so `Anchor<G>`
  (a plain struct) stays move-tracked at the `with a` consume site (`lowered_implementor`, `mod.rs:3948`).
  Keep `Anchor` (ordinary affine value) distinct from `AdmitAnchor` (the threaded capability).

### Increment ordering (delete the coherence checker LAST)
1. **inc1 (byte-identical):** rename the inert `Fix<T>` -> `Anchor<T>` in `derive.fe`; add inert sibling
   `AdmitAnchor<G>`; rename the recognizer surface (`CoreDeriveItem::Fix`->`Anchor`, `FIX_TY`->`ANCHOR_TY`,
   `fix_capability_in_scope`->`anchor_capability_in_scope`) and ADD `CoreDeriveItem::AdmitAnchor`; rename the
   inert seam consumer `mod.rs:4011`. Everything stays inert -> all fixtures byte-identical.
2. **inc2:** parser + HIR `with a` trailing impl clause (recognize-only, additive; mirrors the `as Name` inc1).
3. **inc3:** prelude `anchor()` + default-allow `GrantsAnchor` blanket for `!goal_is_canonical`; rides the live
   barrier-respecting effect-query walk. (Verify a `const fn` with a `uses` clause type-checks.)
4. **inc4:** flip the floor at `mod.rs:4022` (`if canonical` -> consult anchor-consumption) + a new
   "canonical, unanchored" diagnostic + the double-consume use-after-move fixture.
5. **inc5:** root mint of `AdmitAnchor<G>` (`RootProvider`, `mod.rs:445/1668`) + contract-root delegation; the
   rung-3.4 scope read lands here. The one genuinely open POLICY question (who-may-mint, where) is confirmed
   with Micah before this inc.
6. **inc6 (LAST):** delete the global coherence checker, once inc4's anchor floor demonstrably holds canonical
   scarcity AND `does_impl_trait_conflict` still rejects non-default overlap. Keep locality/orphan checks.

Open to verify at build time: `EffectFamily` keying for `AdmitAnchor<G>` (inc3); salsa-key safety of reading
anchor-consumption at the tracked establish-gate (inc4; the `with a` value is a property of the HIR node, not
scope, so expected safe).

### inc3 PROBE RESULTS (measured 2026-06-22, the spike's open items resolved)
- `const fn` + `uses` IS rejected (`error[8-0055]` from `const_check.rs:27`). FIX: declare `anchor` as an
  `extern const fn` (bodyless), the precedent `panic`/`size_of` use; `check_const_fn_body` is gated on
  `!is_extern` (`ty_check/mod.rs:385`), so extern skips it. Extern is also the right shape because `Anchor`
  is unforgeable (no Fe body could construct it); the compiler binds the value, like `Evidence`.
- Fe has NO turbofish and NO explicit generic call args. `anchor<G>()` / `anchor::<G>()` do NOT parse
  (`mk<Eq<Foo>>()` lexes as comparison operators and ICEd at lowering; `::<>` is a parse error). So `anchor()`
  MUST use INFERENCE: G is inferred from the binding/consume context (`let a: Anchor<Eq<Foo>> = anchor()`,
  or the `with a` site). Do NOT write the turbofish (this was a scrapped agent's false "blocker").
- The full shape WORKS, verified end-to-end:
  `extern const fn mk<G: Constraint>() -> Anchor<G> uses (grant: AdmitAnchor<G>)` plus
  `let a: Anchor<Eq<Foo>> = mk()` type-checks the call, and the grant obligation IS enforced at the inferred
  call site: `error[8-0036]: missing effect AdmitAnchor<G> required by mk`. So the gate is real; inc3's only
  remaining work is the default-allow SEEDING (ambient `AdmitAnchor<G>` for `!goal_is_canonical`).
