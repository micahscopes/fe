# Derivers as first-class scoped provisions — gap analysis + phased plan

> **SUPERSEDED 2026-06-19 → `FCO_THE_SLIDE_2026-06-19.md`.** The "MOUNTAIN / Layer-0/1/2 / multi-quarter / decompose-by-FV-feature" framing here is the DRIFT the slide corrects (uphill vs the slide's downhill). The CODE FACTS (six-resolver file:line, the stratum-crossing, the variable-`V` cliff) remain accurate; the SHAPE/sequencing is wrong.

**Date:** 2026-06-19 · **Status:** read-only architect gap analysis (opus), grounded in code + prior docs.
HEAD `e84f30b4a`. Target: collapse `derive V for T using P` into `uses (a V-deriver for T)` /
`with (a V-deriver for T = P)` resolved by the ordinary provision system.

## Verdict: a MOUNTAIN (multi-quarter), gated on 3 bodies of work + 1 decisive scoping call

Achievable as a large program **at concrete-`V` scope** (every demand names a specific trait `V`); a
**research frontier — do NOT build — at variable-`V` scope** (a fn demanding "*some* deriver for *whatever*
trait" = the shelved abstract-head cliff: live `P : * -> Constraint` reaching selection, anti-proven, empty demand).

## The load-bearing reconciliation (what changed since prior scoping)
- **Cut-1 refinement is binding:** the solver CANNOT *select* scoped values (`FCO_RUNG3_SCOPE_REFINEMENT`).
  So "the resolver selects a deriver" MUST mean **enumerate-via-scope, verify-via-solver**, applied one level
  up to a constraint-indexed key. Buildable for concrete `V`. Any "just route derivers through the solver" plan
  is the cut-1 error (conflating selection with verification) and at variable-`V` steps onto the cliff.
- **Derivers and the provision system are two strata that never meet today:** derive selection/execution is
  HIR macro-expansion CTFE (`provider.rs`, `provider_executor.rs`); effects are type-check trait solving
  (`effect_env.rs`). A deriver isn't even a *value* — it's a string-marked `impl` (`DERIVE_MARKER`, `provider.rs:31`)
  run by a bespoke interpreter with hardcoded capabilities (`provider_executor.rs:941-948`).

## Phased plan (strictly sequenced; each layer multi-rung)

### Layer 0 — `Derive` graduation (the big unbuilt prerequisite: derivers become real trait values)
Below the landed kind layer (`Constraint`/`ConstraintTerm`/`TraitCtor`/Form-2 chosen), this is unbuilt.
- **L0.1 TD1/TD2** substitution-on-instantiation (`P:=V` pinned at `impl Derive<V> for P`). MED, test.
- **L0.2 TD4** `Derive` an ordinary trait — delete `DERIVE_MARKER` recognition (`provider.rs:31,149-158`).
  HIGH (load-bearing string-path deletion; Micah signoff).
> **⚠️ SUPERSEDED (2026-06-21) — see `FCO_THE_SLIDE_2026-06-19.md` "KEYSTONE INSIGHT".** The framing below ("ordinary CTFE" / "executor → CTFE de-magic" / "provider bodies become ordinary effectful CTFE") describes engine **fusion** and is superseded. The settled decision is **stage, don't fuse** (twice-measured): the executor is a **quasiquoter backend** (GenExpr→HIR, not a value-evaluator) run as a **downstream salsa query** producing a real `impl`; it is NOT folded into the CTFE value-evaluator (CTFE-inside-the-solver = Salsa-cycle ICE, a measured dead-end). Near-term the **cascade SELECTS** among existing impls; the keystone later **RUNS** a deriver to **GENERATE** one — distinct steps.

- **L0.3 TD5** executor → ordinary CTFE; capabilities bound through the provision env, not hardcoded `Value`s
  (`provider_executor.rs:941`). **First crack in the expansion↔type-check stratum boundary.** HIGH, largest blast radius.

### Layer 1 — override-safety substrate (scoped shadowing + `Fix`/`fixed`) — none built, policy-gated
- **L1.1 canonical-marker mechanism (PS5)** — the money-soundness gate; none exists (`FCO_PROVISION_REVIEW:A1` "grep empty"). **Micah policy.**
- **L1.2 solver-level scoped shadowing (non-canonical)** — inner scope selects a different `ImplementorId`. NOT byte-identical; must write the override `ImplementorId` into `ImplEnv` so 3.3's determinism assertion covers it. HIGH (coherence: REPLACE candidate set → `Unique`, not ADD → `Ambiguous`).
- **L1.3 `Fix`/`fixed` + `fix` verb** — `FCO_FIX_CAPABILITY_PACKET_2026-06-19.md`; decisions A–D; non-ambient propagation. HIGH; lattice FV-tests.

### Layer 2 — derivers as provisions (the target; built on L0+L1)
- **L2.1 `with (a V-deriver for T = P)`** — a `Derive<V>`-typed value in an `EffectEnv` frame; selection stays
  lexical (enumerate-via-scope), then verify. Needs L0.3 stratum-crossing so the runner reaches the frame. MED–HIGH.
- **L2.2 `uses (a V-deriver for T)` (concrete `V`)** — constraint-indexed capability, `V` pinned concrete
  (concrete-before-the-solver). Each `(V,T)` a saturated key. MED.
- **L2.3 variable-`V` (THE CLIFF) — DO NOT BUILD** unless a Lean result for variable-headed solving AND a real
  consumer both appear. Named-reject (`6-0008`). `FCO_ABSTRACT_HEAD_RESEARCH_DOSSIER`.

## Hardest novel piece
**The stratum-crossing (L0.3):** how the type-check-stage `EffectEnv` reaches the expansion-stage deriver
runner without breaking stratification (`provider.rs:13-14`; merged-graph read would cycle). No precedent in the codebase.

## Human decisions
1. **Concrete-`V` vs variable-`V` scope** — THE decision; determines bounded-program vs research-frontier. *Rec: concrete-`V`* (variable-`V` demand is empty).
2. **Canonical-marker policy (PS5/A1)** — property-derived vs enumerated; global vs contract-scoped. Money-soundness gut-call.
3. **`Fix`/`fixed` A–D** (Fix packet).
4. **Ship Layer 0 (`Derive` graduation) independently first?** — valuable standalone (de-magics executor, deletes marker), prerequisite regardless. *Rec: yes.*
5. **Stratum-crossing architecture (L0.3)** — genuine open design question.

## Critical files
- `crates/hir/src/core/lower/provider.rs` (`DERIVE_MARKER:31`, `select_provider:884`, `goal_matches_provider:775`)
- `crates/hir/src/core/lower/provider_executor.rs` (bespoke executor, hardcoded caps `:941-948`)
- `crates/hir/src/analysis/ty/ty_check/effect_env.rs` (`lookup_precise:162`), `expr.rs` (`check_with:1157`, `evaluate_unkeyed_trait_provider:1914`, `trait_effect_goal_satisfiability_in_scope:2331`)
- `crates/hir/src/analysis/ty/ty_check/env.rs` (`ProvisionEnv:61`), `ingots/core/src/derive.fe` (caps to graduate)
