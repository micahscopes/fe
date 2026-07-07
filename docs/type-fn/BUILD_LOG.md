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
