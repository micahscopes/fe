# PS-MR — Sizing & Decision Record

**Date:** 2026-06-17 · **Status:** read-only sizing spike (no code change), citations
spot-verified in-tree. Decision: **pursue framing-2 (solver-verified-obligation sidestep) to
complete TD5.2b**; framing-1 (general method-resolution dedup) is the deferred general fix, to be
witness-keyed when done. Resolved via doc-scrutiny + spike under the post-architect methodology
(no human architect; see memory `fco-autonomy-protocol`).

## What PS-MR is
A provider-generated impl carrying `where <Concrete>: Trait` (from a concrete
`builder.require<AbiSize>(u256)`) produces `8-0026` "multiple trait candidates" for
`self.field.<method>()` in the generated body. TD5.2 (`cfce8d366`) deliberately SKIPS concrete-
require predicates because of this; the skip is the open part of **TD5.2b**.

## Root cause (VERIFIED)
Method-call candidate assembly (`method_selection.rs::assemble_trait_method_candidates`) puts
impl candidates and where-assumption candidates into one undifferentiated set; the assumption
loop (~`:218-241`) is textually adjacent to the PS2/PS3 impl-gating (~`:170-216`). The existing
dedups all key on `Canonical<TraitInstId>` — **verified** `ty_check/mod.rs:978` & `:993`
(`dedup_equivalent_{trait_solutions,pending_method_candidates}`) and the `select_trait_methods`
collapse. The bare assumption inst carries **no assoc bindings** while the impl-derived inst does
(`provider_synthesis.rs` `requirement_where_clause` doc), so the two `TraitInstId`s differ →
canonical-keyed dedup misses them → both survive → `8-0026`. Only CONCRETE heads break (a generic
param has no competing impl, so its where-bound is the sole route — fine).

## Why framing-2 is sound *by construction* (the keystone, VERIFIED)
`proof_forest.rs:373` — `goal_needs_assumptions = goal.args.any(has_param || has_var || AssocTy ||
QualifiedTy)`; assumptions are consulted only `if goal_needs_assumptions` (`:422`). **A fully
concrete goal (`NoAbi: AbiSize`) makes this false → the solver discharges purely via the impl;
the where-assumption buys the solver nothing and only pollutes method resolution.** So carrying a
concrete `require` as a solver-VERIFIED obligation (prove `Concrete: Trait` dischargeable) — never
as a param-env assumption — cannot create the phantom candidate. Precedent (VERIFIED):
`analysis/ty/provider_goal.rs` already lowers concrete constraints to
`CapabilityGoal::ConcreteTrait(TraitInstId)` via `lower_hir_constraint_application`, explicitly
"never reaches the solver as a live head" (`:25`, `:243-244`). The "verify don't assume" queue
exists: `TraitObligation`/`DeferredTask::Obligation` → `process_trait_obligation`
(`ty_check/mod.rs:1258`) → `is_goal_query_satisfiable` → `6-0003`.

## Costed comparison

| | **Framing-1: general dedup** | **Framing-2: solver-verified sidestep (RECOMMENDED)** |
|---|---|---|
| Touch points | `assemble_trait_method_candidates` / `select_trait_methods` + provenance field | lower each concrete `ProviderEffect::Require` to `TraitInstId`, check via solver/`check_trait_inst_wf`; reuses `lower_hir_constraint_application` + `provider_goal.rs` shape |
| Size | ~20–40 LOC mechanical, high policy surface | ~40–60 LOC, additive; one new fn + one call site |
| Blast radius | **High** — universal method-call path (whole corpus) | **Low/contained** — provider-synthesis path only; zero change to general method resolution |
| Soundness | OK only if keyed on solver-resolved witness, not bare `TraitInstId` (else coherence bridge) | sound by construction — concrete goal verified, never assumed |
| Provision-scoping fwd-compat | becomes coherence-bridge unless witness-keyed (Q4) | naturally fwd-compatible ("is it dischargeable?" is what scope resolution answers) |
| Unblocks TD5.2b? | yes (broad) | **yes** (concrete require enforced, `6-0003` on fail, no `8-0026`); leaves general dedup unsolved (fine — not needed) |

## Forward-compat note (Q4, VERIFIED machinery)
The framing-1 "drop assumption, use *the* impl" assumes one-impl-per-head (global coherence). The
solver already names the committed witness: `TraitGoalSolution { inst, implementor: ImplementorId }`
(`trait_resolution/mod.rs`), `ImplementorOrigin = Hir | VirtualContract | Assumption`. So when
framing-1 is eventually done it must key on the resolved `ImplementorId` **witness**, not on
"an impl exists" — that survives provision scoping (companion/scoped overlays) instead of being a
coherence-assuming bridge.

## Decision
- **TD5.2b completion = framing-2.** Implement after the current TD5c step (one step at a time).
- **Framing-1 (general dedup) = deferred**, witness-keyed when taken; it is the general home for
  assumption-vs-impl dedup and aligns with the eventual `ProvisionEnv` resolver.
- **PS-MR is NOT solved inside TD5** (framing-2 sidesteps it; framing-1 stays separate).

## The one thing a build-probe must confirm before framing-2 lands
Where the generated provider impl is run through a WF/obligation check today (so we know whether
framing-2 is "add a call" vs "add a call site"). A one-shot throwaway probe (debug-print the two
diverging insts on the array/tuple-field reproducer) also confirms the root-cause inequality and
that a witness-keyed dedup would collapse them. Do this as the first move of the framing-2 step.

## Citation accuracy note
The spike's structural claims are all verified in-tree; two citations were mis-pathed/mis-lined by
the agent (`provider_goal.rs` is under `analysis/ty/`, not `ty_check/`) — corrected here. Pattern:
these agents are reliable on structure, loose on exact paths/lines → always spot-verify before
acting (the standing discipline).
