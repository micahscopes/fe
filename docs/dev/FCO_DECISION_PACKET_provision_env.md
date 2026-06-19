# Architect-decision packet — Scoped ProvisionEnv (PS1, the #2 prerequisite)

> **SUPERSEDED — `ProvisionEnv` landed via rung 3 (`FCO_THE_SLIDE_2026-06-19.md` step 1), as a read-wrapper over `TraitSolveCx`, NOT "grown from `EffectEnv`" as proposed here.** Historical decision packet only.

**Status: DECISION/CO-DESIGN REQUESTED. Prepared 2026-06-14 by the implementor;
design only, no code.** This is the second of the two big prerequisites
(the first is `FCO_DECISION_PACKET_typed_provider_capabilities.md`). It implements
the project's *second originating intent*: collapse traits + effects + capabilities
+ generated-impl overlays + const-predicate assumptions into **one scope-indexed
provision mechanism** (demand → provision lookup-or-discharge → evidence), demote
global `impl` to a *companion provision* tier, and retire the orphan rule + global
coherence checker (coherence = placement, not a checker). Architect's existing
design: `/workspace/fe-provision-scoping-design-2026-06-10.md` (Q0 charter). This
packet is the **repo-grounded implementation staging** of that design, not a
re-design of its semantics.

## What already exists (the migration skeleton — verified `file:line`)

- **`EffectEnv` is already frame-shaped** (`crates/hir/src/analysis/ty/ty_check/effect_env.rs`):
  a stack of `EffectFrame`s (`push_frame`/`pop_frame`, `:75/:79`), keyed entries
  (`insert_witness` `:103`, `insert_forwarder` `:120`, `insert_barrier` `:139`,
  `insert_unkeyed` `:147`), and tiered lookup (`lookup_precise` `:162` +
  `lookup_family_fallback` `:228`). **This is the ProvisionEnv skeleton**: a
  scope-indexed, keyed, fallback-tiered resolver already exists for effects. PS1
  grows it; it does not invent a new env.
- **The global trait env** (`trait_def.rs:35` `ingot_trait_env`) is the *other*
  resolver: a flat, ingot-global `impl` table. This is the bridge to demote
  (BR13 / PS0): it becomes the lowest-priority **companion** tier of the unified
  env, not a separate pathway.
- **Global coherence checking (5-0001)** (`ConflictTraitImpl`, `ty/diagnostics.rs:886`;
  orphan/overlap in `trait_lower.rs`) is the comfortable bridge (BR13). Under the
  charter it is replaced by *placement* rules, not a runtime overlap checker.
- **Const-predicate assumptions** (`ty_check/mod.rs` `const_predicate_assumptions`)
  and **provider capabilities** are the other two demand/provision channels that
  should share the one lookup path.

## The ~6 pathways to collapse (the value)

Today a "do I have X for type T" question is answered by up to six separate
mechanisms: (1) global `impl` table (`ingot_trait_env`), (2) in-scope `where`
bounds (`assumptions`), (3) `uses (..)` effect/capability frames (`EffectEnv`),
(4) generated-impl overlays from providers, (5) const-predicate assumptions,
(6) blanket/conditional impls (now gated by PS2/PS3). PS1 routes all six through
one scope-indexed `ProvisionEnv` with a defined priority order, so "lookup vs
discharge" is the only branch, and evidence is produced uniformly.

## The decision surface

### Q1. Priority/tier order (charter says `with` > uses/where > module > import > companion)
Confirm the tier ladder and where each current pathway lands:
- `with`-block / local provisions → highest.
- `uses`/`where` (function-scoped) → next (this is today's `EffectEnv` frames +
  `assumptions`).
- module / import → next.
- **global `impl` → lowest (companion tier)** — this is the demotion (PS0/BR13).
→ Decision: exact ladder + whether generated-impl overlays sit at the provider's
scope or at companion tier.

### Q2. Canonical-only safety valve (PS5) — required before any local shadowing ships
Consensus/layout-sensitive traits (`Ord`, `Hash`, ABI/storage/serialization) must
**not** be shadowable by a nearer scoped provision, or you get two different
`Hash`es for one type across scopes. Charter calls for a canonical-only marker:
these resolve only through the companion/canonical frame.
→ Decision: the marker mechanism (attribute? trait-level flag?) and the initial
canonical set. **This must land with or before local shadowing**, not after.

### Q3. Coherence-by-placement (retire 5-0001 the checker)
If global `impl` is just the companion tier and shadowing is scoped + canonical-
guarded, the orphan rule + overlap checker become placement rules. Decision: do we
delete the `ConflictTraitImpl` checker, or keep it scoped to the companion tier
only (a companion impl still can't overlap another companion impl)?

### Q4. Witness capture in type identity (PS6) — explicitly post-ladder
Witness-dependent data structures (a `BTreeMap` keyed by *which* `Ord`) need the
witness in the type identity. Charter defers this. Decision: confirm PS6 is
post-ladder (documented as future) so PS1 does not block on it.

## Implementor recommendation / staging

1. **PS1a — unify the read path behind one resolver interface.** Introduce a
   `ProvisionEnv` that *wraps* today's `EffectEnv` frames + `assumptions` +
   `ingot_trait_env` behind one `lookup(demand) -> Option<Evidence>` with the Q1
   tier order, **without changing resolution outcomes** (companion tier reproduces
   today's global behavior). Pure refactor; regression-guarded by the existing
   suite. This is the load-bearing, low-risk first slice.
2. **PS1b — demote global `impl` to companion tier + PS5 canonical markers.** Now
   that all reads go through the env, mark the canonical set and make global impls
   the lowest tier. Still no user-visible shadowing yet.
3. **PS1c — enable scoped provisions (`with`-blocks / local).** The first
   user-visible feature: a nearer provision overrides a farther one, except for
   canonical-only traits. Coherence-by-placement (Q3) lands here.
4. **PS6 (witness capture)** — separate, later.

PS2/PS3 (conditional-blanket gating, already landed) is a *partial* PS1: it is the
companion-tier discharge of a conditional provision. PS1a should subsume it, not
duplicate it.

## Dependencies & sequencing vs the other prerequisite
PS1 and typed-provider-capabilities are largely independent (PS1 is resolution;
capabilities are the provider type story), but they meet at **generated-impl
overlays as provisions** (a provider's emitted impl is a provision at the
provider's scope, carrying provenance evidence — D3 of the capabilities packet).
Recommend: PS1a (read-path unification) can start immediately and in parallel with
the capabilities decision; PS1b/PS1c want the canonical-set policy decided first.

## Acceptance
- PS1a: zero diagnostic/resolution changes across the full suite (pure refactor).
- PS1b: canonical-only traits cannot be overridden by a nearer provision (new
  negative fixture); global behavior otherwise unchanged.
- PS1c: a `with`-scoped provision overrides a companion impl for a non-canonical
  trait (new positive fixture); 5-0001 either deleted or scoped to companion-vs-
  companion per Q3.
