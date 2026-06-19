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

## Governing principle: get the GUTS right, don't get hung up on sugar (2026-06-19, Micah, binding)
The win is the **guts** — ONE streamlined resolver, **zero** ad-hoc competing systems, **zero** shims-into-sugars /
intrinsics / bolt-ons. Surface syntax and policy *canon* are **deferred-tunable, NOT blockers**: pick a sensible
default now and proceed; never ask Micah or block on them. Two binding consequences:
- **Decision C (the canonical/`fixed` SET) is NOT a gate.** Default = narrow (nothing `fixed` by default; opt-in
  marker). Land the mechanism against that default; the set is tunable later. *Default the POLICY, never the PROOF*
  — the soundness obligations (unforgeable `Fix`, the gate actually blocks, FV per the Fix packet) stay mandatory.
- **Surface syntax is NOT a gate.** The `fix` verb / `fixed` marker / "provide a deriver in scope" spellings are
  illustrative; pick the plainest and move. A correct shim-free resolver desugars many surfaces onto the one
  provision — getting the guts right *buys* the tuning flexibility. This is metaprogramming; that is the point.

Corollary: syntax polish has been a distraction from the guts. Rank work by *core streamlining / deletions*, not
surface nicety. If a choice is tunable-later, choose and proceed — do not surface it.

## The brakes to remove (each a deletion)
- the `Derive` **string marker** (`provider.rs:31` `DERIVE_MARKER`) — vs a real trait;
- the **expansion↔type-check stratum** that keeps the deriver runner off the one walk (`provider_executor.rs`
  runs in `expanded_items_impl`, base-graph-only; hardcoded caps `:941-948`) — collapsed by *staging*, NOT by
  folding the executor into CTFE (measured impossible: see step-2 correction);
- the **6 separate resolvers** (global tabled solver, keyed effect env, effect bounds, CapabilityEnv, scoped
  generated-impl overlay, const-predicate prover) — vs one `ProvisionEnv` walk;
- the **global coherence checker** (`ty/diagnostics.rs` `ConflictTraitImpl`) — vs placement.

## The slide (top → bottom)
**CORRECTION (measured 2026-06-19):** the byte-identical runway is steps 1–2. Making `ProvisionEnv` the actual
first-match *walk* (impl table as a tier, innermost-wins over the proof-forest's collect-all) is **NOT
byte-identical — it IS scoped shadowing**. So the walk + canonical tier + `Fix` + checker-demotion are ONE
new-behavior push (step 3), FV-gated — not a free consolidation.

**CORRECTION 2 (measured 2026-06-19, TD5 spike):** step 2's "fold the executor into ordinary CTFE" does not
survive the code. The provider executor is a **quasiquoter** (`provider_executor.rs:run` → `GenExpr` →
`provider_synthesis.rs:replay_expr` materializes raw HIR `Expr` nodes), not a value evaluator; ordinary CTFE
(`ctfe/machine.rs`) folds to runtime values (`SemConstId`) — different universes, not fusible. North-star-safe:
the executor is a **backend** ("single pathway, multiple backends" — the impl-generation backend), not a resolver
to merge. The stratum collapses by *staging* (skeleton upstream / bodies downstream), not engine fusion — see the
x-0…x-4 ladder in step 2. Scope shrinks: x-3 narrows overlay #5 (not deletes); CapabilityEnv #4 is NOT eaten here.

1. **Construction SSOT (byte-identical) — IN PROGRESS.** Fold every hand-rolled `TraitSolveCx` construction onto
   the one `ProvisionEnv` (cut-1 + seam-1 done; ~25 clean sites remain in `diagnosable.rs`, `core/semantic`,
   `analysis/ty/*`). Unifies *how* the solver context is built so the eventual walk is a one-place flip. Does
   NOT yet make it a walk. *Deletes scattered constructions.*
2. **De-magic the deriver (byte-identical).** Two parts. **(a, TD4 — landing)** Delete `DERIVE_MARKER`; graduate
   `Derive` to a real solver-trait (so `D: Derive<Eq>` resolves) — recognition keys on resolved trait-identity.
   **(b, TD5 — measured 2026-06-19, CORRECTION below)** *Stage* the executor downstream of name resolution so the
   one walk can reach the deriver runner. The ladder, smallest-blast-radius first, all byte-identical until x-4:
   - **x-0 — DEFERRED (measured).** Salsa-tracking `run` is not drop-in (`ProviderOutput`/`TargetReflection`
     aren't `salsa::Update`); that storability reshape IS x-3a's work, so do it there — not as standalone memoization.
   - **x-1 — DONE (`25d6dcfef`).** All replay rides the typed `ProviderEffect` trace; `BuilderCommand` deleted.
   - **x-2 [byte-identical] — the clean cleave (verified).** `exprs`+`pats` = bodies; `sigs`+`tys`+`effects` =
     skeleton (sigs hold resolved `TypeId`s; no `GenExpr` references a `GenTyId` → disjoint arenas, no tangle).
     **x-2a** nest `ProviderSkeleton`{effects,sigs,tys} / `ProviderBodies`{exprs,pats} inside `ProviderOutput`,
     repoint the ~5 `provider_synthesis` accesses + `dump_effects` (golden-gated; BUILD FIRST). **x-2b** thread
     skeleton+bodies as two refs into `synthesize_provider_impl`/`ReplayCtxt`.
   - **x-3a+b — DONE (`f0dd2df8f`).** `salsa::Update`+`PartialEq,Eq` on the ~11 `Gen*`/`ProviderOutput`/
     `ExecError`/`TargetReflection` families (derives-only, byte-identical). The storable substrate for a body query.
   - **x-3c/x-3d — DEFERRED (measured 2026-06-20): the trivially-byte-identical de-magic runway ENDS at x-3a/b.**
     Keying is solvable (a `ProviderRun` `#[salsa::tracked]` struct holds the arenas — tracked structs don't hash
     fields, so no wide `Hash` churn; body query keyed on `(ProviderRun, member-idx)`). The REAL wall: a generated
     `Body` is a `#[salsa::tracked]` struct whose identity = `TrackedItemId` (from the `FileLowerCtxt` walk
     position, `body.rs:28`) + a **query-scoped disambiguator** (`tracked_struct.rs:407-431`). Moving `Body`
     construction into a downstream query (x-3d = the part that narrows overlay #5) risks changing that identity;
     **byte-identity is RUNTIME-ONLY, not provable by reading, and the snapshot gate may miss a latent id-drift
     that bites cut-1 MIR re-resolution determinism.** ⇒ x-3d is deliberate-care territory (id-equality assertion
     as the soundness tripwire; the safe x-3c fallback = query returns the replayed `Expr`/`Pat` VALUE while the
     ctxt still builds `Body`, but that adds surface with NO deletion). HOLD for a deliberate id scheme. The
     higher-value headline is the PUSH (collapses #1+#2 + deletes the checker) — and this soundness class belongs
     there. Acyclicity itself is PROVEN (body query strictly downstream of the merged graph).
   - **x-4 (NEW behavior, gated)** begin resolving deriver-body paths (`require<Trait>`, `<Ty as Trait>::CONST`)
     through real merged-graph name resolution instead of the base-graph shims (`canonical_trait_path:368`,
     `resolve_trait_def:484`). First point where a generated byte can change; land per-resolver with golden diffs.

   *Eats* the stratum boundary + narrows overlay #5 (x-3). Does **NOT** eat CapabilityEnv (#4) for free: the host
   caps (`Reflect`/`ImplBuilder`) have no value representation, so "caps bound through the provision env" is the
   separate caps-as-provisions thread (tied to the open surface question), not part of this byte-identical runway.
3. **THE PUSH — one first-match walk + `fixed`/`Fix` gating (NEW behavior; PROVEN).** Make `ProvisionEnv` the
   actual scope-chain walk: impl table = companion/outermost tier, effect frames = inner tiers, innermost-wins
   first-match — **this IS scoped shadowing** (the measured non-byte-identical step), and it collapses systems
   #1 (solver) and #2 (effect env) into the one walk. Add the `fixed`/canonical non-overridable tier; the
   `Fix<T>` capability (A1 narrow `Evidence`-clone; `Fix<*>`-as-kind master — ∀ over Constraint, instantiate-
   only, never solved) gating overrides via the `fix` verb; and **demote `ConflictTraitImpl` into the canonical
   tier — delete the global checker** (coherence = placement). **Money-soundness ⇒ provable** — FV obligations
   per `FCO_FIX_CAPABILITY_PACKET` (unforgeability, non-ambient propagation, attenuation lattice, gate
   soundness, MIR re-resolution determinism). Per the governing principle, decision C (the canonical/`fixed`
   *set*) is **defaulted, not gated** — land the mechanism against the narrow default and tune the set later; the
   *proof obligations above stay mandatory*. Plan the push as ~8 substeps in two halves: **Half A** = the walk
   (collapse #1+#2 + coherence-as-placement + write override `ImplementorId` into `ImplEnv`); **Half B** = the
   gate (`fixed` tier, `Fix<T>`, mint-at-root + non-ambient propagation, `fix` verb + attenuation, FV). Half A
   delivers the six→one headline on its own; Half B is the safety rail, sequenced second. **Measure the walk-flip
   blast radius (spike) before building**, exactly as TD5.
4. **Runout (emerges, nothing built):** a deriver provided as a scoped provision is run by the walk to satisfy
   `where Eq<T>`. Derivers-as-provisions falls out. The **load-bearing invariant is the mechanism, not the
   surface** — surface-neutral, it is exactly four steps: (1) a deriver is a *value* that produces
   `Evidence<Eq<T>>`, typed `D: Derive<Eq>`; (2) it is placed as a *provision* in a scope tier; (3) a demand
   `where Eq<T>` ≡ `uses (_: Evidence<Eq<T>>)` walks the scope chain; (4) the walk *runs* the deriver to
   discharge the obligation, yielding the evidence. **Demanding a deriver is itself first-class and cliff-safe:**
   `D: Derive<Eq>` has concrete head `Derive<·,Eq>` with `D` the roaming subject = ✅ — `StableEq` satisfies it,
   `StableClone` does not — so "require/provide a deriver" sits on the safe side of the cliff law, no abstract
   head needed. **The SURFACE syntax stays OPEN/deferred-tunable** (governing principle): the illustrative
   `with (Derive<Eq> = …)` is one of several spellings a shim-free resolver desugars to the same provision; pick
   the plainest at the runout, don't gate on it. System #6 (const-predicate) rides along as the CTFE backend.

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
  fable logs), **never block on Micah** for tunable-later policy or syntax — pick a sensible default and proceed
  (governing principle). Surface only a true correctness/soundness wall, never a sugar or canon-set choice.
- Re-verify "can't / too big" verdicts by *measuring what can be deleted next* ([[reverify-inherited-blockers]];
  surface-area is the metric, [[burndown-value-is-surface-area]]; clean SSOT increments, [[tracing-must-be-a-joy-to-maintain]]).
