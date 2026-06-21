# Provision Scoping & the Trait/Effect Unification — Synthesis + Design Push

> **LIVE (ratified design anchor; §4 anchors the slide).** Still authoritative for the ratified provision-scoping decisions (one resolver, ladder of tiers, impls = companion/outermost tier, innermost-wins, canonical = non-overridable). `FCO_THE_SLIDE_2026-06-19.md` is the SSOT built ON this; `FCO_MAP.md` is the one-page entry point.

**Date:** 2026-06-17 · **Audience:** architect · **Status:**
`DOC_DECISION_PACKET / ARCHITECT_RATIFIED_DESIGN_DIRECTION / PUBLIC_SEMANTICS_NOT_IMPLEMENTED`
(ratified 2026-06-17, architect directive). Synthesis of prior fable logs + design-wizard
verdicts + Micah's 2026-06-17 steer. Nothing here is built; the provision-scoping spectrum is
design-only and architect-gated (it touches public semantics + coherence).

**Architect ratification (2026-06-17) — decisions are now binding direction:**
(1) **default tier = companion** (existing `impl I for T` = a companion/default provision, NOT
ambient global-coherence law); (2) **global ≡ canonical** (no separate "global" tier unless a
concrete use forces it; canonical = exactly one, everywhere, non-overridable); (3) **consensus-
soundness lever = canonical-only markers NOW (Ord/Hash/ABI/storage-layout-sensitive), witness-
capture LATER**; (4) **orphan rule demoted** from a universal law to the rule *for canonical
provisions* (ordinary scoped provisions may relax later; canonical stays coherence-restricted);
(5) **resolver = one `ProvisionEnv`** (`demand → scope-chain provision lookup/discharge →
evidence`); traits/effects/capabilities/generated-impl-overlays/const-assumptions converge into
scoped provisions — phrase as "converge into scoped provisions," NOT "replace traits with
effects". **NO public provision syntax yet** (no `provide impl`, canonical/module/ingot syntax,
orphan relaxation, or witness capture until separately scoped). Internal `ProvisionEnv` read-path
unification IS authorized if debt-negative (one real caller migrated, one old path de-blessed, no
facade-only commit). The named method-resolution hazard is **PS-MR** (provider-origin where-clause
evidence vs ordinary impl evidence = duplicate evidence routes for one goal; must dedup, not treat
as unrelated candidates) — do NOT solve it accidentally inside TD5.

Citation keys:
- **PS-DOC** = `fe-provision-scoping-design-2026-06-10.md` (the canonical design note; in
  `fe-sessions-bundle-2026-06-13/generated-docs/obligations-review/`)
- **CHARTER** = `fe-obligations-next-push-plan-2026-06-09.md` (Q0 charter)
- **OBL-TX** = `obligations-review_session_322265f5/transcript.jsonl` (verbal genesis; cited by line)
- **CLEAN-SPLIT** = `fe-design-wizard/references/effects-and-orphan-rule-analysis.md` (the prior pole)
- **DEEP-LORE** = `fe-design-wizard/references/language-design-deep-lore.md`
- **D7** = `fe-design-wizard-kinded-derive-verdict-2026-06-15.md` (projection verdict)
- **ABSTRACT-HEAD** = `docs/dev/FCO_ABSTRACT_HEAD_RESEARCH_DOSSIER.md`
- **TD5** = `docs/dev/TD5_PROVIDER_BODY_EFFECTS.md` + `TD5_PROVIDER_COMMAND_SURFACE.md`

---

## 0. The ask in one line

Ratify (or correct) a **single provision mechanism with first-class scope gradation** —
global coherence retained as an explicit opt-in, **not** the default — and **one unified
resolver** over it. Then tell us the two things only you can decide (§9): the **default
tier**, and the **consensus-soundness lever** (canonical markers now vs. witness-capture).

---

## 1. The thesis (and the precise reframe)

There is exactly one judgment under both traits and effects (PS-DOC §"The unification"):

> *In this context, type T has an implementation of interface I.*

Call the pair (interface-instantiation, implementation) a **provision (witness)**. Traits
and effects are "two answers to one question: **where may provisions be introduced, and how
do use sites find them?**" (PS-DOC). The move is therefore **not "replace traits with
effects"** but **collapse the spectrum** — one provision mechanism at scopes of different
granularity.

This already exists in Fe in degenerate form: an effect provision *is* a finely-scoped
trait impl; a Rust-style `impl` is the globally-coherent one. The design only adds the
**missing middle** (module/ingot scope) and a **uniform resolver**.

---

## 2. Two poles in the corpus — and why this supersedes (not contradicts) the earlier one

The corpus holds two recorded positions; the chronology matters.

**Pole A — the clean split (earlier, CLEAN-SPLIT, 2026-03-28).** A cautionary analysis:
do *not* let effects become a general orphan-rule workaround. It predicts a slippery slope
— "the `uses` tax," ecosystem bifurcation into "trait style" vs "effect style," and
**semantic erosion** ("effects lose their meaning… `uses` becomes a second way to spell
trait bounds with different rules"). Its prescription: traits for pure abstractions
(Eq/Ord/arithmetic/serialization), effects for environmental capabilities (precompiles,
hash backend, storage), and keep the effect set **small + fixed** (echoed in DEEP-LORE #3
"effects color functions — multiplicative API tax," #5 "minimal viable effect system…
NOT user-extensible," #6 "effect tunneling… current fixed-effect-set avoids this entirely").

**Pole B — the unification (later, the genesis).** In OBL-TX the assistant had *Pole A*
loaded as the only recorded position ("the only recorded position is actually the opposite
pole — the effect-critique agent's 'keep Reflect a trait, stay at capability effects'",
**OBL-TX L696**). Micah flipped it on gut:

> **OBL-TX L709 (Micah):** "well hold on now cowboy, fe effects are basically finely scoped
> rustlike trait impls."

Restated (**OBL-TX L711**): the unification already exists; the only difference is
**provision scope**; so "lean into effects" is "**collapsing one axis** — demote the global
end, add a coarse module/ingot middle. One mechanism, three scopes." This **dissolved
charter Q1** (Reflect: trait vs capability — "the dichotomy stops existing", L720) and
**resolved Q6** (cross-ingot generated impls — "a derive is just a provision-introduction
form"), folding both into **charter Q0 "what is the substrate?"** (L730). Micah flagged it
was a brief, gut call (**L735**); assistant: "gut is allowed to make the calls; the files
just have to remember them" (**L742**).

**The synthesis (PS-DOC) does not discard Pole A — it answers it.** Pole A's real worry is
soundness for witness-dependent data and ecosystem coherence. The synthesis re-provides both
*without* global coherence-as-a-law: **companion provisions** for ergonomic canonicity, and
**canonical-only markers / witness-capture** for soundness (see §5). "**Coherence stops being
a law enforced by a checker and becomes a default produced by placement**" (PS-DOC). What is
*deleted* is the orphan rule + the global-coherence checker; what is *added* is shadowing in
narrower scopes and local provisions for foreign types.

---

## 3. Micah's steer (2026-06-17) — the design constraint to honor

> "There may still be a use for globally scoped trait impls but they shouldn't be the default;
> we should have a nice design that empowers **gradation** and **unify the resolution**
> accordingly."

This sharpens PS-DOC in three ways:
1. **Global is retained, not deleted** — but as an **explicit opt-in escalation**, not the
   default. (PS-DOC's "demote to companion" is the floor; Micah keeps an explicit *global*
   ceiling too.)
2. **Default authoring lands at a narrower scope** (companion or module).
3. **Gradation is a first-class deliverable** — moving a provision along the scope spectrum
   should be smooth and intentional — and **one resolver** spans it.

---

## 4. The design: the gradation ladder + one unified resolver

**The ladder** (author picks the tier by placement/keyword; the resolver treats all tiers
uniformly). Innermost wins.

| Tier | Introduction form | Visibility / lifetime | Coherence | Status |
|---|---|---|---|---|
| Expression | `with X { }` | lexical block | local | exists (effects; effort2 scoped derives) |
| Function demand | `uses (x: I<T>)` | callee-declared, propagates as obligation | callee | exists |
| **Module / ingot** | `provide impl I for T` (+`pub`) | the module; importers if `pub` | scope-local | **missing middle — to design** |
| **Companion (proposed default)** | `impl I for T` in T's/I's home scope | flows with the type's import | shadowable | reframing of today's global `impl` |
| **Canonical / global (opt-in)** | explicit `canonical impl …` / `#[canonical]` | everywhere, **non-overridable** | the coherence tier | the *retained* global, now explicit |

So today's `impl Eq for Point` becomes the **companion** default (Rust-like ergonomics,
*minus* global enforcement); genuine global coherence is an **explicit escalation** to the
canonical tier, used exactly when you want the coherence-class guarantee. This makes
"derive a foreign trait for a foreign type" (Rust orphan-rule-forbidden) safe and natural:
a standalone `derive Eq for ForeignType` is a scope-local provision that cannot pollute
anyone else (PS-DOC §"Derives under provision scoping").

**The unified resolver** (PS-DOC §"single resolution pathway"; CHARTER Q0):

> **demand = obligation; resolution = scope-chain provision lookup; result = evidence.**

One rule: walk the scope chain innermost→outermost, first match wins; ambiguity *within* a
tier is an error; the canonical tier **forbids shadowing** for its goal. Single *pathway*,
multiple *backends* — CTFE now (SMT later) discharge computational constraint kinds at the
**same chokepoint**. This collapses the **~6 pathways Fe has today** (global tabled trait
solver, legacy keyed effect env, effect bounds, CapabilityEnv, scoped generated-impl overlay,
const-predicate assumption prover — PS-DOC, CHARTER). Classic coherence "degenerates into
'the outermost scope contains at most one provision per goal.'"

---

## 5. The load-bearing coupling: consensus soundness (the one real hazard)

Global coherence silently provides **soundness for witness-dependent data** (PS-DOC §"The
canonicity problem"). Removing it as the default re-exposes it, and **for Fe this is
consensus-class**: a storage map's key ordering/hashing is *persistent state layout*. Two
functions of one contract resolving different `Ord<K>` witnesses "disagree about what storage
*means*" (PS-DOC) — the Scala `Set`-ordering bug, but with money.

Micah's "empower gradation" makes the answer **mandatory, not optional**. Two levers, and we
likely need both:
- **Now — canonical-only markers.** Consensus/storage/ABI interfaces (Ord, Hash, the ABI
  family) *must* resolve at the canonical tier; no overrides. "A small coherent core inside
  the scoped world" (PS-DOC). **We already run this in miniature:** `AbiSize` is deliberately
  non-canonical-auto yet named-only and effectively pinned in `#5b`/`#5c` (commits
  `43ebe7efb`, `fbfbba0ef`); it is exactly a "this interface admits one companion provision"
  policy.
- **Later — witness-capture-in-type-identity.** The witness becomes part of the type
  (ML-functor: `BTreeMap(IntOrd).t` ≠ `BTreeMap(RevOrd).t`), so re-resolution per scope is
  impossible by construction (PS-DOC; OCaml modular implicits). This is **evidence with
  first-class identity, tracked in types** — and is what eventually lets even consensus traits
  be graded safely.

**Net:** "free-for-all middle tiers" are safe *because* consensus traits can't live there
without an explicit witness story. Gradation and money-safety are the same design, joined at
the canonical tier.

---

## 6. Why this needs first-class obligations — and how the FCO/TD5 work is already paying it down

PS-DOC's load-bearing claim: this direction **"doesn't merely coexist with first-class
obligations; it *requires* them."** Scoped provision forces the question *"which witness?"*
(evidence as a first-class object) instead of coherence's *"is there a witness?"*. The
obligations vocabulary — EvidenceId, witnesses, scoped overlays — **is the substrate this
runs on.**

That is precisely the FCO ladder. Concretely, already-landed substrate toward the unified
resolver:
- **TD5.2 (`cfce8d366`):** `builder.require` no longer flows as a bespoke executor command —
  it records a typed `ProviderEffect::Require` and re-enters ordinary obligation checking.
  This is a literal down-payment on "demand = obligation": the provider body now *emits an
  obligation* rather than a command the synthesizer silently drops. (Verified byte-identical:
  fe-hir 129/0, cli_output 353/0, build_foundry 1/0.)
- **W-B (`608f39c84`) + D7 (the projection verdict):** concrete `where Eq<T>` is collected as
  an ordinary `TraitInstId` obligation; D7 ratified **projection over `PredicateListId`/
  `TraitInstId`, NOT a new `ConstraintTerm`** — i.e., keep one obligation representation, the
  exact discipline the unified resolver wants.
- **Capabilities-as-constraints (K04a):** `uses`-things and `where`-things already lower into
  **one vocabulary** (PS-DOC §"What effort2 already prototypes") — the precondition for one
  resolver.
- **Scoped overlays + scoped-conflict diagnostics (effort2):** the prototype of the tier-1
  provision + the within-tier ambiguity rule.

So the burn-down isn't a detour from provision scoping; it is building its only viable
substrate. **What's still missing for the unified resolver:** a scope-indexed provision
*environment* abstraction (today's global `TraitEnv` should become its outermost-scope
configuration — PS-DOC §"Implications for the ladder": "do not hard-code global assumptions
the way effort2 did").

---

## 7. The kinded-obligations connection (don't lose this — it's the deepest layer)

When Micah asked what fully supporting `A<B> -> *` / `* -> A<B>` buys in the deep layers
(**OBL-TX L1509**), the answer tied the kind system to provision scoping (**OBL-TX
L1512/L1515**):

> `A<B> -> *` is **obligations at the kind level**; `* -> A<B>` is **evidence at the kind
> level** … proper support turns "HKT-driven" into the mechanism — `Derive` becomes an
> ordinary higher-kinded citizen … and the solver gains **constructor-attached universal
> evidence that composes perfectly with Q0's companion provisions.**

Direction (DEEP-LORE; ABSTRACT-HEAD; D7): **substrate first** (the single obligation pathway
everything discharges through), **syntax rejected-not-faked** until the kind checker can keep
its promises. The kinded-obligations spine (K00–K08) and provision scoping (Q0) are the *same
program viewed at two levels*: provisions are the term-level "which witness," kinded
obligations/evidence are its type-constructor-level lift.

---

## 8. What is settled vs. open (carried from fable / design-wizard, do not relitigate)

**Settled (ratified):**
- **Projection, not `ConstraintTerm`** for the obligation representation (D7; W-B landed).
  The unified resolver should ride `TraitInstId`/`PredicateListId`, not a new type-kind.
- **The abstract head (`trait Derive<P>`, `where P<T>`, live variable-headed solving) stays
  SHELVED** (ABSTRACT-HEAD). Two independent gates, both unmet: (i) *feasibility* is open +
  hard — genuinely variable-headed solving sits next to undecidable higher-order matching, no
  guaranteed theorem; (ii) *demand* is empty — every real consumer is met by the concrete
  fragment + monomorphization. **Relevance here:** provision scoping does **not** need the
  abstract head; companion/canonical provisions are first-order. Do not let "unify resolution"
  smuggle the abstract head back in.
- **The solve-line invariant:** everything eliminates to a concrete `TraitInstId` *before* the
  solver runs (the solver never sees a live `P`). The unified resolver must preserve this.
- **Effects stay small + fixed at the control level** (DEEP-LORE #5/#6): provision scoping is
  about *type-indexed provisions*, NOT about making effects user-extensible control effects —
  tunneling/coloring concerns do not reopen.

**Open (the design owes answers — PS-DOC §"Open sub-questions" 1–7):**
1. Tier-2 surface + import semantics (does `use module::*` bring provisions? explicit `use
   provisions`? default-on with opt-out?).
2. Priority/shadowing fine-print (can a module provision shadow a *canonical-marked* trait?
   proposed: no).
3. Bounds in type defs (`struct S<T> where T: Eq`) — demand captured at instantiation,
   interacts with witness capture.
4. Assoc consts/types under scoping (`HEAD_SIZE` etc. — the stdlib leans on these; same
   canonicity hazard as methods).
5. Method/UFCS resolution (`a.eq(b)` through the in-scope provision — confirm no ergonomics
   cliff).
6. Keyed-effect dimension: how today's effect *keys + modes* compose with type-indexed
   provisions in one environment.
7. Stdlib migration mechanics (~485 impl blocks → companion provisions in place, ~zero
   textual change).

---

## 9. Key questions for the architect (the decisions only you should make)

**Q-A (the default tier).** Micah's steer says "not global." Is the default **companion**
(type's home scope, flows with import — minimal churn, ~all existing code keeps its meaning)
or **module/ingot**? Recommendation: companion default + module as the explicit coarse tier;
this makes the 485-impl migration near-textless. Confirm or correct.

**Q-B (the consensus-soundness lever — the gating decision).** For witness-dependent
persistent data, do we ship **(1) canonical-only markers now** (Ord/Hash/ABI forced to the
canonical tier; cheap, sound, boring) and defer witness-capture post-ladder — or do you want
witness-capture designed up front? Everything else can proceed on (1); (2) is the long-term
relaxant. This is the one decision that gates real soundness.

**Q-C (what "global retained" means precisely).** Is the retained global tier simply
**`canonical` = "exactly one, everywhere, non-overridable"** (i.e., the canonical-marker tier
*is* the global tier, user-available), or do you want a separate notion? Folding "global" and
"canonical" into one tier is the simplest unification consistent with Micah's steer — confirm.

**Q-D (orphan rule disposition).** Confirm the intended end-state: **the orphan rule is
deleted as a global law; coherence becomes opt-in via the canonical tier (mandatory for marked
consensus traits).** This is a public-semantics commitment — explicit sign-off wanted before
any implementation.

**Q-E (resolver build order).** Agree the next substrate step is a **scope-indexed provision
environment abstraction** with today's `TraitEnv` as its outermost configuration (PS-DOC
ladder implication), landing behind the M5/M6 obligation work — *not* a hard-coded global env.
This is decision-free debt-negative refactoring if you bless the direction; we'd sequence it
after the current TD5 reassessment.

**Q-F (sequencing vs. the TD5 reassessment).** TD5.2 (require→obligation) is paused for your
read on the no-shim bar (see the TD5 board). Provision scoping gives that reassessment its
*north*: the value of TD5 is precisely that it builds the single obligation pathway the unified
resolver needs. Does that reframe how far you want TD5 pushed now (e.g., route `require` all
the way into the provision environment, not just `requirement_where_clause`)?

---

## 10. One-paragraph recommendation

Ratify provision scoping as **one graded provision mechanism + one resolver**, with
**companion as default**, **`canonical` as the explicit retained-global tier**, the **orphan
rule deleted in favor of opt-in coherence**, and **canonical-only markers now / witness-capture
later** as the consensus-soundness path. Keep projection (no `ConstraintTerm`), keep the
abstract head shelved, keep the solve-line invariant. Treat the FCO/TD5 obligation work as the
substrate it explicitly requires, and add a scope-indexed provision-environment abstraction as
the next substrate rung. The gut call (Micah, OBL-TX L709) is sound; what it needs from you is
the default tier (Q-A), the soundness lever (Q-B), and the public-semantics sign-off (Q-D).
