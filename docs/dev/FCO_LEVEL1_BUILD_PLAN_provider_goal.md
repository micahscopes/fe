# FCO Level 1 — Build Plan (provider-goal `CapabilityGoal`, "W-D")

> **OUTCOME 2026-06-19 — obsolete: this `CapabilityGoal` ("W-D") build plan was superseded by `TyData::ConstraintTerm` (R1–R3 landed); the `CapabilityGoal` representation was deleted. Historical only.** SSOT: `FCO_THE_SLIDE_2026-06-19.md`.

**Status:** READY TO EXECUTE — **gated on architect approval** (de-exempting provider signatures
changes accepted-program semantics: shapes that silently compile today start erroring). Do not start
the semantics-flipping steps without sign-off.
**Date:** 2026-06-16. **Grounds:** spike `FCO_PROBE_provider_goal_representation.md` (VIABLE) +
packet `FCO_DECISION_PACKET_provider_goal_representation.md` (§0.5 placement correction, §4 fixtures).
**Verdict carried in:** VIABLE, ~190–290 LoC, analysis + provider-lowering layers only, **no `TyData`
churn, no solver change, no structural salsa/`Update` ripple.**

This plan turns the spike's findings into an ordered, verifiable build. Each step states its files,
whether it is *decision-free* or *semantics-flipping*, and its check. The carrier invariant is already
locked (`provider_capability_goal_lowers_concrete_rejects_nonconcrete`, landed `bd10ec966`), so the
representation contract is fixed before any wiring moves.

## Design recap (what we are building)

A compile-time-only, analysis-layer carrier:
```rust
enum CapabilityGoal<'db> {
    ConcreteTrait(TraitInstId<'db>),                                       // Eq<T>
    PredicateList(PredicateListId<'db>),                                   // Encode<T> + Decode<T>
    AliasExpanded { alias: PathId<'db>, expanded: PredicateListId<'db> },  // Serializable<T> (Tier A)
}
```
produced by a query `provider_capability_goals(func) -> Vec<(Capability, CapabilityGoal | GoalError)>`
that **intercepts the `Evidence`/`ImplBuilder` argument position by resolved identity (K04a) and lowers
the ONE inner type via the existing `lower_hir_constraint_application`** — never via the ordinary
`*`-kinded type walk (that walk is what would demand `ConstraintTerm`; spike fact #3). No live head is
representable; the solver only ever sees the eliminated `TraitInstId`/`PredicateListId`.

## Ordered steps

### Step 0 — feature gate (decision-free, recommended)
Land the wiring behind an off-by-default toggle (a `const` or cfg in the new query module) so Steps
2–5 can land incrementally without flipping behavior until the gate is flicked. Lets the build and its
fixtures merge as green no-ops, then flip in one reviewed commit. **Check:** suite unchanged with gate
off.

### Step 1 — declare `Evidence<G>` (SEMANTICS-ADJACENT — needs approval; it is a public API addition)
`ingots/core/src/derive.fe`: add `pub struct Evidence<G> { handle: u256 }` alongside `Reflect`/
`ImplBuilder` (NOT in prelude; private field; no constructor — same shape, same rationale). Reason it is
first: the spike found de-exemption's *first* failure is `2-0002 Evidence is not found`; nothing else
can be checked until `Evidence` resolves. **Risk:** must be a no-op for accepted programs while
signatures stay exempt — verify the full suite stays green with Step 1 alone (Steps 2+ gated off).
**Check:** `cargo test -p fe` derive fixtures + `fe-uitest` green; `Evidence` resolvable only as
`core::derive::Evidence`.

### Step 2 — `CapabilityGoal` + `provider_capability_goals(func)` query (decision-free; new analysis code)
New analysis-layer module (e.g. `crates/hir/src/analysis/ty/provider_goal.rs`). The query:
1. confirms `func` is a derive-provider `derive` fn;
2. for the witness param and each `uses` capability, recognizes the `Evidence`/`ImplBuilder` position by
   resolved identity (reuse `provider.rs` `path_names_derive_capability` / `canonical_trait_path`, K04a);
3. extracts the single inner HIR type argument (the spike's `inner_goal_hir_ty`: strip `Mode`, take
   `TypeKind::Path` → first `GenericArg::Type`);
4. lowers it via `lower_hir_constraint_application(db, inner, scope, PredicateListId::empty_list)` →
   `Some(TraitInstId)` becomes `CapabilityGoal::ConcreteTrait`; `None` becomes a typed `GoalError`
   (missing / unsaturated / live-head — distinguish for diagnostics).
**MUST run post scope-graph merge** (analysis layer), NOT in expansion-stage `validate_provider` —
`lower_hir_constraint_application → resolve_path → scope_graph_impl` would salsa-cycle (spike finding
#1). `CapabilityGoal` is keyed off the `Func`, NOT stored on the expansion-layer `Capability` enum
(layer inversion). **Check:** unit tests mirroring the landed carrier test, asserting the query returns
`ConcreteTrait{Eq,[T]}` for the positive and the right `GoalError` per negative.

### Step 3 — diagnostics for `GoalError` (decision-free; additive)
`diagnosable.rs` + `analysis/ty/diagnostics.rs`: render `GoalError::Missing` (resolution), `Unsaturated`
(arity/kind), `LiveHead` (the abstract-head boundary — reuse the existing `6-0008`
`ConstraintCtorParamUnsupported`). Name the capability + the goal spelling (the spike noted the witness
`Evidence<…>` bites first; the `ImplBuilder<…>` uses-key check is **net-new**, `Func::diags()` does not
currently visit the `uses` clause — add that visit). **Check:** snapshot tests for each code.

### Step 4 — de-exempt the signature (THE SEMANTICS FLIP — needs explicit approval)
`crates/hir/src/analysis/ty/mod.rs:721`: stop skipping the provider *signature* for the witness/uses
goal positions; route those positions through `provider_capability_goals` (emitting Step-3 diagnostics)
instead of the ordinary `collect_hir_ty_diags` `*`-kinded walk (which rejects `Eq<T>` as a type
application, `2-0011`, spike fact #3). **Keep the body exemption at `:389`** (that is TD5, separate).
Flip the Step-0 gate here. **Check:** the §4 end-to-end fixtures (below) flip from compile→error;
existing derive fixtures stay green (providers name *concrete* goals, so they pass the new check).

### Step 5 — end-to-end fixtures (lands with Step 4)
Author the packet §4 set as real `.fe`/`.snap` (the carrier unit test already covers the representation
level):
- **Positive** `provider_signature_concrete_goal_checked.fe` — `Evidence<Eq<T>>`/`ImplBuilder<Eq<T>>`
  checked as concrete; derive still executes (end-to-end exec fixture passes).
- **Neg A** live head `Evidence<P<T>>` → `6-0008`.
- **Neg B** `Evidence<MissingTrait<T>>` → resolution diag (this is the `Bogus<T>` case from §1 — the
  intentional flip from "compiles" to "errors").
- **Neg C** `Evidence<Eq>` → arity/kind diag.
- **Neg D** — documented invariant (no runtime `Evidence<C>`); no new fixture needed.

### Step 6 — retire what Level 1 retires (decision-free cleanup, after Step 4 is green)
Per packet Q6: the goal-arg-is-decoration gap is closed; the bespoke `Eq<T>`-in-`Evidence` accept now
shares the W-B lowering. Does NOT touch K04a identity recognition, the `DERIVE_MARKER`/`DERIVE_FN`/
`DERIVE_MODULE` string shim (K04a-C3), or the body executor (TD5).

## What this plan deliberately does NOT do
- No `TyData::ConstraintTerm` (Level 2) — gated separately; only on a real abstract-head consumer.
- No live variable head to the solver — structurally impossible (`CapabilityGoal` has no such variant).
- No change to provider *body* checking (TD5) or to the trait solver.
- No prelude exposure of `Evidence` — it stays a `core::derive` capability type.

## Verification gate (M5 standard, per step)
`cargo test -p fe-hir` (carrier + Step-2 unit tests) + `fe-uitest` `ty_check` (Step-3 snapshots) + the
`derived_*` exec fixtures (Step-5 positive, end-to-end). Baseline to preserve: existing derive/ty_check
suites green; the only intended diffs are the new negatives flipping compile→error at Step 4.

## Execution note
Build via a worktree-isolated agent (the established cadence), verified + cherry-picked here. Steps
0/2/3 can land green (gated off) before approval; Steps 1/4/5 are the approval-gated semantics flip.
