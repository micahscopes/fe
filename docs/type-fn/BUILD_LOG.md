# Recursive type fn: build log

Running, dated log of what each slice landed. Branch `type-fn` off `fco-sgk`.
No AI attribution in commits. No em-dashes.

## 2026-07-07 - Landing study

- Created worktree `/workspace/fe-worktrees/type-fn` (branch `type-fn`) off
  `fco-sgk` (HEAD `8d1d99bd8`).
- Read spec (`fe-recursive-type-fn-spec-2026-06-10.md`), plan section 4, sketch.
- Wrote `docs/type-fn/IMPL_MAP.md`: exact extension points (file:line + enum/struct
  names) for parser, HIR item layer, ty layer (`TyBase`/`Kind`/`TypeNormalizer`),
  and the exhaustive by-kind match-site lists (`ConstraintTerm`/`TraitCtor`
  precedent, and the `TyBase` Category-C sites a new base breaks).
- Key decisions recorded: `recursive` is a contextual keyword (derive/with
  precedent); parser reuses `parse_generic_params_opt` / `parse_kind_bound` /
  `parse_where_clause_opt` / `parse_type`; the value `MatchExpr` parser is NOT
  reused (arm RHS is a type); deep semantic laws (one-const-last, self-call-only,
  subject-shape whitelist, termination, exhaustiveness) are deferred to the HIR WF
  module `type_fn.rs` (S1.4), matching the codebase precedent that
  `ConstGenericParam` bounds are "checked in hir"; positive parser fixture must be
  added to `EXCLUDED_FILES` in `tests/tree_sitter_parse.rs` so
  `tree_sitter_parse_strict` skips it (tree-sitter grammar is a deferred
  follow-up).

## 2026-07-07 - Slice 1.1: parser + AST/CST

Landed the hand-written parser and AST/CST for `recursive type fn`.

- `crates/parser/src/syntax_kind.rs`: 7 new CST node kinds (`RecursiveTypeFn`,
  `TypeFnRetKind`, `TypeFnBody`, `TypeFnMatch`, `TypeFnArmList`, `TypeFnArm`,
  `TypeFnArmPat`) + `describe()` arms (the match is exhaustive).
- `crates/parser/src/parser/item.rs`: contextual `recursive` dispatch in
  `ItemScope::parse` (derive/with precedent; `pub` allowed, `unsafe` rejected) +
  `RecursiveTypeFnScope` and the sub-scopes `TypeFnRetKindScope`,
  `TypeFnBodyScope`, `TypeFnMatchScope`, `TypeFnArmListScope`, `TypeFnArmScope`,
  `TypeFnArmPatScope`. Reuses `parse_generic_params_opt`, `parse_kind_bound`
  (made `pub(crate)` in `param.rs`), `parse_where_clause_opt`, `parse_type`;
  mirrors `MatchArmListScope` for arm separators. Named parse-time diagnostics
  for: non-empty value params; non-integer/`_` arm pattern; the mandatory `=>`;
  and `if`/`let`/nested-`match` at an arm head.
- `crates/parser/src/ast/item.rs`: `RecursiveTypeFn` AstNode (GenericParams /
  WhereClause / AttrList / ItemModifier owner; `name()` skips the contextual
  `recursive` token by taking the `Ident` after `fn`; `ret_kind()`, `body()`)
  plus `TypeFnRetKind`, `TypeFnBody`, `TypeFnMatch`, `TypeFnArmList`
  (IntoIterator), `TypeFnArm`, `TypeFnArmPat`. Deliberately NOT yet added to the
  `ItemKind` enum / `Item::kind()` dispatch: that bridge is paired with S1.2 so
  the workspace stays green (fe-hir / fe-fmt / language-server match
  `ast::ItemKind` exhaustively and would break on a new variant without their
  new arms).
- Fixtures: positive `test_files/syntax_node/items/recursive_type_fn.fe` (RPow +
  Bush; round-trips, snapshot verified; added to `EXCLUDED_FILES` in
  `tests/tree_sitter_parse.rs`); negatives
  `test_files/no_recovery/items/recursive_type_fn_{nested_match,if_body}.fe`
  (assert the named diagnostics).

Deferred to S1.4 (HIR well-formedness), by design and per codebase precedent
(`ConstGenericParam` bounds are parsed permissively and "checked in hir"): the
"exactly one `const` subject declared last" rule (parser reuses the standard
generic-param parser, so `<const M, const N>` parses); the `{N - k}`/`{N / k}`
subject-shape whitelist (a braced subject lands as an ordinary `ConstGenericArg`
block; validating its shape is entangled with the DefId-based self-call
determination and would require forking the type/generic-arg parser, which the
plan forbids); exhaustiveness, termination, and self-call-only. These get uitest
negative fixtures at S1.4.

Verification: `cargo build -p fe-parser` clean; `cargo test -p fe-parser` all
green (lib 83, error_recovery 15, no_recovery 17, syntax_node 60,
tree_sitter_parse 3). Note: the parser test harness pulls in `tree-sitter-fe`
whose build script needs `node` (absent here); reused the already-generated
`crates/tree-sitter-fe/src/parser.c` from the `fco-rebuild` worktree (identical
grammar.js) as a gitignored build artifact.

## 2026-07-07 - Slice 1.3: ty-layer base + signature + saturation walk

Landed the ty-layer representation of `recursive type fn` and, per the Fable
steering (finding 1), the STRUCTURAL SATURATION / ARITY WALK in this same slice
(not deferred to S1.4), plus the v1 return-kind rule.

Base + signature (mirrors `TyBase::Adt(AdtDef)`, keeping const-ness OUT of the
kind language):

- `crates/hir/src/analysis/ty/ty_def.rs`: new `TyBase::TypeFn(TypeFnDef)`
  variant. Applications reuse the curried `TyData::TyApp`, so
  fold/visitor/binder are unchanged. New `TypeFnSig { arity, ret_kind }` +
  `type_fn_sig(db, def)` reader (arity from the memoized `collect_generic_params`
  query, `ret_kind` from `lower_kind` of the HIR bound; non-tracked because
  `Kind` is not a salsa `Update` value). New `impl HasKind for TypeFnDef`
  mirroring `AdtDef::kind` but ending the `*`-arrow chain in the declared return
  kind rather than `Star` (the const subject slot renders as `Star`). New
  `TyId::type_fn` constructor. Category-C exhaustive arms wired:
  `TyBase::pretty_print`, `HasKind for TyBase`, `TyId::as_scope`,
  `TyId::name_span`, and `applicable_ty` (REQUIRED so `foldl` recognizes the
  `const N: usize` subject as a const slot and evaluates the subject arg against
  `usize`; not in the map's Category-C list, found by construction).
- `crates/hir/src/analysis/ty/visitor.rs`: `visit_type_fn` hook + `walk_ty_base`
  arm.
- `crates/hir/src/analysis/ty/term.rs`: `callee_from_func_ty` non-func arm.
- `crates/mir/src/runtime/stable_key.rs`, `crates/codegen/src/function_symbols.rs`,
  `crates/hir/src/analysis/diagnostics.rs`,
  `crates/hir/src/analysis/name_resolution/visibility_checker.rs`: remaining
  exhaustive `TyBase` consumers (defensive; a `TypeFn` base is normalized away
  before MIR/codegen at S1.5).
- `crates/hir/src/core/hir_def/item.rs`: `TypeFnDef::ret_kind_bound` accessor
  (analysis layer cannot read the `pub(in crate::core)` field) +
  `TopLevelMod::all_type_fns`.

Saturation walk (the fix that had to land here): because the const subject
renders as `Star`, a bare or partial `TypeFn` kind-MATCHES a `* -> *` parameter,
so kind checking cannot reject it. `find_unsaturated_type_fn` recursively
descends a lowered `TyId`, flagging any `TyBase::TypeFn` not at the head of a
`TyApp` spine of exactly its signature arity, INCLUDING occurrences nested in
generic arguments. Hook: `path_resolver.rs` now lowers an `ItemKind::TypeFn`
path to `TyBase::TypeFn` applied via `foldl`, then runs the walk; an unsaturated
occurrence becomes `Invalid(TypeFnNotSaturated)`, which the existing
`emit_invalid_ty_error` `TyVisitor` surfaces wherever the type is checked (field
types, fn signatures, etc.). Over-application is already a `KindMismatch` inside
`foldl`. Removed the S1.2 `PathResErrorKind::TypeFnNotYetSupported` placeholder.

Return-kind rule (v1: `*` and arrows over `*` only): `kind_mentions_constraint`
+ a minimal `RecursiveTypeFnAnalysisPass` (registered after `TypeAlias`) that
emits `TyLowerDiag::TypeFnConstraintRetKind` for a declared `Constraint` return
kind. Full definition WF (grammar/exhaustiveness/termination/self-call) stays
S1.4. New diagnostics: `TyLowerDiag::TypeFnNotSaturated` (code 43),
`TypeFnConstraintRetKind` (code 44); new `InvalidCause::TypeFnNotSaturated`.

Deliberately NOT in this slice (per the task boundary): normalization/unfolding
(S1.5), the MIR-boundary assert (S1.5), and the definition-site grammar +
termination WF checks with the distilled `SubjectStep::Sub(k)/Div(k)` arm data
(S1.4).

Verification: `cargo check --workspace` clean (no warnings). Two focused
fe-hir unit tests pass in debug (`cargo test -p fe-hir --lib type_fn_`, 15.5s):
`type_fn_saturation_walk_rejects_partial_application` (partial `Dup<u8>` and bare
`Dup` reject with `TypeFnNotSaturated { expected: 2, given: 1/0 }`; saturated
`Dup<u8, 3>` stays a well-formed unnormalized `TyBase::TypeFn` application) and
`type_fn_constraint_return_kind_rejected` (`Constraint` return kind rejected via
the sig and end-to-end via the pass; `*` accepted; subject-only arity is 1).

## 2026-07-07 - Slice 1.4: definition WF + distilled arm data (the S1.5 gate)

Landed the definition-site well-formedness checker and the distilled arm
representation that structurally gates unfolding.

New analysis module `crates/hir/src/analysis/ty/type_fn.rs`:

- Distilled types (S1.5 consumes exactly these):
  - `SubjectStep = Sub(IntegerId) | Div(IntegerId) | Lit(IntegerId)` (spec
    sec 3.3), a `salsa::Update` `Copy` enum. `IntegerId` (interned `BigUint`)
    keeps it salsa-friendly.
  - `TypeFnArmData { pat: TypeFnPat, rhs_ty: TypeId, self_calls: Vec<SubjectStep> }`
    (analysis-layer; distinct from the HIR `hir_def::TypeFnArmData` which is the
    raw `pat`+`ty` store). `rhs_ty` is the HIR `TypeId`, retained intensionally
    (spec sec 6.1); S1.5 substitutes/lowers it.
  - `TypeFnWfData { def, subject_idx, arms: Vec<TypeFnArmData> }`.
  - `TypeFnWfResult { data: Option<TypeFnWfData>, diags: Vec<TyLowerDiag> }`.
- `#[salsa::tracked(return_ref)] fn type_fn_wf(db, def) -> TypeFnWfResult`. `data`
  is `Some` IFF `diags` is empty and all arms are structurally complete, so
  "WF gates unfolding" is a STRUCTURAL fact (the future unfold queries take
  `TypeFnWfData`, which cannot exist for an ill-formed def).

Checks enforced, each a named `TypeFnWfError` (rendered via the new
`TyLowerDiag::TypeFnIllFormed { primary, error }`, code 45):
- subject param: exactly one `const _: usize`, declared last
  (`MissingSubject` / `MultipleSubjects` / `SubjectNotLast` / `SubjectNotUsize`);
- `match` scrutinee equals the subject (`ScrutineeMismatch`);
- exhaustiveness: mandatory final `_` (`MissingWildcardArm`), no arms after it
  (`ArmAfterWildcard`), no duplicate literals (`DuplicateArmLit`);
- at least one self-call (`NoSelfCall`, keyed on a `saw_self_call` flag so an
  ill-formed self-call still counts as present);
- no foreign type-fn application, self-detected by DefId (`ForeignTypeFnCall`);
- no assoc-type projection in an arm RHS (`AssocProjInArm`);
- arm RHS restricted to the body TypeExpr grammar: paths + self-calls only,
  tuple/array/ptr/mode/`!` rejected (`DisallowedArmType`);
- termination (spec sec 3.3), per self-call, independently: whitelisted
  `{N - k}` (k>=1, arm lower bound `L >= k`) / `{N / k}` (k>=2, `L >= 1`) /
  literal `m < L`; else `SubjectNotDecreasing` / `SubjectMayUnderflow` /
  `SubjectDivZeroFixpoint` / `LiteralSubjectNotSmaller`. The base-case trap
  (`{N - 1}` in arm `0`) falls out as `SubjectMayUnderflow` (L=0 < 1);
- self-call type args = the def's own type params forwarded verbatim, in order
  (`SelfCallArgsNotVerbatim`) - removes the polymorphic-recursion soundness
  class from Slice 2 (Fable steering);
- `where` clause carries only type-param bounds (`WhereNotTypeParamBound`;
  const predicates and bounds on the subject rejected);
- return kind required (`MissingReturnKind`); `Constraint` return kind rejected
  via the S1.3 `TypeFnConstraintRetKind` (folded into `type_fn_wf`).

Structural CTFE-acyclicity kept intact (Fable steering finding 2): the subject
is destructured purely syntactically (`Bin(Sub|Div)(Path(subject), IntLit)` or an
integer literal, unwrapping the parser's single-stmt block), and self-call
detection uses the EARLY name resolver (`resolve_ident_to_bucket`), which never
lowers generic args. No body block is ever handed to `evaluate_const_ty`, so its
`NotIntExpr` -> CTFE fall-through stays unreachable from type-fn bodies. The one
lowering call (`lower_hir_ty` on each arm RHS, to exercise S1.3's
`find_unsaturated_type_fn` on the definition itself) runs ONLY after all subjects
are validated and only on otherwise-WF defs, so it never routes an unvalidated
block through the evaluator; a residual unsaturated occurrence there emits
`TypeFnNotSaturated` and withholds the data.

v1 narrowing (documented): self-calls must be single-segment (the syntactic self
identifier); multi-segment references are inspected only for a root-type-param
assoc projection. Alias/qualified-path self- or foreign-type-fn references are
not special-cased here (no motivating workload needs them; caught downstream).

Wiring: `RecursiveTypeFnAnalysisPass` now delegates to `type_fn_wf` (the S1.3
inline constraint-ret-kind check was folded in). New public accessors on
`TypeFnDef` (`hir_generic_params`, `hir_where_clause`, `match_subject_ident`,
`hir_arms`) expose the raw HIR to the analysis layer (mirroring `ret_kind_bound`).
The S1.3 `type_fn_constraint_return_kind_rejected` fixtures gained self-calls so
`Bad`/`Good` are WF except for the property under test (pass still emits exactly
one diag).

Verification: `cargo check --workspace` clean (no warnings). 10 new focused
fe-hir unit tests pass in debug (`cargo test -p fe-hir --lib
analysis::ty::type_fn::tests`, 32s): negatives for two const subjects, subject
not last, missing `_`, duplicate literal, `{N-1}` in the `0` arm (underflow),
`{N+1}` and `{N-1+1}` (non-whitelisted), no self-call, and an assoc-type
projection; plus a `Bush`-style multi-self-call POSITIVE that produces
`data.arms[1].self_calls == [Sub(1), Sub(1)]`. The two S1.3 tests still pass.

Deliberately NOT in this slice (S1.5): normalization/unfolding queries consuming
`TypeFnWfData`, the `TypeNormalizer::fold_ty` hook, and the MIR-boundary assert.

---

## S1.4b: close two type-fn WF holes (Fable steering-02 findings 1a + 2)

Prerequisite amendment before any normalization, per the S1.5 steering. Closes
the two residual paths the pre-build review found in the committed S1.4 code, so
the type-fn -> CTFE edge is severed structurally and no mutual-reference cycle
can reach normalization.

HOLE 1 (Fire-Triangle leak, finding 1a): `walk_arm_ty`'s `Other` arm ignored
`GenericArg::Const`, so `Wrapper<{helper(N)}, ...>` passed WF; after S1.5
substitution the `UnEvaluated` const would force through `evaluate_const_ty`'s
`NotIntExpr` fall-through into the real CTFE machine, reopening the edge with no
termination cover. FIX: `check_arm_const_arg` restricts every arm-RHS const
argument on a non-self-call path to an integer literal or the bare subject `N`;
anything else (incl. a `_` hole) raises the new named
`TypeFnWfError::DisallowedArmConstArg`.

HOLE 2 (salsa cycle, finding 2): `classify_path` detects type-fn-ness only for
single-segment paths, so a qualified path or alias to another type fn bypasses
the `ForeignTypeFnCall` ban; two mutually-referencing defs would then cycle
`normalize(F<n>) -> normalize(G<n+1>) -> normalize(F<n>)`. FIX: a lowered-RHS
cross-check (`collect_type_fn_heads`, a `TyVisitor` over `visit_type_fn`) walks
each lowered arm RHS and requires every `TyBase::TypeFn` head to equal the
defining def with occurrence count == `self_calls.len()`; otherwise the new named
`TypeFnWfError::ForeignTypeFnRefInArm` fires. The cross-check is gated on
`!lowered.has_invalid(db)` so a kind-ill-typed RHS (whose collapsed `Invalid`
spine would drop a head) does not produce a spurious foreign-ref error; kind
errors surface at use sites as before.

Tests (targeted, all green): `rejects_disallowed_arm_const_arg` and
`rejects_helper_call_const_arg` (hole 1); `rejects_foreign_type_fn_via_qualified_path`
(hole 2 cross-check, via `m::G<3>` which passes hole 1 with a literal subject but
lowers to a foreign `G` head) and `rejects_direct_mutual_recursion` (the direct
single-segment route, caught by `ForeignTypeFnCall`); the pre-existing
`accepts_bush_multi_self_call` positive was made kind-correct (`Bush -> (*)` so
`Comp<*, *>` type-checks) since the old fixture's `* -> *` args to a `*`-param
`Comp` were a latent `KindMismatch` the cross-check surfaced. 14/14 type_fn unit
tests pass; `cargo check --workspace` clean.

---

## S1.5: ground normalization on the corrected design

Ground (subject = concrete integer) `recursive type fn` applications now reduce
to their concrete normal form; symbolic subjects stay opaque inside type-fn
bodies and are rejected outside (v1; Slice 2 lifts). Reduction consumes ONLY the
distilled `TypeFnWfData`, applies each `SubjectStep` as a direct `BigUint` op,
and never enters the CTFE machine.

New machinery in `crates/hir/src/analysis/ty/type_fn.rs`:

- `unfold_type_fn_step(db, app)` (salsa, pure one step): selects the arm by the
  ground subject, lowers the arm RHS in the def's scope, and folds it with
  `Unfolder`, which substitutes the def's type params + subject and rebuilds each
  self-call spine as a NEW smaller SATURATED ground application. The self-call
  subject is re-distilled occurrence-locally from the LOWERED const (an
  `Abstract(ArithBinOp)`, or `Evaluated(LitInt)` / `UnEvaluated` fallbacks) and
  stepped by `BigUint`; its `UnEvaluated`/`Abstract` body is never folded (that
  is the CTFE leak the design severs) and the result is reinterned as a canonical
  `Evaluated(LitInt)` reusing the root subject's integral type. Memoizing the
  step is what makes `Bush<n>` a DAG, not an exponential tree.
- `normalize_type_fn_app(db, app)` (salsa driver): scheme A per the steering, a
  single memo entry with a ROOTED local step counter (`normalize_all`, iterative
  head reduction + structural child recursion). The `~4096` ceiling is a plain
  constant, NOT part of any memo key, so a subterm and a root reduce identically
  (no fuel poisoning, confluence preserved); on breach it returns a dedicated
  `InvalidCause::TypeFnRecursionLimit`, never a partial app.

Containment (three lines):

1. GUARANTEE at the S1.3 path-lowering site (`path_resolver.rs`
   `ItemKind::TypeFn` arm): a saturated app is eager-expanded via
   `normalize_type_fn_app` when its subject is ground AND the lowering scope is
   NOT inside a recursive type fn body (`scope_in_type_fn_body`). Inside a body
   the self-call is left opaque (the unfolder owns it, and expanding there would
   re-enter `type_fn_wf` and cycle). A symbolic subject outside a body is a hard
   `InvalidCause::SymbolicTypeFnUnsupported` ("symbolic type-fn application not
   yet supported"). This keeps `TyBase::TypeFn` out of stored types (ADT fields,
   fn sigs) that are never routed through the normalizer.
2. `TypeNormalizer::fold_ty` also reduces any substitution-formed ground app
   (second line, for body checking).
3. `stable_key.rs` TypeFn arm upgraded to a `debug_assert!(false, ...)` tripwire
   (keeps the release-safe key), the last-line MIR-boundary guard (spec sec 7.4).

Diagnostics: two new `InvalidCause` variants (`SymbolicTypeFnUnsupported`,
`TypeFnRecursionLimit`) route through `ty_error.rs` to two new occurrence-site
`TyLowerDiag` variants (`TypeFnSymbolicUnsupported` code 46, `TypeFnRecursionLimit`
code 47), following the `TypeFnNotSaturated` precedent (NOT the def-site
`TypeFnIllFormed` family).

Reductions verified (targeted tests, all green):
- `RPow<Pair, 3>` -> `Comp<Comp<Comp<Par, Pair>, Pair>, Pair>`
- `LPow<Pair, 2>` -> `Comp<Pair, Comp<Pair, Par>>`
- `Half<4>` (a `{N / 2}` Div case) -> `Comp<Comp<Comp<Par, Pair>, Pair>, Pair>`
- `Bush<2>` (multi-self-call) -> `Comp<Comp<Pair, Pair>, Comp<Pair, Pair>>`
- symbolic `RPow<Pair, M>` in a signature rejected outside a body; self-calls
  stay opaque inside (the tests reduce, which requires the arm-lowering opacity).
Each positive test asserts `collect_type_fn_heads` is empty on the normal form
(no `TyBase::TypeFn` survives).

The S1.3 `type_fn_saturation_walk` "Saturated" case was updated: a ground app is
now eager-expanded (S1.5), so `Dup<u8, 3>` (made WF with a self-call) lowers to
`u8`, not to a stored `TyBase::TypeFn` head. The Partial/Bare unsaturated cases
(caught by the saturation walk before expansion) are unchanged.

`cargo check --workspace` clean; 19 `type_fn` + 2 `ty::tests` type-fn unit tests
pass (targeted, debug). Full release `nextest` CI is the orchestrator's at the
slice-1 boundary.

---

## S2.0 (a): explicit impl-target ban (spec sec 5.1 / sec 9.9)

Slice-1 boundary release CI passed 2475/2475 (orchestrator, at HEAD `adfeaaec4`)
before this slice started. S2.0 is the Slice-2 precondition: it welds the trap
doors shut BEFORE the S2.1 gate lift, with NO semantics change (the gate stays
closed; the `SymbolicTypeFnUnsupported` reject is UNTOUCHED). Per Fable
steering-03 sec 5 (S2.0) and sec 1.4 G1.

Landed the headline of S2.0: an EXPLICIT, INDEPENDENT ban on a `recursive type
fn` application appearing in an `impl` header, symbolic OR ground. Until now the
ban was delivered only IMPLICITLY by the S1.5 symbolic reject (a symbolic header
lowers to `Invalid(SymbolicTypeFnUnsupported)`); a GROUND header
(`impl Tr for RPow<Pair, 1>`) was in fact ACCEPTED-BY-EXPANSION (eager-expanded
to an impl on the normal form `Comp<Par, Pair>`). Both are now rejected.

What it rejects and how it hooks in:

- New structural check `type_fn::impl_header_type_fn_site(db, impl_trait) ->
  Option<ImplHeaderTypeFnSite>` walks the impl's HIR types (self type, then
  trait-ref args) and reports the FIRST occurrence of a `recursive type fn`
  head, recursing through path generic args, tuples, arrays, ptr/mode wrappers
  ("in impl headers anywhere"). It is purely structural over HIR paths (resolved
  via the early `resolve_ident_to_bucket`/`resolve_leaf_scope`), run BEFORE any
  lowering/normalization, so it recognizes symbolic AND ground uniformly and
  cannot be un-done by the S2.1 gate lift.
- Two hooks, so the ban is both a DIAGNOSTIC and a soundness barrier:
  1. `implementor_with_errors` (`core/semantic/mod.rs`): checked FIRST, before
     `self.ty(db)`; on a hit returns `(None, [TypeFnInImplHeader])`, so the ban
     is the single clean diagnostic (no trailing symbolic-unsupported /
     missing-assoc noise) and surfaces through `ImplTraitAnalysisPass`.
  2. `lower_impl_trait` (`analysis/ty/trait_lower.rs`): returns `None` on a hit,
     so a banned impl NEVER registers in the trait-impl table. This is the
     soundness half: it stops `impl Tr for RPow<Pair, 1>` from becoming a live
     impl on `Comp<Par, Pair>`, and drops the pre-existing latent risk of the
     symbolic case registering with an everything-unifying `Invalid` self type.
- New diagnostic `TyLowerDiag::TypeFnInImplHeader { span }` (code 48), rendered
  in `analysis/diagnostics.rs`: "cannot implement a trait for a `recursive type
  fn` application", with notes on transparency and the "impl on the combinators"
  remedy. Span points at the self-type or trait-ref site.

Verified there was NO pre-existing `TyBase::TypeFn` impl-target check
(`trait_lower.rs` / `trait_def.rs` / `def_analysis`: zero hits), matching the
steering-03 G1 finding.

v1 narrowing (documented in code): only SINGLE-SEGMENT type-fn heads are
recognized (`resolve_ident_to_bucket` accepts root paths only), mirroring the
S1.4 single-segment self-detection. A qualified/aliased head
(`impl Tr for m::RPow<..>`): its symbolic form is still caught by the S1.5 gate;
the ground qualified form is a v1 gap tracked with the other qualified-path
cases.

`SymbolicTypeFnUnsupported` reject: UNCHANGED (still at
`path_resolver.rs:1928-1934`); the `rejects_symbolic_type_fn_outside_body` test
still passes. Gate stays closed.

Verification: `cargo check --workspace` clean (no warnings). All 23
`analysis::ty::type_fn::tests` pass (19 pre-existing unchanged + 4 new:
`rejects_impl_on_symbolic_type_fn_application`,
`rejects_impl_on_ground_type_fn_application`,
`rejects_impl_on_nested_type_fn_application`, `allows_impl_on_combinator`
over-fire guard).

---

## S2.0 (b) + (c): precondition discharge + mono-time normalization

The other two S2.0 items from steering-03 sec 5. Both turned out to be
substantially satisfied by S1.5 machinery; recorded here per the "verify and
record rather than duplicate" instruction, with the one genuine SSOT addition
that (b) warranted.

### (c) mono-time normalization: ALREADY SATISFIED by S1.5 (verified)

Spec sec 7.4 asks for normalization "when an instance's substitution is applied"
so no `TyBase::TypeFn` reaches MIR. VERIFIED this is already in place:

- MIR instance substitution routes EVERY instance type through the ty-layer
  `normalize_ty` AFTER generic-arg substitution:
  `semantic/instance/semantic.rs::instantiate_normalized_ty` (subst via
  `instantiate_checked`, then `normalize_ty`), `RuntimeInstance::normalized_ty`
  / `normalized_field_types`, and `mir/runtime/lower/type_info.rs:255,276`.
- `normalize_ty` -> `TypeNormalizer::fold_ty` (`normalize.rs:137-148`, the S1.5
  "second containment line") reduces any substitution-formed GROUND type-fn
  application to its normal form; symbolic subjects stay opaque. So a symbolic
  app that becomes ground at instantiation is normalized before MIR, and the
  `stable_key.rs` `TyBase::TypeFn` `debug_assert!` tripwire stays the backstop.

No new mono plumbing was needed (adding a second normalize pass would duplicate
the existing one). Added ONE focused unit test,
`normalize_ty_reduces_ground_type_fn_app`, that HAND-BUILDS a saturated ground
`RPow<Pair, 3>` (bypassing path lowering's first-line eager expansion) and feeds
it to `normalize_ty` (the exact function MIR consumes post-substitution),
asserting it reduces to `Comp<Comp<Comp<Par, Pair>, Pair>, Pair>` with no type-fn
head surviving.

### (b) application-precondition discharge

Spec sec 2.4: the type fn's `where` clause is its application precondition, to be
discharged at every application site.

VERIFIED current state (matches steering-03 sec 1.5): a GROUND site
eager-expands at path lowering BEFORE any WF check sees the application, so the
type fn's OWN `where` clause is presently discharged only INDIRECTLY, via the
combinator constraints of the expanded normal form (the ordinary `ty_constraints`
+ `check_ty_wf` machinery on `Comp<..>`). The resulting concrete type's
well-formedness is enforced; only the direct attribution to the type fn is
missing, and there is no unsoundness (ground apps are fully computed, not
symbolic; the gate is closed).

LANDED the SSOT half of (b): a `TyBase::TypeFn` arm in `ty_constraints`
(`trait_resolution/constraint.rs`) that returns the type fn's `where` clause
(`collect_constraints(GenericParamOwner::TypeFn(def))`) instantiated at the
application args, exactly parallel to the existing `TyBase::Adt` / `TyBase::Func`
arms. This makes the precondition a first-class WF constraint of any SURVIVING
type-fn application, discharged by the ordinary `check_ty_wf` machinery. It is a
deliberate no-op at reachable S2.0 positions (ground apps expand first; symbolic
apps are rejected, so no `TyBase::TypeFn` head reaches `ty_constraints`), so it
cannot change S2.0 behavior (gate stays closed); it is the mechanism S2.1's
symbolic obligations (`P(RPow<F, M>)`) will consume to discharge the precondition
from caller assumptions, giving ground/symbolic parity. Unit test
`ty_constraints_carries_type_fn_where_clause` pins it: `RPow` with
`where F: Marker` yields `Pair: Marker` on a hand-built `RPow<Pair, 3>`; a
`where`-less twin yields the empty list.

DEFERRED into S2.1 (recorded, not built): the DIRECT ground-occurrence discharge
that reports a precondition violation NAMING the type fn at the site. Its clean
home is the shared occurrence-site check that lands with symbolic propagation in
S2.1 (where the surviving symbolic app discharges the precondition through the
`ty_constraints` arm above). Doing it in the S2.0 ground branch of the
path-resolver `ItemKind::TypeFn` arm would require calling the trait solver from
path resolution (salsa-cycle / perf risk) for a non-soundness-critical, gate-
closed parity/diagnostic improvement, which is not worth destabilizing a
must-stay-green precondition slice.

Verification: `cargo check --workspace` clean (no warnings); all 25
`analysis::ty::type_fn::tests` pass (targeted, debug), incl. the 3 new S2.0
(b)/(c) tests. Full release `nextest` CI is the orchestrator's at the slice
boundary.

---

## S2.1: symbolic propagation + assumption-only discharge (ZERO new proof power)

Per Fable steering-03 sec 5 (ladder S2.1) and sec 1.4 G1/G5. Lifts the S1.5
symbolic reject at the single gate into an OPAQUE saturated `TyBase::TypeFn`
application, but POSITION-AWARE: only in non-stored positions. Symbolic
obligations `P(RPow<F, M>)` now reach the solver and are dischargeable
EXCLUSIVELY via the existing assumptions leg (a caller `where RPow<F, M>: P`
bound). No induction engine, no solver strictness change: every accepted
symbolic obligation is an assumption that existing machinery re-checks ground at
every instantiation.

### The gate became position-aware (path_resolver.rs `ItemKind::TypeFn` arm)

The symbolic-subject-outside-a-body branch splits on
`symbolic_type_fn_position_is_stored(scope)` (new, in `type_fn.rs`):

- STORED ADT positions (`scope.item()` is `Struct`/`Enum`/`Contract`): keep
  rejecting with `InvalidCause::SymbolicTypeFnUnsupported`. Field types,
  where-clauses, generic bounds and defaults of an ADT all lower under the ADT
  item's own scope, so a single nearest-item test covers them. A stored field is
  never routed through the ground normalizer and an unresolved symbolic app has
  no defined layout, so this surface stays closed (S2.2+ may lift it). The
  (rare) ADT where-clause case is conservatively folded in; the S2.1 discharge
  workload lives on fn/impl/trait where clauses, which are NOT stored.
- Everything else (fn signatures, where clauses on fns/traits/impls, method
  bodies, type aliases): propagate the saturated app OPAQUELY (`PathRes::Ty(ty)`
  on the live `TyBase::TypeFn` head). An ADT method's signature/body is an
  `ItemKind::Func` scope, so it is correctly non-stored; only the ADT's own
  positions resolve their nearest item to the ADT.

The existing `scope_in_type_fn_body` (leave self-calls opaque) and the ground
eager-expansion branches are unchanged and still take precedence.

### Assumption-only discharge, and no other route for the opaque head

An opaque obligation `P(RPow<F, M>)` enters the ordinary solver. The S2.0
impl-target ban guarantees no registered impl has a `TyBase::TypeFn` self-type
head, so the only sound discharge routes are (a) a blanket impl (self type a
bare param; valid at every ground instantiation) and (b) the caller's
assumptions leg (`proof_forest.rs` `goal_needs_assumptions`: the goal's self
type `has_param`, so assumptions are consulted and unified; a hit yields
`ImplementorId::assumption` / `ImplementorOrigin::Assumption`). No induction
engine exists, so there is no engine route. The type fn's own where clause
discharges at the symbolic site through the S2.0 (b) `ty_constraints`
`TyBase::TypeFn` arm.

SOUNDNESS TRIPWIRE (`proof_forest.rs` `GeneratorNodeData::new`, debug-only):
when the goal's self type is an opaque type-fn application
(`type_fn_app_head(..).is_some()`), `debug_assert!` that NO candidate implementor
has a type-fn self-type head. This directly pins the S2.0 ban: a leaked type-fn-
headed impl is exactly the sec 1.4 G1 "coherence depends on arithmetic" hazard.
Blanket impls remain the documented sound exception.

### ICE audit

Opaque symbolic heads now flow through fn signatures / where clauses / bodies.
The S1.3 Category-C `TyBase` consumers already carry `TypeFn` arms
(`pretty_print`/`kind`/`as_scope`/`name_span`/`applicable_ty`, the visitor hook,
visibility, diagnostics rendering); MIR stays unreachable (a generic def is not
monomorphized by the analysis pass, and `normalize_ty` reduces any app that
becomes ground at instantiation, with the `stable_key.rs` tripwire as backstop).
Verified end-to-end by a full-pass test that puts an opaque app in a parameter
type, a return type, and a `return x` body with zero diagnostics.

### Tests (all targeted, debug; the gate-don't-select cross-check is the core)

- `s21_symbolic_obligation_discharged_by_assumption` (POSITIVE): a generic fn
  whose signature forces `RPow<F, M>: Marker` (via a `Requires<T> where T:
  Marker` wrapper parameter) type-checks WITH the `where RPow<F, M>: Marker`
  assumption. No diagnostics.
- `s21_symbolic_obligation_fails_without_assumption` (NEGATIVE): the same fn
  WITHOUT the bound fails with the trait-bound-not-satisfied diagnostic (opaque
  head, no impl candidate, no assumption).
- `rejects_symbolic_type_fn_outside_body` (NEGATIVE position, pre-existing,
  re-documented): a symbolic app in an ADT FIELD stays rejected.
- `s21_opaque_head_flows_through_signature_no_ice` (AUDIT): param/return/body
  opaque flow, no ICE, no spurious diag.
- `s21_cross_check_gate_matches_ground_select` (CROSS-CHECK, gate-don't-select):
  GATE leg: `Marker(RPow<F, N>)` (RPow's own rigid params) is Satisfied ONLY from
  the assumption, with `ImplementorOrigin::Assumption`; UnSat without it. SELECT
  leg: for n in {0, 1, 2, 4, 7}, ground resolution of the UN-normalized
  `Marker(RPow<Pair, n>)` and of the pre-normalized `Marker(NF_n)` return the
  IDENTICAL unique `ImplementorId` (hence identical origin + `SelDiscriminator`),
  with `ImplementorOrigin::Hir`, and `select_impl` == `Selection::Unique` of it
  on both forms. Since the instantiated assumption becomes exactly
  `RPow<Pair, n>: Marker` at a call site, the gate never diverges from ground
  selection on the normal form. (An e2e codegen twin snapshot was not built;
  ground reduction + selection identity is pinned directly at the solver level.)

Explicitly OUT OF SCOPE (S2.2+, a Fable steering pass runs first): the induction
engine, strict-`Satisfied`-only engine hardening, per-occurrence opaque params,
the deferred `lemma_satisfied` helper, fixpoint goal-set growth, ADT-field
symbolic positions.

Verification: `cargo check --workspace` clean (no warnings); all 29
`analysis::ty::type_fn::tests` pass (targeted, debug), incl. the 5 new S2.1
tests. The debug tripwire is active during the cross-check (debug build) at each
n and does not fire. Full release `nextest` CI is the orchestrator's at the
slice boundary.

## S2.2a: induction-engine preconditions (strict engine + opaque minting + minimal-class recognizer; ZERO proof power)

Fable steering-04 §5 S2.2a. This slice lands the three trapdoor primitives the
minimal induction engine (S2.2b) will consume, each executable and unit-tested
now. It is INERT: every new item has ZERO non-test callers, nothing is wired into
any discharge site, so it adds zero proof power and cannot perturb the baseline
by construction.

New file: `crates/hir/src/analysis/ty/type_fn_induct.rs` (module
`analysis::ty::type_fn_induct`, `#![allow(dead_code)]` documenting the zero-caller
state until S2.2b). Supporting additive changes: `is_query_satisfiable` bumped
private -> `pub(crate)` (the "pub export if needed" the steering allows);
`resolve_leaf_scope`/`body_root_expr`/`collect_type_fn_heads`/`bare_path_ident`
in `type_fn.rs` bumped private -> `pub(super)` for read-only reuse; a dedicated
`Variant::Induction` + `TyParam::induction_opaque`/`is_induction` in `ty_def.rs`.

### 1. Strict satisfaction (steering-04 §1.2 / §1.3)

`enum StrictResult { Proven, NotProven }` is a DISTINCT type from
`GoalSatisfiability`, so no engine result can be routed through the permissive
`is_satisfied` / UnSat-only WF pattern by accident. `NotProven` means "no lemma",
never "refuted for all n" (no negative lemmas).

`strict_prove(db, origin_ingot, goal, assumptions) -> StrictResult`
(`origin_ingot` restored to the task shorthand because impl visibility is keyed
on it): builds the SAME `CanonicalGoalQuery` the ordinary solver builds, runs a
HAS_INVALID/HAS_VAR PRE-FLIGHT on the whole canonical query (goal + bound-extended
assumptions) and declines before querying (`Invalid` unifies with everything, and
`is_query_satisfiable` turns HAS_INVALID into `ContainsInvalid` which
`is_satisfied` then passes: the two traps compose), then calls the EXISTING
tracked `is_query_satisfiable` READ-ONLY and maps:

- strict `Satisfied(_)` -> `Proven`
- `NeedsConfirmation(_)` (empty = depth-cap give-up `proof_forest.rs:161-164`;
  non-empty = multi-solution / coexistence) -> `NotProven`
- `ContainsInvalid` -> `NotProven` (belt; unreachable after the pre-flight)
- `UnSat(_)` -> `NotProven`

No change to the global solver, `is_satisfied`, `GoalSatisfiability`, or the proof
forest. Salsa: the strict path reads the same tracked query every consumer reads
and adds no writes.

### 2. Opaque IH minting (steering-04 §1.3 / C3)

`mint_induction_opaque(db, def, arm_idx, occurrence_idx, kind) -> TyId`
deterministically mints a rigid `TyParam` with the dedicated `Variant::Induction`.
It is `TyParam`-shaped, so `visit_param` sets `HAS_PARAM` (LOAD-BEARING: the
solver's assumptions leg only fires for param-carrying goals,
`proof_forest.rs:396-400`), yet the distinct variant makes it UNEQUAL to every
real param under identity unification (`unify.rs` TyParam arm), so it can never
spuriously unify with a real param. Fresh per occurrence (distinct rigids prove
strictly less, the conservative direction). Index is minted past the owner's real
param count (`OPAQUE_IDX_BASE = 1<<20`); the reserved `<ih#arm.occ>` name renders
in diagnostics but cannot be a source identifier.

Tripwires: a debug collision assert at mint time (opaque != every real param, idx
past the real count); and `Variant::Induction` PANICS in `TyParam::original_idx`
and `TyParam::scope` (an opaque asked for its source index / scope has leaked into
a generic-resolution position, which would be shared-state perturbation).

### 3. Minimal-class recognizer (steering-04 §5 S2.2a item 3)

`minimal_class(db, wf, goal) -> MinimalClass` (`InClass` | `Declined(reason)`),
PURE over `(TypeFnWfData, goal)`, no solver call, no minting. Declining is always
sound (S2.2b falls back to the S2.1 assumption route), so every uncertain shape
declines with a `ClassDecline` reason. Checks: no assoc-type bindings; self type
is a live application of THIS type fn; bare rigid const subject
(`ConstTy(TyParam(normal))`, which an opaque is NOT); forwarded type args
var/invalid/hole/type-fn-free; unary trait (subject-free trait args, a sound
deferred widening for multi-arg traits); <=1 self-call per arm; and the G3 guard
(no bare-subject value argument on a non-self-call path). The G3 walk is a
read-only mirror of the WF checker's `walk_arm_ty`+`check_arm_const_arg`, catching
the bare subject whether it parses as a `GenericArg::Const` (non-int-literal) or a
`GenericArg::Type` bare path equal to the subject name (a bare identifier parses
as a type until lowering slots it into a const param).

### Anti-vacuity divergence (mandatory pin)

`strict_diverges_from_permissive_on_needs_confirmation`: two coexisting impls make
`Marker(Amb)` a genuine multi-solution `NeedsConfirmation` (var-free, so the
pre-flight does not fire first, exercising the mapping arm). In ONE test:
`is_goal_satisfiable(..).is_satisfied()` is TRUE while `strict_prove` returns
`NotProven`. A second pin `strict_diverges_from_permissive_on_contains_invalid`
does the same for the `ContainsInvalid` trap. Both prove the strict check is
genuinely stricter, not a synonym.

### Tests (all targeted, debug; 12 new)

- strict: `strict_maps_unique_satisfied_to_proven` (Satisfied->Proven, permissive
  agrees), `strict_maps_unsat_to_not_proven` (UnSat->NotProven, both agree),
  plus the two divergence pins above.
- opaque: `opaque_mint_distinct_and_rigid` (distinct occurrences -> distinct
  TyIds; deterministic re-mint; HAS_PARAM set, HAS_VAR not; `unify(O,O)` ok,
  `unify(O1,O2)` err, `unify(O, concrete)` err), `opaque_no_collision_and_distinct_queries`
  (no collision with any real param; goals differing only in O1 vs O2 canonicalize
  differently).
- recognizer: `minimal_class_admits_rpow` (InClass), and declines for
  multi-self-call (`Bush`), the G3 bare-subject const arg (a WELL-FORMED `G3Bad`
  with `Wrapper<G3Bad<F,{N-1}>, N>`), a ground subject, a non-type-fn goal, and a
  type-fn head in a forwarded type arg.

Verification: `cargo check --workspace` clean (no warnings); all 12 new tests
pass; the adjacent 29 `analysis::ty::type_fn::tests` (incl. the S2.1 cross-check)
still pass. Zero non-test callers of every new item (grep-confirmed), so the 2475
baseline is untouched by construction. Full release `nextest` CI is the
orchestrator's at the slice boundary.

### Next step (S2.2b): wire the minimal induction engine

At the `type_fn_app_head(goal.self_ty).is_some()` discharge site (the S2.1
`check_ty_wf` / `check_trait_inst_wf` UnSat branch), behind C1 (consult OUTSIDE
the tracked solve, never inside `proof_forest`), gated by `minimal_class ==
InClass`: build per-occurrence opaques via `mint_induction_opaque`, inject the IH
`{P(O)}` into assumptions and re-query via `strict_prove`; literal/base arms
discharge GROUND (C5). On all-arms-Proven, discharge per C2 (assumption-injection
re-query, `ImplementorOrigin::Assumption` required). Cross-check
`default_tier_selection(db, ground_goal) == None` at n in {0,1,2,4,7}, with the
constrained-combinator fixture `impl<A: Marker, B: Marker> Marker for Comp<A,B>`
so the IH is load-bearing (C4), plus the IH anti-vacuity twin.

## S2.2b: the minimal induction engine, WIRED IN (course-of-values, gate-don't-select)

Fable steering-04 §5 S2.2b. Wires the S2.2a primitives into the WF discharge
sites as the actual engine, adding the first symbolic-type-fn PROOF POWER. Slice 2
is complete after this.

### How it hooks in (C1: OUTSIDE the tracked solve)

Two new engine entry points in `type_fn_induct.rs`:

- `try_prove_by_induction(db, solve_cx, goal) -> StrictResult`: runs
  course-of-values induction over the WF-checked subject metric.
- `try_discharge_by_induction(db, solve_cx, goal) -> bool`: the discharge wrapper
  (C2).

Wiring: at BOTH WF discharge sites the S2.1 fixtures exercise, the `check_ty_wf`
constraint loop (`trait_resolution/mod.rs`, the `RPow<F,N>: Marker` route from a
`Requires<..>` where clause) and `check_trait_inst_wf`, the existing `UnSat`
branch now first tries the engine:

```
if type_fn_app_head(db, goal.self_ty(db)).is_some()
    && type_fn_induct::try_discharge_by_induction(db, solve_cx, goal)
{ continue; }   // engine proved it -> treat as WF
return WellFormedness::IllFormed { goal, subgoal };   // S2.1 fallback unchanged
```

This is the well-formedness layer, NOT inside `ProofForest`/`is_query_satisfiable`
(C1: the third instance of the codebase's "consult outside the tracked solve"
rule, alongside CTFE const-predicates and scoped provisions). The engine only ever
calls the STRICT `strict_prove` helper (never the permissive solver, never
`check_ty_wf`/`check_trait_inst_wf`), so solver->engine is one-directional and
there is no salsa cycle (verified: `proof_forest.rs` does not call the WF queries).
The `type_fn_app_head(..).is_some()` gate means BASELINE goals (no type-fn head)
never reach the engine: zero baseline perturbation by input-disjointness.

### The arm-by-arm induction + per-occurrence opaque flow

For a symbolic goal `P(F<A, N>)` in the minimal class (`minimal_class == InClass`,
else immediate decline -> S2.1 fallback), for each arm of `F` (from
`TypeFnWfData`), lower the arm RHS in the def's scope (self-calls stay opaque) and
fold it with `InductionSubst`, which (a) replaces the WHOLE spine of each self-call
with a FRESH per-occurrence rigid induction opaque `O_i` (via
`mint_induction_opaque`, never recursing into the self-call's args) and (b)
substitutes the def's type params with the forwarded args `A`:

- BASE arm (no self-call, C5): `P(body)` strictly proved GROUND under the caller
  assumptions only (the substituted body is exactly the ground normal form at the
  matched subject; a `debug_assert` pins it type-fn-head-free).
- STEP arm (self-calls): inject the induction hypotheses `{P(O_i)}` into the
  assumptions and strictly prove `P(body')`.

All arms `Proven` -> engine `Proven`; any arm `NotProven` -> `NotProven` (decline;
NO negative lemma, never a partial proof). On `Proven`, C2 consumes the lemma by
RE-RUNNING the query with the goal INJECTED as an assumption and accepting only a
strict proof; for the opaque type-fn head that proof can come ONLY from the
injected assumption (the S2.0 impl-target ban leaves no impl candidate), so the
discharge is the exact S2.1 `ImplementorOrigin::Assumption` route and mono
re-resolves ground.

### Soundness tests (gate-don't-select is the whole point; all green, targeted)

New CONSTRAINED-combinator fixture `impl<A: Marker, B: Marker> Marker for
Comp<A, B>` so the IH is genuinely load-bearing (C4):

- `engine_proves_constrained_rpow_without_where_bound` (e2e POSITIVE): a fn whose
  signature forces `RPow<F, N>: Marker` type-checks with NO `where` bound, from
  `F: Marker` alone. The new proof power.
- `engine_declines_when_arg_not_marker` (e2e NEGATIVE, conservatism): the same fn
  with `F` NOT `Marker` is DECLINED (the step arm's `Marker(F)` subgoal is UnSat)
  and the ordinary trait-bound diagnostic fires.
- `engine_cross_check_gate_matches_ground_select` (the CORE cross-check): the
  engine PROVES `Marker(RPow<F, N>)` from `F: Marker` (and the C2 wrapper accepts
  it), DECLINES without it; and for n in {0,1,2,4,7} ground resolution of the
  un-normalized `Marker(RPow<Pair, n>)` and of the normal form `Marker(NF_n)`
  select the IDENTICAL unique implementor (equal `ImplementorId` => equal origin +
  `SelDiscriminator`), pin a real `Hir` impl, are `select_impl::Unique` on both
  forms, AND `default_tier_selection == None` at every n (the tier / N1 dedup never
  engages: the engine never proves a coexistence shape).
- `engine_ih_is_load_bearing` (IH ANTI-VACUITY twin): the step-arm goal
  `Marker(Comp<O, F>)` is `Proven` under `{Marker(O), Marker(F)}` and `NotProven`
  with only `{Marker(F)}` — removing the injected IH genuinely breaks the arm.
- `engine_declines_multi_self_call_shared_opaque` (SHARED-OPAQUE negative): the
  two-self-call `Bush` is `Declined(MultiSelfCallArm)` by the class gate, so the
  engine returns `NotProven` — the gate (not luck) forecloses the `Comp<A, A>`
  hazard. Per-occurrence distinctness itself is pinned by the S2.2a
  `opaque_mint_distinct_and_rigid`.

### Deviation (documented): one S2.1 test's premise was lifted

`s21_symbolic_obligation_fails_without_assumption` asserted the S2.1-era ABSENCE of
proof power. Under its (deliberately UNCONSTRAINED, per C4) `impl<F,G> Marker for
Comp<F,G>` fixture, `RPow<F, n>: Marker` is UNCONDITIONALLY true (`Par` at 0,
unconstrained `Comp` at n>=1), so the engine now SOUNDLY proves it without an
assumption. Renamed to `s21_symbolic_obligation_proven_by_induction_engine` and
flipped to assert clean compilation, with a doc note. All OTHER S2.1 tests
(positive, cross-check, opaque-flow) and the shared `S21_FIXTURES` are UNCHANGED,
so the S2.1 cross-check keeps its exact original coverage; the conservatism pin it
gave up moves to `engine_declines_when_arg_not_marker` (constrained fixture, where
`F: Marker` is truly load-bearing). No other test changed.

### Verification

`cargo check --workspace` clean (no warnings). All 17
`analysis::ty::type_fn_induct::tests` (12 S2.2a + 5 new engine tests) and all 29
`analysis::ty::type_fn::tests` (incl. the S2.1 cross-check, with the one renamed
test) pass in debug, targeted. Zero baseline perturbation: the engine is gated on
`type_fn_app_head(..).is_some()`, which no baseline goal satisfies, so no baseline
tracked query acquires a new dependency edge. Full release `nextest` CI is the
orchestrator's at the Slice-2 boundary (this slice ends Slice 2).

---

## S3a: the Conal-Elliott payoff demonstration (whole pipeline, one narrative)

A realistic, realistically-named demonstration that exercises the WHOLE pipeline
(parser -> type fn -> ground normalization -> symbolic induction) and shows the
feature's REASON TO EXIST: ONE generic algorithm stated over a type-fn-defined
shape family, whose trait obligation is discharged BY THE INDUCTION ENGINE with
NO hand-written `where` bound. This is the Conal-Elliott "generic parallel
algorithm" pattern in miniature. No engine/semantics change: this slice adds
tests and a readable artifact only.

### What was added

- `docs/type-fn/generic-reduce-demo.fe`: the human-readable demonstration
  PROGRAM. In docs/ (NOT a test_files dir), so no harness compiles it and the
  `tree_sitter_parse_strict` concern does not arise; its compiled-and-asserted
  mirror is the `DEMO` const in the tests below (kept in sync manually; the tests
  are the source of truth).
- `crates/hir/src/analysis/ty/type_fn_induct.rs` (`tests`, "Slice 3a" section):
  a `DEMO` fixture plus four tests.

### The demonstration program

- Shape family via two `recursive type fn`s: `RPow<F, N>` (right, top-down) and
  `LPow<F, N>` (left, bottom-up), both perfect binary trees of 2^N leaves reduced
  over the SAME `Comp`/`Par` combinators, differing only in association.
- A CONSTRAINED trait family: `trait Reduce`, `impl Reduce for Par`, and the
  load-bearing `impl<A: Reduce, B: Reduce> Reduce for Comp<A, B>` (the constraint
  makes the induction hypothesis non-vacuous; a blanket `Comp` impl would
  discharge every step arm for free).
- The obligation site: `struct Reducer<S> where S: Reduce {}`; naming it in a
  signature is the application whose WF must discharge `S: Reduce`.
- The generic algorithm: `fn reduce_rpow<F: Reduce, const N: usize>(x:
  Reducer<RPow<F, N>>) {}` and the `LPow` twin, with NO `where RPow<F, N>:
  Reduce` / `where LPow<F, N>: Reduce` bound.

### What the four tests prove

- `demo_generic_reduce_over_shape_family_no_where_bound` (POSITIVE payoff): both
  `reduce_rpow` and `reduce_lpow` type-check to ZERO diagnostics; the engine
  discharges `RPow<F, N>: Reduce` and `LPow<F, N>: Reduce` from `F: Reduce` alone.
- `demo_negative_twin_arg_not_reduce_rejected` (NEGATIVE, non-vacuous): drop
  `F: Reduce` and `reduce_rpow` is rejected (the step arm's `Reduce(F)` subgoal is
  UnSat, so the engine proves nothing).
- `demo_negative_twin_combinator_impl_removed_rejected` (NEGATIVE, impl is
  load-bearing): remove the `Comp` impl and, even WITH `F: Reduce`, `reduce_rpow`
  is rejected (nothing reduces a `Comp`; the induction step is UnSat).
- `demo_ground_normalization_and_engine_discharge_route` (the confirmation, in the
  demo's own vocabulary; mirrors S2.2b `engine_cross_check_gate_matches_ground_select`):
  1. NORMALIZATION: `RPow<Pair, 3>` reduces (interned-id equality against a
     hand-built tree, robust to pretty-print spacing) to
     `Comp<Comp<Comp<Par, Pair>, Pair>, Pair>` with no type-fn head left; ground
     `Reduce` selects the IDENTICAL `Hir` implementor on the un-normalized app and
     on the normal form.
  2. ENGINE ROUTE (symbolic `Reduce(RPow<F, N>)`): `try_prove_by_induction` returns
     `Proven` from `F: Reduce` and `try_discharge_by_induction` (C2) accepts it;
     the discharge is the INDUCTION route, NOT blanket and NOT vacuous, pinned three
     ways: (a) NOT blanket -- the ORDINARY solver (never the engine, which is
     consulted only from the WF layer) with only `F: Reduce` returns UnSat on the
     opaque type-fn head; (b) route is `ImplementorOrigin::Assumption` -- injecting
     the goal as an assumption (exactly what C2 does) resolves via the Assumption
     origin; (c) NOT vacuous -- dropping `F: Reduce` makes the engine `NotProven`.

### Surface-syntax gaps recorded (nothing faked)

The demonstration proves the TYPE-LEVEL payoff (a generic item over the shape
family whose membership obligation is discharged by induction with no `where`
bound) end-to-end and green. Two honest narrowings versus the full
`generic-parallel-fe-sketch.fe`:

1. RETURN KIND `*`, not `* -> *`. The working type fns return `(*)`, so
   `RPow<F, N>`/`Comp<A, B>`/`Par` are `*`-kinded DATA-shape trees, and `Reduce`
   is implemented on those `*`-kinded ADTs. The sketch's higher-kinded
   `-> (* -> *)` functors (whose instances you `map`/`zip`/`scan` over `Self<A>`)
   are NOT exercised; the S1.3 return-kind rule admits `*` and arrows over `*`,
   but the engine's minimal class + these fixtures live at the `*`-kinded form.
2. OBLIGATION RAISED BY A WF CARRIER, not by a value-level method call. The
   `Reducer<S> where S: Reduce` carrier raises `S: Reduce` as a signature
   well-formedness obligation. A value-carrying generic algorithm -- build a value
   of the shape type and call `x.reduce()` / `<RPow<F, N> as Reduce>::reduce()`
   through a symbolic type-fn head, then monomorphize to concrete SSA -- is not
   demonstrated; method resolution through a symbolic type-fn head and the
   value/codegen path are beyond the S1-S2 type-level discharge this build lands.
   The load-bearing Conal-Elliott insight (generic code over the family with no
   per-instantiation `where` proof) is fully shown; the value/codegen half is the
   next surface to open.

### Verification

`cargo check --workspace` clean (no warnings). The four `demo_*` tests pass in
debug, targeted (`cargo test -p fe-hir --lib
analysis::ty::type_fn_induct::tests::demo_`, 4 passed). The pre-existing 17
`type_fn_induct` and 29 `type_fn` tests are unchanged. Next step: the capstone
OVERVIEW doc (the reason-to-exist writeup that this demonstration underpins).

---

## Capstone: `docs/type-fn/OVERVIEW.md`

Wrote the review-facing capstone document for Sean, the architect, and Micah:
motivation (why this is the plan section 4 keystone rung, who benefits),
what the feature is (the three pieces: the restricted `recursive type fn`
form, ground normalization, the induction engine), architecture (how it rides
the FCO carrier, the ty-layer/WF/normalization/engine pieces), the soundness
story (Fire-Triangle containment, the gate-don't-select cross-check, zero
baseline perturbation, the two release CIs, and the specific holes the four
Fable steering reviews caught before or during build), the Slice 3a
demonstration, an honest BUILT-vs-DEFERRED ledger, and how to evaluate the
branch (architect-gated via plan owner decision O3, commit list, verification
commands).

Cross-checked the two release-CI numbers the doc cites (Slice-1 boundary
2475/2475 at HEAD `adfeaaec4`, Slice-2 boundary 2502/2502 at HEAD `d3d88a85b`)
against the actual `cargo nextest run --release --workspace --all-features
--no-fail-fast --locked` logs from those runs (both `NEXTEST_EXIT=0`) rather
than restating the BUILD_LOG prose alone; the FCO base's 2451/2451 comes from
the `fco-sgk` PR #1506 record. No code changed; this is a docs-only commit.

---

## S3b (1 of 2): diagnostic quality pass (reviewer polish, no behavior change)

Audited every type-fn diagnostic named in the S3b task (WF family / code 45,
`TypeFnInImplHeader` 48, `TypeFnSymbolicUnsupported` 46, `TypeFnNotSaturated` /
`TypeFnConstraintRetKind` 43/44, `TypeFnRecursionLimit` 47) against the current
gating logic in `path_resolver.rs` / `type_fn.rs`, not just the rendered text,
to make sure wording still matches what actually triggers each one. Text/notes
only: no diagnostic's trigger condition, span source, or error code changed.

**Found and fixed one real staleness, not just a wording nit.**
`TypeFnSymbolicUnsupported`'s message ("symbolic type-fn application not yet
supported") and note ("v1 reduces ... only when the subject is a concrete
integer; a symbolic subject is planned for a later slice") were written at
S1.5, before S2.1 lifted the gate for non-stored positions and S2.2b's
induction engine started proving symbolic obligations outright. Checked
`path_resolver.rs`'s `ItemKind::TypeFn` arm: this diagnostic is reachable
ONLY from `symbolic_type_fn_position_is_stored(scope)`, i.e. a symbolic
subject in a stored ADT position (a struct/enum/contract field or its `where`
clause) — every other position (fn/trait/impl signatures, `where` clauses,
bodies) has propagated the application opaquely to the solver since S2.1. The
old text told the user a categorically false thing: that symbolic type fns
are unsupported, when the real (and permanent, not "not yet") restriction is
narrower and the escape hatch (moving the obligation to a non-stored
position, where it can be proven automatically or discharged with a `where
<app>: <Trait>` bound) already exists in the same release. New message:
"recursive type fn application with a symbolic subject cannot be stored
here", naming the actual position class in the sub-diagnostic, plus two notes
naming the escape hatch and the fix. Updated the one test that pinned the old
substring (`rejects_symbolic_type_fn_outside_body`, now asserts on `"cannot
be stored here"`).

**`TypeFnRecursionLimit`** had zero notes. Added two: naming the likely
cause (a `const` subject far larger than intended, or self-call nesting that
keeps growing) and stating the ceiling is a fixed compiler constant, not
user-configurable (matching the honest status in `OVERVIEW.md` sec 6 item 3,
which still tracks making it a manifest-config knob as a separate, unbuilt,
follow-up).

**`TypeFnNotSaturated`** (43) already showed expected-vs-given arity in its
sub-diagnostic and named the fix in its note; left as is, message/notes were
already clear.

**`TypeFnConstraintRetKind`** (44) had zero notes; added one explaining a type
fn's return kind describes the KIND of type it produces (not an obligation),
since `Constraint` there reads as a plausible-but-wrong thing to reach for.

**`TypeFnIllFormed` family (code 45, 21 `TypeFnWfError` variants).** 8 of 21
already had a clear one-line message and 3 already had a note
(`SubjectNotDecreasing`, `SubjectMayUnderflow`, `ForeignTypeFnRefInArm`).
Added notes to the remaining 16 (all naming the concrete fix, not restating
the message): `MissingSubject`, `MultipleSubjects`, `SubjectNotLast`,
`SubjectNotUsize`, `MissingWildcardArm`, `ArmAfterWildcard`,
`DuplicateArmLit`, `ForeignTypeFnCall`, `AssocProjInArm`, `DisallowedArmType`,
`SubjectDivZeroFixpoint`, `LiteralSubjectNotSmaller`,
`SelfCallArgsNotVerbatim`, `WhereNotTypeParamBound`, `MissingReturnKind`,
`DisallowedArmConstArg`. Also widened `SubjectNotDecreasing`'s existing note
to name all three accepted subject forms (`{N - k}`, `{N / k}`, and a smaller
literal — it previously named only the first two), and reworded
`SubjectNotDecreasing`'s message itself, which read as comparing "the
subject" to itself ("must be strictly smaller than the subject") -> "must
strictly decrease toward the base case". `ScrutineeMismatch` and `NoSelfCall`
were left note-free: their one-line messages already name the fix directly
(the exact expected identifier; the two concrete alternatives), so a note
would only restate them. The `notes()` match is now exhaustive over the 21
variants (was a trailing `_ => vec![]`), so a future new variant must be
given an explicit note decision rather than silently falling through.

Primary spans were audited variant-by-variant against `type_fn.rs`'s
`Checker`: all point at the right node already (arm-specific errors at
`arm_ty_span(idx)`, subject-shape errors at the generic-param list, the
scrutinee mismatch at the `match` subject, wildcard/duplicate-arm errors at
the arm list or the specific arm). No span changed.

Verification: `cargo check --workspace` clean. Targeted `cargo test -p fe-hir
--lib analysis::ty::type_fn::tests` (29/29, incl. the updated
`rejects_symbolic_type_fn_outside_body`) green. No uitest/snapshot fixture
renders type-fn diagnostic text (grepped the repo; only parser CST-node
snapshots mention "recursive type fn"), so no snapshot regen was needed.

---

## S3b (2 of 2): ground-normal-form hover, wired into the real language server

Surfaces the ground normal form of a `recursive type fn` application on
hover, e.g. hovering `RPow<Pair, 3>` in source shows `Normal form:
Comp<Comp<Comp<Par, Pair>, Pair>, Pair>` alongside the ordinary item info,
following the existing footer pattern hover.rs already uses for discharged
trait obligations / const predicates (`discharged_obligations_footer`,
`discharged_const_predicates_footer`). LSP wiring is fully landed, not
deferred: no follow-up needed here.

### A real bug found and worked around along the way

First attempt gated the new hover footer on the LSP's already-resolved
`Target::Scope(scope)` being `ItemKind::TypeFn(_)`, on the assumption that
hovering a type-fn application resolves its scope to the type fn item
(mirroring how `Target::Local` carries the checked type for a value). Wiring
this and testing it through a real mock-LSP session
(`mock_lsp_hover_shows_type_fn_ground_normal_form`) surfaced that the
assumption is backwards for exactly the case this feature needs: ground
normalization happens transparently INSIDE ordinary path resolution (the
S1.3 eager-expansion guarantee spec sec 4.1), so `resolve_path` on a GROUND
`RPow<Pair, 3>` returns `PathRes::Ty` of the already-reduced
`Comp<Comp<Comp<Par, Pair>, Pair>, Pair>` — and the goto/hover-navigation
helper `resolve_path_with_recv_fallback` that turns a `PathRes` into a
`Target::Scope` therefore reports the scope of `Comp` (the reduced
combinator's own definition), never `RPow`. Confirmed with a throwaway probe
test dumping `find_enclosing_items`/`reference_at`/`target_at` results
directly (not committed): hovering the `RPow` token genuinely resolves, by
design elsewhere in this branch, to `Comp`'s item scope. So gating on the
RESOLVED target can never fire for a ground application — exactly backwards
from what a "ground normal form" feature needs, and the reverse case
(gate DOES fire, application is symbolic) is precisely the case with no
normal form to show.

Fix: `type_fn_application_normal_form` re-derives its own "does this path
name a type fn" gate directly from the path's WRITTEN head, via the early
name resolver (`resolve_leaf_scope`, the same one `type_fn.rs`'s WF checker
already uses for self-call detection), BEFORE calling the full
`resolve_path` that performs ground normalization. `hover.rs`'s footer no
longer looks at `Target` at all; it hands the raw `PathView`'s path + scope
straight to the hir-layer function and lets it decide. This is not a
narrowing gap or a v1 limitation: it is the correct fix, and it is the
reason the mock-LSP test (not just the hir-layer unit test) was worth
writing before calling this done.

### What was added

- `crates/hir/src/analysis/ty/type_fn.rs`:
  `type_fn_application_normal_form(db, path, scope) -> Option<String>`.
  Gates on `resolve_leaf_scope` naming a `TypeFnDef` item, then calls the
  ordinary `resolve_path` (same route the compiler takes for any other
  occurrence of the path) and returns the pretty-printed result only if it
  is invalid-free and carries no residual `TyBase::TypeFn` head (i.e. it was
  ground and reduced all the way). Returns `None` for: a path that does not
  name a type fn, a bare/partial occurrence, a symbolic subject rejected at
  a stored position, or a live opaque symbolic application. No new
  normalization path: reuses `normalize_type_fn_app` transitively through
  the same `resolve_path` route S1.3/S1.5 already built, so there is nothing
  new to keep in sync with the compiler's own reduction.
- `crates/language-server/src/functionality/hover.rs`:
  `type_fn_normal_form_footer(db, reference)`, wired into `hover_helper`
  alongside the pre-existing obligation/const-predicate footers, appending
  `` Normal form: `<pretty>` `` when the function above returns `Some`.
- Tests: 3 targeted `fe-hir` unit tests
  (`hover_normal_form_for_ground_application`,
  `hover_normal_form_none_for_symbolic_application`,
  `hover_normal_form_none_for_unsaturated_application`) feeding a
  source-derived `PathId` + scope straight from a struct field's HIR type
  (mirroring exactly what `hover.rs` feeds it, no hand-built `TyId`), plus
  one real end-to-end mock-LSP test
  (`mock_lsp_hover_shows_type_fn_ground_normal_form` in
  `mock_client_tests.rs`) driving the actual server through `did_open` /
  `did_change` / `hover` and asserting the rendered footer text.

Verification: `cargo check --workspace` clean. `cargo test -p fe-hir --lib
analysis::ty::type_fn::tests::hover_normal_form` (3/3) and `cargo test -p
fe-language-server --lib mock_client_tests::mock_lsp_hover` (4/4, all
pre-existing hover tests plus the new one) green. Ran the full
`mock_client_tests` module both in isolation and alongside the rest of the
suite: one PRE-EXISTING flaky test unrelated to this work,
`repro_concurrent_goto_burst_does_not_deadlock` (a concurrency-timing stress
test, documented in its own docstring as sensitive to available CPU
permits), fails under parallel load in this sandbox; confirmed by stashing
all changes from this slice and reproducing the identical failure on
unmodified HEAD `dd08e87e0`. Not a regression from this work.

No behavior change beyond diagnostics/hover: `type_fn_application_normal_form`
has zero non-test, non-hover callers, and reuses existing queries read-only
(no new salsa-tracked query, no new mutation of any existing one).
