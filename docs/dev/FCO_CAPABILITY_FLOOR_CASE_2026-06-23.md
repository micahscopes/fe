# FCO capability floor: the case for "effect-carried capability + barrier count, no root body"

**Date:** 2026-06-23 (Micah-directed). **Status:** RATIFIED design rationale for the T3 money floor (#83).
The audience is Sean and Yoshi: this is the sellable case, not an internal scratchpad. No emdashes.

It supersedes the floor-ENFORCEMENT claims in `FCO_FIX_SURFACE_2026-06-22.md` (the "use-once falls out of
affine `own` move" framing). The SURFACE in that doc (`anchor()`, `Anchor<G>`, `with a`, `AdmitAnchor<G>`,
default-allow) stands unchanged and correct; only the claim that the move-checker enforces the floor is wrong,
and is replaced here. Grounded in three read-only research spikes (#90 value-flow linearity, #91 compile-time
execution at root, #92 done-properly comptime + effects-as-capabilities) and one design spike (#93 multitarget
intrinsics), 2026-06-22/23.

## 1. The claim being made convincing

For a "one-of-a-kind" goal (storage layout, ABI encoding: two impls mean two parts of the program disagree
about the bytes) there must be at most one impl, program-wide, and only whoever holds delegated authority may
establish it. We claimed this is an **effect-compatible capability with consume-once semantics, enforced with
no synthetic root body and no compile-time execution (CTFE).** That sounds too good (consume-once usually wants
affine machinery, and affine machinery wants a function body to run in), so here is why it holds.

## 2. The shape: two layers, different jobs, different arity

The design factors cleanly into two checks. Keeping them separate is the whole argument.

- **Authority = an effect-carried capability.** `AdmitAnchor<G>` rides the existing effect system
  (`uses` / `resolve_effect_query`). It is minted once at the program root and delegated down (to the
  per-deployed-contract root for storage/ABI goals). The check is the ordinary effects-as-capabilities check:
  is the capability in scope at this site. It is LOCAL and per-site, and it is a presence test (>= 1 in scope),
  not a count. A grant is non-linear: one grant discharges many obligations. That is correct, and it is the
  reason layer two exists.

- **Scarcity = a count at a barrier.** At most one establishment of a canonical goal program-wide, enforced as
  a pure predicate over the complete, already-materialized impl set at the establish gate (`lowered_implementor`),
  alongside the coherence overlap check that already runs there. It is AFFINE (<= 1, zero is legal: a canonical
  goal with no impl is fine), never linear (= 1, which would wrongly force every canonical goal to be
  implemented).

An anchored impl is subject to both. A non-scarce capability-gated impl (a backend intrinsic, see section 6) is
subject to authority only. The two checks share no mutable state and never interleave, so they compose without
interference.

## 3. Why no root body, and why that is the MORE correct form

The intuition "consume-once needs a move-checker" is true for a value consumed in sequence inside a function
body. It does not apply here, for a concrete substrate reason and a conceptual one.

- **Substrate (measured, spikes #90/#91).** An `impl` is a top-level item, not a body. Fe's borrow/move checker
  is a no-op on impls (`borrowck/check.rs`: `ItemKind::ImplTrait` falls in the no-op arm; only `Func` / `Const` /
  contract bodies are walked). A second top-level `impl AbiSize for Ledger` is not, and cannot be, a
  use-after-move. The affine substrate is real but lives one phase below where the establishment happens. So
  "second `impl .. with a` is a use-after-move" describes a check that physically does not run.

- **Conceptual.** The canonical establishments are a flat, UNORDERED, whole-program SET. Two impls of the same
  goal are equally invalid as a pair; there is no meaningful "first." Affine over an unordered set with no
  order is exactly set-cardinality <= 1. The move-checker's sequencing (use-after-move) is machinery for a
  problem we do not have. **The count IS the affine check, specialized correctly to a set.** Being order-free is
  not a compromise: it is precisely what makes the check salsa-safe (the cache key is the unordered impl set,
  not a traversal order) and what keeps it out of the order-dependence hazard described next.

So we did not drop affine. We recognized that affine-over-a-set is a count, and the count is the order-free
realization of the same guarantee.

## 4. The precedent backbone: "done properly" is a barrier over the complete set

Every language that enforces a whole-program invariant at compile time does it the same way, and the way they
all avoid the known trap is the way we are choosing.

- **The trap (Zig issue 5718).** Zig once allowed `comptime var` captured in a closure, which let you "build a
  borrow checker" or "lazily accumulate a global list of what gets compiled." That is almost exactly the
  synthetic-root-body idea. Zig DELETED it. The defect is specific: mutable comptime state, accumulated by
  interleaved execution whose order is observable, aliased across calls by memoization. Zig's fix was to make
  comptime a pure, memoized transform on data, no global mutable accumulation. The named precedent for the
  root-body idea is therefore a cautionary tale, removed for the exact determinism reason Fe's salsa discipline
  forbids.

- **What everyone who does it properly actually does.** The principled mechanisms are pure-CTFE (D, C++,
  Zig post-5718), declarative-table-query (Coq / Lean canonical structures and instance resolution), or
  barrier-over-the-complete-set (Jai's compiler message loop, which collects declarations and enforces at the
  `TYPECHECKED_ALL_WE_CAN` synchronization barrier; Racket's whole-module `#%module-begin`). The strongest direct
  analogs to our exact problem, "at most one impl program-wide," are Rust coherence and Coq canonical
  structures, and BOTH enforce it as a whole-program table check, never as a synthesized body running an
  ownership analysis. The negative result is the punchline: no surveyed language safely offers order-free
  whole-program MUTABLE accumulation. The principled designs drop either the "mutable" leg or the
  "order-observable" leg.

- **Fe does it more properly than the precedent.** Jai's barrier check is sound only by programmer discipline
  (the metaprogram is ordinary mutable host code; the author must write an order-insensitive check). Fe keys the
  establish-gate query on the unordered impl set (`ingot_trait_env.impls`, an ingot-keyed map), so
  order-independence is a property of the salsa cache key, by construction, not by discipline. That is the
  declarative-table-query model (Coq / Lean) applied to Fe's existing coherence environment.

This is what makes "no root body" the principled choice rather than the cheap one: the root-body-that-executes
is the shape every cautionary tale points away from, and the count-over-the-complete-set is the shape every
sound system converges on.

## 5. What "consume-once" actually means here (so the story is honest)

There is no literal token moved and discarded. The same guarantee is produced by two facts that cooperate:

1. The authority is mintable once: for a held-back canonical goal, `AdmitAnchor<G>` is non-ambient (the
   default-allow blanket excludes canonical goals, via the single predicate `goal_is_canonical`), so the only
   source is the root's single mint, delegated down.
2. At most one establishment is admitted: the barrier count rejects a second establishment of the same canonical
   goal.

Fact 2 is load-bearing precisely BECAUSE the effect grant in fact 1 is non-linear (one grant could otherwise
discharge two impls' obligations). Naming that is what makes the design honest rather than hand-wavy: the effect
system gives authority; the count gives scarcity; neither alone is the floor.

## 6. The unification: anchors are instance #1 of capability-gated establishment

The reason this is worth the trouble is that the canonical floor stops being a bespoke branch in the coherence
checker and becomes an instance of one general mechanism: **establishment of an impl is gated on holding
effect-carried capabilities, with scarcity an optional extra layer for the subset that needs it.**

- **Anchors:** authority (`AdmitAnchor<G>`) + scarcity (<= 1). The money floor.
- **Backend intrinsics (the second instance, see #93):** authority only. An intrinsic declares
  `uses (t: EvmIntrinsics)`; the toolchain seeds the selected target's capability at the root; an impl/call is
  admitted only where that capability is in scope. Per-target exclusivity falls out for free (only one target's
  capability is ever seeded). No scarcity layer, because using an intrinsic is freely multiple.

Effects-as-capabilities (Koka, Eff, Effekt, System C) is exactly this local in-scope check, and Fe's only
genuine novelty is applying it at the establishment site rather than only at a call site, a narrow, well-founded
extension. The non-capability baseline (Rust `#[cfg(target)]`) is erasure-selection: the unselected impl ceases
to exist before type-checking. Fe's design is strictly more expressive and type-system-internal: both backend
impls genuinely exist, and the threaded capability decides which is establishable.

## 7. Constraints to aim for now (banked from #93, NOT a committed design)

#93 sketched the intrinsic surface; we are not committing to its specific spelling. We are banking the
constraints it proves, so the floor work does not paint us into a corner:

- **The root-mint is the spine, and it already exists.** Target selection is already "which type fills
  `<Target as core::contracts::Target>::RootEffect`," seeded as the sole `RootProviderRegistration`
  (`provider.rs:117-131`, consumed at `semantic/mod.rs:434-452`). The ambient tail already has a
  runtime-valueless authority resolution class (`AmbientGrant`). So intrinsic capabilities and `AdmitAnchor<G>`
  are two occupants of the same tail and two consumers of the same root seed. Build the mint generic, not
  anchor-specific: inc5 should be "capability-generic root mint," with anchors contributing only the
  canonical-ness flag.

- **Authority is multiple; scarcity is <= 1; never conflate them.** The use-grant must stay non-linear (two call
  sites of an intrinsic are fine). Scarcity lives ONLY at the establish-gate count, only for canonical goals.
  Making the use-grant affine would wrongly reject legitimate multiplicity.

- **The establish gate must filter impls by in-scope capability BEFORE the coherence overlap check.** Otherwise
  two backend variants of the same goal/type (an EVM and a wasm impl) would falsely flag as conflicting. This is
  the one real integration constraint, and it must be respected when the floor is wired.

- **Keep the capability types unforgeable.** Sealed private field, no public constructor, not in the prelude by
  bare name, recognized by resolved identity (the existing `derive.fe` pattern). This is what makes "trust" a
  capability rather than ambient authority.

## 8. Why the scarcity line sits exactly at canonical goals (the Haskell lesson)

Capability-gating that can admit a SECOND impl of a goal whose downstream semantics assume uniqueness is the
classic global-uniqueness unsoundness: two `Ord` instances merging two `Set`s into a corrupted structure
(ezyang's coherence/confluence/global-uniqueness note; the same hazard motivates Rust coherence). The scarcity
layer is the backstop that prevents this, and the goals that need it are exactly the canonical ones (storage /
ABI, whose layout other code trusts to be singular). So drawing the <= 1 line at `goal_is_canonical` is not
arbitrary: it is precisely the set whose correctness depends on uniqueness. Anchors-as-the-scarce-special-case
is the right place to draw the line, not a kludge.

## 9. What this changes for the build (#83 inc4 onward)

The increment ordering in `FCO_FIX_SURFACE_2026-06-22.md` is right; the ENFORCEMENT mechanism it names is not.
Corrections:

- **inc4 is a count, not a use-after-move.** Flip the canonical branch at the establish gate so it consults
  (a) authorization (does this impl hold a matching `Anchor<G>` / capability, a per-HIR-node + path-resolution
  check, scope-free) and (b) the <= 1 count over `ingot_trait_env.impls` for the canonical goal (reusing the
  existing overlap loop verdict). Add the "canonical, unanchored" diagnostic. Drop the "double-consume
  use-after-move fixture"; replace it with a "second canonical establishment" count fixture, plus the
  generated-impl + explicit-impl convergence fixture (section below).
- **The convergence fixture is mandatory.** CTFE-generated impls and hand-written impls already converge into
  one population: the expansion stage materializes generated impls into `all_items`
  (`lower/expansion.rs`: downstream consumers see generated items exactly like hand-written ones), which flows
  to `ingot_trait_env.impls`, the set the gate counts. There is no separate solver evidence-witness route that
  satisfies a canonical goal without a materialized impl (`Evidence` / `ImplBuilder` are provider
  signature/execution handles, upstream of materialization). Pin this with a fixture: a generated impl and an
  explicit impl of the same canonical goal must both reach the floor as a conflict, so the uniformity is locked,
  not asserted.
- **inc5 builds the mint generic** (section 7).
- **inc6 deletes the coherence special-case last**, once inc4 demonstrably holds the floor and the overlap check
  still rejects non-default overlap.
- **Drop the affine-VALUE requirements** (the surface doc's "keep `Anchor` out of `as_capability` or the affine
  floor turns off" / D2). The floor is not the move-checker, so `Anchor`'s `as_capability` status is irrelevant
  to it. (Leaving `Anchor` a plain struct is still fine; it just is not load-bearing for the floor.)

## 10. Honest caveats

- **The novelty is narrow but real.** Effects-as-capabilities gates operations at call sites; gating
  ESTABLISHMENT sites by capability is a Fe extension. It is structurally the same in-scope check at a new kind
  of site, and the ocap literature (authority minted at one root, delegated down) backs the mint model, but no
  surveyed language gates trait/instance establishment by a threaded capability. We are first here, by a short
  step.
- **Intrinsic-provision uniqueness, pre-Sketch-C, is a Rust-side invariant.** Today the opcode binding is a
  hardcoded Rust path-match (`corelib.rs`), implicitly unique (one arm per path). Until/unless intrinsic
  provision is modeled as a counted provider trait (#93 Sketch C, behind `goal_is_canonical`), "one provider per
  (intrinsic, target)" is not a Fe-checked property for non-canonical intrinsics. Flagged, not solved; not
  needed for the first slice.
- **Evidence provenance.** The PL precedent in #90/#91/#92 was gathered under WebFetch returning HTTP 403, so it
  rests on search-result summaries with canonical source URLs, not full-text fetches. The load-bearing Zig
  5718 claim is corroborated across multiple results and matches the well-known issue, but it is
  search-summary-grounded. The Fe substrate claims are file:line-verified in the worktree.

## 11. Affine's actual role: the three-layer dovetail (Micah, 2026-06-23)

Affine is not the floor, but it is not idle either. It has a real complementary job in the CTFE half, and the
full design is three cooperating layers, all dynamic only with respect to comptime (nothing runtime).

1. **Static count (scarcity, whole-program).** The barrier count over the materialized impl set
   (`ingot_trait_env.impls`) at the establish gate. Bounds the total to <= 1 per canonical goal. Runs where
   there is no body (top-level items).
2. **Affine in CTFE (registration anti-leak, per provider body).** Provider fns ARE bodies, so the move/borrow
   checker runs on them. It keeps each CTFE registration well-formed: a `builder.finish() -> Evidence<G>` value
   used once, not duplicated, not escaped, the builder not emitted-to after finish. This is the linear-token /
   nonce-use-once idea applied IN CTFE, the one place it actually runs, not at the top-level floor.
3. **Intrinsic-exposed static results (comptime-dynamic logic).** A read-only intrinsic (built on the #93
   root-effect-object mechanism + the reflection/CTFE-handle work) surfaces the COMPLETE static facts ("is goal
   G established?", "the final impl set") into CTFE, so providers can build richer comptime-dynamic logic on top.

The cooperation: layer 2 makes each generated registration trustworthy at its source; layer 1 bounds the
whole-program total; they shake hands at the convergence point (the materialized impl set, where generated and
explicit impls already unify). Neither alone is complete: count without layer 2 lets a provider misbehave
internally; layer 2 without count does not bound the program-wide total. Layer 3 is the payoff built on both.

**Substrate (measured 2026-06-23).** `Evidence<G>` / `ImplBuilder<G>` are ordinary structs; `as_capability`
returns `None` (`ty_def.rs:289-298`), so a local `Evidence` value gets normal move semantics
(`local_has_runtime_move_semantics`, `ir.rs:215`; use-after-move is real, cf. `mir_move_error.fe`). The anti-leak
role is therefore PARTLY present already. The gap: a provider-supplied `mut ImplBuilder<G>` param can lower to a
`Provider` borrow root (`ir.rs:230`), which is exempt from move semantics, so "builder used after `finish()`" and
"evidence escapes the provider body" are not necessarily caught today. Closing that is a separable small
workstream (layer 2 below), not a precondition for the floor (layer 1).

**The one rule that keeps layer 3 safe.** The intrinsic must expose a COMPLETE, barrier-computed static result
(or a monotone/confluent query), NEVER an in-progress "what is registered so far" accumulator. If CTFE could
read the live, mid-pass registration state and register conditionally on it, that registration becomes
order-observable and we are back in the Zig-5718 trap the floor is designed to avoid. Read the finished set, not
the live one.

## 12. Consolidated build plan + validation

Governing rule (Micah, 2026-06-23): **every increment ships with validation that PINS the property it claims, and
lands only behind a full cold gate** (the M5 semantic standard: a fixture that fails before and passes after, snap
diff = 0 beyond intended). Agent work is cold-verified by cherry-pick + an independent gate, never trusted from a
self-report.

- **inc4 (count + convergence; mostly safe, no mint dependency).** The scarcity <= 1 count is ALREADY the floor
  (landed in T1.2: a canonical goal rejects a second overlapping impl via `does_impl_trait_conflict`,
  `mod.rs:4022-4027`). inc4's new, tree-safe work:
  - (a) **Convergence fixture (the meeting point).** A CTFE-generated impl and an explicit impl of the same
    canonical goal/type both reach the gate and conflict. VALIDATION: the fixture errors as a conflict, with a
    snap; this LOCKS the generated+explicit uniformity (which expansion already provides:
    `lower/expansion.rs` folds generated impls into `all_items` -> `ingot_trait_env`).
  - (b) **Authorization recognition (live but not yet mandatory).** Turn the `_anchor_capability_present`
    recognition seam (`mod.rs:4012`) into a real per-impl-node check: resolve `self.hir_anchor` against an
    `Anchor<G>` matching the goal. Recorded, not enforced. VALIDATION: a fixture shows an anchored canonical
    impl (on a currently-grantable goal) is recognized as authorized; `cascade_canonical_floor` stays green.
  - **Sequencing note (why mandate waits):** making anchoring MANDATORY for canonical goals (plus the
    "canonical, unanchored" diagnostic) must wait for inc5, because the mint for a held-back canonical goal does
    not exist yet. Forcing it now would break std's existing canonical impls (AbiSize/Encode/...), which carry
    no `with a` and cannot obtain one until root delegation lands.

- **Affine anti-leak (layer 2; separable, can interleave with inc4/inc5).** Make `Evidence<G>` / `ImplBuilder<G>`
  affine end-to-end in provider bodies, closing the provider-param gap measured in section 11. VALIDATION:
  (i) double-use of an `Evidence` value -> use-after-move; (ii) builder emitted-to after `finish()` -> error;
  (iii) `Evidence` escaping the provider body -> error.

- **inc5 (generic root mint + mandate).** Build the mint CAPABILITY-GENERIC (so `AdmitAnchor<G>` and intrinsic
  capabilities share it), seeded at `RootProvider`, delegated to the contract root. Flip canonical anchoring to
  MANDATORY + add the "canonical, unanchored" diagnostic. VALIDATION: (i) a held-back canonical impl with a
  minted+delegated anchor compiles; (ii) an unanchored canonical impl errors with the new diagnostic; (iii) std
  builds green (existing canonical impls now anchored via the mint). INTEGRATION CONSTRAINT (validate
  explicitly): the gate filters impls by in-scope capability BEFORE the overlap loop, pinned by a backend-variant
  fixture (two impls of the same goal/type gated on different target capabilities do NOT falsely conflict).

- **inc6 (delete the coherence special-case, LAST).** Only once inc4/inc5 demonstrably hold the floor and
  `does_impl_trait_conflict` still rejects non-default overlap. VALIDATION: full suite green after deletion; the
  floor fixtures still error; no snap churn beyond intended.

- **Intrinsic exposure (layer 3; later, on #93 + reflection handles).** The read-only static-results intrinsic.
  VALIDATION: (i) a provider reads the complete established-set fact and branches on it; (ii) a NEGATIVE test
  enforcing the barrier-vs-accumulator rule, the result a provider observes is order-independent (it cannot see
  in-progress registration).

Cold-gate scope per increment: `fe-hir` + `ty_check` + `cli_output` + `fe_test` (and `corelib` / `build_foundry`
where touched), run inline, snap diff = 0 beyond intended, before any commit.

## 13. Provenance

- Spike #90 (`tasks/a47e1760...`): value-flow linear/affine is always body/arrow-scoped; no language makes a
  top-level binding linear; scarcity must be a static count over the whole-program impl set.
- Spike #91 (`tasks/a41edd04...`): compile-time-execution-at-root precedent; the synthetic-root-body is
  essentially un-precedented and the closest prior art (Zig comptime closures) was deleted; the "observe all
  decls + enforce" pattern is universally count/registration/solver-shaped.
- Spike #92 (`tasks/a562bd38...`): done-properly comptime = pure-function-over-complete-set-at-a-barrier;
  effects-as-capabilities is a local lexical check; the two-layer (authority + scarcity) design is sound; the
  generic inc5 shape and the filter-before-overlap integration constraint.
- Design spike #93 (`tasks/a56095...`): multitarget intrinsics as root effect objects; the `Target::RootEffect`
  spine is already real; intrinsic capability = second occupant of the same ambient tail; scarcity (Sketch C)
  only behind `goal_is_canonical`.
