# Provision Scoping — Design-Wizard Architect Review + ProvisionEnv v0 Design

> **SUPERSEDED (ProvisionEnv landed) → `PROVISION_SCOPING_SYNTHESIS_2026-06-17.md` (ratified) + `FCO_THE_SLIDE_2026-06-19.md` (SSOT) / `FCO_MAP.md`.** The `ProvisionEnv` v0 reviewed here landed via rung 3 (as a read-wrapper over `TraitSolveCx`); the scoping decisions were ratified in the synthesis doc. Historical review. SSOT: `FCO_THE_SLIDE_2026-06-19.md`.

**Date:** 2026-06-17 · **Status:** architect-review (the human GPT-Pro architect is gone;
this is the self-review-via-design-wizard mechanism per memory `fco-autonomy-protocol`).
Read-only, no code change. Load-bearing citations spot-verified in-tree (noted ✓).

## TASK A — review of the 5 ratified provision decisions
Decisions (1) companion default, (2) global≡canonical, (4) orphan-demoted-to-canonical-rule,
(5) one ProvisionEnv **ship as-is**. Decision (3) (canonical-only markers) needs a REVISE.

### A1 — CRITICAL · REVISE decision (3): the marker family omits `StorageKey`, Fe's real consensus witness — **OPEN, money-soundness, needs Micah's gut**
The synthesis frames the consensus hazard around `Ord`/`Hash`/ABI. But the persistent-state-layout
witness in Fe is **`trait StorageKey { fn write_key(ptr, self) -> u256 }`** (✓ `ingots/std/src/evm/
storage_map.fe:7-11`) — `write_key` computes the storage-slot preimage (`keccak256(key++slot)`). It
is user-extensible, and it is **not** Ord/Hash/ABI and **not** in the proposed marker set. Two
shadowing `provide impl StorageKey for MyId` (slot-derivation v1 vs v2) → writer and reader resolve
**different** witnesses → reads land on the wrong slot → **silent fund loss**, invisible to audit
(the bug is *which witness resolved*, not in source). And there is **no canonical-marker mechanism
in the compiler today** (✓ grep empty; "canonical" in-tree = only `Canonical<T>` inference machinery;
`AbiSize`'s one-provider-by-name at `provider.rs:1482-1548` is a manual stand-in, weaker than "we
run this in miniature" implies).
- **Recommendation (OPEN — money-soundness ⇒ Micah-gut, not auto-applied):** make the canonical set
  **property-derived, not an enumerated trait list** — *any trait whose witness participates in
  persistent-state layout or cross-call/cross-verifier determinism is canonical-by-construction* —
  and name `StorageKey` explicitly alongside the ABI family. Stronger: enforce at the **layout USE
  site** (a trait used in a storage-slot/ABI-layout position must resolve to a canonical EvidenceId),
  so the constraint attaches to the use, not a remembered annotation (Move "abilities are structural"
  instinct).

### A2 — HIGH · temper the "free middle tiers" claim
Marking Ord/Hash canonical drags them canonical **transitively**: anything in a sorted/hashed
collection forces `K: Ord` canonical; `#[derive(Eq)]` over an `Ord` field forces the field's `Ord`
canonical (super-trait/assoc-bound closure is aggressive — `extend_all_bounds`, `trait_resolution/
mod.rs:514-569`). So "free middle tiers" = everyday *minus* anything touching a sorted/hashed/storage
collection — which in a contract stdlib is most of it. This recreates Rust's orphan pain exactly for
the most-wanted-generic traits. Decisions survive; the *claim* should be tempered + a `BTreeMap<MyKey>`
worked example added showing which provisions get forced canonical.

### A3 — HIGH · under-specified seam: foreign-foreign canonical is contradictory — **OPEN, Micah-gut**
(1)+(2)+(4) compose to: who may author the canonical `StorageKey` (foreign trait) for `MyKey` (foreign
type)? Orphan-at-canonical (4) forbids both `std` and the app — but A1 says it *must* be canonical.
PS-DOC's clean foreign-foreign answer ("scope-local, can't pollute") is exactly the answer A1 proves
*unsound* for layout traits. **Proposed fork:** canonical provisions for layout traits are
**contract-scoped canonical** (the deployed contract is the coherence boundary — the right unit for
EVM persistent state; OCaml generative-functor shape), not *global* canonical. Genuine fork, touches
canonical-tier semantics ⇒ Micah-gut.

### A4 — MEDIUM · the central Task-B risk: 3 environments still don't consult each other
Verified separate stores: flat `assumptions: PredicateListId` (env.rs:77, the only thing the solver
reads), scope-indexed `effect_env: EffectEnv` (effect_env.rs frames, innermost-first — **the solver
never reads it**), and the ingot impl table (trait_def.rs:70). Today's "convergence" is
convergence-**by-flattening** (`effect_bounds` → `assumptions`, ✓ env.rs:319), which *loses* scope
structure — the opposite of the scope-indexed env the ladder needs. This is open sub-Q6 and it is the
real feasibility gate on "one resolver." **Make it the FIRST thing v0 exercises, not the last.**

### A5 — MEDIUM follow-up · the "485 impls" migration figure is understated
Direct count is materially higher than 485 (≈970–1350 depending on match; the doc's 485 is stale).
Migration still mostly mechanical (companion = today's `impl`), but re-baseline + partition into
{textless | needs-canonical-marker (A1/A2) | needs-foreign-foreign-policy (A3)}.

### A6 — LOW · don't mark tunneling "settled": innermost-wins silently shadows across tiers
For *type-indexed* provisions the effect-tunneling/accidental-interception hazard (DEEP-LORE #6,
Zhang & Myers POPL 2019) genuinely reopens — the fixed-effect-set defense applies only to *control*
effects. §8 currently dismisses it on grounds that don't hold here. Reserve a "provision shadowed
here" compiler note (don't block).

### A7 — LOW · terminology: (2) and (3) conflate canonical-as-*scope* vs canonical-as-*policy*
A3's contract-scoped canonical is policy-canonical but not outermost-scope. Keep two concepts:
coherence-root *scope* vs canonical *policy*. (Root of why A3 reads as a contradiction.)

## TASK B — internal `ProvisionEnv` v0 (vetted, implementer-ready; the authorized debt-negative step)
**B1 — pathway map (✓):** 6 real pathways — global tabled solver + param-env assumption (one proof-
forest engine, two candidate loops, `proof_forest.rs`), keyed effect env (`effect_env.rs`, solver
never reads), effect-bounds→assumptions (env.rs:319), provider-goal lowering (`provider_goal.rs`),
const-predicate prover (mod.rs:1394). The de-facto read-context already exists: `TraitSolveCx::new(db,
scope).with_assumptions(assumptions)` repeated at ~20 sites — **that IS the embryonic ProvisionEnv**,
built ad-hoc, carrying only 2 of the 3 environments.

**B2 — migrate first: `process_trait_obligation`** (✓ `ty_check/mod.rs:1258`, inline `TraitSolveCx`
build at `:1278`). Hot path (the body-checker obligation drain), cleanest de-bless, and it sits at the
A4 seam (reads `assumptions`, ignores `effect_env`) — so routing it through `ProvisionEnv` is the wedge
that later folds in the scoped env. NOT the ~20 leaf `TraitSolveCx::new` sites first (that'd be facade).

**B3 — shape:** `ProvisionEnv { scope, assumptions }` read-wrapper + `.solve_cx(db) -> TraitSolveCx`
(the **single** place a (scope, assumptions, [later] frames) triple collapses to solver inputs) +
`TyCheckEnv::provision_env()`. The migrated site holds a `ProvisionEnv` instead of hand-building the
context — byte-identical solver inputs.

**B4 — de-bless (no facade):** delete the inline `TraitSolveCx::new` from the body checker, centralize
in `solve_cx()`; add a GUARD fixture ("no new `TraitSolveCx::new` in the body checker; route through
`provision_env()`"). Real consolidation of a 20-site pattern's first/hot consumer, not a wrapper layer.

**B5 — forward-compat (by construction):** consumers only call `.solve_cx()`/`.assumptions()`, so later
adding a `frames`/`scope_chain` field (the gradation ladder + folding `effect_env`, A4) is a change to
`solve_cx()`'s body, not to consumers; the witness for PS-MR Q4 is already in `TraitGoalSolution.
implementor` (✓ `trait_resolution/mod.rs`); a `is_canonical(trait_)` predicate (A1) has a home.

**B6 — risks/test/size:** R2 = do NOT fold `effect_env` in v0 (scope creep; A4 is hard/open) — read-
interface only. Test = full gates (cli_output + fe_test_runner + ty_check + fe-hir + `fe check ingots/
std`) byte-identical + M5 discharge-count standard + the de-bless guard. **~25–40 LOC net, debt-
negative** (centralizes the first/hot consumer; remaining ~19 sites become smaller follow-ups).

## Verification + confidence
A1 (StorageKey + no marker mechanism), B1/B2 (pathways + migration site), A4 (3-env divergence),
PS-MR-relevant witness — all spot-verified ✓. A5 exact count was loose (agent 757/591 vs my 560/411;
claim "much >485" holds). Build-only unknowns (flag for throwaway probe): B2 byte-identical-at-runtime;
whether folding `effect_env` into the solver is even decidable without re-architecting the proof
forest (the real "one resolver" feasibility gate — wants a Lean/PoC spike before the synthesis calls
it settled).

## Net actions
- **OPEN for Micah's gut (money-soundness / canonical-tier semantics):** A1 (property-derived markers +
  `StorageKey`; enforce-at-layout-site?) and A3 (contract-scoped canonical for layout traits). These
  REVISE/clarify ratified decision (3) — do NOT auto-apply.
- **Temper (doc edits):** A2 (free-tiers claim), A5 (485→~970–1350), A6 (un-settle tunneling), A7 (two
  "canonical" senses).
- **Ready to implement (authorized, after TD5c):** `ProvisionEnv` v0 per Task B (migrate
  `process_trait_obligation` first).
