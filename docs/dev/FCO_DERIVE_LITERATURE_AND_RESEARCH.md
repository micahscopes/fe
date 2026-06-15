# Derive / Constraint-Kind — Literature & Research Record (2026-06-15)

Consolidated from the architect's web-precedent pass, CTCubFe-connection pass, and research-directions
pass (2026-06-15), cross-checked against the Fable findings. **Verdict: full convergence, no pivot** — the
external precedent + theory independently confirm the Fable/architect design (the TD/AH split, the
solve-line, Tier A as the win, the two gates, "True Derive ≠ abstract head", Generic<T>-before-solver).
This file is the citable record; the live decision lives in `FCO_ABSTRACT_HEAD_RESEARCH_DOSSIER.md` +
graph key `derive_track_split_2026-06-15` / `lit_review_2026-06-15`.

## The sharpened thesis

> The dangerous solver is NOT "a solver for `* -> Constraint`." It is a solver for **variable-headed**
> goals — `P<T>` where `P` is still unknown/live at solver time. The Fe **solve-line** converts every safe
> case back to first-order concrete trait solving *before* the solver runs.

And the research reframe:

> The most promising Fe research is **a disciplined elaborator + small verified fragments that keep the
> ordinary solver concrete** — NOT a more powerful trait solver.

## Precedent mapping (external → Fe tiers)

| External | Maps to | Lesson |
|---|---|---|
| Haskell **ConstraintKinds** + constraint synonyms (`type Stringy a = (Show a, Read a)`), elaborated before solving | **Tier A aliases** | the common, useful, safe face — build aggressively |
| Haskell **QuantifiedConstraints** (`forall b. Eq b => Eq (f b)`), "a bit like a local instance decl", "raises expressiveness to ~first-order logic" | **Tier C** | a *serious solver feature*, not sugar — named-reject |
| GHC: general constraints in instance ctx can loop → needs `UndecidableInstances` | the two gates | backs refusal to casually generalize |
| **Rust/Chalk**: trait solving = goals/clauses in a logic program; Rust deliberately omits variable-headed trait constructors; next-gen solver is a multi-year stabilization | W-C / explain layer | even first-order trait solving is a big project; represent obligations as explicit goals/clauses for audit, keep admitted forms reducible to concrete goals |
| **Lean** instance synthesis: `outParam`/`semiOutParam` — non-output params must be known before synthesis (`[Mul β] : Add α` rejected) | **mode discipline** | "don't solve until the head is known" — supports Tier B (substitute/monomorphize first), supports the solve-line |
| **Coq/Rocq** typeclasses = programmatic proof/term search | `Evidence<P<T>>` | general `Evidence<P<T>>` is a real proof-search request, not a harmless type → keep "eliminate to concrete first" |
| **Scala 3** context bounds `[T: Ord]` → `(using Ord[T])` witness | `where Eq<T>` / `Evidence<Eq<T>>` | mainstream = explicit concrete witnesses, not solving unknown heads |
| **HOU theory**: general higher-order & second-order unification undecidable (Goldfarb 1981); decidable fragments exist (Miller patterns, unitary) | the AH feasibility gate | `P<T>` with live `P` risks becoming 2nd-order matching; a usable fragment must be *deliberately chosen and proven* |

## CTCubFe connection (the destination arc)

FCO/constraint-kind work is the **obligation-and-evidence substrate**; CTCubFe is the destination.
CTCubFe does **not** need a live `P<T>` solver to climb the capability ladder. The solve-line is now a
CTCubFe design principle. Tier → Form mapping:

- **Tier A aliases → Form 1 (Bounds With Receipts) / Form 4 (Laws That Cost Nothing) ergonomics** — named
  bundles of obligations (`constraint FieldLike<F> = Add<F> + Mul<F> + Eq<F> + FieldLaws<F>`), expanded to
  `PredicateListId`.
- **Generic<T> → Form 2 (One Algorithm, Every Size) / Form 5 (Proven at Every Size)** — the lever for
  shared structural traversal/proofs is a universal representation, NOT a generic-`P` solver.
- **const_assert + VC sites + SMT route → Form 3 (Quiet Verification)** — uses the FCO evidence substrate.
- **erased proofs/laws + Constraint kind → Form 4.**
- **proof transport + Path/IsoAt + Sonatina v2 resources → Form 6 (Prove Once, Represent Freely).**
- **live abstract head → shelved; not the spine of any Form.**

**Sonatina v2** is the lower-level counterpart: source obligations state facts ("this backend has storage",
"this layout is static"); the resource layer proves/frames the operation (a const predicate over
compile-time constants does NOT prove runtime pointer-arithmetic safety — that needs the resource layer).
**SMT is the first real producer of non-empty `premises`**; proof-transport/certificates are later
consumers. (Reserve the `premises: Vec<CheckPremise>` slot, per the FV census.)

## Research backlog (best Fe-native directions, in rough priority)

- **A. Constraint aliases as a verified elaboration fragment** (≈ Tier A). Build + show: expansion
  terminates, is confluent, preserves diagnostics/evidence origin, yields exactly concrete
  `PredicateListId` entries. Low-risk, immediately useful.
- **B. Total / closed-world constraints** (lead: *Total Type Classes*, Haskell 2025 — verify a set of
  instances by inspecting class instance decls, not per-demand). Fe form: certify a constraint family
  *covers all shapes* in a closed ingot / `Generic<T>` representation ("all variants have Encode", "all
  fields satisfy Law", "all sizes have a proof route"). A **closed-world alternative to live `P<T>`
  solving**; helps CTCubFe Forms 4–5. Research node **TR**.
- **C. Mode-checked constraint constructors** (à la Lean `outParam`): `P<T>` admissible only if `P` is
  known by substitution / alias expansion / explicit evidence. Makes Tier B principled.
- **D. Pattern-headed constraint fragment** (Miller-pattern-inspired): isolate a fragment and PROVE
  termination + confluence/UNF + selection-is-a-function + elaboration-to-concrete-when-monomorphized + no
  live `P` to the solver — *or* document the counterexample. This rigorously answers "not impossible." Must
  include conjunction (`P<T> + Q<T>`) and decide on associated projection (`<T as P>::Assoc`) — the known
  traps from the runway (don't prove a tautological coherence thm; model first-order MATCHING vs parametric
  heads not ground-eq; handle redex-creating promotion).
- **E. Certified elaboration (the Fe-native frame)**: rather than prove a big solver sound, prove every
  advanced surface form elaborates to a list of concrete obligations + evidence links. Aliases → conjunction;
  monomorphized provider → `Eq<T>`; future `Generic<T>` fold → per-field concrete obligations.
- **Generic<T> universal structural representation** — a first-order substitute for abstract-head
  meta-derivers; the real lever before any generic-`P` solver.

**Do NOT research/build first:** general quantified constraints; runtime `Evidence<P<T>>` dictionaries;
`TyData::ConstraintTerm`; full higher-order unification; solver plugins for arbitrary constraint
constructors. (These are the cliff.)

## Test standard (inherited from M5)
Any future solver-adjacent work must be ANTI-VACUOUS: positive proves the intended elaboration happened;
negative proves the forbidden solver shape did NOT slip through; diagnostics prove the boundary is named;
evidence proves the route.

## Sources
Architect 2026-06-15 web-precedent / CTCubFe-connection / research-directions passes (this file consolidates
them); Haskell ConstraintKinds + QuantifiedConstraints (GHC docs/proposal); *Total Type Classes* (Haskell
2025); Rust rustc-dev-guide trait-solving + Chalk + next-solver project goal; Lean reference
(`outParam`/`semiOutParam`); Coq/Rocq typeclass manual; Scala 3 context bounds; HOU surveys (Goldfarb 1981,
Miller patterns, Prehofer); Fable corpus + `FCO_ABSTRACT_HEAD_RESEARCH_DOSSIER.md`.
