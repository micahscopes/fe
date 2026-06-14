# Quote fragment spec (provider `quote { .. }` expression language)

**Guard doc, 2026-06-14.** The provider quote fragment is deliberately a *restricted*
subset of Fe, elaborated in `crates/hir/src/core/lower/provider_executor.rs`
(`elab_template_expr`) into `GenExpr` and replayed to HIR in
`provider_synthesis.rs` (`replay_expr`). This spec fences what is supported and
sets the rule for adding more, so the fragment grows by coherent families under
provider pressure — not one-off hacks (the fossilization risk).

## Supported forms (current)

| form | elaborates to | added because |
|---|---|---|
| `true` / `false` | `GenExpr::Bool` | StableEq seed |
| string literal | `GenExpr::StrLit` (exact inline width) | EIP-712 / hashing |
| `self` | `GenExpr::SelfRef` | all method-body providers |
| open name (`quote(other) { other.. }`) | `GenExpr::ArgRef` | StableEq `eq(self, other)` |
| `base.${field}` (member-access hole) | `GenExpr::FieldGet` | field iteration |
| `${expr}` (expression hole / splice) | inlines the captured quote/bool/string | fold accumulation |
| `lhs && rhs` | `GenExpr::And` | StableEq |
| `lhs \|\| rhs` | `GenExpr::Or` | **StableOrd** (lexicographic disjunction) |
| `lhs == rhs` | `GenExpr::EqCmp` → `BinOp::Comp(Eq)` | StableEq |
| `lhs < rhs` | `GenExpr::LtCmp` → `BinOp::Comp(Lt)` | **StableOrd** |
| `lhs > rhs` | `GenExpr::GtCmp` → `BinOp::Comp(Gt)` | **StableOrd** |
| `receiver.method(args)` (non-generic) | `GenExpr::MethodCall` | builder calls |
| `match scrut { arms }` + `${variant}(group)` pat holes | `GenExpr::Match` / `VariantBinder` | StableEq enum branch |
| struct/variant init, tuple, keccak, trait_call, static_call | corresponding `GenExpr` | builder commands |

**Comparisons are the operators (`<`/`>`), NOT `.lt()`/`.gt()` method calls.**
Operators are type-directed and resolve against the operand's `Ord` impl; a bare
`.lt()` method call in the goal-trait (`Ord<Self>`) context mis-resolves (would
require `u256: Ord<Point>`). See commit that added StableOrd.

## Unsupported (rejected with named diagnostics)

- integer literals (`invalid quote: integer literals not supported`)
- generic method calls (`...generic method calls are not supported`)
- `<=`, `>=`, `!=`, arithmetic, bitwise, unary `!`/`-`, and other operators
  (`...operator is not supported in quote bodies (quotes support &&, ||, ==, <, >, and method calls)`)
- quotes outside a provider body → `8-0084` (`QuoteOutsideProvider`)
- a provider whose quote fails to elaborate → `13-00xx` (`ProviderFailed`, DeriveLower)
- hygiene: an open name not bound by the destination method, or a binder used
  without `quote(name) { .. }`, is rejected (`invalid quote: ...`).

## Hygiene / no-capture
Holes capture *values* (quote IDs / `Field` handles / compile-time bool/string),
spliced AST-level (so grouping is preserved, no operator-precedence hazard). Open
names must be declared (`quote(other) { .. }`) and must match a parameter the
emitted method binds. Quotes are hygienic generated HIR, not token macros.

## Rule for adding a fragment
A new fragment lands ONLY with:
1. a real **provider fixture** that needs it (provider pressure, not speculation);
2. a **negative/unsupported-form test** where a sibling form should still be rejected;
3. **generated-HIR + type-check coverage** (the generated code compiles and runs);
4. addition by **coherent family**, not one-off (e.g. add `<=`/`>=`/`!=` together
   *when a provider needs ordered/eq comparison beyond `<`/`>`/`==`* — not before).

Do not design a full quote language ahead of provider need; do not add one-off
operators forever. The fragment is a bridge surface (BR5, BRIDGE_INTENTIONAL) and
should stay minimal-but-coherent until/unless the Constraint-kind graduation
reshapes how provider bodies are written.
