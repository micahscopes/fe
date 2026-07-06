# Recursive type fn: implementation map

Landing study for the restricted type-to-type CTFE feature (`recursive type fn`).
Substrate: branch `type-fn` off `fco-sgk` (HEAD `8d1d99bd8` at study time).

Authoritative design:
- `FE_FUTURE_DIRECTIONS_PLAN.md` section 4 (restatement).
- `fe-recursive-type-fn-spec-2026-06-10.md` (full spec; section numbers below refer to it).
- `generic-parallel-fe-sketch.fe` (`RPow`/`LPow`/`Bush`/`RVec`/`LVec`).

The feature is the SAFE sub-language: primitive recursion on a single `usize`
subject, total by construction, first-order, pure, incapable of invoking
generation/reflection. The boundary is enforced by GRAMMAR, not discipline.

## Slice plan (spec §4.8), and where we are

- Slice 0 (fixed-shape foundry): hand-expanded combinator fixtures. Not this track.
- Slice 1: grammar + definition WF + ground-only normalization. THIS TRACK.
  - S1.1 = parser + AST/CST  (this commit series)
  - S1.2 = HIR item + lowering + name resolution
  - S1.3 = ty-layer base (`TyBase::TypeFn`) + signature + application kind-check
  - S1.4 = `type_fn.rs` definition WF (grammar/exhaustiveness/termination/self-call)
  - S1.5 = ground normalization hook (`TypeNormalizer::fold_ty`) + MIR-boundary assert
- Slice 2: induction engine (symbolic obligations). Deferred.
- Slice 3: polish (depth-limit manifest, E0931/2/3, fmt/LSP). Deferred.

Ground-only means: symbolic subjects are an error ("symbolic type-fn obligations
not yet supported") until Slice 2.

## Target grammar (spec §1.1)

```
recursive type fn Name<TypeParams..., const N: usize>() -> (KIND)
    where BOUNDS
{
    match N {
        LIT => TYPE
        ...
        _   => TYPE
    }
}
```

Structural laws (who enforces each):
- exactly one `const _: usize`, declared LAST (the subject)   -> S1.4 (HIR WF); parser is permissive (mirrors the ConstGenericParam "bounds checked in hir" precedent, `crates/parser/src/parser/param.rs:177`).
- body is exactly ONE `match` on the subject                  -> PARSER (grammar shape).
- arms are int literals + mandatory final `_`                 -> PARSER shape; exhaustiveness/dup-lit checks -> S1.4.
- no nested match, no `if`, no `let`, no const-fn calls,
  no assoc-type projection, no panics                         -> PARSER (arm RHS is a type via `parse_type`; `if`/`let`/`match` heads rejected with named errors).
- self-calls only, resolved by DefId (§1.2)                   -> S1.4 (needs name resolution).
- self-call subject shapes `{N-k}` (k>=1), `{N/k}` (k>=2)     -> S1.4 termination check (needs to know it is a self-call by DefId); parser records the braced arg verbatim as a `ConstGenericArg` block.
- at least one arm has a self-call (§1.1 rule 5)              -> S1.4.

Design note: the braced subject form `{expr}` is, per §1.5, ONLY ever legal as a
self-call subject. The parser does NOT fork `parse_type`; it lets `{N-1}` land as
an ordinary `ConstGenericArg` block-expr (parser precedent: `GenericArgScope`,
`crates/parser/src/parser/param.rs:429-432`). Shape validation of those blocks is
S1.4 because it is entangled with the self-call/DefId determination.

---

## Layer 1: Parser (crates/parser)  [S1.1 - THIS SLICE]

Item dispatch is keyed on leading keywords in `ItemScope::parse`
(`crates/parser/src/parser/item.rs:108-192`). `recursive` is NOT a keyword and is
handled CONTEXTUALLY exactly like `derive` / `with`
(`item.rs:132-146`, via `Parser::is_ident`, `crates/parser/src/parser/mod.rs:108`).

Reused machinery (do NOT fork):
- generic params: `parse_generic_params_opt` (`param.rs:664`), building
  `GenericParamList` of `TypeGenericParam` / `ConstGenericParam` (`param.rs:120-190`).
- return kind: `parse_kind_bound` (`param.rs:258-295`) produces `KindBoundMono`
  (`*`), `KindBoundAbs` (`* -> *`), `KindBoundConstraint`. `-> (* -> *)` parses via
  the `LParen` arm of `parse_kind_bound`.
- where clause: `parse_where_clause_opt` (`param.rs:650`) with
  `WhereBracePolicy::Lookahead` (`param.rs:491-505`) so the body `{` is not eaten
  as a brace predicate.
- arm RHS type: `parse_type` (`crates/parser/src/parser/type_.rs`, re-exported).
- separators: mirror `MatchArmListScope` (`expr_atom.rs:447-476`) newline/comma
  handling for arm lists. NOTE: the value `MatchExpr` parser is NOT reused (its
  arm RHS is an expression; ours is a type).

New CST node kinds (add to `SyntaxKind`, `crates/parser/src/syntax_kind.rs`, Items
section ~line 406; `describe()` match at `syntax_kind.rs:619` is EXHAUSTIVE -> every
new kind needs a describe arm; `is_token()` is `matches!` so nodes default to false):
- `RecursiveTypeFn`  - the item node
- `TypeFnRetKind`    - wraps the `-> (KIND)` return kind
- `TypeFnBody`       - `{ <match> }`
- `TypeFnMatch`      - `match N { <arms> }`
- `TypeFnArmList`    - `{ <arm>* }`
- `TypeFnArm`        - `<pat> => <type>`
- `TypeFnArmPat`     - `Int | _`

New parser scopes (add to `crates/parser/src/parser/item.rs`, plus the dispatch
line in `ItemScope::parse`): `RecursiveTypeFnScope`, `TypeFnBodyScope`,
`TypeFnMatchScope`, `TypeFnArmListScope`, `TypeFnArmScope`. Model closely on
`FuncSignatureScope` (`func.rs:54-110`) for the name/generics/paren/arrow/where
sequence, and on `MatchArmListScope`/`MatchArmScope` for the arm list.

New AST accessors (`crates/parser/src/ast/item.rs`; macro `ast_node!` at
`crates/parser/src/ast/mod.rs:35`): a `RecursiveTypeFn` node implementing
`GenericParamsOwner` (`ast/param.rs:545`), `WhereClauseOwner` (`ast/param.rs:561`),
`AttrListOwner` (`ast/attr.rs:157`), `ItemModifierOwner` (`ast/item.rs:764`), with
accessors `name()`, `ret_kind()`, `body()`; plus `TypeFnBody`, `TypeFnMatch`,
`TypeFnArmList` (IntoIterator<Item=TypeFnArm>), `TypeFnArm`. Add `RecursiveTypeFn`
to the `ItemKind` enum (`ast/item.rs:835`) and the `Item::kind()` dispatch chain
(`ast/item.rs:36-55`).

Fixtures (hand-written parser is authoritative; tree-sitter deferred, like
const-predicates):
- Positive (round-trip + snapshot): `crates/parser/test_files/syntax_node/items/`.
  Runner `test_item_list` (`tests/syntax_node.rs:12`). MUST add the fixture
  filename to `EXCLUDED_FILES` in `tests/tree_sitter_parse.rs:9` (the `quote.fe`
  precedent) so `tree_sitter_parse_strict` (`tree_sitter_parse.rs:208`, scans
  `syntax_node/items`) skips it.
- Negative (errors expected): `crates/parser/test_files/error_recovery/items/`
  (recovery on, snapshot of tree; runner `error_recovery.rs:12`) and/or
  `crates/parser/test_files/no_recovery/items/` (recovery off, snapshot of the
  error list; `no_recovery.rs`). These dirs are NOT scanned by any tree-sitter test.

---

## Layer 2: HIR item + lowering + name resolution (crates/hir/src/core)  [S1.2]

Model `TypeFnDef` on `TypeAlias` (carries `generic_params` + a body-ish `type_ref`)
plus `Func`/`Const` (attach a `where_clause` and a `Body`-like arm store).

- New `#[salsa::tracked]` struct `TypeFnDef<'db>` in
  `hir_def/item.rs` (near `TypeAlias`, `item.rs:1119-1134`). Suggested fields:
  `id: TrackedItemId`, `name: Partial<IdentId>`, `attributes: AttrListId`, `vis`,
  `generic_params: GenericParamListId` (`hir_def/params.rs:88`),
  `where_clause: WhereClauseId`, `ret_kind` (lowered `Kind`), a body carrying
  `subject: ParamIdx` + `arms: Vec<(TypeFnPat, Partial<TypeId>)>` where
  `TypeFnPat = Lit(IntegerId) | Wild`, `top_mod: TopLevelMod`,
  `#[return_ref] origin: HirOrigin<ast::RecursiveTypeFn>`. Arm RHS types are
  ordinary `TypeId` (`hir_def/types.rs:6`, `TypeKind` at `types.rs:92`). The
  const param `const N: usize` is a `GenericParam::Const(ConstGenericParam)` whose
  `ty: Partial<TypeId>` is the `usize` ascription (`params.rs:119,157`).

- Add variant `TypeFn(TypeFnDef<'db>)` to `ItemKind<'db>` (`item.rs:47`).
  Every EXHAUSTIVE `ItemKind` match then needs a new arm (build breaks otherwise):
  `name()` 79, `attrs()` 100, `kind_name()` 122, `name_span()` 144, `vis()` 166,
  `top_mod()` 188, `is_type()` 209, `From<GenericParamOwner>` 221. Companion enums:
  `GenericParamOwner` (`item.rs:261` + its methods 276/288/300/312/328/341),
  `WhereClauseOwner` (`item.rs:373`; add TypeFn since it HAS a where clause),
  `TrackedItemVariant` (`item.rs:1861` + `content_repr` match 1934).

- Lowering `lower/item.rs`: dispatch arm at the `match ast.kind()` site
  (`lower/item.rs:348`, add `ast::ItemKind::RecursiveTypeFn`), plus a
  `TypeFnDef::lower_ast` mirroring `TypeAlias::lower_ast` (`lower/item.rs:642`).
- Scope: `lower/scope_builder.rs:147` (`parent_to_child_edge`) new arm near the
  TypeAlias arm (`scope_builder.rs:259`) adding the generic-param scope + a `type_`
  name edge. Provider match `lower/provider.rs:871`.
- Scope graph: `hir_def/scope_graph.rs:667` (`item_from_scope!` list) + import 13.
- Spans: `span/item.rs` `define_lazy_span_node!` for `LazyRecursiveTypeFnSpan`
  (near `span/item.rs:234`), tracked ast accessor `span/mod.rs:139`, and
  `span/transition.rs` `ChainRoot` enum 99 + `top_mod` 69 + `init` 214/231 +
  `impl_chain_root!` list 307.
- Visitor: `visitor.rs` `visit_recursive_type_fn` (near 126), `walk_item` arm
  (465), `walk_*` free fn (910), `VisitorCtxt::with_*` (2788), `ChainRoot` arm 2610.
- Pretty print: `print.rs` dispatch 1201 + a `pretty_print` impl (near 1771).
- Semantics: `semantic/mod.rs:123`, `semantic/symbol.rs` `SymbolKind` variant
  (30/54/95/635), `semantic/reference/*` (has_references 92, resolver 198/256,
  collector 223).

`match`/arm HIR reference shapes (for the arm store): `Expr::Match`
(`hir_def/expr.rs:47`), `MatchArm` (`expr.rs:153`), `Pat::{WildCard,Lit}`
(`hir_def/pat.rs:8,10`), `LitKind::Int(IntegerId)` (`hir_def/mod.rs:187,155`). The
type-fn arm store is a NEW small structure (types, not exprs), not these.

---

## Layer 3: ty-layer base + signature (crates/hir/src/analysis/ty)  [S1.3]

Add `TyBase::TypeFn(TypeFnDef<'db>)` (or a wrapper carrying a `TypeFnSig`) to
`TyBase` (`ty_def.rs:1725-1730`, currently Prim/Adt/Contract/Func). Applications
reuse the existing curried `TyApp` node (`TyData::TyApp`, `ty_def.rs:1168`):
`RPow<Pair,4> = TyApp(TyApp(TyBase(TypeFn(RPow)), Pair), ConstTy(4))`, so
`fold.rs`/`visitor.rs`/`binder.rs` traversals work unchanged.

The signature is carried OUTSIDE the kind language (spec §2.1), exactly as a
const-generic ADT carries its const params: `AdtDef` (`adt_def.rs:27`) holds a
`GenericParamTypeSet` (`ty_lower.rs:1164`); const-ness lives in the `TyId`/
`ConstTyData`, NOT in `Kind`. `TypeFnSig` = `{ ty_params: Vec<(TyParamId,Kind,
PredicateListId)>, subject: ConstParamId, ret_kind: Kind }`. NO dependent kinds:
reuse `Kind::{Star, Abs}` (`ty_def.rs:1391,1399`); `Kind::does_match`
(`ty_def.rs:1410`) is the checker; saturated-app kind is already computed at
`ty_def.rs:1967` (`TyApp` arm of `impl HasKind for TyData`).

Match sites a new `TyBase::TypeFn` variant BREAKS (exhaustive `TyBase` matches,
must add arms) - Category C:
- `ty_def.rs:1751-1797` `TyBase::pretty_print`
- `ty_def.rs:1989-1998` `impl HasKind for TyBase::kind`  (delegate to a new
  `HasKind for TypeFn` producing `* -> ... -> *`, mirror `AdtDef` at `ty_def.rs:2013`)
- `ty_def.rs:594-597` `TyId::as_scope` (destructures `TyBase`)
- `ty_def.rs:622-625` `TyId::name_span` (destructures `TyBase`)
- `term.rs:754-755` `callee_from_func_ty` (destructures `TyBase::Func` vs others)

`TyId::app` construction/kind-check: `ty_def.rs:735-809`; the unsaturated-ctor
reduction analogue is `reduce_trait_ctor_app` (`ty_def.rs:766-809`) routed through
`TyId::app` (`ty_def.rs:754`) - the FCO precedent for "apply a base to args and
reduce."

Ground-ness predicate (unfold trigger): `ty_is_fully_ground` (`const_ty.rs:529-542`).
A `TyBase::TypeFn` reaches it via `TyBase(_) => true`, so an APPLICATION of it is
"ground" only when its args are ground - which is what we want (subject must be an
evaluated literal). The evaluated-literal subject is
`ConstTyData::Evaluated(EvaluatedConstTy::LitInt(IntegerId), <usize>)`
(`const_ty.rs:2743,2756`); the canonical extraction pattern is
`ty_def.rs:1378-1385` (`string_capacity_from_const_ty`). `IntegerId` wraps a
`BigUint` (`const_ty.rs:523`, read via `value.data(db)`).

Lowering enforcement of §1.5 call-site grammar goes in `ty_lower.rs` (reject
`{expr}` subject outside self-calls; subject must be param / literal / const path).

Equality (§7.2): two type-fn applications are equal iff same DefId + pairwise-equal
args - falls out of interned `TyApp` structural equality; no arithmetic theory
needed because `{N-1+1}` is unrepresentable.

---

## Layer 4: normalization + MIR boundary  [S1.5]

Ground reduction hooks in `TypeNormalizer::fold_ty`
(`crates/hir/src/analysis/ty/normalize.rs:104-139`). Current arms: `Self`-param
resolution (107), `AssocTy` projection (115), and `_ => super_fold_with` (137).
Add: after folding, if the type is a SATURATED type-fn application whose subject is
a ground `EvaluatedConstTy::LitInt`, call two new memoized salsa queries
`unfold_type_fn_step` and `normalize_type_fn_app` (spec §4.1) - one arm selection +
substitution + two `BigUint` ops (NOT the CTFE machine), with a depth counter
(default 64, ceiling 4096; §4.2). Symbolic subject: return unchanged (opaque head).
`normalize_ty` (`normalize.rs:34`) is a plain fn called from many tracked queries;
the memoization lives in the two new `#[salsa::tracked]` unfold queries.

MIR boundary (`crates/mir/src/instance/mod.rs`, spec §7.4): run substituted types
through the normalizer; assert post-condition that no `TyBase::TypeFn` reaches MIR
or layout (the G4-style defensive audit). CTFE acyclicity is structural: type-fn
bodies cannot call const fns, so the unfold queries never re-enter CTFE (§4.3).

---

## Immediate next sub-steps (handoff)

- S1.2 (HIR): add `TypeFnDef<'db>` + `ItemKind::TypeFn`; wire the ~30 match sites
  listed in Layer 2 (start with the build-breaking exhaustive `ItemKind` +
  `GenericParamOwner` + `TrackedItemVariant` sites, then lowering/scope/span/
  visitor/print/semantic). Lower the parser AST from Layer 1.
- S1.3 (ty): add `TyBase::TypeFn` + Category-C arms (Layer 3) + `HasKind` + a
  `TypeFnSig`; application-site kind-check in `TyId::app`.
