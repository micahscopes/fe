# Rung 3 — scope refinement (the 3.4 design spike's pivotal finding)

**Date:** 2026-06-19 · **Status:** read-only design spike (opus) on rung 3.4, applied. Reshapes what the
byte-identical rung-3 cut delivers. Supersedes the 3.4 framing in `FCO_RUNG3_PLAN_2026-06-19.md`.

## The finding: SELECTION cannot be unified onto the solver; only VERIFICATION can

Routing the scope-chain provision **selection** through the canonical solver as a *replacement* for the
lexical effect scan is **NOT byte-identical, and cannot be — by construction.** The two resolvers answer
different questions:
- **Solver** (`is_query_satisfiable`): "does an *impl* exist for `ProviderTy: Trait`?" → returns an
  `ImplementorId`, candidates = the **global impl table** (+ assumptions), **no lexical position**, and
  **ambiguity** when >1 candidate unifies.
- **Lexical scan** (`effect_env.rs:lookup_precise:162-226`): "which *in-scope provider VALUE* satisfies this
  demand, innermost-first?" → returns a `value_expr`/binding. A `with (Console {})` provider is a runtime
  **value** in the `EffectEnv` frame stack — it is **never in the impl table**, so the solver structurally
  cannot select it.

**The codebase already implements the correct split:** `evaluate_unkeyed_trait_provider` (`expr.rs:1914-1948`)
**enumerates the provider via the lexical scan, then calls the solver only to VERIFY `ProviderTy: Trait`**
(`trait_effect_goal_satisfiability_in_scope:2331`). Enumerate-via-scope, verify-via-solver, one-directional.

So rung 3's real byte-identical content is **canonicalize the VERIFY leg** onto the SSOT solver path
(inherit tabling + `MAXIMUM_TYPE_DEPTH` + salsa memo), **without changing which provider the scope selects.**
The "traits ≡ effects ≡ provisions" unification is about the *verification + termination machinery and the
one solve-cx seam* — NOT about making the solver select scoped values.

## Revised cut-1 (byte-identical, behavior-preserving — the autonomous deliverable)
- **3.0** ProvisionEnv seam ✓ · **3.1** retain ScopeId ✓ · **3.2** carry ImplementorId ✓ · **3.3** MIR
  determinism assertion ✓ (all landed + cold-verified).
- **3.4a** — canonicalize the verify-leg: `trait_effect_goal_satisfiability_in_scope` (`expr.rs:2331-2345`)
  currently builds `TraitSolveCx::new(..)` inline (the anti-pattern 3.0 removed at the hot site) → route
  through `provision_env().solve_cx(db)` + guard test. Pure refactor, byte-identical. *This is "the unified
  path demonstrated for one demand class."*
- **3.4b** — convergence assertion (debug-only, like 3.3): when the lexical scan selects a keyed-trait
  provider, assert the solver's canonical goal yields the SAME `ImplementorId` (3.2 provenance). Measures
  whether selector and solver agree on the keyed-trait class — the empirical evidence for whether the
  deferred stage-2 is safe. Byte-identical (never fires on valid code); **if it fires anywhere → STOP +
  escalate** (selector/solver disagree on a class assumed identical).
- **3.4-(c) growing-key fixture** — standalone solver-robustness test proving `MAXIMUM_TYPE_DEPTH` fires
  (pure trait-solver fixture; constructible now).
- **3.5** — collapse redundant verify / solve-cx-construction paths (delete the `env.rs:315-320` flatten,
  prune subsumed `effect_env` verify duplication). Byte-identical, net-negative.

Cut-1 value: ONE solve-cx construction path (SSOT), the verify leg on the canonical solver (termination
robustness inherited), convergence measured, redundant paths collapsed — all byte-identical. A real
surface-area + robustness win, and it produces the measurement that gates stage-2.

## DEFERRED to "rung 3 stage 2" (NOT byte-identical; POLICY-GATED — needs Micah)
- **3.4c transitive arrow** — a `with`-seeded provider raising its OWN demand (the "second arrow" that
  doesn't exist today). Genuinely new behavior; gate fixture-by-fixture.
- **Solver-as-selector / scoped shadowing** — making the solver select among scoped provisions with
  innermost-wins shadowing. This **is** plan decision #2 (StorageKey canonical money-soundness) — it is NOT
  byte-identical and must not be smuggled into cut-1. Needs the fixed-tier / canonical-set policy call.
- **PoC (a) transitive demand + (b) cycle** — (b) is not expressible in surface Fe today (no `provides`
  keyword; `with`-providers are consumed, never re-resolved); a coinductive impl-cycle is the realistic
  substitute. These ride with 3.4c.

## Autonomous-default decisions taken (per "sensible defaults", revisit on Micah's return)
1. **3.4 stays VERIFY-ONLY** — the byte-identical, conservative default. Selection stays scope-based.
2. **Stage-2 (transitive + shadowing) deferred** until the money-soundness policy call (decision #2).
3. **PoC (b)** deferred with stage-2 (substitute TBD).

## Landmine + rigid-key (carried)
- `scope` is excluded from `TraitSolveCx`'s salsa key (3.1). Cut-1 confirmed clean: enumeration reads the
  (non-keyed) `EffectEnv` frame stack and folds the provider to a concrete `TyId` BEFORE the tracked query;
  never read `scope` inside an `is_query_satisfiable`-keyed body. Stage-2 must keep this.
- Rigid-key / concrete-before-solver (`effect_env.rs:108-118,167-171`): preserve a `debug_assert!` at the
  lowering boundary so no provision key reaches `CanonicalGoalQuery::new` with a non-rigid carrier / inference
  var / layout hole.
