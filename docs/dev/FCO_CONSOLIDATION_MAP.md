# FCO Consolidation Map

This document is a repo-grounded map of features in the Fe compiler that
already share — or could be migrated onto — one common obligation/evidence
pathway, plus **one** recommended first implementation task. It is a planning
artifact, not a rewrite. Every claim below cites a file/function/line in the
`first-class-obligations` branch; where the line drifts from the prior
investigation it is noted inline.

## The principle

The recurring shape across many Fe features is:

> **demand → obligation/provision lookup-or-discharge → evidence → consumer**

Something *demands* a fact (a call needs a trait impl; a `where` predicate must
hold; a derive needs a capability; an ABI path needs a static layout). The
demand becomes an **obligation/provision** that is either *looked up* (trait
impl selection) or *discharged* (CTFE of a const predicate, proof, scope/
authority check). Success produces **evidence** — a typed record of *how* the
goal was committed — which a **consumer** (lowering, LSP hover, optimizer,
diagnostics) reads.

The goal is **NOT** "everything becomes a const predicate" and **NOT** "one
solver." It is: *more features become producers/consumers of the same
obligation/evidence pathway, with specialized discharge backends*
(trait = lookup via `ProofForest`; const predicate = CTFE; capability =
scope/authority), so that trait-only, ad-hoc, and string-keyed parallel logic
can be retired.

M5 already built the spine for one backend:

- The shared queue: a single `Vec<DeferredTask>` holding both
  `DeferredTask::Obligation(TraitObligation)` and
  `DeferredTask::ConstPredicate(ConstPredicateObligation)`
  (`ty_check/env.rs:1601-1606`), drained by `resolve_deferred`
  (`ty_check/mod.rs:1473`).
- The const-predicate backend: `process_const_predicate_obligation`
  (`ty_check/mod.rs:1387`) evaluates a closed predicate body by CTFE under the
  call's type substitution.
- The evidence: `DischargedConstPredicate { origin, predicate, generic_args,
  route: DischargeRoute::Ctfe, premises: Vec<CheckPremise> }`
  (`ty_check/mod.rs:3006-3018`), recorded by `record_discharged_const_predicate`
  (`ty_check/mod.rs:1458`), with `DischargeRoute` (`:2955`, only `Ctfe` today)
  and the reserved `CheckPremise` slot (`:2971`).
- The (single) producer wiring: `Callable::enqueue_constraints`
  (`ty_check/callable.rs:759`), const-predicate arm at `:797-813`, gated on
  `CallableDef::Func` only.
- A consumer: per-call accessors `discharged_const_predicates_for_call`
  (`ty_check/mod.rs:3427`) and `discharged_obligations_for_call` (`:3405`).

The verified gaps are that **only call sites** of **functions** produce const
obligations, and **only CTFE under fully-concrete args** discharges them —
every other position (ADT construction, signatures/WF, impl-method
conformance), every other discharge route (assumption/verbatim), and every
other parallel provision path (effects, derive providers, hardcoded ABI/event/
error lowering) is outside the pathway.

---

## Immediate consolidation candidates

| # | Candidate | Current locus | Smell | Target FCO shape | Difficulty | Risk | Label |
|---|-----------|---------------|-------|------------------|-----------|------|-------|
| 1 | Const predicates beyond call sites (ADT construction + signature/WF) | `callable.rs:797` (Func-only), `expr.rs:3634` `check_record_init` (no enqueue), `trait_resolution/mod.rs:278` `check_ty_wf` / `:338` `check_trait_inst_wf` (trait-only) | A `where MIN<=MAX` on a struct/enum parses and is accepted at decl, then is **never enforced** at construction or in signature positions | Enqueue ADT `const_predicates` at construction; check them in WF/signature positions; reuse `DeferredTask`/`DischargedConstPredicate` | M | Med | **CORE_M5_NEXT** |
| 2 | Assumption-route discharge (generic caller restates a bound) | `mod.rs:1423-1425` stub returns `Discharged` silently; `term.rs:425` `lower_hir_to_term` + `:807` `normalize_term` unused by discharge | Symbolic args silently discharge with **no evidence and no exact-match diagnostic**; a mismatched forwarded bound is accepted | New `DischargeRoute::Assumption`; lower predicate to a normalized term; exact-match against caller's harvested const-predicate assumption set | M | Med | **CORE_M5_NEXT** |
| 3 | Impl-method const-predicate conformance | `method_cmp.rs:62` `compare_impl_method`, `:964` `compare_constraints` (trait-only) | An impl method may weaken/strengthen the trait method's const predicates with **no error** | Compare const-predicate sets by normalized-term identity (reuses #2 machinery) | S–M | Low | **M6_OR_POST** |
| 4 | Const-predicate WF in signature positions | `trait_resolution/mod.rs:278/338`; invoked via `ty_def.rs:630` `emit_wf_diag` | `check_ty_wf` runs trait constraints but **ignores** const predicates on the type's where clause | Fold const-predicate checking into WF (the signature half of #1) | M | Med | **CORE_M5_NEXT** (with #1) |
| 5 | Hardcoded ABI/event/error lowering vs derive providers | `core/lower/event.rs`, `error.rs` (`lower_error_abi_size_impl`/`encode_impl` ~365-439), `msg.rs` (HEAD_SIZE/IS_DYNAMIC HIR), `crates/fe/src/abi.rs:~1452` | Parallel Rust provision path: bypasses `DeferredTask`, produces **no evidence**, not extensible from Fe; HEAD_SIZE/IS_DYNAMIC are exactly what const predicates could verify | Migrate to Fe derive providers; verify layout consts via const-predicate obligations | L | High | **M6_OR_POST** |
| 6 | Typed provider `uses` capabilities | `core/lower/provider.rs` (`REFLECT_KEY`/`IMPL_BUILDER_KEY`/`DERIVE_*` string keys, `:30-35`), `provider_executor.rs` (string-keyed dispatch `:1485-1486`, budgets) | Capabilities are **string-keyed**, not typed; provider execution yields an impl but **no `DischargedObligation`** | Type `uses` signatures as compile-time-only capability obligations with evidence | L | High | **NEEDS_DESIGN** |
| 7 | LSP hover renders const-predicate evidence | `language-server/.../hover.rs:243` `discharged_obligations_footer` reads `discharged_obligations_for_call` (`:250`) but never `discharged_const_predicates` | Hover shows trait discharges but **not** const-predicate discharges, though `discharged_const_predicates_for_call` (`mod.rs:3427`) already exists | One-function extension to render route/premises | S | Low | **READY_NOW** |
| 8 | Effect witnesses as obligations | `effects.rs` (`EffectKeyKind`/`ResolvedEffectKey`), `ty_check/effect_env.rs` `EffectEnv`, `constraint.rs:34` `collect_func_effect_provider_constraints` converts `uses` → trait predicates | Parallel resolution path; effect satisfaction produces no `DeferredTask` evidence | Effect witnesses as `DeferredTask` obligations with evidence | L | High | **DO_NOT_REWRITE_YET** |
| 9 | MIR runtime-class merge | `mir/.../runtime/lower/infer.rs` `merge_runtime_class` (~1132-1211) | Hardcoded, order-sensitive class merge | Speculative; out of the obligation path entirely | L | High | **DO_NOT_REWRITE_YET** |

### Per-item notes

#### 1. Const predicates beyond call sites — **CORE_M5_NEXT**
- **Current mechanism.** Const predicates are enqueued **only** in
  `Callable::enqueue_constraints` (`callable.rs:759`), inside a
  `if let CallableDef::Func(func)` guard at `:797`. ADT construction
  (`check_record_init`, `expr.rs:3634`) enqueues **no** where-clause obligations
  of any kind — verified: the trait-obligation `register_trait_obligation`
  calls in `expr.rs` (`:3192,3464,3559,4298,4372`) are for operator/method
  paths, not record construction. Signature/WF runs through `check_ty_wf`
  (`trait_resolution/mod.rs:278`) and `check_trait_inst_wf` (`:338`), both of
  which iterate **trait** constraints only.
- **Smell / duplication.** `WhereClauseOwner` already enumerates `Struct`,
  `Enum`, `Impl`, `Trait`, `ImplTrait` (`hir_def/item.rs:378-385`) and
  `const_predicates` is a real field on every where clause
  (`hir_def/params.rs:115`). The decl-site checker `check_where_const_predicates`
  (`mod.rs:271`) already runs over **all** owners (`analysis/ty/mod.rs:402-403`)
  and *conservatively skips* parameterized predicates (`mod.rs:292`,
  `params_in_scope`), explicitly deferring them "to the use site." But that use
  site only exists for function calls. The fixture
  `where_const_predicate_bare.fe` already declares
  `struct Wrap<T> where T: Sized, T::SIZE <= 32` and `enum Opt<T> where ...` —
  accepted at declaration, enforced **nowhere**.
- **Target FCO shape.** Add a `ConstPredicateObligationOrigin::AdtConstruction`
  (and/or `Signature`) variant (`env.rs:1636`); enqueue ADT `const_predicates`
  at `check_record_init`/`check_record_init` enum arm (`expr.rs:3634,3702`); add
  a const-predicate pass inside `check_ty_wf` mirroring the trait loop at
  `trait_resolution/mod.rs:306-315`. All discharge reuses
  `process_const_predicate_obligation` and `DischargedConstPredicate`.
- **First correctness fixture.**
  ```fe
  struct Bounded<const MIN: u256, const MAX: u256> where MIN <= MAX { lo: u256 }
  fn ok()  { let _ = Bounded::<1, 4> { lo: 1 } }   // discharges
  fn bad() { let _ = Bounded::<4, 1> { lo: 1 } }   // 8-0085 at construction
  fn sig(_ x: Bounded<4, 1>) {}                     // 8-0085 at signature, never called
  ```
- **Difficulty** M. **Risk** Med — touches WF (a hot, salsa-tracked path) and
  record-init checking; must avoid double-reporting against the decl-site check.

#### 2. Assumption-route discharge — **CORE_M5_NEXT**
- **Current mechanism.** When a generic caller forwards its own type parameter
  (`fn mid<B: Platform>() where B::WORD_BITS == 256 { word_op::<B>() }`), the
  args still mention a param, so `process_const_predicate_obligation` hits the
  symbolic branch at `mod.rs:1423-1425` and returns `Discharged` **silently,
  with no evidence** (comment at `:1416-1422` names this the "verbatim-match
  route, not part of this slice"). The test
  `symbolic_assoc_const_lowers_and_forwards_without_error`
  (`tests/m5_const_predicate_discharge.rs:128`) pins exactly this
  no-false-discharge behavior, and a paired **quarantined** test
  `assumption_route_mismatch_is_rejected` (`:466-478`, `#[ignore]`) already
  encodes the target: `bad_forward<B> where B::WORD_BITS == 128` calling a
  callee requiring `== 256` must be rejected `8-0085`.
- **Smell / duplication.** A normalized-term module exists
  (`term.rs`, `TERM_LANG_VERSION = 1` at `:72`) with `lower_hir_to_term`
  (`:425`) and `normalize_term` (`:807`) — but it is **not connected** to
  discharge; `process_const_predicate_obligation` works off the raw `Body` +
  `Vec<TyId>`. The caller's own const predicates are also never harvested:
  `collect_decl_constraints_with_assumptions` (`constraint.rs:326`) gathers
  **`TypeBound::Trait` only** (`:345,376`); const predicates are dropped on the
  floor.
- **Target FCO shape.** Add `DischargeRoute::Assumption`. Harvest the caller's
  where-clause const predicates into a normalized-term assumption set (extend
  `constraint.rs:326` or add a sibling); in the symbolic branch, lower the
  obligation predicate to a normalized term and require an **exact** member of
  the assumption set (no implication, no fuzzy match — mirrors trait
  `is_query_satisfiable` set membership). Hit → record evidence with
  `route: Assumption`; miss → `8-0085`.
- **First correctness fixtures.** (positive) the `mid` forward above now records
  **one** `DischargedConstPredicate` with `route == Assumption`; (negative)
  un-`#[ignore]` `assumption_route_mismatch_is_rejected`.
- **Difficulty** M. **Risk** Med — term lowering must be total enough for real
  predicates or fall back conservatively; must not regress the
  no-false-discharge invariant.

#### 3. Impl-method const-predicate conformance — **M6_OR_POST**
- `compare_impl_method` (`method_cmp.rs:62`) / `compare_constraints` (`:964`)
  compare **trait constraints** only; an impl method can silently
  weaken/strengthen const predicates. Once #2 lands the normalized-term identity
  this is a small superset-check addition. Deferred because it depends on #2's
  term machinery and has no demo pressure yet.

#### 4. Const-predicate WF in signatures — **CORE_M5_NEXT (folded into #1)**
- This is the signature half of #1: `check_ty_wf` (`trait_resolution/mod.rs:278`,
  invoked at `ty_def.rs:630`) must run const predicates. Listed separately
  because it is the part of #1 with the highest blast radius (salsa-tracked WF),
  but it ships together with #1.

#### 5. Hardcoded ABI/event/error lowering — **M6_OR_POST**
- `core/lower/event.rs`, `error.rs` (`lower_error_abi_size_impl`/`encode_impl`
  ~365-439), `msg.rs` (generates `HEAD_SIZE` u256-sum and `IS_DYNAMIC` bool-OR
  HIR), and `crates/fe/src/abi.rs:~1452` (ABI assoc consts) form a parallel
  provision path that bypasses `DeferredTask`, records no evidence, and cannot
  be extended from Fe. The `HEAD_SIZE`/`IS_DYNAMIC` consts are precisely what a
  const-predicate obligation could **verify** (cf. the
  `where_const_predicate_abi_static_layout.fe` fixture, which already gates an
  ABI path on `T::IS_DYNAMIC == false`). Prime migration target — but it depends
  on candidate 6 (typed providers) and on #5's own large surface; not first.

#### 6. Typed provider `uses` capabilities — **NEEDS_DESIGN**
- `provider.rs` keys capabilities by head identifier string (`REFLECT_KEY`,
  `IMPL_BUILDER_KEY`, `:30-35`); `provider_executor.rs` dispatches on those
  strings (`:1485-1486`) under a step/command budget; `provider_synthesis.rs`
  replays builder commands into impl HIR (`synthesize_provider_impl`). Execution
  produces an impl but **no `DischargedObligation`** evidence. Typing `uses`
  signatures as compile-time-only capability obligations is the bridge to #5,
  but the capability grade/key system is explicitly minimal today
  (`provider.rs:176-187`) and needs a design pass before code.

#### 7. LSP hover renders const-predicate evidence — **READY_NOW**
- `discharged_obligations_footer` (`hover.rs:243`) reads
  `discharged_obligations_for_call` (`:250`) but never the **already-existing**
  `discharged_const_predicates_for_call` (`mod.rs:3427`). A second filter_map
  block rendering `route`/`premises` is a self-contained, low-risk consumer
  extension. (This is the *cheapest* item but not the highest-leverage; see
  "First recommended rewrite.")

#### 8–9. Effects / MIR — see "Not-yet candidates."

---

## Not-yet candidates (do NOT rewrite now)

- **Effect-env unification (8).** `effect_env.rs` `EffectEnv` and
  `collect_func_effect_provider_constraints` (`constraint.rs:34`) already
  *partially* converge by lowering `uses` to trait predicates. Folding effect
  witnesses into `DeferredTask` evidence is plausible long-term, but the effect
  resolution model (keys, providers, scoping) is still moving and would couple
  two large subsystems mid-flight. **DO_NOT_REWRITE_YET.**
- **Full provision-scoping.** Capability/authority scoping (who may provide
  what, where) is the back half of candidates 6/8 and has no design yet.
  **NEEDS_DESIGN**, not now.
- **MIR runtime-class merge (9).** `merge_runtime_class`
  (`mir/.../runtime/lower/infer.rs` ~1132-1211) is hardcoded and order-sensitive
  but lives entirely below type checking, outside the obligation/evidence path.
  Speculative/post-M5. **DO_NOT_REWRITE_YET.**
- **Full provider body typing.** Typing the full bodies of provider functions
  (vs. just their `uses` signatures) is a much larger effort than candidate 6
  and is not required to retire string-keyed *dispatch*. **NEEDS_DESIGN.**
- **Full EIP-712.** `ingots/std/src/eip712.fe` `StableEip712` is a real Fe
  provider already; a full EIP-712 implementation is product scope, not
  consolidation. **DO_NOT_REWRITE_YET.**

---

## Arithmetic-unchecked caveat

A const predicate proves a fact about **compile-time constants** under a known
type substitution: `B::WORD_BITS == 256`, `MIN <= MAX`, `T::IS_DYNAMIC ==
false`. It is evaluated by CTFE on a **closed** term
(`process_const_predicate_obligation`, `mod.rs:1432`).

It does **NOT** prove anything about **runtime** values. In particular, a
discharged const predicate does **not** prove that a runtime expression like
`ptr + const_offset` cannot overflow: the pointer is a runtime value; only the
offset is a compile-time constant. Compile-time **layout facts** (sizes,
alignments, "is this type statically sized") are categorically different from
runtime/resource facts (this address is in bounds, this addition does not wrap,
this resource is still live).

Therefore: **do not** propose replacing `#[arithmetic(unchecked)]` with a const
predicate. That attribute asserts a *runtime* non-overflow property that
const-predicate CTFE cannot establish. The honest future story is the
`premises` slot (`CheckPremise`, `mod.rs:2971`, empty for every M5 route by
design, `:3015-3017`): a *runtime* check-elision discharge would record, via
`premises`, exactly which prior facts (layout fact + a separately-proven range
fact, e.g. from a future resource/proof backend) it depended on. Evidence that
cannot record those dependency edges cannot be retrofitted into a sound
check-elision argument — which is the whole reason the slot exists now even
though nothing populates it. Keeping layout facts and runtime facts on the same
pathway but on **different routes with explicit premises** is what preserves
that future, *without* pretending compile-time facts discharge runtime ones.

---

## First recommended rewrite

### Choice: **Candidate 1 — extend const-predicate discharge to ADT construction + signature/WF positions** (CORE_M5_NEXT)

#### Why this one (vs. B/2 and the others)

I evaluated all three steered options against five criteria — clear before/
after, reduces special-case code, reuses M5 evidence/obligation machinery,
strong demo fixture, and no dependency on Sonatina v2 / full provider typing /
full provision scoping.

- **(B) Typed provider capabilities (candidate 6)** scores worst on
  *reuse* and *no-dependency*: the capability grade/key system is explicitly
  minimal (`provider.rs:176-187`) and labeled **NEEDS_DESIGN**; it would need a
  design pass and produces no `DischargedConstPredicate`, so it does not reuse
  the M5 evidence type. Strong eventual payoff (it unlocks candidate 5), but not
  a *first* step.
- **(C) Assumption route (candidate 2)** is genuinely attractive — it has a
  pre-written quarantined fixture (`assumption_route_mismatch_is_rejected`) and
  reuses `term.rs`. But it is the *narrower* win: it generalizes the
  **discharge backend** for one already-wired position (function call sites). It
  does not broaden *where obligations are produced*, and it carries the risk
  that `lower_hir_to_term` must be total enough for arbitrary predicates.
- **(A) ADT construction + signature/WF (candidate 1)** best matches the
  consolidation thesis: it makes **more positions producers** of the *same*
  obligation, reusing `DeferredTask`, `process_const_predicate_obligation`, and
  `DischargedConstPredicate` essentially unchanged. The structural plumbing
  already exists (`WhereClauseOwner` covers Struct/Enum, `const_predicates` is a
  real field, the decl-site checker already deliberately defers parameterized
  predicates "to the use site" at `mod.rs:292`). It closes a **visible
  soundness hole** demonstrated by an in-tree fixture (`Wrap`/`Opt` where
  clauses accepted but unenforced). And it has the strongest demo:
  `Bounded<4,1>` rejected at construction **and** at a signature it never calls.

The prior reviewer leaned A-then-B; I concur on **A first**, but I rank **C
(assumption route) as the immediate follow-up** rather than B, because C also
reuses the M5 evidence machinery and has its fixture already written — together
A + C make const predicates a first-class obligation at *every position* and via
*both routes*, which is the cleanest consolidation milestone before touching the
provider/ABI surface.

#### Before / after

- **Before.** A const predicate is enforced **iff** it sits on a `fn` *and* that
  `fn` is *called* with fully concrete args. `struct Bounded<...> where MIN<=MAX`
  is accepted, then `Bounded::<4,1>{...}` and `fn f(_ x: Bounded<4,1>)` compile
  silently.
- **After.** A const predicate is enforced at ADT construction and in signature/
  WF positions too, via the same obligation queue and the same
  `DischargedConstPredicate` evidence. `Bounded::<4,1>` and the signature usage
  fail with `8-0085`; `Bounded::<1,4>` discharges and records evidence.

#### Exact files to touch

1. `crates/hir/src/analysis/ty/ty_check/env.rs` — add
   `ConstPredicateObligationOrigin::AdtConstruction { call_expr, adt, predicate_idx }`
   (and a `Signature`/`Wf` origin if WF needs to key evidence) near `:1636`;
   extend `DischargedConstPredicate::call_expr` match (`mod.rs:3022`)
   accordingly (currently exhaustive on one variant).
2. `crates/hir/src/analysis/ty/ty_check/expr.rs` — in `check_record_init`
   (`:3634`, both the `PathRes::Ty` struct arm `:3676` and the
   `PathRes::EnumVariant` arm `:3702`), after resolving the concrete `ty`,
   harvest the ADT's `const_predicates` and `register_const_predicate_obligation`
   with the resolved generic args (mirror `callable.rs:801-813`).
3. `crates/hir/src/analysis/ty/trait_resolution/mod.rs` — in `check_ty_wf`
   (`:278`), after the trait-constraint loop (`:306-315`), add a const-predicate
   loop that evaluates each closed predicate by CTFE; on `false`/fault return an
   `IllFormed`-equivalent carrying the predicate span. (May need a sibling
   `WellFormedness` arm or a const-predicate diagnostic at the `ty_def.rs:630`
   call site to surface `8-0085`.)
4. `crates/hir/src/analysis/ty/ty_check/mod.rs` — no logic change to
   `process_const_predicate_obligation`; only the origin match in `call_expr`
   (`:3022`) and `record_discharged_const_predicate` (`:1458`) gain the new
   origin variant(s).

#### Discharge-route / evidence changes

- **No new `DischargeRoute`.** All new positions still discharge by CTFE under
  concrete args, so `route: DischargeRoute::Ctfe` is unchanged.
- **New origins only.** `ConstPredicateObligationOrigin` grows variant(s) so the
  evidence can say *where the demand came from* (construction vs. signature),
  which is what hover/diagnostics need to phrase the message. `premises` stays
  empty (still premise-free CTFE).

#### Concrete fixtures (pos + neg)

1. **Positive — construction discharges.**
   ```fe
   struct Bounded<const MIN: u256, const MAX: u256> where MIN <= MAX { lo: u256 }
   fn ok() { let _ = Bounded::<1, 4> { lo: 1 } }
   ```
   No diag; `discharged_const_predicates()` records one `Ctfe` entry keyed to
   the construction expr.
2. **Negative — construction rejected.**
   ```fe
   fn bad() { let _ = Bounded::<4, 1> { lo: 1 } }   // 8-0085 at the construction site
   ```
3. **Negative — signature rejected, never called.**
   ```fe
   fn sig(_ x: Bounded<4, 1>) {}                     // 8-0085 at the parameter type, even though sig() is never called
   ```
   (This is the headline demo: the soundness hole is closed at the *signature*,
   independent of any call.)

Plus a regression assertion that the **decl-site** check (`mod.rs:271`) does not
double-report: `struct Bounded<...> where MIN<=MAX` with parameters in scope is
still skipped at declaration (`mod.rs:292`) and reported only at use.

#### Risks

- **Double-reporting.** The decl-site checker and the new use-site checks must
  not both fire. Mitigation: the decl-site checker already early-returns on
  `params_in_scope` (`mod.rs:292`); concrete instantiations only exist at the
  use site, so the partition is clean — but add the no-double-report regression
  fixture above.
- **WF is hot and salsa-tracked.** Adding CTFE into `check_ty_wf` (`:278`) runs
  per type occurrence. Mitigation: only evaluate predicates whose args are fully
  concrete (`!has_var && !has_param`), exactly the gate already in
  `process_const_predicate_obligation` (`mod.rs:1408,1423`); symbolic signature
  positions defer to candidate 2 (assumption route), not to a false error.
- **Origin enum exhaustiveness.** `DischargedConstPredicate::call_expr`
  (`:3022`) and the two `resolve_deferred` arms (`mod.rs:1600,1773`) match
  without wildcards by design; new origins must be threaded through all match
  arms (this is a *feature* — the compiler will point at every site).

#### Ordered commit plan

1. **Origins + evidence plumbing.** Add the new
   `ConstPredicateObligationOrigin` variant(s) in `env.rs`; update the exhaustive
   matches (`mod.rs:3022`, the `resolve_deferred` arms). No behavior change yet;
   `cargo check` green.
2. **ADT-construction enqueue.** Wire `check_record_init` (struct + enum arms,
   `expr.rs:3676/3702`) to enqueue the ADT's const predicates. Land fixtures 1
   and 2 (positive construction + negative construction).
3. **Signature/WF enforcement.** Add the const-predicate loop to `check_ty_wf`
   (`trait_resolution/mod.rs:306`) and surface the diagnostic at
   `ty_def.rs:630`. Land fixture 3 (signature, never called) + the
   no-double-report regression.
4. **Consumer + docs.** (Optional, can fold candidate 7 here) extend the hover
   footer (`hover.rs:243`) to render the new construction/signature discharges,
   and update `M5_DEMO_SPINE.md` to note the broadened positions.

#### Top risk (one line)

Folding CTFE into the salsa-tracked WF path (`check_ty_wf`) without regressing
compile times or double-reporting against the existing decl-site checker — gated
by the same concrete-args predicate the M5 call-site path already uses.

---

# Milestone ladder (M5 → post-M7)

Labels, refined:

- **M5** finishes the *obligation surface*: const predicates are first-class
  everywhere a type/impl is required to hold, with exact matching and no
  specialization.
- **M6** makes *symbolic* obligation discharge principled and explainable.
- **M7** turns providers / high-level generation into FCO consumers/producers.
- **Post-M7** is SMT + Sonatina v2 + proof/resource evidence.

## M5 done = all eight hold

1. Call-site discharge works. — **landed** (`d3df3cca1`)
2. ADT / signature / WF discharge works (construction, parameter types, return
   types, struct/enum fields, local bindings, never-called signatures), via the
   shared `check_ty_wf` — **landed** (`70b0bbfdb`). *Open:* WF-position evidence
   recording; bodyless-declaration signatures.
3. Generic assumption-route exact match (matching predicate discharges by
   `Assumption`; wrong predicate fails; no boolean splitting / direction
   flipping / implication). — **required** (the ignored
   `assumption_route_mismatch_is_rejected` encodes the target).
4. Impl predicates gate, not select (selected-impl residuals discharged after
   selection; two impls differing only by a const predicate overlap; no SFINAE).
   — **required**.
5. Predicate overlap is conservative. — **required** (folds into 4).
6. CTFE faults are hard errors. — **landed** (pair 12, `3-0025`).
7. Unsupported forms fail by name. — **partial** (chained projection / unconstrained
   subject emit `2-0002`; a dedicated "not yet expressible" code is pending — see
   the diagnostic-contract note).
8. Receipts render somewhere visible (hover / `fe explain` reads
   `discharged_const_predicates`: goal, route, origin, premises). — **required**.

### Diagnostic contract (reconcile before declaring M5 done)

Live codes: `8-0085` predicate-formed-and-evaluated-false; `3-0025` hard CTFE
fault; `2-0002` true name-not-found / formation. Recommended: keep those, and
add/use `8-0086` only for "recognized but not yet expressible" forms
(e.g. chained projections). Update the exit-criteria doc's `8-0082/83/84` refs
to match, or implement the specific codes.

## M6 — symbolic obligations become principled

1. **Assumption-route exact matching** (term identity via `term.rs`;
   `lower_hir_to_term` + `normalize_term`; un-ignore the quarantined test).
   Closes gates 3/5 above.
2. **AC1 canonical assoc-const projection identity**: subject `TyId` +
   canonical trait instantiation/args + assoc-const def-id, replacing
   `AssocConst { inst: TraitInstId, name: IdentId }`. Likely bumps
   `TERM_LANG_VERSION` (treat as an internal cache/evidence fuse).
3. Any obligation surface that slipped from M5 (e.g. bodyless signatures).
4. **`const_assert` / body-assertion receipts** (`VcSite::ConstAssert`).
5. **Method-conformance queue routing + evidence origin** (the bidirectional
   check itself is M5/B8; `compare_constraints` is trait-only today — exact
   matching, no implication solver).
6. First useful **`fe explain` / hover** (and `--chain`).
7. HKT assoc-const ICE fix (`body.rs:730`) if still open.

## M7 — providers become typed FCO participants

1. **Typed provider capabilities**: `Reflect<T>`, `ImplBuilder<Goal>`,
   `Evidence<Goal>`, `Quote` as compile-time-only capability types, retiring the
   string-keyed (`REFLECT_KEY` / `IMPL_BUILDER_KEY`) and `is_derive_provider_fn`
   exemptions. **NEEDS_DESIGN first.**
2. **One high-level rewrite in Fe** — ABI/static-layout first (small,
   contract-relevant, uses assoc-const facts + where predicates, produces
   receipts `fe explain` can render). Event/error follow; EIP-712 last.
3. Generated-impl provenance evidence (derive site → provider → generated impl →
   layout predicates).

Do **not** start with full EIP-712, full Sonatina v2, full SMT, effect-witness
unification, or a MIR runtime-class rewrite.

## Post-M7 — architecture

1. **Provision scoping**: collapse trait env + effect env + capability env +
   generated-impl overlays + const-predicate assumptions into one scope-indexed
   provision/obligation environment (`demand → obligation`,
   `resolution → provision lookup or discharge`, `result → evidence`).
2. **SMT discharge backend**: the first real producer of non-empty `premises`
   (checked-op-as-fact).
3. **Sonatina v2 (VSDG + separation logic)**: explicit resource/state
   dependencies; cross-resource joins proven / materialized / rejected — the
   eventual successor to `merge_runtime_class` (keep it contained until a
   concrete repro forces a local fix).
4. Proof transport / certificates / law systems.

## Invariant to protect across all phases

The evidence key + `route` + origin + `premises` slot must stay non-breaking
through AC1's `TERM_LANG_VERSION` change and the later `fe explain` / certificate
consumers. The `premises` slot is reserved (empty at M5) precisely so the SMT /
Sonatina futures remain reachable without a format migration.

---

# Dependency graph (the order is a DAG, not a line)

The milestone ladder above is a reasonable default *reading* order, but it
over-serializes the work. The real constraints form a DAG; within a layer,
items are independent and may be done in any order or in parallel.

```mermaid
graph TD
    FOUND["foundation: obligation queue + evidence record (DONE)"]
    A["A · call-site discharge (DONE)"]
    B["B · ADT/sig/WF discharge (DONE)"]
    C["C · assumption route / term identity (DONE)"]
    D["D · impl predicates gate-not-select"]
    E["E · named expressibility diagnostics"]
    F["F · receipts render in hover"]
    X["X · diagnostic-code reconciliation"]
    AC1["AC1 · canonical assoc-const identity (TERM_LANG_VERSION bump)"]
    CA["const_assert / body receipts"]
    MC["method-conformance routing"]
    EX["fe explain / --chain"]
    HKT["HKT assoc-const ICE fix"]
    TP["typed provider capabilities (NEEDS_DESIGN)"]
    ABI["ABI/static-layout provider rewrite"]
    PV["generated-impl provenance evidence"]
    PS["provision scoping"]
    SMT["SMT discharge backend"]
    SV["Sonatina v2 (VSDG + separation logic)"]
    PT["proof transport / certificates"]

    FOUND --> A & B & C & D & F
    A -. evidence .-> F
    C --> AC1
    AC1 -. re-keys terms .-> C
    F --> EX
    C --> EX
    A --> CA
    FOUND --> MC
    HKT --> AC1
    TP --> ABI --> PV
    A -. premises slot reserved .-> SMT
    PS --> SMT
    AC1 --> PT
    SMT --> PT
```

## What the edges mean (and what they free)

- **M5 remainder `{D, E, F, X}` are mutually independent.** They sit directly on
  the done foundation; none blocks another. Do them in any order / in parallel.
  F (hover receipts) is cheapest and READY_NOW; E and X are diagnostic/doc-local;
  D is the only one needing a probe (are const predicates allowed on impls
  syntactically?). Do **not** force a 1→2→3 sequence here.
- **Real hard edges only:** `A → F` (hover renders evidence records — note WF
  positions don't record evidence yet, a sub-dependency for *full* F);
  `C ↔ AC1` (the assumption matcher uses term identity, so AC1's canonical
  projection both builds on C and re-keys C's evidence — **co-design them**, do
  not strictly sequence AC1 after C); `F → EX`; `TP → ABI → PV`; the reserved
  `premises` slot → `SMT`; `SV` supersedes `merge_runtime_class`.
- **Parallel tracks:** the M5-finish set, the AC1/term-identity track, and the
  M7 provider-design track do not block each other until they converge at
  `EX` / `PT`.

## Live status (proven, not inherited)

| Node | Status | Evidence |
|---|---|---|
| A call-site | COMPLETE_AND_TESTED | `d3df3cca1`, fe-hir tests |
| B ADT/sig/WF | COMPLETE_AND_TESTED | `70b0bbfdb`, 7 paired fixtures + uitest snapshot |
| C assumption route | COMPLETE_AND_TESTED | `6e6a5a9b0`, route=Assumption asserted; mismatch/none fail |
| D impl gate-not-select | ABSENT (probe needed: impl const-pred syntax) | — |
| E named diagnostics | PARTIAL (chained/unconstrained → 2-0002; 8-0086 reserved) | — |
| F receipts in hover | ABSENT (evidence exists; hover doesn't read it) | hover.rs reads only trait evidence |
| X diag-code reconcile | PARTIAL (live: 8-0085/3-0025/2-0002; exit-doc says 8-0082/83/84) | — |

Caveat carried from B/C: WF-position discharges **reject correctly but do not
record `DischargedConstPredicate` evidence** (only call-site + assumption routes
do). Bodyless declarations (trait method sigs, externs) are not yet covered.
Both are tracked follow-ups, not blockers.

---

# Reconciliation with the architect's dependency graph (v0)

The full node-level decomposition lives in `docs/dev/fco_dependency_graph_v0.mmd`
(the architect's draft). It is finer-grained than the layers above and is the
canonical map; the coarse DAG above is the executive summary. We adopt its
**node IDs and ready-frontier model** as the shared vocabulary. We do **not**
(yet) adopt its Tooling (T*) / Governance (G*) apparatus or the deep
Runtime/resource (RT*), SMT (S*), and Provision-unification (PV*) tracks — those
stay post-M7 architecture until a concrete driver appears.

Live status by node ID (proven from code/tests, not inherited):

- **Done & tested:** F00 (evidence schema), F01 (term language), F02 (symbolic
  AssocConst term), F03 (where-pred parse/lower), F04 (shared deferred queue),
  F05 (CTFE-outside-solver, CI-gated), F06 (call-site spine), F07 (paired
  fixtures); W00 (WhereClauseOwner), W11 (ADT/sig/WF discharge), W13 (WF
  duplicate-diagnostic guard — the return-type double-report was found and
  removed), W14 (generic symbolic WF guard), W15 (never-called signature/field);
  A00 (const preds enter assumption pool), A10 (assumption-route exact match);
  R00 (evidence sink / accessor), R10 (LSP hover seed); D10 (false-predicate
  8-0085), D40 (CTFE-fault 3-0025).
- **Partial:** A11 (route=Assumption + empty premises recorded; premise-*origin*
  link to the matched assumption not yet stored), A12 (mismatch rejected;
  explicit flip/split/implication fixtures pending), D20 (formation → 2-0002),
  D30 (unsupported projection → 2-0002, not a dedicated "not yet expressible"
  code), W12 (**WF-position discharges record no evidence** — the carried caveat).
- **Next / ready frontier:** I00 (are impl `where` const predicates even
  representable? — probe) → I10/I20/I30/I40 (gate-not-select + B2b overlap);
  D00 (diagnostic taxonomy) + U10 (code-policy mismatch) — the 8-0082/83/84 vs
  8-0085/2-0002/3-0025 reconciliation; D50 (actual-value rendering); W12.
- **Uncertain zones acknowledged:** U10 (diag codes), U30 (full ConstraintKind
  necessity — we deliberately did NOT do the UC* refactor; obligation-level
  unification sufficed), U40/U50 (assoc-const/HKT depth), U80 (method exactness).

The architect graph's `⚠` "uncertain" nodes line up with our open scope calls;
nothing in the executed M5 work contradicts it.
