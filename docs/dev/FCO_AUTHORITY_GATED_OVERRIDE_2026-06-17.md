# Authority-Gated Provision Override — Design Exploration + Decision Menu

**Date:** 2026-06-17 · **Status:** design exploration (self-review-via-design-wizard; human
architect gone). NON-BLOCKING — public-provision-layer policy, gated behind "no public syntax
yet." Feeds a Micah-gut decision. Read-only; load-bearing citations spot-verified (✓).

**Reframe:** the ratified synthesis treats consensus soundness as binary (canonical = no override
/ scoped = free shadow). Micah's steer rejects the binary: *"we might WANT those changeable, but
gate WHERE and WHO can override; sometimes ensure a provider comes FROM ABOVE and isn't tampered;
other times allow overrides."* That's a third thing — **authority-gated override** — = Fe's
capability/grant model applied to provisions.

## 5 design axes
1. **Subject:** per-trait (sealed `StorageKey`) · per-provision · **per-use-site** (the consumer
   that does the dangerous op demands a property of whatever witness resolves — Move "abilities
   are structural"; strongest, attaches the rule to the money op not a remembered annotation).
2. **Locus of authority:** property-of-overriding-scope ("you may override by *where* you are") ·
   capability-token ("you hold a grant from above") · defining-scope-policy (`#[sealed]`).
3. **"From above":** tamper-**prevention** (closer shadow structurally disallowed — Fe already has
   the `Barrier` primitive, ✓ `effect_env.rs` `KeyedEffectEntry::Barrier`/`blocked_by_barrier`) vs
   tamper-**evidence** (shadow allowed but the consumer can check provenance). Prevention = simple,
   audit-trivial; evidence = the only one that honors "sometimes allow overrides."
4. **Who is "above" (coherence root):** global · trait-home-ingot (`std`) · **deployed-contract
   root** (the EVM-correct unit — one contract = one storage namespace; the only root that lets
   "override but from above" be non-contradictory, resolving the foreign-foreign deadlock).
5. **Default posture:** sealed-default-for-layout (crypto safe-default) vs open-default
   (ergonomics) — best derived from a *property* (layout-canonical ⇒ sealed) not a remembered mark.

## 4 options (sketches; NO public syntax authorized yet)
- **A — sealed/open markers, override-authority ≥ sealing-scope.** Rust `sealed trait` idiom;
  prevention; cheap; app *cannot* shadow below the sealing scope. Con: binary at trait level.
- **B — override capability token granted from above.** Grant model; actionable error. **Wrong
  primitive for consensus**: a lexical token guarantees only the *block*, not whole-contract
  agreement (a token in `set` but not `get` reproduces the bug). Fine spelling for *control* caps.
- **C — consumer-side "from above" demand at the use-site.** Trait stays open; the slot-derivation
  op (`storage_map.fe:131-157`) demands `K: StorageKey from contract-root`; compile error if the
  witness resolved from a closer shadow. Strongest+most-permissive. Heaviest substrate ask
  (needs witness provenance to survive monomorphization).
- **D — hybrid (recommended):** open/companion default; layout-canonical set is **property-derived**
  (incl. `StorageKey`); coherence root = **deployed contract** (override allowed at/above contract
  root → uniform across the contract); enforcement = C's use-site demand, with A's prevention
  barrier as the cheap first-ship fallback. **Only option holding all 3 of Micah's clauses.**

## Recommendation: ship Option-D hybrid, sequenced A → C
1. **A first** (cheap sound floor, uses the existing `Barrier`, contract-root not global) — = the
   synthesis's "canonical markers now," CORRECTED on two points the review found: the protected set
   is **property-derived and must include `StorageKey`** (the live consensus witness, ✓
   `storage_map.fe:7-11`; Ord/Hash/ABI alone miss it), and the root is the **contract, not the
   universe** (so override-from-above stays expressible).
2. **C later** (use-site provenance demand) once the substrate carries provenance — this is what
   makes "sometimes allow overrides" real and safe.
Grounded in: grant-not-deny model (DEEP-LORE #10), Christoph "authority visible in the signature,"
security ref "no dynamic dispatch unless annotated" (a silent provision override IS dynamic
dispatch over money). Keeps settled invariants (projection-not-ConstraintTerm, abstract-head
shelved, solve-line).

## Forward-compat — the ONLY thing touching current work (ProvisionEnv v0 carry-list)
Witness provenance LARGELY already exists: `TraitGoalSolution { inst, implementor: ImplementorId }`
with `ImplementorOrigin = Hir | VirtualContract | Assumption`; from `Hir(impl_trait)` you recover
top_mod/ingot, scope, `attributes` (where a `#[sealed]`/`#[canonical]` marker lives), derive-origin
→ "who authored / from what scope / with what authority" all have a home TODAY. So A/B/C/D layer as
changes to `solve_cx()`'s body + new predicates, NOT a witness-type or consumer rewrite.
- **GAP 1 (ACTIONABLE — changes the ProvisionEnv v0 spec):** scope granularity is DISCARDED at
  solver entry — ✓ `TraitSolveCx::new` does `origin_ingot: scope.ingot(db)` (mod.rs:117),
  collapsing `ScopeId → Ingot`. The gradation ladder + override-gating need the scope chain.
  **ProvisionEnv v0 MUST retain the originating `ScopeId`/scope-chain** (even if v0 still collapses
  to ingot internally), so finer tiers later change `solve_cx()`'s body, not every caller. Cheap
  now, expensive to retrofit. (Supersedes the review B-section's v0 field set: it's
  `{scope: ScopeId, assumptions}`, NOT ingot.)
- **GAP 2:** the 3 envs (flat `assumptions`, scope-indexed `effect_env`, ingot table) don't consult
  each other; convergence is by-flattening (`env.rs:319`) which destroys scope structure. Do NOT
  deepen flattening; do NOT fold `effect_env` in v0 (may be undecidable without re-architecting the
  proof forest — the real "one resolver" feasibility gate; wants a Lean/PoC spike).

## Gut-questions for Micah (genuine human calls — not blocking)
1. Money-floor: prevention (A, override can't happen below contract root — denies custom encodings)
   or evidence (C, override allowed + compiler proves provenance — flexible, heavier)? Rec: A→C.
2. Default posture for layout traits: sealed or open? Rec: derive from the layout-canonical property.
3. Coherence root for overridable consensus traits: global / std-ingot / **deployed-contract**?
   Rec: contract. (Or should libraries pin a canonical the contract can't touch?)
4. Is `StorageKey` protected, and is the set enumerated or property-derived? Rec: property-derived +
   name StorageKey.
5. Override authority = scope-property (D) or held-token (B)? "A place you stand" vs "a value that
   flows / attenuatable." Taste call.

## Needs a spike / FV (not resolvable by reading)
- Does witness provenance (C's `from contract-root`) survive monomorphization/generic propagation?
- Is folding `effect_env`'s frames into the proof forest even decidable without re-architecting it?
  (the real "one resolver" gate.)
- `BTreeMap<MyKey>`/`StorageMap<MyKey>` worked example: which provisions get dragged canonical
  transitively (sizes how much "free middle tier" the policy actually loses).
