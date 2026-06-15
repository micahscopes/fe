# Wiring-Party Decision Record — kinded derive API, grounded in the fable corpus

**2026-06-15.** Decisions for graduating the derive/metaprogramming bridge to its kinded form
(`Derive : (* -> Constraint) -> Constraint`, `Evidence`/`ImplBuilder : Constraint -> *`,
traits-as-`* -> Constraint`, `where P<T>`). Every load-bearing call below is anchored in the
fable logs (FV experiments, ~100 subagent threads, design corpus) — not freelanced — per Micah's
directive to "anchor carefully into the richness of the fable logs, document decisions, cite
sources." Three deep-dig agents (FV / subagent-threads / design-synthesis) produced the citations.

## D1 — Constraint representation: project over `PredicateListId`/`TraitInstId`; NO `ConstraintId`
**Decision (architect, ratified by the dig).** Represent a trait-application constraint (`Eq<T>`) as
a `TraitInstId`, projected over the existing `PredicateListId` + obligation queue. Do NOT introduce
an effort2-style `ConstraintId`/`ConstraintTerm` unless that machinery *provably cannot* carry
stable identity / origin links / kinded-application shape.
- **Source:** `FCO_K03_K04_EXECUTION_MAP.md:55-64` (architect, 2026-06-14): "fco models constraints as
  `PredicateListId` + the obligation queue … K03 should project … Do NOT re-introduce an effort2-style
  `ConstraintId` unless `PredicateListId`/`TermId` provably cannot give … kinded-application shape."
- **Corroboration (live code):** the *landed* `term.rs` (`TERM_LANG_VERSION=1`, commit `0059cf583`)
  keys assoc-const projections by `AssocConst{inst: TraitInstId, name}` — trait+args ARE key material —
  deliberately, because it disambiguates `<T as Tr<A>>::C` vs `<T as Tr<B>>::C`
  (`fe-m5-course-corrections-2026-06-12.md:55-81`; reconciliation thread `a39210b…`). This is the live
  precedent: the codebase already prefers `TraitInstId` over a bare `(TyId-subject, item)` pair.
- **My W0 spike (independent):** `TyData::QualifiedTy(TraitInstId)` is the WRONG carrier — it means
  `<T as Iterator>` (a *type*, `Kind::Star`, `ty_def.rs:1844`, ~6 consumers), not a constraint.
  But `Evidence<C>`/`ImplBuilder<C>` are capabilities (exempt bodies) and `where Eq<T>` is a predicate
  — neither needs `Eq<T>` as a general `TyId`; both lower to a `TraitInstId` at the constraint/capability
  layer. So the projection holds with **no new `TyData` variant**.
- **Caveat / staging:** the literal `TyData::ConstraintTerm` (constraint as a `TyId` of `Kind::Constraint`,
  for a *general* `Evidence<Eq<T>>` type application) remains the K03 end-state, **staged LAST** because it
  "breaks every exhaustive `match TyData`" (`FCO_K03_K04_EXECUTION_MAP.md:68-71`). We avoid it for the
  capability/predicate positions; adopt it only if a real type-position need appears.

## D2 — Abstract `where P<T>` (P a `* -> Constraint` *param*): DEFER + escalate; do NOT solo
**Decision.** The concrete head (`P` = a named trait, e.g. `Eq`) is buildable now (D1, `TraitInstId`).
The **abstract head** (`P` a generic param of kind `* -> Constraint`) is net-new research, gated behind a
named-rejection diagnostic; escalate before building.
- **Grounding (uniform across all sources):**
  - effort2's `ConstraintKind::ConstraintApplication{head: GenericParam, args}` was **dead scaffold** —
    "zero producers/dischargers" (`fe-m5-const-predicates-port-map-2026-06-11.md:88`); its discharge was
    interned-ID identity match, no head-promotion (subagent `abf5f4bb…:L259`).
  - **Substitution-on-instantiation (`P`→concrete trait → `TraitInstId`) was NEVER designed** — confirmed
    by all three digs (FV: none; subagents: explicit gap; design corpus: only value-term `ConstParam`
    substitution exists, `fe-m5-normalized-term-spec-2026-06-10.md:294-299`).
  - Explicit M5 non-goal: "Constraint applications (`where P<T>`, `ConstraintApplication`, constraint-kinded
    generic params) — not ported" (`fe-m5-const-predicates-port-map-2026-06-11.md:453-456`).
  - The one positive use (Form 6 higher-kinded family) routes *around* the kind system with a name-matched
    "family parameter sort", forbidding higher-order unification (`fe-prove-once-represent-freely-2026-06-11.md:285-287`).
- Today on fco: `where P<T>` → clean `8-0030 NotValue` (no ICE, no silent `Kind::Any`). Acceptable as-is;
  the BR7/K01 "reject-by-name" posture is the model.

## D3 — Self-convention: first type-arg is the subject (`Trait<Self, Args...>`)
`trait Eq<T = Self>` / `trait Ord<T = Self>` (`ingots/core/src/ops.fe:119,126`) ⇒ `Eq<T>` means subject `T`
(fills the `T=Self` slot); the provider surface `Evidence<Eq<T>>`/`ImplBuilder<Eq<T>>`
(`ingots/core_derives/src/lib.fe:26-29`) is "evidence/builder that T is Eq". So the constraint-application
lowering binds **Self = first provided arg** (NOT `lower_trait_ref_impl`'s `Self = t.self_param`, which is
the bound-receiver convention). Abstract-head Self-convention is undocumented (→ D2).

## D4 — The kinds (confirmed, no revision) + staging
`Derive : (* -> Constraint) -> Constraint`; `Evidence`/`ImplBuilder : Constraint -> *`; `Reflect : * -> *`
(`FCO_K03_K04_EXECUTION_MAP.md:37-38`; `FCO_CONSOLIDATION_MAP.md:895-898`; effort2 `ty_def.rs:2035-2055`).
Order: **K04a (typed recognition, DONE) → K04b (give the capabilities their kinds) → K03 (`ConstraintTerm`,
LAST, largest blast radius).** W-A=K04b, W-B=K03-concrete-via-projection, W-C=provider-by-identity map onto
this; the heavy `ConstraintTerm` is *not* on the critical path for the capability/predicate forms.
"Graduation" = string→identity + opaque→kinded (`REFLECT_KEY`/`IMPL_BUILDER_KEY`/`DERIVE_MARKER` deleted;
`Value::Reflect/Builder/Evidence` → kinded). K04a already did string→identity for capabilities.

## D5 — Design constraints inherited from effort2's failure (the five blockers)
Any wiring MUST honor (subagent core review `a8f50…`, vision review `abf5…`, architect-bundle `a880…`):
1. **CTFE never inside the trait solver** (Salsa-cycle ICE) — discharge at obligation level, gate-not-select.
2. **Never drop residual (non-trait) constraints** — one discharge chokepoint.
3. **No dual pipeline** — one collection routine; `PredicateListId` a projection, not a parallel SSOT.
4. **Evidence must persist with a real consumer** (effort2's was write-only).
5. **Reject (don't fake) kind syntax you can't check** (`Kind::Placeholder` wildcard-matched everything).
Wisdom-transfer law (`a217fc…`): "never add comparison cleverness to a prover — strengthen the normalizer
and bump `TERM_LANG_VERSION`; or you recreate effort2's `expected X, found X`."

## D6 — FV grounding (what's proven vs design-only) — honesty boundary
- **PROVEN (Lean litmus `fe-lean-litmus-2026-06-11.lean`, sorry-free):** term-language **confluence + unique
  normal forms + strong normalization** (§9) — so content-addressed term/`TraitInstId` identity is sound,
  given two imported premises the wiring must preserve: **(a) impl selection stays a function (coherence),
  (b) the resolution context is fixed across a normalization run.** Also: evidence-only impls are coherent
  (§7); verbatim matching is exactly calibrated (§4/§6).
- **GROUNDED (z3-vc-census, `fe-vc-census-z3-2026-06-11.md`):** const-predicate arithmetic 19/19 unsat,
  none vacuous; W6 nonlinear is intractable in QF_BV past bv16 → only the **premise-gated NIA** route closes
  at u256 ⇒ evidence schema needs a `premises: Vec<CheckPremise>` slot from the first SMT release.
- **UNGROUNDED by FV:** the kind system / constraint representation / `Evidence`-`ImplBuilder` typing — these
  rest on the design corpus + the architect decision, NOT on any machine-checked artifact. State plainly when
  asked.

## D7 — Ergonomics adjudication (fe-design-wizard, 2026-06-15): SHIP THE PROJECTION
**Decision (ergonomics-driven, confirms D1).** A max-effort `fe-design-wizard` simulation treated the
kinded derive surface as a language-ergonomics challenge (surface-first; representation follows) and ran the
full method — classify, environment-weighted precedent (Rust `#[derive]`/proc-macros, Scala-3 `derives`/
`Mirror`/`${}`, Haskell `GHC.Generics`, Move), sensibilities-in-tension, ramification simulations
(refactoring trap, generic context, cross-boundary/orphan, cost surprise, composition gauntlet, teaching,
futures). Verdict, in full at `/workspace/fe-design-wizard-kinded-derive-verdict-2026-06-15.md`
(agent `aa7c371c3cc46ab8c`):
- **(a) Surface — SHIP WITH GUARDRAILS.** The concrete kinded provider surface is right *for Fe specifically*
  because authority-to-generate-code rides the same `uses(...)` capability grammar as the rest of the language
  — a property no other derive system has. Keep the verbose signature (its verbosity IS the auditability);
  pay the authoring-site learnability debt with error quality.
- **(b) Representation — SHIP THE PROJECTION.** Project `where Eq<T>` and `Evidence<Eq<T>>`/`ImplBuilder<Eq<T>>`
  over `TraitInstId`/`PredicateListId` (D1/D3), **no new `TyData` variant**. *Decisive argument:* every
  ergonomic property the simulations require (generic-context inference, cross-boundary, futures/export) is
  delivered by `TraitInstId` identity (Lean-proven sound) + `PredicateListId` predicate synthesis. A
  `ConstraintTerm` of `Kind::Constraint` buys only a general type-position `Evidence<C>` that **no simulation
  produced a need for**, at the largest blast radius in the compiler. Cost/benefit lopsided. → `ConstraintTerm`
  stays off the critical path (confirms the D1 caveat); adopt only on a concrete type-position need.
- **(c) Abstract `P<T>` head — DEFER behind a NAMED diagnostic; escalate.** Confirms D2. Upgrade the current
  generic `8-0030 NotValue` to a NAMED rejection ("constraint-constructor generic params not yet supported —
  monomorphize per trait"), so a library author learns the language's shape (honors D5.5).
- **Two guardrails to land alongside the wiring (both cheap, no representation work):**
  **G1** error quality — missing-capability/unsatisfied-`require` diagnostics name the specific capability and
  target/field path (Effect-Maze-for-providers). **G2** provenance (decision-packet D3, minimal) — generated
  impls carry a minimal `Evidence` record (provider id + discharged `require<…>` obligations) via the existing
  receipt path; reserve the `Evidence` payload slot (`premises: Vec<CheckPremise>`, D6 FV) so it is additive.
  G2 subsumes the P50 provenance follow-up (K04a-C4).
- **Futures note:** the proof-compiler extension point is `Evidence`'s *payload*, NOT the constraint's
  type-kind — so the projection is enough for the CTCubFe/SMT-Lean era. `own` linearity keeps future proof
  transport unforgeable.

## Open / flagged (need a written call before they calcify)
1. **`ConstraintListId`-primary (M5 port map `:90`) vs `TraitInstId`-primary (K03/K04 map `:55-64`)** —
   newer governs; reconciliation (const-predicates need heterogeneous `ConstraintListId`; trait-application-head
   rides `TraitInstId`) is **not stated anywhere** — a real gap.
2. **AssocConst subject key**: landed `TraitInstId` vs spec `(TyId-subject, item)` — deferred to next
   `TERM_LANG_VERSION` bump; currently favors `TraitInstId` (informs the `where Eq<T>` choice).
3. **Abstract `P<T>` head representation + Self-convention** (D2) — undesigned. **RESOLVED-AS-DEFERRED by D7(c):**
   defer behind a named diagnostic, escalate before building; not solo. No longer "needs a call" — the call is
   "named-reject + escalate."
4. **`Evidence<C>` arg at K04a (opaque) vs K04b (kinded)** — **RESOLVED by D7(b):** ship the projection; the
   kinded form is `Evidence<TraitInstId-goal>` over `PredicateListId`, no `ConstraintTerm`. (Decision packet's
   DECISION REQUESTED is now answered: opaque→kinded via projection.)

## Net direction (grounded)
Proceed: **W-B** (`where Eq<T>` → `TraitInstId`, Self=first-arg, projection — D1/D3) + **W-C** (capabilities
carry their goal `TraitInstId`, provider selection by identity — D4) on the existing `PredicateListId`/
`term.rs` substrate, honoring D5; **escalate the abstract `P<T>` head (D2)**; keep `ConstraintTerm`
off the critical path (D1 caveat). FV says the substrate is sound; the kinds are design-grounded.

### Sources
fable bundle `/workspace/fe-sessions-bundle-2026-06-13/` (transcripts + 100 subagent threads under
`*/subagents/`, generated-docs, MANIFEST); FV at `/workspace/fe-lean-litmus-2026-06-11.lean`,
`/workspace/z3-vc-census/` + `/workspace/fe-vc-census-z3-2026-06-11.md`, `/workspace/fe-assurance/`;
design corpus `fe-m5-normalized-term-spec-2026-06-10.md`, `fe-term-assocconst-unified-spec-2026-06-11.md`,
`fe-m5-const-predicates-port-map-2026-06-11.md`, `fe-m5-course-corrections-2026-06-12.md`,
`fe-obligations-docs-review-2026-06-09.md`; fco repo docs `FCO_K03_K04_EXECUTION_MAP.md`,
`FCO_CONSTRAINT_KIND_REVIVAL.md`, `FCO_DECISION_PACKET_typed_provider_capabilities.md`,
`FCO_CONSOLIDATION_MAP.md`, `FCO_BRIDGE_AND_REIFICATION_TARGETS.md`.
