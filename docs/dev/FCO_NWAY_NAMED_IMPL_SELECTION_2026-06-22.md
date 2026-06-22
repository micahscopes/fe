# FCO T-Nway — N-way scoped selection via named impls (`impl … for … as Name`)

**Date:** 2026-06-22 · **Status:** DESIGN (Micah-directed). Task #84. Builds on the cascade
(C3b scoped selection, M1/M2/M3) + the keystone. Makes the last 2 dream fixtures
(`cascade_nested_shadowing`, `cascade_greeting_dialects`) pass.

## The decision (Micah, 2026-06-22)
- Surface is **`impl Trait for Type as Name`** — a trailing optional `as Name` on an ordinary impl.
  NOT a `provision` keyword; NOT the deleted colon-overload `impl Name: Trait for Type`.
- **Elidable / auto-named.** `as Name` is OPTIONAL. The distinguishing identity is ALWAYS present
  (it is the impl's existing `ImplementorId` — N coexisting impls of one `(Trait,Type)` already have
  distinct ImplementorIds). `as Name` is a thin USER-FACING ALIAS over that identity, so explicit
  naming can be "turned off easily later" without disturbing the machinery. Un-`as`'d impls get an
  anonymous per-module auto **display** name (for diagnostics only — not `with`-selectable by name).
- **Diagnostics are a first-class deliverable**, not an afterthought.

## Why this reconciles with `no-impl-naming` (2026-06-21)
The earlier principle "users must NOT name impls" was about the 2-slot default/override case, where
`with (<T as Trait>)` suffices (exactly one override, machinery distinguishes by provenance). N-way
(>1 override) inherently needs the user to signal *which*. The reconciliation: users **needn't** name
impls (auto-identity + 2-slot path still work with no names), but **may** (`as Name`) when they want
N-way selection. Naming is opt-in sugar over an identity that already exists.

## Model
- `ImplementorId` is the identity (exists today; keystone-stable for generated, positional-stable for
  hand-written). The cascade already records a per-goal `selected_implementor: ImplementorId`
  (`ImplEnv`, consumed at MIR C1 `classify.rs:2305`). N-way changes only *how the user picks which
  ImplementorId* — by name — not the recording/consumption rail.
- `as Name` binds `Name` in the enclosing module scope as a reference to that impl item (a nameable
  "named impl"). `with (Name) { .. }` resolves `Name` → its `ImplementorId` → records the selection
  (extends `provisional_scoped_selection`, alongside the existing `with (<T as Trait>)` goal form).
- Elision: no `as` → no user-facing name → not `with`-selectable by name (still gets default/override
  treatment, still has an ImplementorId, still gets an auto display-name in diagnostics).

## Increments (each cold-gated, byte-identical where noted)
1. **Parse + HIR `as Name` (foundational, byte-identical when absent).** Optional `as Name` after the
   for-type, before the body, on `ImplTrait` (and `Impl`). Store `Option<IdentId>` on the item. No
   behavior change when elided → existing fixtures byte-identical.
2. **Name binding.** `as Name` introduces `Name` into the module scope graph as a named-impl reference
   (resolvable by ordinary path resolution). Auto display-name for the elided case (diagnostics only).
3. **`with (Name)` selection (the guts).** Extend `provisional_scoped_selection` to accept a path that
   resolves to a named impl → its `ImplementorId` → `ProvidedEffect { selected_implementor }`.
   Generalize `sole_scoped_selection_implementor`: when the `with` head names a specific impl, select
   THAT ImplementorId (N>1 no longer returns `None` — the name disambiguates). Keep the goal-form
   (`<T as Trait>`) 2-slot path unchanged.
4. **Diagnostics (first-class).** `with (Unknown)` → "no impl named `Unknown` …" + suggest available
   named impls for goals in use; `with (Name)` where `Name` is not an impl → precise kind error;
   selected impl whose `(Trait,Type)` doesn't match the call → "impl `Name` does not apply here";
   collisions / duplicate `as Name` in a module → error. Lean on existing diagnostic infra.
5. **Fixtures.** Rewrite `cascade_nested_shadowing` / `cascade_greeting_dialects` to
   `impl Trait for Type as Name` + `with (Name)`; install to `fe_test`; add negative-diagnostic
   fixtures (unknown name, non-impl name, non-applying impl). Update `dream_fixtures/MANIFEST.md`.

## Non-goals / boundaries
- Does NOT touch the abstract-head cliff (`P : * -> Constraint`) — N-way *selection* is fully concrete.
- Canonical/money-floor goals stay exactly-one (selection never applies; `goal_is_canonical` floor holds).
- Does NOT re-introduce the deleted colon-overload grammar (#87) — `as Name` is the new spelling.
