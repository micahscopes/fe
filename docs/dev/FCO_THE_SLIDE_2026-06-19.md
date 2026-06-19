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

## The slide (top → bottom; byte-identical until the one short push)
1. **Consolidate 6 → one walk.** Fold each bespoke resolver into the `ProvisionEnv` walk (cut-1 = step 1:
   one solve-cx construction + verify-leg + scope retained + provenance + determinism). Keep folding,
   byte-identical (the walk picks the same provision because nothing shadows across tiers *yet*). *Deletes seams.*
2. **De-magic the deriver.** Delete `DERIVE_MARKER`; graduate `Derive` to a real solver-trait (so
   `D: Derive<Eq>` resolves); fold the executor into ordinary CTFE (continue the TD5 series); capabilities
   bound through the provision env, not hardcoded. *Deletes magic* — and dissolves the "stratum-crossing"
   the gap analysis feared (no boundary once the executor is ordinary evaluation). Byte-identical generated impls.
3. **Demote coherence.** Once the canonical tier exists, `ConflictTraitImpl` becomes placement — delete the
   checker (keep uniqueness only for the canonical tier). *Deletes a checker.* Byte-identical for existing code.
4. **The one genuine push — `fixed`/`Fix` gating (BUILT IN, not deferred).** `Fix<T>` = a single `Evidence`
   clone (A1 narrow; `Fix<*>`-as-kind is the sanctioned future master — ∀ over Constraint, instantiate-only,
   never solved); `fixed`/canonical = the non-overridable tier; `fix` verb = capability-gated override;
   scoped shadowing = the one-walk "stop forcing outermost-only." **Money-soundness ⇒ provable, not green
   tests** — the FV obligations from `FCO_FIX_CAPABILITY_PACKET` (unforgeability, non-ambient propagation,
   attenuation lattice, gate soundness, MIR re-resolution determinism for a scoped override). Smallest, last,
   gated on 1–3.
5. **Runout (emerges, nothing built):** `with (Derive<Eq> = StableEq) { cmp(p, q) }` — the resolver runs the
   provided deriver to satisfy `where Eq<T>`. Derivers-as-provisions falls out.

Steps 1–3 are surface-area-NEGATIVE; 4 is one short push (small new surface that *also* deletes the checker
in 3); 5 is free. A slide with a shove at the very end — not a mountain.

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
