# K03/K04 execution map — `Derive` bridge graduation (re-integration, not port)

**Architect-ready scope, 2026-06-14.** K02a landed the `Constraint` *kind*
(commit `804dc959a`). K03/K04 is the *graduation*: make `Derive`/`Evidence`/
`ImplBuilder`/`Reflect` real kinded constructs and `Eq<T>` a real `* -> Constraint`
application, replacing the string-marker bridge (BR0/BR1). This is
**re-integration onto fco's substrate, not a cherry-pick** — effort2 wired it
through `analysis/elab/` + `proof_forest` + its own `constraint.rs`, all of which
fco replaced.

## Status distinction (post-K02a) — keep these separate

| thing | status |
|---|---|
| `Constraint` *kind* (`Kind::Constraint`, `* -> Constraint` parses+lowers) | **LANDED** (K02a, `804dc959a`) |
| traits as `* -> Constraint` (`Eq<T>` a kinded application / `ConstraintTerm`) | **OPEN** (K03) |
| `Evidence<Eq<T>>` / `ImplBuilder<Eq<T>>` as ordinary kinded application | **OPEN** (K04) |
| `Derive` bridge graduation (marker → kinded construct) | **OPEN** (K04, gated on K07) |

Adding `Constraint` to `Kind` is **not** the same as graduating the bridge.

## effort2 commit span

| commit | what | size | layer |
|---|---|---|---|
| `7caf6343f` | Lower constraint generic args by expected kind; `PrimTy` Evidence/ImplBuilder/Reflect/TypeInfo + kinds; `TyData::ConstraintTerm` | 16 files / +442 | K03+K04 core |
| `f6affe4f3` | Add Derive constraint scaffold (`PrimTy::Derive`) | 7 files / +114 | K04 |
| `71e000cd4` | Integrate core derives across tooling | — | K04 |
| `9135f5f88`,`b096ccb11`,`286f9d14a`,`cede0607d` | provider hooks / evidence-sig validation / `uses` as capability constraints / harden CT-only types | — | **Layer 2 — NOT portable** (effort2 `analysis/elab/` + `proof_forest`; fco replaced both) |

## The two clash families

### Clash 1 — `PrimTy` builtins vs the string-marker bridge (K04)

effort2 makes `Evidence`/`ImplBuilder`/`Reflect`/`TypeInfo`/`Derive`/`Field`/… real
`PrimTy` variants (`prim_ty.rs`) with real kinds (`ty_def.rs` `HasKind`:
`Derive : (* -> Constraint) -> Constraint`, `Evidence`/`ImplBuilder : Constraint -> *`).

fco recognizes the same names as **strings**: `REFLECT_KEY = "Reflect"`,
`IMPL_BUILDER_KEY = "ImplBuilder"` (`core/lower/provider.rs:30-31`, matched at
`:171-172`); provider bodies bind them as opaque `Value::Reflect/Builder/Evidence`
(`provider_executor.rs`), and provider bodies are exempt from type/borrowck
(`is_derive_provider_fn`).

**The clash:** adding these as `PrimTy` makes name resolution resolve `Reflect<T>`
as a prim type — two recognition paths for the same names. Resolution requires
rewiring provider recognition from string-keys to the typed prims. **That is
exactly K07 / BR2 (retire string-keyed authority).** → **K07 is a hard
prerequisite for K04.**

### Clash 2 — `TyData::ConstraintTerm` vs fco's constraint model (K03)

effort2 adds `TyData::ConstraintTerm(ConstraintId)` (traits-as-types of
`Kind::Constraint`) backed by effort2's `analysis/ty/constraint.rs` (`ConstraintId` /
`ConstraintKind`). **Correction (2026-06-14): fco DOES have a
`trait_resolution/constraint.rs`** — but it is a *different* thing: salsa-tracked
`collect_constraints` / `super_trait_cycle` returning `PredicateListId`, with **no
`ConstraintId` type**. So fco models constraints as `PredicateListId` + the
obligation queue, and K03 should **project traits-as-`*->Constraint` over that
existing machinery** (architect decision, 2026-06-14). Do NOT re-introduce an
effort2-style `ConstraintId` unless `PredicateListId`/`TermId` provably cannot give
stable identity / origin links / kinded-application shape — adding one otherwise is
exactly the scaffolding the debt-negative rule forbids. `7caf6343f`'s "lower
`Eq<T>` by expected kind = Constraint" still needs re-integrating against fco lowering.

## Rollback hazards (blast radius)

- **`TyData::ConstraintTerm` (K03) breaks every exhaustive `match TyData`** in the
  compiler — `TyData` is the core type representation, matched in ty/codegen/mir/
  name-res. This is the **largest blast radius** of the whole spine. Stage it last,
  behind a clear plan for each match site.
- **New `PrimTy` variants (K04) break every exhaustive `match PrimTy`** (codegen,
  mir, abi) — smaller than TyData but workspace-wide.
- `Reflect`/`Evidence`/`ImplBuilder` becoming real types can **shadow/relocate
  name resolution** for those identifiers — provider signature validation
  (`provider.rs`) must move in lockstep.
- Provider-body type/borrowck **exemption** (`is_derive_provider_fn`) must be
  retired carefully (BR2/BR3) or generated/provider code regresses validation.

## Phasing (smallest safe first, then up)

1. **K04a (= K07/BR2 core): typed capability recognition.** Add `Evidence`/
   `ImplBuilder`/`Reflect`/`TypeInfo` as `PrimTy` variants (+ exhaustive-match
   triage) **and** rewire `provider.rs`/`provider_executor.rs` to recognize
   capabilities via the resolved prims instead of `REFLECT_KEY`/`IMPL_BUILDER_KEY`.
   Acceptance: existing provider `fe_test` fixtures (`derived_eq_default`,
   `quote_provider`, `derived_eip712`) **still pass**; `Reflect<T>` resolves to the
   builtin. This is the load-bearing graduation slice and the one that needs K07
   signoff (it deletes the string-keyed authority).
2. **K04b: kinds + `Derive`.** Give the builtins their `HasKind` kinds
   (`Derive : (* -> Constraint) -> Constraint`, etc.); add `PrimTy::Derive`
   (`f6affe4f3`). Now `Evidence<Eq<T>>` is a kinded application at the type level.
3. **K03: `ConstraintTerm`.** Introduce constraints-as-types over fco's model and
   `lower Eq<T> by expected kind = Constraint`. Largest blast radius — do last.

## Acceptance fixtures

- All existing provider `fe_test` fixtures green after K04a (no regression from the
  string→typed rewire).
- A negative fixture: a provider body with a bad capability use fails through
  **normal** checking (retires the `is_derive_provider_fn` exemption — BR3 guard).
- `Evidence<Eq<T>>` / `ImplBuilder<Eq<T>>` type-check as kinded applications (K04b).
- `Eq<T>` usable where a `Constraint`-kinded arg is expected (K03).
- The StableClone repro (`docs/dev/repro_stable_clone_blanket_ambiguity.fe`) is a
  standing PS1/PS2 pressure fixture — orthogonal but exercised by the same
  resolution path.

## Graph nodes touched

K03, K04 (status OPEN; this map is their plan), **K07** (hard prereq for K04a —
retire string-keyed provider authority, BR2), BR0/BR1 (graduate here), BR3
(provider-body validation exemption), OD3 (`Derive : * -> Constraint` = K04b),
PS0 (global trait env tier — adjacent).

## Recommendation

Do **K04a first**, *with architect/K07 signoff* (it deletes the string-keyed
authority). It is the single load-bearing slice that turns the bridge into typed
capabilities; everything else (K04b kinds, K03 ConstraintTerm) builds on it. Do
**not** start with `ConstraintTerm` (largest blast radius, depends on K04's typed
foundation). Do **not** PrimTy-ize the builtins without simultaneously rewiring
`provider.rs` (the two-recognition-paths clash).
