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
