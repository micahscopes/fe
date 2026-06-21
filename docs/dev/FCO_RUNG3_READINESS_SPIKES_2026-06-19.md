# Rung 3 (unified provision resolver) — readiness spikes

> **SUPERSEDED (rung 3 built) → `FCO_THE_SLIDE_2026-06-19.md` (SSOT) / `FCO_RUNG3_SCOPE_REFINEMENT_2026-06-19.md` / `FCO_MAP.md`.** Both de-risking spikes resolved into the slide's "stage, don't fuse" + scope-refinement steers; rung 3 landed. Historical de-risking record. SSOT: `FCO_THE_SLIDE_2026-06-19.md`.

**Date:** 2026-06-19 · **Status:** two de-risking spikes complete (opus, read-only). Both **YELLOW**
(achievable with bounded, well-localized changes; neither RED / no fundamental blocker). They
**converge on one architectural steer.** This gates building rung 3 alongside Micah's coherence gut-calls.

## The headline: build rung 3 *on top of* the tabled trait solver, not as a parallel resolver

Both spikes independently point at the same design: the unified `ProvisionEnv` should **lower provision
demands to canonical trait-solver goals** and let the existing proof-forest do the resolving — rather than
growing a second scope-chain resolution engine. That single decision reuses the proven *termination* AND
*provenance* machinery and concentrates the new work (and the remaining risk) in three localized places.

## Spike 1 — witness-provenance-through-mono (VERDICT: YELLOW)

- Fe has **no single mono pass**; instantiation is lazy at 3 layers (semantic `SemanticInstance`, MIR
  `RuntimeInstance`, codegen symbol). Trait evidence flows via **re-resolution at instantiation time**
  (`const_ref.rs:81-100`, `classify.rs:2280`) — the selected impl becomes `BodyOwner::Func(impl_func)`.
- The witness threaded through mono is only the **`TraitInstId`** (the *demand*), NOT the `ImplementorId` /
  `ImplementorOrigin` (provenance is dropped at `resolve_trait_method_instance`, `trait_def.rs:197-235`,
  which returns `(Func, args)`). It works today because impl identity survives as the owner-func (hashed
  into the codegen symbol, `stable_key.rs:83-99`), effect-provider provenance IS explicit
  (`EffectProviderSpecialization.provenance` → `stable_key.rs:276-290`), and re-resolution is deterministic
  under coherence.
- **For rung 3:** to make *evidence-with-provenance survive* mono (vs. be recomputed 3×), add an evidence
  field to `ImplEnv` / `SemanticInstanceKey` and serialize it — localized/additive. **The real risk is
  re-resolution DETERMINISM at the MIR boundary** (`classify.rs:2200-2319`, assumption rebuild `:2329-2344`):
  as "provision" broadens (effects/caps/where/cascade unified), MIR-time assumption-set reconstruction is the
  single point a *wrong* provision could be silently selected. Today mitigated by hard-fail-on-ambiguity
  (`Selection::Ambiguous → None → LowerError`).
- **Narrow PoC:** extend `resolve_trait_method_instance` to also return `ImplementorId`; carry it in
  `ImplEnv`; thread to `stable_key`; assert MIR re-resolution picks the SAME impl as typeck. Existing
  stressors: `crash_regressions/monomorphize_unresolved_trait_method.fe` + `..._failed_instantiate_*`.

## Spike 2 — effect_env-fold decidability (VERDICT: YELLOW)

- `effect_env` today is a **lexical stack** (`frames: Vec<EffectFrame>`, `effect_env.rs:64`), a single bounded
  reverse scan (`lookup_precise` `:162-226`), **no recursion, no fixpoint**. Providers are **consumed, never
  re-resolved** — so demand→provision→demand does not exist today; termination is trivial (finite AST walk).
- The trait solver is a **tabled proof forest**: memoization (`query_to_node`, `proof_forest.rs:242-246`),
  `MAXIMUM_TYPE_DEPTH = 256` (`:34`, the coinductive-cycle guard), solution cap 2, salsa memoization on the
  canonical query (`mod.rs:248`), super-trait occurs-check (`constraint.rs:174-189`).
- **Key structural fact: the bridge is already one-directional and safe.** Effects call INTO the solver
  (leaf consumer); the solver never calls back. A SUBSET of effect demands (keyed) are **already lowered to
  solver goals** via `collect_func_effect_provider_constraints` (`constraint.rs:34-140`). Rung 3's fold
  **generalizes this existing lowering**, not a new mutual recursion.
- **The risk:** rung 3's "cascade rides the dynamic extent of `with`" makes a `with`-seeded provider able to
  raise its OWN demands transitively — the second arrow that doesn't exist today. The lexical `frames` scan
  has **no cycle guard** (relies on never re-entering), so a unified loop could hit effect→provision→effect
  cycles or growing-key unbounded chains. The solver's guards cover this **only** where demands flow through
  `is_query_satisfiable`; the raw scope-chain lookup dimension is **not** in the proof forest.
- **The bound that restores decidability (named):** *preferred* — lower ALL provision demands to canonical
  trait-solver goals (extend `collect_func_effect_provider_constraints` to all provisions) so the existing
  tabling + `MAXIMUM_TYPE_DEPTH` + salsa cover cycles for free; *fallback* — if provision lookup stays in the
  `effect_env` layer, add a provision-resolution depth limit (mirror `MAXIMUM_TYPE_DEPTH`) on transitive
  provider→demand hops AND enforce the existing rigid-key / "concrete-before-the-solver" invariant
  (`effect_env.rs:108-118,167-171`) on every provision key entering the transitive loop.
- **Narrow PoC:** route one transitive provision demand (a `with`-seeded provider with its own effect
  requirement) through `is_query_satisfiable`; confirm tabling absorbs a deliberate `A provides B, B provides
  A` cycle; one adversarial growing-key test to confirm `MAXIMUM_TYPE_DEPTH` fires on the effect path. Formal
  /Lean NOT warranted yet (reduces to the published tabled-resolution result + a finite depth bound).

## Where the correctness risk concentrates (for sequencing the build)

1. **The lowering of all provision demands → canonical solver goals** (generalize
   `collect_func_effect_provider_constraints`). This is the new core; it's what buys termination + provenance
   reuse. Highest design care.
2. **Re-resolution determinism at the MIR boundary** — the unified resolver must guarantee MIR-time
   re-resolution selects the same provision as typeck, especially as "provision" broadens. (Spike 1.)
3. **Provenance threading** (`ImplementorId` into `ImplEnv`/`SemanticInstanceKey` + `stable_key`). Mechanical
   but must land for traceability. (Spike 1.)

The deletions/wiring (collapsing the ~6 resolvers) are the cheap verifiable bulk; the proof effort goes into
#1–#2. Reuse the tabled solver = don't re-prove termination.

## Still gated on (not a spike — Micah's calls)
The `fixed`/canonical tier policy, scoped-provision priority, the money-trait rules, coherence-as-placement.
These are gut-calls, not measurements. The spikes say the *mechanism* is buildable; the *policy* is yours.
