# FCO Abstract-Head Research Dossier & Charter

**2026-06-15.** Consolidated literature review + research-program charter for the
**abstract head** of the kinded-derive system: constraint-constructor *parameters*
(`P : * -> Constraint`), their application (`where P<T>`, `ImplBuilder<P,T>`,
`Evidence<P<T>>`), substitution-on-instantiation (`P := Eq` → `TraitInstId`), and
variable-headed solver goals (`P<T>`, `P<Option<T>>`, `P<T> + Q<T>`).

This is the charter document for a bounded background research program (below).
It exists so the abstract-head question is never re-excavated from scratch, and so
that if a concrete consumer ever appears the staged path + soundness requirements
are already on the shelf. Sources are cited inline; the load-bearing claims come
from a 2a viability spike + three fable-corpus digs (designs / FV / cautions) +
two historian passes run 2026-06-15.

## 0. Provenance — where the idea came from, and the immediate reaction it drew

- **Canonical origin (2026-05-22 architect/Codex session, `codex-session-019e4df9-readable.md`).**
  "derive isn't even the goal here as much as metaprogramming in general. reflect
  gives us access to some compile time context, builder lets us generate it." The
  kind sketch was first written there: `Eq : * -> Constraint`,
  `Derive : (* -> Constraint) -> Constraint`. The abstract head
  (`trait Derive<P> where P: * -> Constraint { const fn derive<T>() uses(reflect, builder: mut ImplBuilder<P,T>) }`)
  and the variable-headed goal forms (`P<T>`, `P<Option<T>>`, `P<T> + Q<T>`) were
  sketched there and *explicitly flagged*: "the solver needs to handle
  variable-headed goals, not only concrete trait names. **That is a major extension
  to trait resolution.**" The original 4-tier roadmap put it in the **"Eventually"**
  tier (MVP/Next/Later/Eventually), never the MVP.
- **The hand-off to the Fable agent for its take (2026-06-10, `obligations-review_session_322265f5/transcript.jsonl`, model = Fable 5).**
  Hand-off (07:30): *"the full fledged `A<B> -> *` and `* -> A<B>` syntax, what would
  properly supporting it yield us in the deeper layers? are we truly moving toward
  that?"* Fable's immediate reaction (07:32) led with the durable reframe:
  *"`F: A<B> -> *` — a bounded domain: the kind arrow carries a demand … kind
  application becomes evidence application. `G: * -> A<B>` — a bounded codomain: the
  solver gets `forall T. G<T>: A<B>` as an axiom attached to the constructor itself,
  no per-instantiation solving. … kinds that carry obligations and kinds that carry
  evidence … it's first-class obligations, one level up."* It immediately flagged the
  caution that aged perfectly: the kind checker is *"the weakest link,"* *"reject the
  syntax until it has semantics,"* and rank codomain (universal-evidence) bounds
  **last** because *"universally quantified evidence touches coherence."*

## 1. State of the art — three irreducible layers (spike + designs dig)

| Layer | Status | Evidence |
|---|---|---|
| **Represent** `P<T>` (variable head) | **buildable; rejected by D1** | effort2 built `ConstraintKind::ConstraintApplication(ConstraintApplicationId{head: ConstraintHeadId, args: Vec<TyId>})` with `ConstraintHeadKind::{ConcreteTrait, GenericParam(TyId), Builtin}` (`metaprogramming-effort2/.../ty/constraint.rs:21-31,165-185,559-575`). It parsed + collected (`pretty_print == "P<T>"`), but the whole file is `#![allow(dead_code)]`. The fco projection (`TraitInstId`) structurally cannot hold a variable head — `lower_hir_constraint_application` requires `PathRes::Trait`; a TyParam head → `None` (confirmed by 2a spike: `where P<T>` → `2-0007 expected trait, found type P`). |
| **Substitute** `P:=Eq` → `TraitInstId` | **VOID — never designed** | effort2's fold maps `GenericParam(ty) → GenericParam(Eq)` — substitutes the type but **never promotes the head to `ConcreteTrait`, never reifies a `TraitInstId`** (`constraint.rs:601-655`). The only substitution that exists anywhere is value-term `ConstParam` (`fe-m5-normalized-term-spec-2026-06-10.md:294-299`), never head-level. |
| **Solve** `P<T>` (variable-headed goal) | **VOID — never done; + anti-proven** | discharge is literal interned-ID `==` against assumptions, zero impl search (`ty_check/mod.rs:1577-1617`); `Derive` goals fall through to `UnsupportedConstraint`. Port map: "zero producers/dischargers … OUT" (`fe-m5-const-predicates-port-map-2026-06-11.md:88,94`). |

The vision review's one-liner frames it exactly: *"the branch's **nouns** are the right
nouns (the kind algebra composes, `HasKind` is fine); the **verbs** (syntactic
matching, identity discharge, evidence-dropping, in-forest evaluation) are
placeholders."* The kind *declaration* (`P: * -> Constraint`, K02a) is sound and
landed; the *verbs* (substitute + solve) are the whole problem.

## 2. FV finding — the abstract head is *anti*-proven, not merely unproven (FV dig)

**No FV artifact ever modeled the abstract head** (Lean litmus, 57-file z3 census,
fe-assurance, ~100 subagent threads — all concrete/ground). Worse, a variable head
**violates the four assumptions the proofs IMPORT (do not prove)**:
1. **Coherence-as-a-function** — Lean §9 takes "`ctfe` is a function (impl selection
   deterministic)" as an imported parameter (`fe-lean-litmus-2026-06-11.lean:295-301`).
   A `P` bound to different traits in different contexts makes the head non-functional
   in the goal → premise fails.
2. **Fixed resolution context Γ** — the proof warns verbatim: "concreteness judged
   against a context that moves mid-normalization would be exactly the ambiguity that
   breaks confluence" (`:302-306`). A variable head resolved during solving is exactly
   that.
3. **Full concreteness of subjects** — `isConcrete Γ` is `false` for a variable head;
   the rewrite rules do not fire on it. `P<T>` falls *outside* the proven (reducible)
   fragment — the proofs are silent, not supportive.
4. **First-order / binder-free** — the term language is deliberately binder-free;
   variable-headed goals are second-order matching ("higher-order unification — that
   door stays closed through Stage 3 at minimum," `fe-ct-cubical-compiler-roadmap-2026-06-10.md:466`).

## 3. Cautions, ranked (cautions dig)

1. **DECISIVE (observed):** the mechanism doesn't exist — substitution-on-instantiation
   never designed; effort2's discharge was a faked `==` with zero dischargers.
2. **HIGH (theory, repeatedly stated):** undecidability of variable-headed solving
   (higher-order unification) — forecloses the G6 decidability the solver rests on.
3. **MODERATE (debt):** every variable-headed goal is a *residual* (re-arms the
   "never drop residual constraints" blocker, D5.2); wiring `-> Constraint` codomains
   now "would pre-commit the Constraint-kind story under time pressure" at max blast
   radius.
4. **LOW/STALE for fco:** Salsa-cycle ICE, `Kind::Placeholder`/`Any` accept-and-ignore,
   dead scaffold — all *already mitigated* on fco (CI forbids in-forest CTFE; `where
   P<T>` clean-rejects; the carrier was never ported). Treat as "don't re-introduce."

**The escape hatch the corpus itself points to (C6):** the single strongest real demand
(Form 6 `IsoAt` family parameter) was satisfied **without** a true `P: * -> Constraint`
param — "a `family` parameter sort … matched by name only. No `Kind` extension, no
higher-order unification" (`fe-prove-once-represent-freely-2026-06-11.md:285-287`). The
abstract head is *over-powered relative to any real need*; first-order routing or
monomorphization has met every actual demand so far.

## 4. The research program (bounded; kill-switch first)

Run as a **single-pass background inquiry**, not an open program — because there is no
current consumer, and an open program researching machinery for an empty use-set is the
exact anti-pattern the wizard's Temperance note warns against.

- **Track 0 — demand gate (kill-switch).** Is there a real Fe program whose need the
  concrete head + first-order routing / monomorphization *cannot* meet? If no, the
  abstract head stays shelved (named-reject) regardless of feasibility.
- **Track 1 — FV feasibility (Lean4).** Is there a **decidable, coherence-preserving
  fragment** of variable-headed solving for Fe's restricted form (concrete-trait
  instantiation only, first-order args, no `P<T> + Q<T>` initially)? Deliverable: a Lean
  sketch extending the term language with a constraint-head sort that re-establishes
  selection-is-a-function + confluence/UNF — *or* a precise impossibility result.
  Adversarially verified (try to break the claimed fragment).
- **Track 2 — design (wizard).** Substitution-on-instantiation (head promotion) +
  monomorphize-vs-solve boundary; simulate `trait Derive<P>` ramifications in real Fe;
  pin the minimal consumer that would justify it.
- **Synthesis → architect decision packet** with an explicit **BUILD TRIGGER**: code
  starts only when *(a)* Track 1 finds a sound + decidable fragment **and** *(b)* a
  concrete consumer exists (Track 0). Until both, ship the concrete surface (W-C/G1) and
  keep the named rejection.

## 4b. PROGRAM RESULT (2026-06-15, workflow `abstract-head-feasibility`, run `wf_2e7b8127-d83`)

**Recommendation: SHELVE-WITH-RUNWAY.** All four tracks converge.

> **Framing to not lose — TWO INDEPENDENT GATES.**
> 1. **Feasibility is open + hard, not merely "needs design."** The abstract head is *not impossible*
>    (no impossibility result; the concrete head already works), but the open part is a **theorem that may
>    not exist**: genuine variable-headed solving sits next to a known cliff — higher-order matching is
>    undecidable in general. Three honest outcomes: (i) a useful decidable + coherence-preserving
>    sub-fragment is provable; (ii) only a *narrow* fragment works (less than the full `trait Derive<P>`);
>    (iii) the useful form is undecidable → monomorphization stays the answer. The adversary *proved the
>    cheap fragment collapses to concrete solving* (order-isomorphic to W-B); the harder one is genuinely
>    unknown. So it needs a sound design **plus a feasibility proof not guaranteed to exist** — which could
>    come back partial or negative.
> 2. **Demand is the BINDING constraint, and it is currently EMPTY (C6).** Even a perfect, proven design
>    would not justify building today — there is no consumer; every real need is met by the concrete head
>    + monomorphization. This (not feasibility) is the actual reason it is shelved, and it is the cheaper
>    kill-switch. Re-open only when a real consumer appears *and* the feasibility proof is in hand.

- **Track 0 (demand) — EMPTY. C6 CONFIRMED.** Every named consumer (one-provider-for-a-trait-family,
  `derive Functor`, multi-trait conjunction `P<T>+Q<T>`, the ABI Encode/Decode/AbiSize serialization
  family, and the corpus's own strongest Form-6 `IsoAt` demand) is met by monomorphize-per-trait or
  first-order name-matched routing. Decisive reason: every live provider body is *irreducibly
  trait-specific* (StableEq folds `&&` over `eq`; StableOrd folds lexicographic OR over lt/le/gt/ge;
  StableDefault over `default`) — there is **no shared `derive<P>` body to factor**, so a `P` parameter
  is pure ceremony. The one thing the abstract head uniquely expresses (general `Evidence<C>` in arbitrary
  type position) was produced by no real program.
- **Track 1 (Lean FV) — mechanically GREEN, but a category narrower than its headline; the adversary
  (also mechanically clean) showed it COLLAPSES to concrete solving.** The "restricted fragment"
  compiles (`[propext]`-only, same axiom base as the litmus), but `attack6 varhead_adds_no_solvable_goals`
  proves its solvable set is **order-isomorphic to the concrete-trait fragment W-B/the litmus already
  cover** — the variable head appears only in inputs eliminated by a fixed σ *before* any solve
  (= monomorphization, not solving) or dropped as `none`. The PRIMARY use — abstract-mode body checking
  of `fn foo<P, T>() where P<T>` (the live `constraint_kind_constructor.fe` fixture) — promotes to `none`,
  i.e. *outside* the proven fragment. The coherence theorem is a tautology (the adversary reproduced it
  for a demonstrably incoherent selector); decidability proved ground-equality, not first-order matching
  against parametric impl heads; confluence's orthogonality held only because the arg type forbids
  head-projecting args (`<T as P>::Assoc`, ordinary in derive bodies, is redex-creating). So clause (a)
  of the build trigger is satisfied only for the concrete residue — **genuine variable-headed solving
  remains unproven and the clean theorem is still owed.**
- **Track 2 (design) — no live tension.** Shipping, safety, auditability all point at the concrete head;
  only elegance pulls the other way and collapses once you see there is nothing to abstract. Both verbs
  are designed and shelved: **V1** head-promotion `TyFolder` (~30 LOC: on meeting impl-param `P`, look up
  the expansion-site `TraitDef` binding and re-enter the *existing* W-B `lower_hir_constraint_application`
  reifier — no new representation); **V2** widen the existing `has_param` carry gate to treat a head-param
  as symbolic-carried-by-assumption (this carry path's confluence/UNF is the unproven theorem owed).

**BUILD TRIGGER (both must fire):** (a) a Lean result covering *genuinely* variable-headed solving —
abstract-mode `P<T>` (live `P`, σ unbound) succeeding as a hypothesis, with confluence + UNF +
selection-is-a-function re-established once conjunction (`P<T>+Q<T>`) and head-projecting args are
admitted — NOT the concrete residue Track 1 proved; AND (b) a real in-tree consumer (the pinned **Sim-C
meta-deriver**, with its hidden precondition now made explicit): **a `Generic`-style universal structural
representation exists** (so derives share ONE traversal body — Fe has none today; `Reflect`+`quote` build
each body imperatively, so there is nothing to share), **AND** ≥2 traits over that representation differ
only in a leaf operation, **AND** the trait set is open/un-editable by the consumer (per-trait concrete
providers not authorable). Today (a) holds only for the concrete residue and (b) is empty — and the first
conjunct (a universal representation) is the actual upstream lever: it, not the constraint-constructor
solver, is what would make Sim-C authorable, and it is a separate demand-driven investigation.

## 4c. GENERATIVE RE-EXAMINATION (2026-06-15, design-wizard "is Sim-C too narrow?")

A max-effort design-wizard imagined eight consumer shapes *beyond* Sim-C and tested each against the
monomorphization-escape: (A) `derive_all<P>` shared-body meta-deriver, (B) capability/permission framework
over `P`, (C) proof-carrying/verification API over property `P`, (D) ABI/serialization codec family,
(E) quantified constraints `forall T. P<T> =>`, (F) constraint aliases/bundles, (G) `Evidence<P<T>>` as a
runtime value, (H) access-control/role framework. **Verdict: "empty demand" is ROBUST; the box was not
drawn too small.** Every *demanded* shape escapes to a first-order mechanism Fe already ships
(capabilities→effects `uses`/`with`; spec-checking→const-predicates+concrete obligations, where
concreteness IS the feature for the AI-spec-layer thesis; backend variation→`Platform` assoc-consts;
bundles→alias sugar over the concrete head; roles→runtime storage). The one shape that is *irreducible*
(E, quantified/entailment constraints) has zero Fe demand and is the hardest + most-dangerous form (the
coherence cliff Fable ranked last) — so it *strengthens* the shelve. Recommendation: **ship-trigger-as-is**
(refine the Sim-C wording per §4b, do not broaden the gate). The deep reason imagination can't rescue
demand: Fe's effects/traits split + "maximally minimal" axiom route every would-be abstract-head use to a
first-order mechanism; the abstract head is the *elegant unifier*, never the *only* expression of any real
need.

**SHIP NOW (the only code endorsed, independent of the trigger):** upgrade the abstract-head rejection
from the bare `expected trait, found type P` to a workaround-naming diagnostic
("constraint-constructor generic parameter `P` (`* -> Constraint`) is not yet a supported trait head;
write one provider per concrete trait, or monomorphize the consumer"). Honors D5.5; zero solver/
representation surface. (Emitted in the analysis pass `diagnosable.rs::constraint_application_diags`,
not in the diagnostic-free collection lowering.)

Full packet + the five adversarial Lean attacks + Track-1/Track-2 dossiers:
`/tmp/claude-1000/.../tasks/wf1lto04p.output` (run `wf_2e7b8127-d83`).

## 5. Key sources
effort2 `crates/hir/src/analysis/ty/constraint.rs` (`:1,21-31,165-185,559-575,601-655`),
`ty_check/mod.rs:1577-1617`, `ty_def.rs:1363-1379,2014,2022`; architect bundle
`codex-session-019e4df9-readable.md` (`:756-847,1234-1293`); Fable hand-off
`fe-sessions-bundle-2026-06-13/obligations-review_session_322265f5/transcript.jsonl`
(L1509/L1512); `fe-lean-litmus-2026-06-11.lean` (`:294-313`), `fe-fv-question-ledger-2026-06-11.md`;
`fe-m5-const-predicates-port-map-2026-06-11.md` (`:88,94,453-456`),
`fe-m5-normalized-term-spec-2026-06-10.md:294-322`,
`fe-prove-once-represent-freely-2026-06-11.md:285-287`,
`fe-ct-cubical-compiler-roadmap-2026-06-10.md` (`:462-466,643-649`);
`docs/dev/FCO_WIRING_PARTY_DECISION_RECORD.md` (D2/D5/D7),
`/workspace/fe-design-wizard-kinded-derive-verdict-2026-06-15.md`.
