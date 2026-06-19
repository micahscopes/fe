# FCO Probe — Provider-Goal Representation (Level 1 viability spike)

> **SUPERSEDED — the narrow `CapabilityGoal` carrier this spike probed was built then DELETED, replaced by `TyData::ConstraintTerm` (R1–R3 landed, `71bd772c0`). Historical only.** SSOT: `FCO_THE_SLIDE_2026-06-19.md`.

**Status:** SPIKE COMPLETE (throwaway). **Date:** 2026-06-16.
**Question:** Can a narrow, compile-time-only `CapabilityGoal` carrier let a derive
provider's capability/witness goal argument (`Eq<T>` in `Evidence<Eq<T>>` /
`ImplBuilder<Eq<T>>`) be REAL-kind-checked as a CONCRETE constraint — WITHOUT a
general `TyData::ConstraintTerm` variant and WITHOUT the trait solver ever seeing
a live variable head?
**Companion spec:** `FCO_DECISION_PACKET_provider_goal_representation.md` (Level 0/1/2).

---

## VERDICT: **VIABLE** — with one load-bearing architectural correction to the packet.

The narrow `CapabilityGoal` carrier CAN carry the concrete goal and de-exempt the
signature **without `ConstraintTerm` and without a live head** — *provided the
goal-lowering is done by a position-scoped intercept in the ANALYSIS layer (post
scope-graph merge), not in the expansion-stage `validate_provider` the packet
names.* The packet's mechanism is right; its **placement is wrong** and would hit
a salsa cycle (the STOP-SIGN-shaped hazard, dodged by relocating the check).

Three empirical facts decide it:

1. **The concrete projection works.** Feeding the *inner* HIR type `Eq<T>`
   (extracted from the witness param's `Evidence<…>` by resolved position) into
   the existing W-B `lower_hir_constraint_application` yields exactly
   `TraitInstId{Eq, [T]}` (`T: Eq`, Self = T), kind-checked, **with no
   `ConstraintTerm` and no live head ever constructed** — directly tested, green.
2. **Every forbidden shape is declined to `None`** by that same lowering (missing
   trait, unsaturated `* -> Constraint` head, live `* -> Constraint` param head).
   `None` is the carrier's "not a concrete constraint" verdict, which the
   diagnosable layer renders as a typed diagnostic.
3. **But the ordinary type checker cannot be the one doing it.** If you simply
   stop exempting the signature and let the normal checker walk `Evidence<Eq<T>>`,
   it rejects `Eq<T>` *as a type application* (`2-0011 incorrect number of
   generic arguments for Eq; expected 0, given 1`) before any constraint lowering
   runs. So Level 1 must **intercept the `Evidence`/`ImplBuilder` argument
   position and route it to `lower_hir_constraint_application`, bypassing the
   ordinary `*`-kinded walk for that one slot.** That intercept is the whole
   design; it is exactly the packet's "position-scoped projection, not a general
   `Constraint -> *` constructor" — and the spike confirms it neither needs nor
   tempts toward `ConstraintTerm`.

---

## What the code actually is (the real exemption, mapped)

The Level-0 exemption is **two `is_derive_provider_fn`-keyed skips in the analysis
layer**, plus the fact that `Evidence` is an undeclared type:

- `crates/hir/src/analysis/ty/mod.rs:721` — `FuncAnalysisPass` skips
  `func.diags(db)` (the **signature** checker) for provider derive fns.
- `crates/hir/src/analysis/ty/mod.rs:389` — `BodyAnalysisPass` skips
  `check_func_body` for provider derive fns (out of scope here; TD5).
- `Evidence` is declared **nowhere** (`ingots/core/src/derive.fe` declares only
  `Reflect<T>` and `ImplBuilder<G>`, both with `*`-kinded params); it exists only
  as `Value::Evidence`, a typeless opaque executor unit
  (`crates/hir/src/core/lower/provider_executor.rs:336,394`).

`Func::diags()` (`crates/hir/src/diagnosable.rs:1419`) checks `diags_param_types`
(the witness `ev: Evidence<…>`), `diags_return`, and where-clause predicates. It
does **NOT** check the `uses`-clause effect key types — so `ImplBuilder<Eq<T>>`
and `Reflect<T>` are not even reached by the signature checker today. The witness
parameter `Evidence<Eq<T>>` is therefore the de-exemption's first contact surface.

The W-B precedent (`lower_hir_constraint_application`,
`crates/hir/src/analysis/ty/trait_lower.rs:337`) is **not** called at provider
expansion time. `require<Trait>` works by emitting a *syntactic* `where Eq<T>`
predicate into the **generated impl's** HIR (`provider_synthesis.rs:135`
`requirement_where_clause`), which is lowered later when that generated item is
ordinarily type-checked — after the scope graph is merged, no cycle. This is the
template Level 1 should follow: do the constraint lowering in the analysis layer,
not in the `HirDb`-only expansion stage.

---

## The code shape I used (and the corrected carrier home)

### The carrier (unchanged from the packet — narrow, sidecar, no `TyData`)
```rust
enum CapabilityGoal<'db> {
    ConcreteTrait(TraitInstId<'db>),       // Eq<T>  -> TraitInstId{Eq,[T]}, Self = T
    PredicateList(PredicateListId<'db>),   // Encode<T> + Decode<T>
    // (AliasExpanded deferred per spike scope)
    // NOTE: no variable-head variant by construction => no live head representable.
}
```

### Where the goal is lowered (CORRECTION to the packet)
The packet says wire `CapabilityGoal` onto `Capability`
(`crates/hir/src/core/lower/provider.rs:51`, populated in `validate_provider`).
**That cannot work:** `validate_provider` runs in the expansion stage on `HirDb`
and must never read the merged scope graph. `lower_hir_constraint_application` ->
`resolve_path` (`name_resolution/path_resolver.rs:695`) reaches
`ScopeId::scope_graph` -> `TopLevelMod::scope_graph` ->
`lower::scope_graph_impl` (the **merged** graph,
`crates/hir/src/core/hir_def/item.rs:463-464`). For a provider declared in the
requesting ingot's module, calling that from expansion re-enters
`scope_graph_impl` of that same ingot — the cycle the stratification doc forbids
(`expansion.rs:13-19`). Additionally `TraitInstId`/`PredicateListId` are
analysis-layer (`HirAnalysisDb`) types; `Capability` is an expansion-layer
(`HirDb`) type, so storing them there is a layering inversion.

**Correct placement:** the goal lowering + kind-check belongs in the **analysis
layer**, as a provider-signature pass (or a branch in `Func::diags()` /
`FuncParamView::ty_diags`), gated on `is_derive_provider_fn`. There:
- the merged scope graph is legitimately available (no cycle — it is a normal
  post-merge analysis pass, just like every other `Func::diags()` call);
- `TraitInstId`/`PredicateListId` are in-layer;
- the position-scoped intercept replaces the ordinary `collect_hir_ty_diags`
  walk for the `Evidence<…>` / `ImplBuilder<…>` argument slot.

### How it stays position-scoped (the discipline, satisfied)
The intercept recognizes the capability/witness *position* by resolved identity
(the K04a machinery — `path_names_derive_capability` / `canonical_trait_path`),
extracts the ONE inner type argument as a `HirTypeId`, and lowers **that** via
`lower_hir_constraint_application`. `Evidence`/`ImplBuilder` are NEVER modeled as
`Constraint -> *` constructors that the ordinary kind-checker applies; the inner
`Eq<T>` never travels through `*`-kinded `lower_hir_ty`. So `Eq<T>` never needs to
be a kind-`Constraint` `TyId`, and `ConstraintTerm` is never reached.

### Exact extraction code used in the probe (load-bearing)
```rust
let ev = func.params(db).next().unwrap();           // witness param `ev`
let ev_ty = ev.hir_ty(db).unwrap();                  // Evidence<Eq<T>>  (HIR)
let ev_ty = strip own-mode wrapper;
let TypeKind::Path(p) = ev_ty.data(db);              // path `Evidence<Eq<T>>`
let GenericArg::Type(ta) = p.generic_args(db)[0];    // the ONE arg: Eq<T>
let goal_hir = ta.ty;                                // inner HIR type Eq<T>
lower_hir_constraint_application(db, goal_hir, func.scope, empty_assumptions)
//  -> Some(TraitInstId{Eq,[T]}) for the concrete goal, None for everything else
```

---

## Per-case results (empirically observed, exact diagnostics)

Two probes were run. **Probe 1** removed the `FuncAnalysisPass` exemption and let
the ORDINARY checker see the de-exempted signature (shows why a naive de-exempt
fails). **Probe 2** is the position-scoped lowering — feed the inner `Eq<T>`
straight into `lower_hir_constraint_application` (the proposed Level-1 path).

| Case | Probe 1 (naive de-exempt, ordinary checker) | Probe 2 (position-scoped lowering) |
|---|---|---|
| **Positive** `Evidence<Eq<T>>` | **FAILS** `2-0011 incorrect number of generic arguments for Eq; expected 0, given 1` — ordinary checker treats `Eq<T>` as trait-in-type-position | **`Some("T: Eq")`** = `TraitInstId{Eq,[T]}`, kind-check OK ✅ |
| **Neg A** `Evidence<Bogus<T>>` (undeclared) | `2-0002 \`Bogus\` is not found` (today: **silently compiles**) | `None` (declined) — would be a typed resolution diag ✅ |
| **Neg B** `Evidence<Eq>` (unsaturated `*->Constraint`) | `2-0006 expected type item here` | `None` (declined — no subject arg) ✅ |
| **Neg C** `Evidence<P<T>>` (live `*->Constraint` param head) | `3-0001 invalid type argument kind` | `None` (declined — abstract head not concrete) ✅ |
| **Neg D** runtime `Evidence<…>` value | structurally impossible (see below) | structurally impossible (see below) |

Probe 1 also showed: before `Evidence` is declared at all, **all** cases collapse
to `2-0002 \`Evidence\` is not found` — confirming the witness type is undeclared
and must be given a `*`-kinded declaration before its argument can even be
reached. That declaration is part of Level 1's cost.

**Neg D (runtime evidence value) — enforcement point:** structurally impossible,
no new diagnostic needed. `Evidence` is declared **nowhere** in `core`/`std`
(only *used* in provider signature position, e.g. `std/src/eip712.fe:251`).
`Value::Evidence` (`provider_executor.rs:336`) is a typeless opaque unit bound to
provider params at executor start (`:394`), never constructed, with no field and
no constructor; it cannot escape the restricted command language as data. So
binding/returning an `Evidence<…>` as a runtime value is unrepresentable. **The
enforcement point is the executor's value model + the absence of any `Evidence`
type declaration** — Level 1 must keep `Evidence` provider-only (not prelude, no
public constructor) to preserve this, exactly as `Reflect`/`ImplBuilder` already are.

**`derived_eq_default`:** baseline 3/3 PASS (verified before edits). Both probes
left the production exemption intact at the end (de-exempt edit reverted), so the
fixture is unaffected; the slow CLI suite was not re-run for a confirmed no-op.

All probe assertions pass (`cargo test -p fe-hir --features testutils
provider_goal_spike` / `spike_position_scoped` — green). `cargo build -p fe-hir`
clean after revert.

---

## Why this is NOT a STOP-SIGN (and where the STOP-SIGN would have been)

The STOP-SIGN trigger the prompt warned about is "the code demands a general
constraint-as-`TyId` / `ConstraintTerm`." That demand appears **only on the naive
path** (Probe 1): to make the *ordinary* checker accept `Evidence<Eq<T>>`, `Eq<T>`
would have to be a kind-`Constraint` `TyId` sitting in `Evidence`'s `*`-slot —
i.e. `ConstraintTerm`. The exact line that would demand it is the ordinary
HIR-type walk at `crates/hir/src/core/semantic/mod.rs:1207-1208`
(`collect_hir_ty_diags`), which visits `Eq<T>` as a generic arg in type position
and resolves `Eq` to a 0-arg trait (`2-0011`).

The position-scoped intercept **side-steps that line entirely** by never sending
the inner `Eq<T>` through `collect_hir_ty_diags`; it sends it through
`lower_hir_constraint_application` instead, which produces a `TraitInstId` (already
in the IR) and never a `TyData` node. So the STOP-SIGN is avoided precisely by
honoring the position-scoping discipline — which is why the discipline is the
whole point.

One honest caveat: position-scoping means `Evidence<Eq<T>>` is **not** "checked
like ordinary Fe" in the literal sense — it is a *provider-only position the
checker is taught to route specially*, not a type the general type system can
represent. This is the intended semantics of Level 1 (the packet's Q3/Q4: the
carrier is legal only in provider capability/witness positions), but it should be
named plainly: Level 1 replaces "exempt the whole signature" with "kind-check the
goal argument via a dedicated provider-position rule," not with "the goal is now
an ordinary type."

---

## Effort / blast radius to land Level 1 for real

Small-to-moderate, all confined to the analysis + provider-lowering layers. No
`TyData` churn, no solver changes, no broad salsa `Update` ripple.

| File | Change | ~LoC |
|---|---|---|
| `ingots/core/src/derive.fe` | declare `struct Evidence<G>` (`*`-kinded, private field, no ctor — mirror `ImplBuilder`); keep out of prelude | ~4 |
| new: `crates/hir/src/analysis/ty/provider_goal.rs` (or branch in `diagnosable.rs`) | `CapabilityGoal` enum + a salsa-tracked `provider_capability_goals(func)` that, gated on `is_derive_provider_fn`, recognizes `Evidence`/`ImplBuilder` positions by K04a identity and lowers each ONE arg via `lower_hir_constraint_application`; `None` -> typed diag | ~120-180 |
| `crates/hir/src/diagnosable.rs` (`Func::diags` / `FuncParamView::ty_diags`) | for provider derive fns, route `Evidence<…>` witness param (and optionally `ImplBuilder<…>` uses-key) to the new check; bypass `collect_hir_ty_diags` for that slot | ~40-60 |
| `crates/hir/src/analysis/ty/mod.rs` | un-skip provider derive fn **signatures** in `FuncAnalysisPass` (line 721) once the position-scoped rule exists; keep the **body** skip (line 389) | ~2 |
| `crates/hir/src/analysis/ty/diagnostics.rs` | one or two diagnostic variants (or reuse `2-0002`/`2-0006`/`6-0008`) for non-concrete provider goal | ~20-40 |
| `crates/fe/tests/fixtures/fe_test/` | positive + Neg A-D fixtures per §4 of the packet | fixtures |

- **Salsa/Update churn:** none structural. `CapabilityGoal` derives the same as
  `Capability` (`Debug, Clone, Copy, PartialEq, Eq, salsa::Update`); `TraitInstId`
  (interned, Copy/Update) and `PredicateListId` already satisfy it.
- **Layering note:** do NOT put `CapabilityGoal` on `provider::Capability`
  (`HirDb`/expansion layer) — it would force an analysis-layer type below the
  analysis layer and risk the merge cycle. Keep it as an analysis-layer query
  keyed by `Func`.
- **The `uses`-clause `ImplBuilder<Eq<T>>` is a bonus, not free:** `Func::diags()`
  does not currently check effect key types at all, so kind-checking the
  `ImplBuilder` goal is *additional* surface (decode `EffectParam.key_path`'s
  generic args), not a reuse of an existing walk. The witness `Evidence<…>` is the
  minimum viable de-exemption.

---

## Surprises

1. **The packet's proposed wiring point is unbuildable as written.** Putting the
   carrier on `Capability` in `validate_provider` (expansion stage, `HirDb`) cannot
   call `lower_hir_constraint_application` — it cycles through the merged scope
   graph and inverts the layer stack. The packet treated "reuse W-B" as
   placement-neutral; it is not. W-B for `require<Trait>` works *because* it defers
   to the generated item's later, post-merge type-check — not because it lowers at
   expansion time.
2. **The de-exemption's FIRST failure is `Evidence` being undeclared
   (`2-0002`), not the `2-0006`/`ConstraintTerm` issue the packet predicted.** The
   constraint-as-decoration problem is real but it is the *second* wall; you must
   first declare `Evidence` as a `*`-kinded type. Once declared, the positive then
   fails with `2-0011` (trait-in-type-position), which is the constraint-carrier
   problem in its true form.
3. **`Func::diags()` never checks the `uses` clause.** The capability goals
   (`ImplBuilder<Eq<T>>`, `Reflect<T>`) are not kind-checked by the signature pass
   at all today; only the witness param (`Evidence<…>`) and where-clauses are. So
   the witness parameter — not the `uses` clause — is where de-exemption bites
   first, and the `uses`-clause check is net-new work rather than un-skipping.
4. **The negatives need no special machinery.** `lower_hir_constraint_application`
   already declines (`None`) the live head, the unsaturated head, and the missing
   trait — the exact safety surface Level 1 needs, returned by the *same* function
   that produces the positive. The carrier never has to represent a forbidden
   shape to reject it.

---

## Spike artifacts (throwaway, in this worktree)

- `crates/hir/src/analysis/ty/trait_resolution/constraint.rs` — added test module
  `provider_goal_spike` (Probe 1 diag-code observation + Probe 2 position-scoped
  lowering assertions). Both green.
- `crates/hir/src/analysis/ty/mod.rs` — de-exemption edit was applied for Probe 1
  then **reverted**; tree builds clean with the production exemption restored.
