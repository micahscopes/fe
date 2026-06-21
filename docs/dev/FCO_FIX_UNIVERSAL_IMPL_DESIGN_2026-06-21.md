# FCO — `Fix<G>` as the universal linear capability for trait-impl establishment

**Date:** 2026-06-21 · **Status:** DESIGN (architect synthesis). Sits ON TOP of, and cites, the SSOT
`FCO_THE_SLIDE_2026-06-19.md` and the Half-B appendix `FCO_FIX_CAPABILITY_PACKET_2026-06-19.md`. This doc
unifies four mechanisms — global coherence, the canonical/money-floor, derive, and the built cascade — under one
model and survives an adversarial filter applied to six design lenses. Per the governing principle
(`fco-guts-over-sugar`): the proof is mandatory; the policy (canonical set, fix surface, coherence root) is a
deferred-tunable default. Every claim is tagged **SETTLED** / **OPEN** / **REFUTED**. Where the thesis breaks,
it is said plainly.

This doc does NOT re-derive the cascade, the keystone, the FV obligations, or the skeptic-leak list — those live
in the SSOT and the FIX packet. It cites them. Its job is the *unification* and the *honest delta*.

---

## 0. The thesis, and the one-line verdict

**Thesis.** Every trait impl — hand-written OR derived — is the act of ESTABLISHING an implementation of a goal
`G` by CONSUMING a linear, unforgeable capability `Fix<G>`. The per-goal MINT POLICY (how many `Fix<G>` exist,
who may mint) is a single dial yielding ordinary coherence (budget 1, ambient), cascade coexistence (budget N,
ambient), and canonical/money-floor (budget 1, root-scarce). Enforcement reuses CTFE + `own`/move; the keystone
(CTFE produces an installable impl with stable identity) is the linchpin. CRUCIAL INVARIANT: `Fix` gates impl
EXISTENCE, never the USE of the implemented functions.

**Verdict (survives, with two corrections that change the headline).** The thesis is sound *as a unifying
vocabulary and a sequencing spine*, and most of its substrate is already in the tree. But two of its load-bearing
mechanical claims are **REFUTED** as stated and must be re-cast, or the design ships an unsound or unbuildable
story:

1. **It is not ONE consumption event; it is two layers.** Canonical-floor establishment-scarcity is enforced by
   the HIR-item coherence checker (`does_impl_trait_conflict` + the `canonical ⇒ false` branch,
   `core/semantic/mod.rs:4001`), NOT by a `Fix` value the move-checker sees. The move-checker only ever sees the
   `Fix` *override token* at a body-level `fix` site. "Money-Floor = Linearity + Scarcity" is a **correspondence
   between two mechanisms**, not a single unified `Fix`-consumption-at-establishment. (Lens `model` refutation,
   confirmed.)
2. **Making `Fix` a threaded capability turns the affine floor OFF for it.** `local_has_runtime_move_semantics`
   (`borrowck/ir.rs:215`) returns `false` — i.e. *no* use-after-move tracking — when `local.ty.as_capability(db)`
   is `Some` OR the borrow-root is `NBorrowRoot::Provider`. Today `Fix<T>` is an ordinary struct so it IS
   move-tracked; the moment it is plumbed as a capability/provider-binding (the exact non-ambient threading the
   FIX packet skeptic-leak #2 demands), it falls into the exemption. Affine-as-the-floor and
   capability-threading are **mutually exclusive in the live borrowck**. (Lens `substrate` refutation, confirmed.)

These do not refute the *direction*. They mean the unified model carries a **two-layer enforcement** and a
**lane decision for `Fix`**, stated up front below. The cascade is genuinely built and sits cleanly under the
model as budget-N; the keystone is genuinely the open linchpin; `own` is genuinely affine.

---

## 1. The unified model (what survives)

### 1.1 Three grounded definitions

**`Fix<G>` (SETTLED, inert).** A constraint-indexed capability `pub struct Fix<T: Constraint> { handle: u256 }`
(`ingots/core/src/derive.fe:78`) — single private field, no public constructor, not in prelude. Kind
`Fix : Constraint -> *`, identical shape to `Evidence`/`ImplBuilder`. `G` ranges over **saturated CONCRETE**
constraint terms (`Eq<Speaker>` = `TyData::ConstraintTerm`), never a free `* -> Constraint` head — **the cliff
law** (SSOT §"the cliff law", lines 220-228; `FCO_CONSTRAINT_INDEXED_CAPABILITIES_2026-06-18.md`). Recognized by
FULL resolved `core::derive::Fix` identity (`scope_is_fix_capability`, `provider_goal.rs:459`, keyed on name +
`derive` module + `IngotKind::Core`), so a user struct named `Fix` gets zero authority. Currently
`#[allow(dead_code)]` / INERT — nothing mints or consumes it (B1b is the consumer gate).

**"Establish an impl" (SETTLED).** The admission event `lowered_implementor` performs
(`core/semantic/mod.rs:3973-4048`): construct an `ImplementorId`, run the coherence gate
(`does_impl_trait_conflict` against the ingot trait env), admit on success. For DERIVED impls this already
extends to real establishment: the expansion executor synthesizes a real HIR `ImplTrait` (origin
`ImplementorOrigin::Hir`) that flows through `collect_trait_impls` into the same gate. The keystone (SSOT
lines 86-106) is the *downstream* form of this — running the executor as a downstream salsa query producing a
real `TraitInstId`/`ImplementorId` ("evidence is an IMPL, not a value").

**"Budget" = per-goal MINT POLICY (SETTLED as a dial; the integer framing is REFUTED — see §1.3).** How
generously the root admits a second establishment of `G`. Three shapes: budget-1-ambient (ordinary coherence),
budget-N-ambient (cascade — *but see §1.3, it is a 2-slot {default, override} shape, not arbitrary N*),
budget-1-root-scarce (canonical/money-floor). The dial LIVES TODAY as `goal_is_canonical` (`trait_def.rs:883`)
branching `lowered_implementor` (`mod.rs:4001-4022`).

### 1.2 The crucial invariant, re-stated honestly (SETTLED for USE; layered for EXISTENCE)

`Fix` gates impl EXISTENCE, never the USE of the implemented functions. The USE half is **structurally true**:
MIR method resolution consumes the *recorded* `ImplementorId` (`classify.rs:2306`, the C1 rail) and never
re-mints a capability per call. This is sound *because it is a phase distinction* (lens `prior_art`,
Template-Haskell `Q` / Racket phases): authority is a compile-phase effect; the object program's methods are
authority-free.

But "consumed once when the impl is created" is **two layers, not one** (REFUTED as a single event):

- **Existence-scarcity for canonical goals** is enforced at the HIR-item coherence query
  (`does_impl_trait_conflict` + `canonical ⇒ conflict_allowed = false`, `mod.rs:4001-4005`). There is no `Fix`
  value, no body, no `own` move here.
- **The `Fix` override token** is consumed at a body-level `fix` site (FIX packet line 43) — the only place the
  move-checker can see it — to authorize a *second* impl to win *in scope*.

The "Money-Floor = Linearity + Scarcity ⇒ ≤1 canonical establishment" theorem (lens `model`) is **valid as a
correspondence** between these two layers, **not** as one mechanism. Ship it as: *linearity bounds override
authorizations (≤ tokens held); the canonical floor is the coherence checker staying exactly-one; the theorem is
that these two agree.* Do **not** claim the existing move-checker enforces canonical establishment.

### 1.3 The budget UNIT and the budget SHAPE (both corrected)

**Unit (corrected).** The countable unit is the **`ImplementorId` / establishment event**, never a
`(Trait, Type)` pair. The impl table is `impls: FxHashMap<Trait, Vec<Binder<ImplementorId>>>`
(`trait_def.rs:563`). There IS a per-type-base index `ty_to_implementors: FxHashMap<Binder<TyId>, Vec<…>>`
(`trait_def.rs:566`, keyed on `base_ty`) — so the lens `model` premise "there is NO (Trait,Type) index" is
**REFUTED**; soften to: *neither index is keyed by the (Trait,Type) PAIR; a generic `impl<T> Eq for Vec<T>` is
ONE `ImplementorId` in each index.* The conclusion survives: budget is per-establishment, and overlap among
families is decided by `does_impl_trait_conflict` (fresh-var instantiate + unify + merged-constraint-sat,
`trait_def.rs:802-841`), the substrate budgets REUSE rather than replace.

**Shape (corrected — this is the sharpest correction).** The live budget-N branch is **NOT an integer counter of
"`Fix<G>` tokens minted."** It is a fixed **2-slot {derived-default, hand-override}** shape gated on
`implementor_is_default_marked` differing (`mod.rs:4007-4021`, `this_default != cand_default`). Two non-default
user impls of a non-canonical trait STILL hard-conflict (`5-0001`). So "budget = |minted| − |consumed|" (lens
`mint_capabilities`, lens `coherence_migration`) **mis-describes the code** and is **REFUTED** as a
reinterpretation: recasting it as "N arbitrary impls" silently changes semantics from default+override to
N-arbitrary, which the gate explicitly rejects, and selecting among >1 override is `None`
(`expr.rs` `sole_scoped_selection_implementor`, keystone-deferred). **Keep the model honest:** the only two real
shapes are budget-1 (canonical: never a second; non-canonical w/o a derived default: still `5-0001`) and the
2-slot default+override. Arbitrary budget-N is demand-empty and selection-blocked; design it, do not claim it.

### 1.4 Two orthogonal dials, not one (the deepest survivor)

The single most important *design* finding, on which lenses `prior_art` and `mint_capabilities` and the FCO
authority docs converge: **existence-gating is necessary but not sufficient.** The mint dial (Dial 1) closes the
*orphan / who-may-establish* hazard. It is **silent** on *use-site uniformity* (Dial 2): did two use-sites that
feed one shared structure-with-a-global-invariant select the SAME impl? This is the classic
two-`Ord`-into-one-`BTreeMap` corruption. `FCO_PROVISION_AUTHORITY_CONSTRUCT_2026-06-17.md:26-63` independently
rediscovered it as a use-site `P`/provenance demand and proved provider-side seal/override (Dial 1) alone cannot
fix it.

**Honest scope (REFUTED as a live blocker; re-cast as a pre-positioned gate).** The concrete corruption is
**NOT reachable in the current tree.** Fe's stdlib has no `Ord`/`Hash`/`Eq`-keyed shared container; the only
keyed container is `StorageMap<K,V>` keyed on `StorageKey`, which is IN the canonical money-floor set
(`trait_def.rs:895`) and therefore cannot be cascade-overridden. And cascade selection is **per-direct-call**
(call-keyed `MethodSelection`), not ambient-through-callees (`cascade_scoped_override.fe`: "No generic/non-trait
helper; every call is a direct trait-method call"). So Dial 2 is a **MAJOR LATENT obligation to land BEFORE the
first `Ord`/`Hash`/`Eq`-keyed shared container appears**, not a shipping blocker. The money-floor collapses Dial 2
to trivially-satisfied (one impl exists to select), which is WHY canonical goals can defer it.

The two-dial split is the unification's load-bearing intellectual content: Dial 1 = mint policy (this doc's
budget); Dial 2 = use-site uniformity (the Construct-3 / MTC witness-capture work, deferred but named).

---

## 2. Soundness obligations + the skeptic-leaks that remain open

The FV obligations and the seven skeptic leaks are SSOT/FIX-packet content — cited, not duplicated
(`FCO_FIX_CAPABILITY_PACKET_2026-06-19.md:47-57`). What this synthesis ADDS or SHARPENS:

**SETTLED (grounded in the tree):**
- **Unforgeability** — private field + no ctor + not-prelude + resolved-identity recognition. (`derive.fe:78`,
  `provider_goal.rs:459`.)
- **`own` is AFFINE, not linear** — use-at-most-once, may-drop. `mir_move_error.fe` (`consume(x); x` → use after
  move); `SemanticBorrowDiagKind` has no must-use/undropped/leak variant. Affine SUFFICES because the floor is an
  upper bound (≤1) and establishment is not optional anyway. Do NOT add must-consume linearity — it would only be
  needed for "EXACTLY one / mandatory establishment," which canonical goals do not require (a type may simply not
  implement `StorageKey`). The thesis word "linear" should read **affine**.

**OPEN (the skeptic-leaks that remain, with this synthesis's sharpening):**
- **Ambient capture (skeptic-leak #2, "most dangerous") — OPEN, and worse than the packet states.** Not only must
  `Fix` propagate non-ambiently; the cascade's own selection surface inserts its provision into the ambient
  `EffectEnv` frame (`insert_unkeyed`, popped at frame exit). Reusing the cascade rails for `Fix` *authority*
  would inherit the ambient walk. **Selection-scoping ≠ authority-scoping.** `Fix` must thread by explicit demand
  (`uses (_: Fix<T>)`) and/or a `Barrier`-excluded frame — and note `snapshot_provisions` currently *drops*
  barriers (`effect_env.rs`, `Barrier(_) => {}`), so "reuse the barrier to cut propagation" is NEW walk logic in
  the hot path, not a wire-up. (Lenses `substrate`, `mint_capabilities`, `prior_art`.)
- **The `Fix` borrowck-lane leak (NEW, blocker-class) — OPEN.** Per §0(2): capability-ness is the OFF switch for
  the affine floor. **Decide the lane** (Decision §8-D2) and enforce affineness within it; add a fixture that
  passes a `Fix`-typed value twice and asserts use-after-move AFTER whatever capability-plumbing the increment
  adds.
- **The Some-branch determinism gap (NEW, sharpened) — OPEN.** `check_reresolution_determinism` (the MIR
  re-resolution tripwire) fires ONLY on the `None`/re-resolve branch; the `Some(recorded_implementor)` branch
  states verbatim "No determinism assertion: the record IS the source" (`classify.rs:2299`). So a wrong/forged
  `selected_implementor` is consumed by MIR with **zero cross-check**. Selection soundness today rests on
  **record integrity** (the record can only be set from an authorized `ScopedProvision` discharge), not on `Fix`
  consumption. Any budget-N that records a *scope-dependent* `Some` for a money-adjacent goal must either extend
  the determinism assertion to the `Some` branch or prove the record's provenance. This is the concrete near-term
  soundness obligation — more actionable than the abstract MTC end-state. (Lenses `coherence_migration`,
  `fit_and_sequence`.)
- **Generic-context `Fix` (skeptic-leak #5) — OPEN.** "Was the override authorized" becomes an instantiation
  property the determinism rail cannot certify (it certifies the selected `ImplementorId`, not cap-presence; and
  there is no field on `ImplementorOrigin`/`ImplementorId` to record cap-presence into — a representational gap).
  v1 MUST restrict `Fix<T>` to monomorphic/root contexts; the cascade observability is itself direct-call-only,
  so this restriction also matches what the substrate already supports. (Lenses `model`, `mint_capabilities`.)
- **Override-must-REPLACE-not-ADD (skeptic-leak #6) — OPEN, mechanism exists.** The proof forest deliberately
  seeks a 2nd solution (`proof_forest.rs`, `NeedsConfirmation` on >1), so a `Fix`-authorized override must be
  resolved at the verify-leg where it can REPLACE the candidate set (`Selection::Unique`), never inside the
  tracked solve. The default-tier picks at the verify-leg today — the seam is right.
- **Keystone id-stability — OPEN (the linchpin).** See §3.

---

## 3. Grounded implementability on CTFE + own/move; the keystone

**SETTLED runway.** CTFE bodies (const-fn) ARE move-checked (`borrowck/check.rs` iterates `Func` + `Const`),
giving affine `own` for ordinary bodies. The SELECT-side stable identity is **solved**: `ImplEnv.selected_implementor`
folds Some-only into the salsa `SemanticInstanceKey` AND the codegen symbol (`template.rs:66-140` + the matching
discriminator in `stable_key.rs`), consumed by the MIR C1 rail (`classify.rs:2306`). Derived impls already become
real `ImplementorOrigin::Hir` establishments today via the expansion executor.

**Two honest caveats on "enforcement reuses CTFE + own/move":**
- **Derivers SKIP move-checking** (`check.rs`, `if func.is_derive_provider_fn(db) { continue; }` — "checked by the
  provider executor instead"). So for the DERIVED half the affine floor has no live enforcer on the deriver body.
  Reconciliation (cheaper): consume `Fix` at the **executor-invoke / establish site**, not inside the exempt
  deriver body — making that site the single establish point for hand-written and derived alike.
- **"Linearity is free" overclaims** (REFUTED as "no new enforcement"): the *mechanism* (move-checker) is free,
  but the affine *typing* of `Fix` at its consumer is new — the (unbuilt) `fix` verb must take `own Fix<T>` and
  `Fix` must never be `Copy`. And it collides with the lane leak (§0-2).

**The keystone (OPEN — the precise keystone dependency).** Per SSOT lines 86-106, 158-169: run the
quasiquoter-executor as a downstream salsa query whose output is a real impl. The MINT-side identity is the one
open frontier: a generated impl's identity = `Body`'s `TrackedItemId`, and `TrackedItemVariant::ImplTrait(u32)`
is a **positional counter that restarts from zero in the expansion pass** (lens `substrate`: not merely
"runtime-only unprovable" but order-sensitive — reordering derive targets renumbers downstream ids, silently
re-keying `SemanticInstanceKey` and symbols). The trivially-byte-identical runway is **EXHAUSTED** (x-3a/b done
`f0dd2df8f`; x-3c/d DEFERRED). **Precise keystone dependency:** key the minted impl on a CONTENT key
(goal-trait-identity, self-ty, provider-identity), interned, NOT on the expansion ordinal; ship an id-equality
assertion as the soundness tripwire (SSOT line 165); add a fixture that reorders derive targets and asserts
identical symbols. **The keystone is independent of `Fix`** — `Fix` gates existence, identity is orthogonal — so
Half-B `Fix` wiring lands on the EXISTING cascade SELECT substrate with **no keystone dependency**. Do not couple
them. (Lenses `fit_and_sequence`, `substrate`, `model`.)

---

## 4. The capability-articulated, self-hosting mint tower (the central design choice)

The central choice is whether the per-goal mint policy is a **compiler dial** or is **itself articulated by
capabilities, self-hosted, bottoming out at root**. The model's answer: capabilities, with one hard
simplification forced by the adversarial filter.

**The tower (collapsed).** `Fix<G>` = the right to ESTABLISH one impl of `G`. The right to *produce* `Fix<G>` and
to *delegate* that right are higher rungs. **REFUTED as a reified recursive type family:** a literal
`Mint<Mint<…>>` nest is (a) un-enumerable by the kind checker and recognition (which keys one fixed name), and
(b) collides with the SSOT cliff law — "quantify-over-Constraint only at the kind level (instantiate-only, never
solve)" (SSOT lines 220-228). A recognized/solved `Mint<Fix<G>>` tower is solver-level quantification over
capability-indexed types, which is the ratified-forbidden side of the line. **Therefore the headline is the
2-type collapse, not the recursive tower:** one linear `Fix<G>` (consume-to-establish) + one `MintRight<G>`
modeled as a recorded grant-fact (which scope holds it, granted-from-where), with depth as **data on a finite
static grant chain**, never as type nesting. The full reified `Grant` layer is **demand-empty** today (no fixture
delegates minting) — design it, defer building it (`fco-guts-over-sugar`; `spike-outputs-are-inputs`).

**Where it bottoms out at root (SETTLED).** The regress terminates at the SOLE ambient origin in the tree:
`ProviderSource::RootProvider` (`core/semantic/mod.rs:445`, `:1668`), compiler-seeded, never an `Expr` — the same
anchor `seed_func_effect_witnesses` uses, the same anchor the FIX packet nominates for "minted at root"
(packet:24). Root holds the top of every tower by fiat; every non-root rung must have RECEIVED its mint-right
from above. This is the ocap "no ambient authority + authority by explicit grant" tenet (lens `prior_art`)
realized.

**The contract root is the delegation EDGE, not a second origin (SETTLED, per `FCO_AUTHORITY_GATED_OVERRIDE`).**
For backend-specific consensus goals (storage/ABI layout), root delegates establish-authority down to each
**deployed-contract root** (the EVM-correct one-storage-namespace coherence unit, AUTHORITY doc axis 4) and no
further. That delegation edge IS "contract = coherence root," expressed as a grant, honoring Micah's
MECHANISM-NOT-POLICY steer (`FCO_AUTHORITY_GATED_OVERRIDE_2026-06-17.md`): the core carries data
(scope/provenance/witness identity) + a clean seam; the policy is not a per-platform config knob. PREREQUISITE
(GAP 1): `ProvisionEnv` must retain the originating `ScopeId`/scope-chain (today the solver collapses to ingot at
entry); without it, cross-ingot mint authority has no referent finer than the ingot.

**`goal_is_canonical` dissolves into the default-policy table (SETTLED direction).** Keep the function and its
resolved-identity recognition, but read its return as a **default mint-policy** ("root-scarce for this `G`;
ambient-grant default for the rest") rather than a hardcoded canonical *marker*. Canonical-ness becomes a data
row in the delegation graph, not a special branch — satisfying "no special canonical marker." Do not delete it
until a delegation graph can be authored in-language (post-keystone). **Caveat (from `coherence_migration`):** the
live demotion is keyed on `implementor_is_default_marked`, NOT on a trait allowlist; so "restrict cascade to
control traits" is a NEW sibling predicate `cascade_eligible(trait)` consulted in the non-canonical branch,
distinct from `goal_is_canonical` (which forbids any second impl). Two predicates, two branches.

---

## 5. Coherence-as-budget migration that keeps the ~360 fixtures green

**This is a re-labeling + one already-landed demotion, not a rewrite (SETTLED).** The global trait-impl coherence
checker is a SINGLE production gate: `does_impl_trait_conflict` has exactly one non-test caller — the
`lowered_implementor` conflict loop (`mod.rs:4000`). Budget-accounting **REUSES** that predicate's overlap math
verbatim (instantiate-fresh-vars + unify + merged-constraint-sat) and only relabels the **verdict**
(>budget → exhaustion). It does NOT replace the overlap reasoning — which is precisely why blanket/conditional
impls (open families) are handled: budgets never try to count goal-instances, they count establishment events and
let the overlap relation decide collision. (Lens `coherence_migration`.)

**Conservative `NeedsConfirmation` (minor, by design).** `does_impl_trait_conflict` returns "no conflict" only on
`UnSat`/`ContainsInvalid`; indefinite-overlap falls through to "conflict." Over-reject is sound-for-money. The
`Fix` floor inherits this approximation; any "double-mint refused" diagnostic should reuse the coherence path so
the imprecision is single-sourced.

**Green-keeping (SETTLED).** All legacy `5-0001` fixtures are SAME-TIER overlaps (no `CoreDerives`-default
member), so `this_default == cand_default == false ⇒ conflict_allowed = false ⇒ 5-0001 still fires`. The
diagnostic story is a single-site choice at `Err(ImplTraitLowerError::Conflict)` (`mod.rs:4024`): **keep `5-0001`
verbatim** for overlap-exhaustion (zero snap churn) and **add ONE new greppable code** for the genuinely-new
behavior (override of a fixed goal without holding `Fix`) so every money-swap denial is auditable
(`burndown-value-is-surface-area`: a rename for its own sake is churn with no surface-area win).

**Orphan/locality CANNOT migrate (SETTLED).** `trait_impl_admissibility` + `is_sealed_local_marker_anchor`
(`mod.rs:4097-4148`, run BEFORE the conflict loop) answer WHERE an impl may exist (which ingot owns the
trait/type, `5-0000`/`5-0004`), orthogonal to HOW MANY. Leave it as a separate placement pass. It may be *framed*
as a precondition on `Fix`-eligibility ("you may only consume a `Fix<G>` for a (trait,type) you locally own or
anchor"), but that is documentation, NOT a code move.

**Doc reconciliation (REFUTED stale comment).** `goal_is_canonical`'s doc comment (`trait_def.rs:871-882`) still
describes C3c-3 demotion as a FUTURE increment and both branches as byte-identical; the LIVE code
(`mod.rs:4006-4022`) and `cascade_noncanonical_coexist.fe` show demotion is shipped. Cite the fixture + code, not
the comment, as status SSOT; update the comment when wiring (`reverify-inherited-blockers`).

---

## 6. Fit with the built cascade + build sequencing

**The cascade is the model's SELECT/identity substrate — REUSE wholesale, do not rework (SETTLED).** Landed C1
→ C3d (HEAD `da27d0983`); three green fixtures (`cascade_scoped_override.fe`, `cascade_noncanonical_coexist.fe`,
`cascade_unscoped_default.fe`) under the real `test_fe_test` runner. The cascade = budget-N-ambient (the 2-slot
default+override shape); `goal_is_canonical` = budget-1-root-scarce, already live; `Fix<T>` = the inert authority
token whose only missing wiring is mint-at-root + the override gate. The override-records-distinct-`ImplementorId`
invariant (FIX packet 4.2) is ALREADY satisfied by Some-only identity. Nothing in the cascade contradicts the
model.

**The live cascade rides `scoped_selection_exprs` / `MethodSelection`, NOT the Evidence discharge walk
(REFUTED).** `discharge_from_scoped_provision` fires only on `snapshot_evidence_provisions()`, which is documented
ALWAYS-EMPTY ("TODAY the snapshot is always empty," `ty_check/mod.rs:1349`). So "a `Fix<G>` provision rides the
Evidence discharge walk" (lens `mint_capabilities`) points at the **inert** seam. A `Fix` establishment/selection
must enter via the LIVE qualified-path / `MethodSelection` seam (`scoped_selection_exprs`, `env.rs:120`;
call-keyed selection read per-call from discharged obligations), OR `fix`-recognition must be added to the
snapshot filter as a `fix_witnessed_goal` sibling. Do not ground the design on the empty path.

**Existence-vs-override site (the unresolved tension, OPEN).** The reframe "establish-one-impl token" wants the
check at `lowered_implementor` (a salsa-TRACKED lowering query); the FIX packet's override gate (and its
cache-safety) lives at the **verify-leg** (outside the tracked solve). You cannot have both. **Resolution for
v1:** keep EXISTENCE-scarcity at the coherence query (canonical floor, where it already is) and put `Fix`
OVERRIDE-SELECTION at the verify-leg (where the packet safely puts it). The unified "establishment event" is a
*conceptual* unification; it is NOT a single enforcement site. Re-prove salsa-safety for whichever site each
check lands at; keep `ingot_trait_env` / `impls_for_trait_def` Fix-free and ingot-pure (skeptic-leak #7).

**Build sequencing (the ordering law is binding — SSOT lines 131-138).**
1. (Prereq) Confirm the `Some(None)` forge floor is closed on ALL recording paths (the `with`-provision path may
   still record `None`); extend the determinism assertion to the `Some` branch or prove record provenance (§2).
2. Wire the LIVE canonical floor to the `Fix` gate: turn `goal_is_canonical`'s "always reject 2nd canonical impl"
   into "reject UNLESS scope holds `Fix<T>`," consuming the inert `scope_is_fix_capability` (delete its
   `allow(dead_code)`); byte-identical-first (compute `Fix`-presence, branch nothing, prove consumption — mirror
   C3c-1).
3. Mint-at-root via the `RootProvider` anchor with **NON-AMBIENT** propagation (close skeptic-leak #2); pick the
   `Fix` borrowck lane (§8-D2) and add the double-consume fixture.
4. The `fix` verb + attenuation lattice + FV (the money parts).
5. Coherence-checker DELETION **LAST** — only after the `Fix` gate proves the floor holds without it, AND the
   replacement still rejects overlap for two non-default user impls (`coherence_migration` new hole: deletion has
   TWO obligations, not one).
6. Budget-accounting is a REFRAME/doc layer over existing dials, not a build step.

The keystone runs in PARALLEL as a long-pole (blocked on `Body` identity); derive-grammar retirement (#74) is
GATED on the keystone. Half-A delivers the six→one headline on its own; Half-B is the safety rail, sequenced
second (SSOT lines 199-202).

---

## 7. Prior-art lessons + the hazards to refuse

(Full mapping in lens `prior_art`; the load-bearing distillation.)

- **Rust/Haskell global coherence** exists so a structure with a global invariant trusts one `Hash`/`Ord` per
  type. BORROW: keep budget-1 for any trait a std container's invariant depends on. REFUSE: budget>1 *ambient*
  for such traits. Fe's `goal_is_canonical` is the right shape; its v1 set omits `Ord`/`Hash` deliberately
  (collection-key risk is the Dial-2 latent gate, §1.4).
- **Scala implicits = the cascade** (budget-N lexically-scoped, incoherent by design; the
  TreeMap-with-two-Orderings bug). BORROW: companion-priority default, ambiguity-is-error. REFUSE: shipping
  incoherence for ALL types — Fe adds the `Fix` gate + money-floor Scala lacks.
- **Modular Type Classes (Dreyer/Harper 2007) + ML functors** = the soundness condition for budget-N: the
  structure must CARRY the instance (instance-in-type-identity) so two instances cannot silently merge. Fe's
  Some-only key is the *codegen half*; the *type-level* half is missing — this is exactly the Dial-2 end-state
  (Construct-3 / witness-capture). Name it; do not pretend the cascade already meets it.
- **Coq/Lean canonical structures** = budget-1-with-a-deterministic-priority-ladder. BORROW: total deterministic
  default order (CoreDerives-origin, never declaration order) + ambiguity is a clean diag, never a panic.
- **Object-capability + linear/affine types** = direct prior art for `Fix`. BORROW: unforgeability, no-ambient,
  attenuation. CORRECT the thesis: AFFINE, not strictly linear.
- **Staged metaprogramming (Template Haskell `Q`, Racket phases)** = why "gate EXISTENCE not USE" is sound: a
  phase distinction; the deriver runs OUTSIDE the solver.

**Hazards to REFUSE:** ambient `Fix` (voids the whole guarantee); a reified recursive `Mint` tower (cliff-law
violation); claiming the move-checker enforces canonical establishment (it does not — two layers); claiming
budget-N-arbitrary (the gate is 2-slot); shipping `Ord`/`Hash` cascade-override into a shared container without
Dial 2; coupling Half-B `Fix` wiring to the keystone.

---

## 8. DECISION LEDGER (open knobs → recommendation + blast radius)

Decisions are deferred-tunable defaults (`fco-guts-over-sugar`: default the policy, never the proof). **D1 first.**

### D1 — The mint-policy DEFAULT (the dial's resting position) — FIRST
- **Options:** (a) cosmetic-everywhere (budget-N for all, Scala's mistake); (b) **scarce-for-canonical /
  ambient-for-the-rest** (the live `goal_is_canonical` shape, read as default policy); (c) scarce-for-everything
  (no cascade).
- **Recommendation:** (b). It is the LIVE behavior (`mod.rs:4001-4022`), keeps ~360 fixtures green, and is the
  EVM-correct floor (layout traits scarce, customization traits open). Read `goal_is_canonical` as the default
  mint-policy table; keep `Ord`/`Hash` non-canonical BUT gate the first `Ord`/`Hash`-keyed shared container on
  Dial 2 (§1.4).
- **Blast radius:** LOW now (it is current behavior); the only forward risk is the latent Dial-2 gate — bounded,
  pre-positioned, fires only when stdlib/users add a keyed container.

### D2 — The `Fix` borrowck LANE (the §0-2 leak) — BLOCKER-class
- **Options:** (a) ordinary linear value (keep `Fix` OUT of `as_capability`/provider-binding lowering; consume as
  a normal `own` arg — the `Boxed`-fixture path that already works); (b) capability/provider-binding (needed for
  threading) but NARROW the `local_has_runtime_move_semantics` exemption so a `Fix`-kind capability stays
  consume-once-tracked; (c) accept the floor is structural (coherence checker) and `Fix` is a pure body-level
  override authorizer never recorded on the impl.
- **Recommendation:** (a) for v1 — consume `own Fix<T>` at the establish/`fix` site as an ordinary value so the
  existing move-checker covers it; reserve (b) only if threading forces capability-ness. Add the double-consume
  fixture AFTER plumbing.
- **Blast radius:** MEDIUM. (a) is contained; (b) touches the hot `local_has_runtime_move_semantics` path and the
  provider-binding lowering — measure before flipping.

### D3 — Where `Fix` is CONSUMED (establish-site vs deriver body)
- **Options:** (a) establish-site (executor-invoke + `lowered_implementor` gate) — ONE ledger, executor stays a
  backend, deriver bodies keep their move-check exemption; (b) inside the body + re-route derivers through
  borrowck; (c) both (status quo two-mechanism).
- **Recommendation:** (a). Single convergence point for hand-written and derived; reuses `goal_is_canonical` as
  the ledger; keeps the executor a backend (SSOT "stage, don't fuse").
- **Blast radius:** LOW-MEDIUM. (a) adds a check at one site; (b) would re-enable borrowck for derivers
  (larger).

### D4 — How `Fix` PROPAGATES (the ambient-capture crux) — money-soundness
- **Options:** (a) ambient `EffectEnv` frame (REJECT — voids root-scarcity); (b) explicit thread
  `uses (_: Fix<T>)`; (c) `Barrier`-excluded frame.
- **Recommendation:** (b) as the model, with (c) enforcing the ambient walk cannot leak it — note
  `snapshot_provisions` currently drops barriers, so (c) is NEW walk logic (§2).
- **Blast radius:** HIGH if wrong (a single ambient mistake = money hole). The mechanism is unbuilt; treat as
  deliberate-care.

### D5 — Budget UNIT + SHAPE
- **Options unit:** per-(trait,type) pair (naive, REFUTED for generics); per-`ImplementorId`; per-(trait-def,
  self-type-head). **Options shape:** integer-N (REFUTED — gate is 2-slot); keep 2-slot {default, override}.
- **Recommendation:** consume per-`ImplementorId`; decide scarcity per-(trait-def, self-type-head) via
  `does_impl_trait_conflict`, mirroring `goal_is_canonical`'s per-def discipline (so assoc-bindings/generics don't
  fragment or dodge). Keep the 2-slot shape; defer integer-N (demand-empty, selection-blocked).
- **Blast radius:** LOW (descriptive; the code already counts this way).

### D6 — Generic-context `Fix` in v1
- **Options:** allow now; restrict to monomorphic/root; serialize cap-presence into `stable_key.rs`.
- **Recommendation:** restrict to monomorphic/root in v1 (matches the determinism rail's certification, the
  representational gap on `ImplementorId`, AND the cascade's direct-call-only observability).
- **Blast radius:** LOW (a restriction, not new machinery); lifting later is the heavier path.

### D7 — Diagnostic for the new behavior
- **Options:** keep `5-0001` for everything; rename `5-0001`; keep `5-0001` for overlap-exhaustion + ONE new code
  for fixed-override-without-`Fix`.
- **Recommendation:** the third — zero snap churn on ~360 fixtures, one greppable money-swap-denial code.
- **Blast radius:** LOW.

### D8 — "The root" for cross-ingot / orphan-adjacent canonical impls
- **Options:** global universe; trait-home ingot (std); deployed-contract root (`RootProvider`/`VirtualContract`
  anchor).
- **Recommendation:** compiler-seeded universal root as the tower BASE; deployed-contract root as the delegation
  EDGE for layout/consensus goals. PREREQUISITE: GAP 1 (`ProvisionEnv` retains originating `ScopeId`).
- **Blast radius:** MEDIUM — GAP 1 touches solver entry; defer as policy, but the substrate must carry the scope
  chain.

### D9 — Coherence-checker deletion timing
- **Options:** delete after the demotion (already done); delete after the `Fix` gate proves the floor; never
  delete (keep as placement).
- **Recommendation:** delete the global `ConflictTraitImpl` path LAST, only after the `Fix` gate holds the floor
  AND the replacement still rejects two non-default user-impl overlaps. Locality (`5-0000`/`5-0004`) stays.
- **Blast radius:** HIGH if premature (money window). Sequence per the ordering law.

---

## 9. Where the thesis breaks (honest summary)

- **"One unified `Fix`-consumption-at-establishment" — BREAKS.** It is two layers (coherence checker for
  existence; move-checker for the override token). Ship the correspondence, not the fusion.
- **"Linearity enforced by the existing move-checker" — BREAKS twice.** Capability-ness flips the floor OFF
  (§0-2); derivers skip borrowck (§3). The move-checker is the free *mechanism*, not free *enforcement*.
- **"Budget = N tokens minted" — BREAKS.** The live gate is a 2-slot {default, override} shape, not an integer
  counter; arbitrary N is demand-empty and selection-blocked.
- **"`Fix` provision rides the discharge walk" — BREAKS.** That walk is inert; the live cascade is
  `MethodSelection`.
- **"Recursive self-hosting `Mint` tower" — BREAKS at the cliff law.** Collapse to 2 types + grant-as-data.
- **Use-site uniformity "blocker" — DOWNGRADED.** Not reachable today (no keyed container; `StorageKey` floored;
  per-direct-call selection). A pre-positioned gate, not a shipping blocker.

What does NOT break: affine `own` as the floor's substrate; `RootProvider` as the sole ambient origin; the
cascade as the SELECT substrate; `goal_is_canonical` as the live dial; the keystone as the orthogonal open
linchpin; the cliff law; the two-dial intellectual frame. The unification is real as *vocabulary + sequencing +
the two-layer/two-dial honesty*; it is not a single mechanical event.
