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
