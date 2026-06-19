# The Slide — provision unification by burndown (binding plan)

**Date:** 2026-06-19 · **Status:** BINDING. Supersedes the "mountain/layers" framing of
`FCO_DERIVERS_AS_PROVISIONS_GAP` and the feature-framing of the stage-2 plan. Anchored on
`PROVISION_SCOPING_SYNTHESIS_2026-06-17.md §4` (one resolver, ladder of tiers, impls = companion/outermost
tier, innermost-wins, canonical = non-overridable coherence tier).

## The thesis
The unification is **not a feature to build — it's what's LEFT when the magic is deleted.** Every step is a
DELETION that consolidates; the unified resolver + derivers-as-provisions *accrete* as friction is removed.
Downhill, not uphill. Drop "plan-the-mountain / decompose-by-FV-feature"; resume the rhythm that worked
(cut-1, the TD5 executor de-magic, the ABI providerization): **pick the next seam, delete it byte-identical +
debt-negative, let it accrete.**

## The brakes to remove (each a deletion)
- the `Derive` **string marker** (`provider.rs:31` `DERIVE_MARKER`) — vs a real trait;
- the **bespoke provider executor** (`provider_executor.rs`, hardcoded caps `:941-948`) — vs ordinary CTFE;
- the **6 separate resolvers** (global tabled solver, keyed effect env, effect bounds, CapabilityEnv, scoped
  generated-impl overlay, const-predicate prover) — vs one `ProvisionEnv` walk;
- the **global coherence checker** (`ty/diagnostics.rs` `ConflictTraitImpl`) — vs placement.

## The slide (top → bottom)
**CORRECTION (measured 2026-06-19):** the byte-identical runway is steps 1–2. Making `ProvisionEnv` the actual
first-match *walk* (impl table as a tier, innermost-wins over the proof-forest's collect-all) is **NOT
byte-identical — it IS scoped shadowing**. So the walk + canonical tier + `Fix` + checker-demotion are ONE
new-behavior push (step 3), FV-gated — not a free consolidation.

1. **Construction SSOT (byte-identical) — IN PROGRESS.** Fold every hand-rolled `TraitSolveCx` construction onto
   the one `ProvisionEnv` (cut-1 + seam-1 done; ~25 clean sites remain in `diagnosable.rs`, `core/semantic`,
   `analysis/ty/*`). Unifies *how* the solver context is built so the eventual walk is a one-place flip. Does
   NOT yet make it a walk. *Deletes scattered constructions.*
2. **De-magic the deriver (byte-identical).** Delete `DERIVE_MARKER`; graduate `Derive` to a real solver-trait
   (so `D: Derive<Eq>` resolves); fold the executor into ordinary CTFE (TD5 series); capabilities bound through
   the provision env, not hardcoded. *Deletes magic + eats CapabilityEnv (#4) and the generated-impl overlay
   (#5)* — and dissolves the "stratum-crossing" (no boundary once the executor is ordinary evaluation).
   Byte-identical generated impls.
3. **THE PUSH — one first-match walk + `fixed`/`Fix` gating (NEW behavior; PROVEN).** Make `ProvisionEnv` the
   actual scope-chain walk: impl table = companion/outermost tier, effect frames = inner tiers, innermost-wins
   first-match — **this IS scoped shadowing** (the measured non-byte-identical step), and it collapses systems
   #1 (solver) and #2 (effect env) into the one walk. Add the `fixed`/canonical non-overridable tier; the
   `Fix<T>` capability (A1 narrow `Evidence`-clone; `Fix<*>`-as-kind master — ∀ over Constraint, instantiate-
   only, never solved) gating overrides via the `fix` verb; and **demote `ConflictTraitImpl` into the canonical
   tier — delete the global checker** (coherence = placement). **Money-soundness ⇒ provable** — FV obligations
   per `FCO_FIX_CAPABILITY_PACKET` (unforgeability, non-ambient propagation, attenuation lattice, gate
   soundness, MIR re-resolution determinism). The one human gut-call (decision C: the canonical/`fixed` set) is
   surfaced to Micah before landing.
4. **Runout (emerges, nothing built):** a deriver provided as a scoped provision is run by the walk to satisfy
   `where Eq<T>`. Derivers-as-provisions falls out. **MECHANISM locked; the SURFACE syntax for "provide a
   deriver in scope" is OPEN, chosen at the runout** (not the illustrative `with (Derive<Eq> = …)` — a unified
   shim-free resolver can desugar several surfaces to the same provision). System #6 (const-predicate) rides
   along as the CTFE backend.

Steps 1–2 are surface-area-NEGATIVE (byte-identical); step 3 is the one push (new behavior + the only new
surface, which also DELETES the checker + collapses #1/#2); step 4 is free. A slide with a shove near the
bottom — not a mountain.

## The boundary that keeps every step sound (the cliff law)
Pin the **head concrete before the solver**; never let a `* -> Constraint` *variable* reach it. `Eq<T>`,
`Derive<Eq>`, `Fix<Consensus>`, `Evidence<Eq<T>>` = concrete head, subject roams = ✅. `P<T>` / `Derive<P>` /
`Fix<P>` with `P` free = the cliff = 🚫. Quantify-over-Constraint only at the **kind** level (∀, instantiate-
only, e.g. `Fix<*>`), never at the **solver** level (∃/search).

## Operating rhythm (autonomous, via agents)
- One seam per increment, own worktree agent, parent **cold-verifies** (FF + own gate + inspect the change),
  **byte-identical** for 1–3, **+FV** for the money parts of 4. STOP-on-wall; phone-a-friend (helper agent /
  fable logs), not Micah, unless a genuine money-policy gut-call (decision C: the canonical/`fixed` set).
- Re-verify "can't / too big" verdicts by *measuring what can be deleted next* ([[reverify-inherited-blockers]];
  surface-area is the metric, [[burndown-value-is-surface-area]]; clean SSOT increments, [[tracing-must-be-a-joy-to-maintain]]).
