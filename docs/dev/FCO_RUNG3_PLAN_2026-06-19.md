# Rung 3 — unified provision resolver (`ProvisionEnv`): implementation plan

> **SUPERSEDED 2026-06-19 → `FCO_THE_SLIDE_2026-06-19.md` (step 3) + `FCO_RUNG3_SCOPE_REFINEMENT`.** The 3.4 "make the solver select" framing is refuted; rungs 3.0–3.5 landed. Kept only for the located file:line map of the six resolvers.

**Date:** 2026-06-19 · **Status:** architect plan (opus), grounded in code + the two readiness spikes
(`FCO_RUNG3_READINESS_SPIKES_2026-06-19.md`). Policy = **sensible defaults / behavior-preserving
unification** (Micah, 2026-06-19; adjust later). Build as clean SSOT-respecting increments, byte-identical
gate per rung — NEVER the 370-commit mess.

## The 6 resolvers being unified (located)
1. **Global impl table / tabled solver** — `is_query_satisfiable` (`trait_resolution/mod.rs:248`), `ProofForest::solve`
   (`proof_forest.rs:155`). **This IS the target backend; everything lowers into it.**
2. **`where`-bound / param-env assumptions** — `collect_decl_constraints_with_assumptions` (`constraint.rs:326`),
   consumed via `TraitSolveCx::with_assumptions`. Already a solver input.
3. **`uses(..)` keyed effect frames** — `EffectEnv::lookup_effect_frames` (`effect_env.rs:151`), driven by
   `resolve_effect_query` (`expr.rs:1639`). The scope-chain dimension (innermost-frame reverse scan).
4. **effect-bounds→assumptions flatten** — `env.rs:315-320` (loses scope structure; rung 3 must preserve order).
5. **provider-goal / derive-cascade** — `collect_func_effect_provider_constraints` (`constraint.rs:34-140`).
   **The generalization seed** — already lowers keyed effect demands to `TraitInstId` goals.
6. **const-predicate prover** — `process_const_predicate_obligation` (`ty_check/mod.rs:1394`), CTFE discharge.
   A distinct *discharge backend*, intentionally NOT collapsed into the proof forest.

**"Generalize the lowering" = extend #5** so the scope-chain provision demands (today resolved only inside
`effect_env.rs`, never seen by the solver) also lower to canonical `TraitInstId` goals through
`is_query_satisfiable`, inheriting tabling + `MAXIMUM_TYPE_DEPTH=256` + salsa memo.

**Gap 1 (confirmed):** `TraitSolveCx` (`trait_resolution/mod.rs:109-121`) stores only `origin_ingot`, discards
`ScopeId`. Scope-chain (innermost-wins) needs lexical position → must retain `ScopeId` as **non-key carry
context** (the salsa key for `is_query_satisfiable` must stay keyed on `origin_ingot` only, else memoization
shatters).

## Rung sequence (ordered by where correctness risk concentrates)
| Rung | What | Risk | Behavior-preserving | Gate |
|---|---|---|---|---|
| **3.0** | `ProvisionEnv{scope,assumptions}` read-wrapper; migrate the hot site `process_trait_obligation` (`ty_check/mod.rs:1278`) to `env.provision_env().solve_cx(db)`; guard test | LOW | YES | full suite byte-identical |
| **3.1** | retain `ScopeId` in `TraitSolveCx` (Gap 1); **keep salsa key on `origin_ingot` only** | MED (salsa-key) | YES | byte-identical |
| **3.2** | thread `ImplementorId` provenance: `resolve_trait_method_instance` returns it; carry in `ImplEnv`; serialize in `stable_key.rs` (mirror `EffectProviderSpecialization.provenance`) | LOW–MED | YES | byte-identical + crash regressions |
| **3.3** | MIR re-resolution **determinism assertion** — re-resolved `ImplementorId` must equal typeck's; hard `LowerError` on mismatch (3 sites: `classify.rs:2280`, `synthetic.rs:1520`, `package.rs:1649`) | MED | YES | + adversarial mismatch fixture |
| **3.4** | **THE NEW CORE** — generalize provision-demand lowering to canonical goals; route transitive `with`-provider demands through `is_query_satisfiable`; rigid-key invariant; defaults: innermost-wins, coherence preserved, no fixed-tier | HIGH | YES (selections) | byte-identical + 3 Spike-2 PoC fixtures (transitive demand; A↔B cycle absorbed by tabling; growing-key hits depth limit) |
| **3.5** | collapse redundant resolvers (delete `env.rs:315-320` flatten; prune subsumed `effect_env` paths) | LOW–MED | YES | byte-identical, net-negative LOC |

## FV vs test
- **Test-verifiable (no formal):** 3.0, 3.1, 3.2, 3.5 (byte-identical full-suite + de-bless guard); 3.3 via adversarial fixtures.
- **Decidability for 3.4:** reduces to the published tabled-resolution result + finite `MAXIMUM_TYPE_DEPTH`; discharged by the 3 PoC fixtures. **Lean NOT warranted** (Spike 2). Only needed if a future rung keeps provision lookup OUTSIDE the proof forest (the Spike-2 fallback) — cut 1 takes the preferred (lower-into-solver) path, so it doesn't arise.

## Build-ready detail — rung 3.0 (start here)
1. `ty_check/env.rs`: `pub struct ProvisionEnv<'db> { scope: ScopeId<'db>, assumptions: PredicateListId<'db> }`
   with `fn solve_cx(&self, db) -> TraitSolveCx { TraitSolveCx::new(db, self.scope).with_assumptions(self.assumptions) }`.
2. `TyCheckEnv::provision_env() -> ProvisionEnv { scope: self.scope(), assumptions: self.assumptions() }`.
3. `ty_check/mod.rs:1278`: replace inline `TraitSolveCx::new(db, scope).with_assumptions(assumptions)` with
   `self.env.provision_env().solve_cx(db)`.
4. Guard test: no inline `TraitSolveCx::new` in the body checker outside `solve_cx()`.
5. Full byte-identical gate — expect zero diffs.

## Build-ready detail — rung 3.1
Add `scope: ScopeId` to `TraitSolveCx`; store both `scope` + `origin_ingot=scope.ingot(db)`. **Confirm
`is_query_satisfiable` keys only on `origin_ingot` + `Canonical<query>` (it passes `origin_ingot()` at :266),
and exclude `scope` from any derived Hash/Eq used as a salsa key.** Byte-identical gate.

## Decisions that still need a human (flagged — NOT invented; none block cut 1)
1. **`fixed`/canonical tier + `fix` override verbs** — deferred policy increment; not in cut 1.
2. **`StorageKey` / canonical-marker money-soundness** — no canonical-marker mechanism exists today. Enumerated
   trait-list vs property-derived canonical set; global vs contract-scoped. **Must be decided before any
   user-visible scoped-provision SHADOWING rung (post-3.5).** Cut 1 is byte-identical → sidesteps it.
3. **coherence-by-placement vs keeping `ConflictTraitImpl` checker** (`ty/diagnostics.rs:886`) — deferred; cut 1
   preserves coherence as-is.

## Critical files
- `crates/hir/src/analysis/ty/trait_resolution/constraint.rs` (the lowering to generalize, `:34-140`)
- `crates/hir/src/analysis/ty/trait_resolution/mod.rs` (`TraitSolveCx:109-216`, `is_query_satisfiable:248`)
- `crates/hir/src/analysis/ty/ty_check/env.rs` (`ProvisionEnv` home; flatten `:315-320`)
- `crates/mir/src/runtime/lower/classify.rs` (`resolve_runtime_call_key:2200-2344`, MIR determinism)
- `crates/hir/src/analysis/ty/trait_def.rs` (`resolve_trait_method_instance:197-235`; `ImplementorOrigin:168`)
- supporting: `ty_check/effect_env.rs`, `semantic/instance/template.rs` (`ImplEnv:67`), `mir/runtime/stable_key.rs`,
  `trait_resolution/proof_forest.rs` (termination guards — read-only, do not modify)
