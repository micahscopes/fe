# Recursive type fn: capstone overview

Branch `type-fn`, forked off `fco-sgk` at HEAD `8d1d99bd8` (PR #1506, 2451/2451
release CI), now at HEAD `566e4b3e6`, 13 commits. This document is the ten-minute
map for Sean, the architect, and Micah to evaluate the branch before diving into
code. It cites only artifacts that exist in this tree: `docs/type-fn/BUILD_LOG.md`
(the full slice-by-slice history), `docs/type-fn/IMPL_MAP.md` (the landing study),
four Fable steering reviews (`type-fn-fable-steering-0{1,2,3,4}.md`), the design
source (`FE_FUTURE_DIRECTIONS_PLAN.md` section 4) and spec
(`fe-recursive-type-fn-spec-2026-06-10.md`), and the demonstration
(`docs/type-fn/generic-reduce-demo.fe` plus the `demo_*` tests in
`crates/hir/src/analysis/ty/type_fn_induct.rs`).

---

## 1. Motivation: why this exists

`FE_FUTURE_DIRECTIONS_PLAN.md` section 4 names type-to-type CTFE "the single
most-named one missing feature" since the founding record, and this branch builds
exactly the restricted form the plan specifies, no more. Three reasons it is the
keystone rung rather than one feature among many:

- **It unlocks Conal-Elliott parallelization.** The framework (plan section 5;
  Conal Elliott, ICFP 2017) writes ONE generic algorithm per functor combinator
  (`Par`, `Sum`, `Prod`, `Comp`, ...) and gets DIFFERENT concrete algorithms
  (Sklansky vs. Ladner-Fischer scans, DIT/DIF FFT, NTT) by type instantiation.
  Every shape in that framework (`RPow`, `LPow`, `Bush`, `RVec`, `LVec`) is a
  recursion on a natural at the type level. Without type-to-type CTFE the whole
  framework "collapses permanently to hand-specialized fixed sizes" (plan
  section 1.3).
- **It is Stage 1 of the cubical/proof roadmap**, arriving right after the
  obligation core (FCO, built) and before erased evidence (M6, next in sequence).
  The plan calls it "the restricted dependent compile-time core" of the
  architect's north-star ladder.
- **It reconnects the three founding strands** that fragmented under delivery
  pressure (plan section 1.1-1.2): FCO's typed obligation core
  (`ConstraintTerm`/`TraitCtor`, the interned normalized-term carrier this
  feature reuses), SGK's content-keyed staged generation, and riffcat's
  content-addressing discipline (`GeneratedImplTrait{goal, self_ty}`-style
  identity, extended here from impls to types). Type-fn applications are, per
  plan section 4.7, "the first type-level entities whose identity/equivalence
  split... matches the riffcat discipline already proven on generated impls."

**Who benefits** (plan section 1.4): contract/library authors get one source
that defers backend concerns to the compiler instead of convention; cryptography
users get generic scan/FFT/NTT libraries written once and specialized per size
and backend (the roadmap's 2^20-point NTT target); auditors get erased proof
evidence and content-addressed build facts consumable by external Lean/Agda
tooling; maintainers get the FCO payoff itself, "surface-area reduction and
near-zero per-feature cost" (each future feature discharges at one obligation
chokepoint instead of growing its own resolution pathway).

---

## 2. What the feature is

Three things, landed as one branch.

**(a) `recursive type fn`, the restricted primitive-recursive form.** Zero or
more type parameters plus exactly one `const N: usize` subject, declared last.
The body is exactly one `match` on the subject; arms are integer literals plus
a mandatory final `_`; self-calls are the only type-fn applications permitted in
a body, restricted to two whitelisted subject shapes (`{N - k}`, `k >= 1`; `{N /
k}`, `k >= 2`), each checked per-arm so termination is a syntactic, not
arithmetic, fact:

```fe
recursive type fn RPow<F, const N: usize>() -> (*) {
    match N {
        0 => Par
        _ => Comp<RPow<F, {N - 1}>, F>
    }
}
```

**(b) Ground normalization.** A concrete-subject application reduces at compile
time to a flat combinator nest, with zero runtime residue:

```
RPow<Pair, 3>  ->  Comp<Comp<Comp<Par, Pair>, Pair>, Pair>
```

**(c) The sound course-of-values induction engine.** For a SYMBOLIC subject, the
engine proves `Shape<F, N>: Trait` for every `N` generically, from a
constraint on `F` alone, with no hand-written `where` bound at the call site:

```fe
fn reduce_rpow<F: Reduce, const N: usize>(x: Reducer<RPow<F, N>>) {}
// type-checks with NO `where RPow<F, N>: Reduce` bound: the engine discharges
// it from `F: Reduce` by induction on N.
```

---

## 3. Architecture

- **Rides the FCO carrier.** Type-fn applications are ordinary interned `TyId`
  nodes, exactly the normalized-term discipline FCO already built for
  `ConstraintTerm`/`TraitCtor`. No new identity scheme, no new evaluator.
- **Ty layer:** `TyBase::TypeFn(TypeFnDef)` (`ty_def.rs`) plus `TypeFnSig {
  arity, ret_kind }`. Const params are carried OUTSIDE the kind language,
  mirroring how `AdtDef` already carries const params (`ty_def.rs:2015-2023`,
  `HasKind for AdtDef`); **no dependent kinds are added**, and applications reuse
  the existing curried `TyApp` node unchanged. Because const slots render as
  plain `Star`, kind-checking alone cannot reject a bare or partially-applied
  type fn (it kind-matches a `* -> *` parameter); a dedicated structural
  saturation walk (`find_unsaturated_type_fn`) closes that hole at S1.3.
- **Well-formedness distills the definition** (`crates/hir/src/analysis/ty/type_fn.rs`,
  the `type_fn_wf` salsa query) into `TypeFnWfData { def, subject_idx, arms:
  Vec<TypeFnArmData> }`, each arm carrying `self_calls: Vec<SubjectStep>` where
  `SubjectStep = Sub(k) | Div(k) | Lit(m)`. `data` is `Some` **if and only if**
  every WF check passes, so "WF gates unfolding" is a structural fact, not a
  convention: the normalizer and the induction engine can only ever consume
  already-validated arm data.
- **Normalization** (`unfold_type_fn_step` / `normalize_type_fn_app`) is two
  `BigUint` operations per step, never the CTFE machine, with a rooted memo
  entry, an iterative worklist, and a fixed fuel budget (~4096) kept OUTSIDE the
  salsa memo key (so a subterm and a root reduce identically). The worklist
  covers BOTH weak-head reduction AND the structural child descent
  (`normalize_all`), so native Rust stack usage is O(1) in the fuel budget: an
  over-budget subject whose normal form is a deep spine (e.g. `RPow<Pair, 5000>`)
  reaches the `TypeFnRecursionLimit` diagnostic on the ordinary analysis path
  instead of overflowing the native stack (the fuel backstop is a real backstop,
  not one only reachable on an enlarged-stack thread). Ground apps are
  eager-expanded at the S1.3 path-lowering site (the guarantee mechanism, so a
  ground `TyBase::TypeFn` never survives into a stored type), with
  `TypeNormalizer::fold_ty` as a second line for substitution-formed apps and a
  `stable_key.rs` debug-assert tripwire at the MIR boundary as the last line.
- **The induction engine** (`crates/hir/src/analysis/ty/type_fn_induct.rs`) adds
  a strict-satisfaction helper (`StrictResult::Proven | NotProven`) that is a
  DISTINCT type from the solver's `GoalSatisfiability`, so an engine result can
  never be accidentally routed through the permissive `is_satisfied()` /
  UnSat-only pattern that the rest of the codebase uses. It mints one fresh
  rigid opaque `TyParam` PER self-call occurrence (`Variant::Induction`, sets
  `HAS_PARAM` so the assumptions leg fires, but unifies with nothing else),
  proves each arm generically (base arms ground via normal `normalize_type_fn_app`
  + resolution; step arms under the injected induction hypothesis), and is
  consulted only from the WF/obligation discharge sites, OUTSIDE the tracked
  solve (`proof_forest.rs` never calls it), the third instance of the codebase's
  "consult outside the tracked solve" pattern alongside CTFE const-predicates
  and scoped provisions. Discharge is assumption-injection re-query: on
  `Proven`, the site re-runs the ordinary query with the goal added as an
  assumption and accepts only `ImplementorOrigin::Assumption`, the exact S2.1
  route, so mono re-resolves ground through machinery that was already
  baseline-proven, with zero new solver enum touched.

---

## 4. The soundness story

**Fire-Triangle containment** (plan section 4.4). The effects leg is severed by
grammar: a type-fn body admits only type expressions and self-calls, so no
const-fn call, `Reflect`, builder, or quasiquoter is reachable; the dangerous
ingredient is unrepresentable, not merely disciplined. CTFE-acyclicity is
structural: the unfolder consumes only the distilled `TypeFnWfData` and never
calls `evaluate_const_ty`. This was not true by construction on first landing;
two residual leaks the Fable steering-02 review found were closed in slice
S1.4b before normalization was built (see below).

**Gate-don't-select, verified, not assumed.** The engine's core cross-check
(`engine_cross_check_gate_matches_ground_select`, S2.2b; mirrored by the demo's
own `demo_ground_normalization_and_engine_discharge_route`) runs at
`n in {0, 1, 2, 4, 7}` and asserts that ground resolution on the UN-normalized
symbolic application and on its ground normal form both select the IDENTICAL
`(ImplementorId, ImplementorOrigin, SelDiscriminator)` tuple, that
`select_impl` returns `Selection::Unique` of it on both forms, and that
`default_tier_selection(db, ground_goal) == None` at every `n` (the FCO
coexistence tier never engages), so the engine never proves a shape it would
need the tier to disambiguate.

**Zero baseline perturbation.** The engine is gated behind
`type_fn_app_head(goal.self_ty).is_some()`, which no baseline (non-type-fn) goal
satisfies. By input-disjointness, no pre-existing tracked query acquires a new
dependency edge. `StrictResult::NotProven` means "no lemma," never "refuted for
all n"; the engine records no negative lemmas.

**Two release CIs, full command, both green.** The exact full-workspace
`nextest` command (release, all-features, no-fail-fast) passed **2475/2475** at
the Slice-1 boundary (HEAD `adfeaaec4`) and **2502/2502** at the Slice-2
boundary (HEAD `d3d88a85b`), both built on top of the FCO base branch's own
**2451/2451** (PR #1506). Every intermediate slice's own verification
(`cargo check --workspace` clean, targeted `cargo test -p fe-hir` runs) is
recorded per-slice in `BUILD_LOG.md`.

**Four Fable steering reviews caught concrete soundness holes before or during
build**, each verified against the actual tree, not the design on paper:

- **Const-arg CTFE leak** (steering-02, finding 1a): `walk_arm_ty`'s `Other` arm
  ignored `GenericArg::Const` entirely, so an arm like `Wrapper<{helper(N)},
  ...>` passed well-formedness; after normalization the resulting `UnEvaluated`
  const would fall through into a real CTFE run with no termination cover.
  Closed in `7ca1a8f67` (S1.4b) by restricting every arm-RHS const argument to
  an integer literal or the bare subject.
- **Foreign-ref cycle** (steering-02, finding 2): `classify_path` detected
  type-fn-ness only for single-segment paths, so a qualified path or alias to
  another type fn bypassed the foreign-call ban; two mutually-referencing defs
  could cycle `normalize(F<n>) -> normalize(G<n+1>) -> normalize(F<n>)`. Closed
  in the same commit by a lowered-RHS cross-check requiring every `TyBase::TypeFn`
  head in an arm's lowered RHS to equal the defining def.
- **Saturation-walk misplacement** (steering-01, finding 1): kind-checking
  cannot see const-ness (a const slot renders as plain `Star`), so a bare or
  partially-applied type fn kind-matches a `* -> *` parameter and could leak
  into a body via the back door. Moved the structural saturation walk into
  S1.3, the same slice that makes `TyBase::TypeFn` representable at all, rather
  than deferring it to S1.4 as first proposed.
- **Strict-satisfaction separation** (steering-03 section 1.3, steering-04
  section 1): `GoalSatisfiability::is_satisfied()` and the pervasive
  UnSat-only match pattern both treat `NeedsConfirmation` (including the
  depth-cap give-up) and `ContainsInvalid` as satisfied. The engine's
  `StrictResult` is a structurally distinct type so it can never be routed
  through either trap; two divergence tests
  (`strict_diverges_from_permissive_on_needs_confirmation`,
  `..._on_contains_invalid`, S2.2a) pin that the strict check is genuinely
  stricter, not a synonym.
- **Impl-target ban sequencing trap** (steering-03, finding G1): the spec's
  "no impls on type-fn applications" rule was, before S2.0a, delivered ONLY as
  a side effect of the symbolic reject that Slice 2 was about to lift: an
  explicit, independent ban had to land BEFORE any gate lift, or lifting the
  gate would silently un-ban impls on opaque symbolic heads. Landed in
  `2ffcffbc3` (S2.0a), verified with zero pre-existing impl-target check
  anywhere in the tree.

---

## 5. The demonstration (Slice 3a)

`docs/type-fn/generic-reduce-demo.fe`, mirrored by four `demo_*` tests in
`crates/hir/src/analysis/ty/type_fn_induct.rs`: a shape family (`RPow`, `LPow`,
both perfect binary trees over `Comp`/`Par`), a CONSTRAINED trait
(`impl<A: Reduce, B: Reduce> Reduce for Comp<A, B>`, deliberately not blanket,
so the induction hypothesis is load-bearing), a carrier that raises the
obligation (`struct Reducer<S> where S: Reduce {}`), and one generic algorithm
per shape with **no** `where` bound:

- `demo_generic_reduce_over_shape_family_no_where_bound` (positive): both
  `reduce_rpow` and `reduce_lpow` type-check to zero diagnostics.
- `demo_negative_twin_arg_not_reduce_rejected`: drop `F: Reduce` and it is
  correctly rejected (not vacuous proof power).
- `demo_negative_twin_combinator_impl_removed_rejected`: remove the `Comp` impl
  and, even with `F: Reduce`, it is rejected (the discharge genuinely rides the
  combinator impl, not a standing fact).
- `demo_ground_normalization_and_engine_discharge_route`: confirms
  `RPow<Pair, 3>` normalizes (by interned-id equality) to the hand-built
  `Comp<Comp<Comp<Par, Pair>, Pair>, Pair>`, and that the symbolic route is
  genuinely the induction engine, not a blanket impl (the ordinary solver
  alone, with only `F: Reduce`, returns UnSat on the opaque head) and not
  vacuous (drop `F: Reduce` and the engine declines).

What it proves: the Conal-Elliott payoff in miniature, one generic algorithm
stated over an entire type-fn-defined shape family, whose trait-membership
obligation is discharged automatically at every size, with zero
per-instantiation `where`-bound proof.

---

## 6. Honest status and gaps

**BUILT**, in order (`BUILD_LOG.md` has the full detail per slice):
parser + AST/CST (S1.1) -> HIR item + workspace wiring (S1.2) -> ty-layer base +
saturation walk (S1.3) -> definition WF + distilled arm data (S1.4, plus the
S1.4b hole-closing amendment) -> ground normalization + MIR tripwire (S1.5) ->
explicit impl-target ban + precondition constraint + mono normalization (S2.0)
-> symbolic propagation + assumption-only discharge (S2.1) -> induction-engine
preconditions + wiring (S2.2a/b) -> the Conal-Elliott demonstration (S3a).

**DEFERRED / GAPS**, stated plainly:

1. **Return kind `*`, not `* -> *`.** The S1.3 rule admits `*` and arrows over
   `*`, but every working fixture and the engine's minimal class live at the
   `*`-kinded form: `RPow`/`Comp`/`Par` are `*`-kinded DATA-shape trees, and
   `Reduce` is implemented on those ADTs directly. The sketch's higher-kinded
   `-> (* -> *)` functors (instances you `map`/`zip`/`scan` over `Self<A>`)
   are not exercised.
2. **The obligation is raised by a WF carrier, not a value-level method call.**
   `Reducer<S> where S: Reduce` raises `S: Reduce` as a signature
   well-formedness obligation. A value-carrying generic algorithm (build a
   value of the shape type, call `x.reduce()` / `<RPow<F,N> as
   Reduce>::reduce()` through a symbolic type-fn head, then monomorphize to
   concrete SSA) is not demonstrated. Method resolution through a symbolic
   head and the value/codegen path are the next surface to open.
3. **Slice 3 polish is entirely undone:** depth-limit manifest config (the
   ~4096 ceiling is a hardcoded constant today), the three named diagnostics at
   full quality (spec section 8, E0931/E0932/E0933), formatter and LSP surface.
4. **Fixpoint goal-set growth is not built.** The engine (S2.2b) handles
   exactly one fixed goal predicate per lemma request; growing a goal set to a
   fixpoint (the spec's worked `RPow<F, N>: LScan` example, which needs
   `{LScan, Functor, Zip}`) is Slice 2's S2.3 rung, unbuilt, along with
   multi-self-call arm opaquing beyond conservative decline (a `Bush`-shaped
   two-self-call def is declined by the minimal-class gate, not proven unsound
   by luck) and FCO-coexistence hardening (S2.4).
5. **Tree-sitter grammar not updated.** The hand-written parser is
   authoritative; the positive round-trip fixture is excluded from
   `tree_sitter_parse_strict` via `EXCLUDED_FILES`, an explicit tracked gap
   since S1.1 (the same precedent as const-predicates).
6. **Multi-backend integration is a separate track entirely** (plan sections
   5-6); this branch is HIR/ty-layer work targeting the existing EVM path, not
   the wasm/native/zkVM backend story.
7. **v1 narrowings, documented in code and BUILD_LOG:** self-calls must be
   single-segment; self-call type arguments must be the def's own type params
   forwarded verbatim (removes polymorphic recursion from Slice 2's soundness
   burden entirely); a lemma's subject must be a bare rigid const param
   (compound symbolic subjects like `F<T, {M-1}>` at a use site are deferred);
   any type-fn head appearing in forwarded args declines the lemma; ADT-field
   symbolic positions remain hard-rejected (an S2.1 conservative choice, not
   yet lifted).

---

## 7. How to evaluate, and what the GO needs

**This branch is architect-gated, not merge-ready by CI-green alone.**
`FE_FUTURE_DIRECTIONS_PLAN.md` names the exact gate: owner decision **O3**
("architect, after M6: Stage-1 GO on the restricted `recursive type fn` spec as
written... any weakening reopens the containment argument of 4.4 and needs a
new review, not a code-review comment," section 10.2), which section 9.3 places
strictly after M6 (erased-evidence hooks), itself "next after merge-hardening"
in the north-star sequence. This branch was built ahead of that sequencing gate
as a working proof-of-concept, on explicit instruction, not as a claim that the
gate has been satisfied. Reviewing it should separate two questions: **is the
engineering sound** (section 4 above, and the CI numbers), and **should the
do-not-start directive lift now** (O2/O3, the architect's call, unaffected by
how clean this branch is).

**Commits** (oldest to newest, `8d1d99bd8..566e4b3e6`):

| Commit | Slice | Summary |
|---|---|---|
| `8238d176e` | landing study | Landing study + implementation map |
| `71f9e80c8` | S1.1 | Parser: grammar + AST/CST |
| `645236769` | S1.2 | HIR `TypeFnDef` item + workspace-wide `ItemKind` wiring |
| `f2c9a1b23` | S1.3 | Ty-layer `TyBase::TypeFn` + saturation walk |
| `d617909dd` | S1.4 | Definition WF + distilled arm data |
| `7ca1a8f67` | S1.4b | Closes two WF holes (const-arg CTFE leak + foreign-ref cycle) |
| `adfeaaec4` | S1.5 | Ground normalization + eager expansion + MIR tripwire |
| `2ffcffbc3` | S2.0a | Explicit impl-target ban |
| `22f4e00a2` | S2.0(b,c) | Precondition constraint + mono normalization verification |
| `9f5388dff` | S2.1 | Position-aware symbolic gate + assumption-only discharge |
| `1b533356e` | S2.2a | Induction-engine preconditions (zero proof power) |
| `d3d88a85b` | S2.2b | Minimal induction engine, wired in |
| `566e4b3e6` | S3a | Conal-Elliott generic-reduce demonstration |

**Verification commands:**

Targeted (fast, per-slice, what `BUILD_LOG.md` records at each step):

```
cargo check --workspace
cargo test -p fe-hir --lib analysis::ty::type_fn::tests          # 29 tests
cargo test -p fe-hir --lib analysis::ty::type_fn_induct::tests   # 21 tests, incl. demo_*
cargo test -p fe-parser                                          # grammar fixtures
```

Full release CI (the number that actually counts; a per-crate subset is not
"green" for this branch's purposes):

```
cargo nextest run --release --workspace --all-features --no-fail-fast --locked
```

This command has been run green twice on this branch: **2475/2475** at the
Slice-1 boundary (HEAD `adfeaaec4`) and **2502/2502** at the Slice-2 boundary
(HEAD `d3d88a85b`), both against the FCO base's own **2451/2451**. Slice 3a
(the demonstration) added tests and a docs artifact only, with no engine or
semantics change, and has been verified via `cargo check --workspace` plus the
targeted `demo_*` tests, not yet re-run through the full release command; that
run is the natural next step before this branch is treated as a merge
candidate.
