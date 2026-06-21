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

**CORRECTION 3 (measured 2026-06-20, P1 build attempt — STOPPED clean, no changes):** there is NO byte-identical
pre-step to the walk flip. Verified: **(Wall 1)** trait resolution is invoked from ~28 sites incl. MIR re-resolution
(`mir/.../classify.rs:2280`, `synthetic.rs:1520`, `package.rs:1657`) and context-poor hir sites that have NO
`TyCheckEnv`/effect-frames — only `(scope, assumptions)` (`ProvisionEnv`, `trait_resolution/mod.rs:171-174`); the
lexical scope-chain (effect frames) lives only in `TyCheckEnv.effect_env` (`env.rs:739`). ⇒ the scoped walk can ONLY
run at use-sites; downstream MUST be **record-driven** (the cut-1 `ImplEnv.selected_implementor` rail), never re-walk
with scope. **(Wall 2)** #1 is collect-all (load-bearing for the `5-0001` coherence diagnostics), #2 is first-match —
merging flips `Ambiguous`→`Unique` = the new behavior. **The push's true shape:** use-site scoped walk + recorded
selections downstream + coherence-as-placement/PS5 — a real design, money-soundness-bearing, NOT a wiring flip. The
byte-identical runway ENDS at x-3a/b; the push is the supervised new-behavior phase.

**HIR-QUERY VERDICT + CACHE-SAFE DESIGN (2026-06-20, Micah's "query-based compiler" steer):** the scope-graph
ancestry, the impl tier (`ingot_trait_env`), and the assumptions tier ARE pure salsa queries — so they need NOT
enter a scoped key. The `with`/effect tier is the ONE that resists: its provision payload `ProvidedEffect.ty` is a
mid-inference result (`check_with`→`check_expr_unknown`→`fold_ty`, `expr.rs:1167-1181`) and the `With` expr is not
scope-graph-sited (`lower/expr.rs`), so it can't be a `(scope)` query — but that's fine, `with` is a use-site
phenomenon by nature, and downstream legitimately resolves one tier down (impl table) on the recorded
`selected_implementor`. **Decisive payoff: do NOT add `scope` to the tracked solver key (`is_query_satisfiable:357`)
— that would shatter the whole solver cache. Resolve the scoped `with`-tier override at the VERIFY-LEG
(`expr.rs:2331`, scope already a param, OUTSIDE the tracked solve). Cache blast radius confined to the new override
layer; the impl-table query stays ingot-keyed.** Increment ladder: A1 companion-tier entry-unification
(behavior-preserving) → A2 scoped shadowing (new, additive) → A3 coherence demotion → B1 `Fix` capability
(the PS5/MONEY increment) → B2 the `fixed`/`fix` gate. A2/A3 must NOT enable shadowing of a canonical goal before
B1's gate exists. v1 canonical set (picked, narrow, tunable marker): storage-layout (`StorageKey::write_key`),
ABI-layout, consensus `Ord`/`Hash`.

**PUSH PROGRESS (2026-06-20):** **1a LANDED + cold-verified (`22cc20d53`) — the #1+#2 unification MECHANISM is in.**
A deferred trait obligation can be discharged by an in-scope `Evidence<goal>` provision: snapshot rides the
obligation to drain-time (`TraitObligation.scoped_provisions`), peeled by resolved `core::derive::Evidence`
identity, unified, discharged via a new `DischargeRoute::ScopedProvision` (MIR re-resolution sees it). Additive
(inert until a surface mints `Evidence` into scope), unit-tested, cache stayed scope-free. **1b (the runout that
ACTIVATES 1a) — NOT landed.** A worktree spike (`172dbd652`) demonstrated the e2e path (a `where Eq<T>` discharging
from a scoped provision with NO `impl Eq` — the headline visible) but only via a typeck-only FORGE
(`pub extern fn __derive_evidence<G> -> Evidence<G>`, ungated despite a doc claiming otherwise) that bypasses
`Evidence` unforgeability. Rejected — we do not bake an unforgeability hole. **THE KEYSTONE for the remaining sound
work = the executor↔CTFE crossing:** running a deriver to produce GENUINE (impl-backed) evidence at resolution.
This same crossing gates sound-1b (real scoped evidence) AND x-3c/d (move generated bodies downstream). It is the
central remaining hard problem of the slide; the byte-identical runway + the 1a mechanism are the runway up to it.

**KEYSTONE INSIGHT (Fable-log review 2026-06-21):** "stage, don't fuse" is the ORIGINAL genesis decision (OBL-TX
2026-06-10: "the real CTFE machine can't host provider bodies — no mutation through effect params, no command
side-effects, no reflection iteration"), re-confirmed by the 2026-06-19 measurement — twice-measured, trustworthy.
So the keystone = **run the executor (quasiquoter backend) as a DOWNSTREAM salsa query; its output (a REAL impl,
surfaced as a `TraitInstId`/`ImplementorId`) is the evidence the ordinary solver consumes** — evidence is an IMPL,
not a value or a `term.rs` term. **The ONE open frontier = the `Body`/`TrackedItemId` identity-stability wall**
(acyclicity is PROVEN; byte/id-identity of downstream-constructed `Body`s is RUNTIME-ONLY — no proof, no clean test
yet; this is the genuine soundness obligation). Costed runway already on the shelf: `CTFE_DERIVE_PHASE_BOUNDARY.md`
Option B (post-lowering derive-expansion salsa query taking `HirAnalysisDb` — exactly x-3d's shape; lowering can't
invoke analysis, this is the escape); the quasiquote authority frame (`fe-quasiquote-design-2026-06-10.md`:
`ImplBuilder` gates splice = the unforgeability that rejects the forge); Rung-3 Spike-2 ("lower provision demands to
canonical trait-solver goals, REUSE the tabled proof forest — don't grow a second resolver"). DEAD-END to avoid:
NEVER run CTFE/the deriver inside `is_query_satisfiable` (Salsa-cycle ICE; D5.1) — run it OUTSIDE, feed the impl IN.
Vocabulary: "quasiquoter backend," never "CTFE provider" (the latter phrasing caused the fuse-misread; some
reference docs — dossier §4e, derive sketches, kind-decision rung — still carry the superseded "de-magic the
executor into CTFE" framing; `FCO_THE_SLIDE` is SSOT).

**CASCADE (coherent cascading shadowing — the sound, near-term path; 2026-06-20):** Micah's target = inner scopes
shadow outer for a goal, innermost-wins, canonical/`fixed` non-shadowable (money floor). Built SOUNDLY as
**multiple REAL global impls coexist + scope picks which one, RECORDED so codegen uses it** — NOT scope-local impls
(verified unneeded: `ImplementorOrigin`=Hir|VirtualContract|Assumption `trait_def.rs:170`; MIR floor
`classify.rs:2287`; the impl table is already a `Vec` + the solver already multi-solutions). DISTINCT from the
executor↔CTFE keystone: the cascade SELECTS among existing impls (near-term, sound); the keystone RUNS a deriver to
GENERATE a new impl (later). Ladder: **C1 LANDED+verified (`64166f6a6`)** — MIR consumes the recorded
`selected_implementor` as the source (re-resolve only as fallback) + plumbs it forward; byte-identical incl. codegen
(build_foundry/cli_output/fe-mir). **C2** — the innermost-first discharge records a REAL `ImplementorId` (close the
live `Some(None)` forge floor, `mod.rs:1297`). **C3** — demote the single global coherence checker
(`does_impl_trait_conflict`, sole caller `core/semantic/mod.rs:3989`) to allow >1 impl for NON-canonical goals;
canonical/`fixed` stay exactly-one (the `Fix` floor, B1a's inert recognition wired live here). Order C1→C2→C3 so
there is never a >1-impl + re-resolve + `Some(None)` ambiguity/money window.

**SUMMIT DESIGN (2026-06-21): the selection seam ALREADY EXISTS — the summit = surface + floor + default-rule +
demotion, NOT a new resolver.** `discharge_from_scoped_provision` (`mod.rs:1292`) is already the scope-free,
innermost-first, first-match walk (runs BEFORE the tracked solve; walks `scoped_provisions` innermost-first;
records a real impl) — the Spike-2 steer realized (reuse the forest for the unscoped default; no second resolver).
Literal "candidate-ordering in the proof forest" FAILS: the forest deliberately seeks a 2nd solution to detect
ambiguity (`proof_forest.rs:26,155-187`), so reordering can't collapse `NeedsConfirmation`→`Satisfied` without
scope-keying the tracked solve (forbidden, `trait_resolution/mod.rs:109-164`). **Default-tier rule (a′+b):**
unscoped + >1 impl → pick the default-marked (canonical/CoreDerives) impl at the verify-leg (`expr.rs:2335`,
scope-free); none/multiple marked → clean diag, NEVER the MIR `select_impl`→panic path. **Sub-rung ladder
(default-rule + floor land BEFORE the demotion — the ordering law):** **C3c-1** money floor (wire `goal_is_canonical`
LIVE, byte-identical — canonical stays exactly-one + `5-0001` fires; remove `allow(dead_code)`; the SMALLEST first
rung) → **C3c-2** default-tier rule (verify-leg picks default-marked; clean diag) → **C3b** surface (flip
`None`→`Some(impl)` at the `With` stamp site `expr.rs:1184`; downstream already wired; spelling deferred-tunable +
parser-risk) → **C3c-3** the demotion/prune (allow >1 non-canonical; canonical unchanged). Money risk CLOSED by
C3c-1 + the unforgeable `Fix` gate (the ONLY authority to override a canonical goal). Salsa-key risk ZERO (all
selection at the verify-leg + the discharge seam, outside the tracked solve). Build-only: parser spellability (C3b),
MIR-panic-vs-clean-diag (C3c-2).

> **POINTER:** the now-settled `Fix`/establishment model that unifies this cascade + Half-B floor under one
> vocabulary is distilled below in **"Fix / establishment — ratified model (2026-06-21)"** (end of doc; full
> adversarial pass in `FCO_FIX_UNIVERSAL_IMPL_DESIGN_2026-06-21.md`, commit `775ff0aa2`).

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
   #1 (solver) and #2 (effect env) into the one walk.
   **MEASURED (2026-06-20, Half-A blast radius):** near-ZERO fixture churn — the 4 `5-0001` coherence fixtures are
   all *same-tier* (two top-level impls) → stay conflicts under "outermost tier ≤1 provision," stay green; body
   `AmbiguousTraitInst` fixtures = 0. No existing fixture encodes inner-vs-outer shadowing → the new behavior is
   purely ADDITIVE (new fixtures only). Rails READY: typeck + MIR re-resolution both funnel through `select_impl`,
   so a scoped override written into `ImplEnv.selected_implementor` is determinism-covered for free. The REAL work
   is two non-fixture risks: (1) **salsa-key inversion** — `scope` is deliberately EXCLUDED from the solver key
   today (cut-1 carry, `trait_resolution/mod.rs:109-164`); a real scoped walk makes results scope-dependent → the
   key must change, incrementality blast radius build-only-measurable; (2) **PS5 canonical markers must land WITH
   coherence demotion** (the money gate). #1 = collect-all proof forest (`proof_forest.rs:178`), #2
   (`effect_env.rs:162`) already first-match; the merge makes the impl table the outermost tier of one lexical
   walk. Sequence: gated mechanism-flip (companion tier reproduces today) → PS5 markers + companion-tier coherence
   (delete global `ConflictTraitImpl`) → shadowing fixtures. Add the `fixed`/canonical non-overridable tier; the
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

---

## Fix / establishment — ratified model (2026-06-21)

*The peeled-back, settled version of the design. The full adversarial pass + two consolidation reviews are in
`FCO_FIX_UNIVERSAL_IMPL_DESIGN_2026-06-21.md` (commit `775ff0aa2`) — read it for the SETTLED/OPEN/REFUTED tags,
the lens-by-lens refutations, and the D1–D9 ledger. This section CITES that doc; it does not duplicate it.*

**1. `Fix<G>` — what it is.** An unforgeable, linear (`own`), single-use value = "permission to ESTABLISH one
impl of a consensus-critical goal `G`." The inert type exists today (`ingots/core/src/derive.fe:78`). Three
beats: **BORN** at root (compiler-only origin ⇒ unforgeable), **SPENT** to establish the impl (`own`/move ⇒ used
once ⇒ at most one), **GONE** after. It gates impl **CREATION**, never the **USE** of the implemented functions.
Cosmetic goals (`Eq`/`Ord`/`Clone`…) need no `Fix`; canonical goals (ABI / storage layout) require it.

**2. Enforcement — the two distinct mechanisms (do not conflate).** "No second impl gets created" is enforced by
the **GATE** (`lowered_implementor` → `does_impl_trait_conflict`, live today as `5-0001`), which COUNTS impls.
`Fix` is the unforgeable + scarce + linear **AUTHORITY** the gate honors for a sanctioned canonical impl /
override. Linearity alone is insufficient — it does not stop a no-`Fix` second impl; the gate does the counting,
`Fix` governs the exception. "Never USED" needs no separate check: an impl the gate rejected never enters the
impl table, so use-soundness is transitive on creation-soundness.

**3. One representation, convergence-at-the-gate.** Hand-written and generated impls both become ONE `ImplTrait`
HIR node via the same constructor and meet at `lowered_implementor` (`ImplementorOrigin::Hir`). The executor's
intermediate (`GenExpr` / `ProviderOutput`) is TRANSIENT — never persisted. The only difference is the
`HirOrigin` provenance tag (`raw` source-span vs `desugared`). **`impl`-as-sugar was REFUTED** — it would
relabel, not collapse; demand net-new builder surface; and downgrade hand-written impls to the unstable
desugared identity. `impl` stays its own front-end; the unity is the shared GATE, not a shared builder.

**4. Consolidation verdict.** ONE recognizer family (`Fix` / `Evidence` / `ImplBuilder` / `Reflect` / `Derive`
recognized by one predicate — Tier 1.1), TWO value lanes. `Fix` MUST stay an ordinary `own` value (NOT a
borrowck-capability / provider-binding), because `local_has_runtime_move_semantics` (`borrowck/ir.rs:215`) drops
the move-floor for capability-typed / Provider locals (`as_capability` recognizes only `Borrow*`/`View`).
`Authority` / `grant` / per-platform-mint = a deferred POLICY layer (grant-as-data, bottoming at compiler-seeded
`RootProvider`; NO reified `Mint` tower — cliff law).

**5. Keystone = the linchpin** for soundness AND tooling: content-keyed stable identity for generated impls
(today they get a positional expansion `TrackedItemId` + `HirOrigin::desugared`, unstable vs hand-written
source-AST anchoring). It unblocks derive-via-`uses`/`with`, the remaining cascade fixtures, the derive-grammar
retirement, AND LSP / tracing / debug parity. **Independent of `Fix`.**

**6. Open decisions (D1–D9 ledger).** See `FCO_FIX_UNIVERSAL_IMPL_DESIGN_2026-06-21.md §8`. The two that matter
first: **D1** (mint default — recommend **scarce-for-canonical / ambient-for-rest**) and the **D8**
cross-ingot / orphan-root caveat.

**7. Build sequencing.**
- **Tier 1** — recognizer collapse (→ `Some`-branch determinism + `Fix`-floor byte-identical → this write-up).
- **Tier 2** — keystone (parallel long-pole).
- **Tier 3** — the Half-B money rail (fix-consumption + `Authority`/`grant` + per-platform mint + non-ambient
  propagation → delete the coherence checker **LAST**).
