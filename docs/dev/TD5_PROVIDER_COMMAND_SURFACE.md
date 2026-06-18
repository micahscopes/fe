# TD5.0 — Provider command-surface inventory (FROZEN)

**Status:** TD5.0 deliverable. This is the authoritative, exhaustive enumeration of every
operation the bespoke provider-body executor
(`crates/hir/src/core/lower/provider_executor.rs`) recognizes. It is the table the design
packet (`TD5_PROVIDER_BODY_EFFECTS.md`) deferred to TD5.0.

**Freeze rule (TD5.0 deletion claim):** *no new provider-body operation may be added to the
executor without (a) a TD5 category decision and (b) — for quote forms — an entry in the
quote-fragment spec.* The freeze is enforced by the unit tests
in `provider_executor.rs`'s `#[cfg(test)]` `freeze_guard` module, which pin the
exact set of recognized op-name string literals against the canonical
`RECOGNIZED_BUILDER_OPS` / `RECOGNIZED_REFLECT_OPS` consts (and structurally pin that the
reflection iterables stay off the executor). Adding an op without updating both the dispatch
and the canonical list fails the test. **Update the lists only as part of a TD5 rung** (and
update this doc in the same change).

## Counts (AUTHORITATIVE — see "Count reconciliation" below)

| category | count | what it is |
|---|---|---|
| **R** — reflection reads | 12 | read-only queries on `reflect` / `field` / `variant` handles (method dispatch + `for`-loop iterables) |
| **B-obl** — obligation | 1 | `builder.require<Trait>(ty)` |
| **B-build** — generated-HIR builder ops | 35 | the `builder.*` expression/type/pattern/emit cluster (excludes `require` and `finish`) |
| **Q** — quote | 6 | `quote{}` + the five hole/splice forms |
| **CTFE** — control flow | 9 | `let`/`mut`(assign)/`for`/`if`/`else`/`&&`/`||`/`!`/`return` interception points |
| **FIN** — finish | 1 | `builder.finish()` |
| **total** | **64** | |

The packet estimated **~56**. The authoritative count is **64** (or **55** if you exclude the
9 CTFE control-flow interception points, which the packet did not enumerate as "ops"). The
difference is explained in "Count reconciliation". **DEVX-A** (the `emit_method` signature
inference, Proposal A of `FCO_METAPROG_DEVX_REVIEW_2026-06-18.md`) dropped the four
signature-dance B-build ops (`method`/`with_self`/`with_arg`/`returns`), shrinking B-build 39 → 35
and `RECOGNIZED_BUILDER_OPS` 43 → 39.

Dispatch sites are now in two functions:
- `eval_method_call` — receiver-typed method dispatch (`Value::Builder` delegates to
  `eval_builder_method`; `Value::Reflect`/`Value::Field`/`Value::Variant` handled inline). The
  reflection iterables (`reflect.fields()`/`reflect.variants()`/`variant.fields()`) are
  ORDINARY method calls here now — they return a `Value::Seq` of typed read-only handles built
  eagerly from the reflection (`reflection_sequence`/`variant_field_sequence`), which the
  `for`-loop iterates like any other sequence value.
- `eval_builder_method` — the big `(method, args)` match for all `builder.*` ops. **This is the
  main dispatch site; the freeze comment lives here.**

*(The `eval_iterable` `for`-loop interception that used to pattern-match the iterable
*expression* — surprise #2 — is GONE as of TD5c. There is no longer a special iterable-read
dispatch site.)*

---

## R — reflection reads (12)

These become a typed **read-only CTFE capability** (TD5c): field/variant handles get a typed
CTFE representation and `reflect.*` reads stop being executor-intercepted strings.

**TD5c status — ALL reflection reads DONE (reads AND iterables).** Every `R` read has migrated
off the bespoke executor. The non-iterating reads live on typed read-only handles that own their
own read vocabulary as a data table; the iterables are ordinary method calls returning a
`Value::Seq` of those same handles. The executor no longer knows any reflection read by name.

- *Slice 1 (DONE):* `reflect.is_struct()`/`is_enum()`/`target_name()` → `ReflectHandle`
  (`Value::Reflect` carries it). `RECOGNIZED_REFLECT_OPS` 7 → 4. The executor's ambient
  `target_name` field was deleted.
- *Slice 2 (DONE):* `field.ty()`/`field.name()` → `FieldHandle` (`Value::Field` now
  carries it, preserving the `FieldKey` identity via `FieldHandle::key`);
  `variant.is_default()`/`variant.precedes(other)` → `VariantHandle` (`Value::Variant` carries it,
  preserving the decl-order index via `VariantHandle::index`; `precedes` is a *binary* read in the
  handle's `binary_read` vocabulary). `RECOGNIZED_REFLECT_OPS` 4 → **0** — the
  `eval_method_call` reflection arms are GONE; the Field/Variant arms now just consult the handle.
  The mis-shelved `builder.same_ty`/`same_field` → the free-standing typed read-only
  `ReflectionCompare` table, resolved in the `eval_builder_method` catch-all; their `("name",`
  arms are deleted, so `RECOGNIZED_BUILDER_OPS` 45 → **43**. Total named method/iterable surface
  51 → **45**.
- *Slice 3 (DONE, this rung — the iterables):* `reflect.fields()`/`reflect.variants()`/
  `variant.fields()` are now ORDINARY method calls in `eval_method_call`, returning a
  `Value::Seq` of the SAME typed read-only handles the scalar path builds (`FieldHandle` /
  `VariantHandle`), constructed eagerly from the reflection at the call site
  (`reflection_sequence` / `variant_field_sequence`). The `for`-loop iterates that sequence
  value like any other; the `eval_iterable` interception, the `Iterable` enum, and the
  `RECOGNIZED_ITERABLE_OPS` const are DELETED. `Value::Seq` is the one new (non-`Copy`) value
  variant. Order (declaration order) and identity (`FieldKey` / variant decl-order index) are
  preserved exactly — derive codegen is byte-identical. Named method/iterable surface 45 → **43**
  (now just the 43 `builder.*` ops). **The reflection-read surface is entirely off the executor.**

No live `P`/`ConstraintTerm`/schema change/public syntax: reflection stays read-only CTFE; the
handles copy facts at construction and emit no commands/obligations.

| op | dispatch site | category | future effect | rung |
|---|---|---|---|---|
| `reflect.is_struct()` | `ReflectHandle::scalar_reads` (MIGRATED off executor) | R | typed read-only `ReflectHandle` scalar read (bool) | TD5c ✔ |
| `reflect.is_enum()` | `ReflectHandle::scalar_reads` (MIGRATED off executor) | R | typed read-only `ReflectHandle` scalar read (bool) | TD5c ✔ |
| `reflect.target_name()` | `ReflectHandle::scalar_reads` (MIGRATED off executor) | R | typed read-only `ReflectHandle` scalar read (string) | TD5c ✔ |
| `field.ty()` | `FieldHandle::scalar_reads` (MIGRATED off executor) | R | typed read-only `FieldHandle` scalar read (type) | TD5c ✔ |
| `field.name()` | `FieldHandle::scalar_reads` (MIGRATED off executor) | R | typed read-only `FieldHandle` scalar read (string) | TD5c ✔ |
| `variant.is_default()` | `VariantHandle::scalar_reads` (MIGRATED off executor) | R | typed read-only `VariantHandle` scalar read (bool) | TD5c ✔ |
| `variant.precedes(other)` | `VariantHandle::binary_read` (MIGRATED off executor) | R | typed read-only `VariantHandle` binary read (bool; decl-order index compare) | TD5c ✔ |
| `reflect.fields()` (iterable) | `reflection_sequence` (ordinary method call; MIGRATED off executor) | R | `Value::Seq` of `FieldHandle` over struct fields | TD5c ✔ |
| `reflect.variants()` (iterable) | `reflection_sequence` (ordinary method call; MIGRATED off executor) | R | `Value::Seq` of `VariantHandle` over variants | TD5c ✔ |
| `variant.fields()` (iterable) | `variant_field_sequence` (ordinary method call; MIGRATED off executor) | R | `Value::Seq` of `FieldHandle` over variant fields | TD5c ✔ |
| `builder.same_ty(a, b)` | `ReflectionCompare::binary_read` (MIGRATED off executor) | R | typed read-only CTFE type-identity compare (bool) | TD5c ✔ |
| `builder.same_field(a, b)` | `ReflectionCompare::binary_read` (MIGRATED off executor) | R | typed read-only CTFE field-identity compare (bool) | TD5c ✔ |

**Note on placement:** `same_ty`/`same_field` are spelled as `builder.*` methods but are pure
reflection-style *reads* (they compute a `Value::Bool` from already-evaluated operands and emit no
command/expr). They were catalogued as **R** because that is what they *became* — read-only CTFE
comparisons. TD5c moved their vocabulary off `eval_builder_method`'s bespoke arms onto the typed
read-only `ReflectionCompare` table (consulted in the catch-all), so they are **removed from
`RECOGNIZED_BUILDER_OPS`** (their `("name",` arms are deleted).

---

## B-obl — obligation (1)

Becomes ordinary obligation emission (TD5b/TD5.2). **This is the next migration after TD5.0.**

| op | dispatch site | category | future effect | rung |
|---|---|---|---|---|
| `builder.require<Trait>(ty)` | `provider_executor.rs:1578` | B-obl | ordinary provider-origin FCO obligation (goal=`Trait<ty>`) | TD5b |

**Surprise #1 (silent drop of concrete requirements).** `require` does NOT produce a real
obligation today. The executor records a `BuilderCommand::Require { ty, trait_path }` (built at
`provider_executor.rs:1601-1603`). Synthesis then turns commands into a where-clause in
`provider_synthesis.rs::requirement_where_clause` (`provider_synthesis.rs:135`): it walks the
recorded requirements (`:142`) and emits a `WherePredicate` **only for the target generic
params the required type mentions** (`ty_mentions_param`, `:156`/`:161`). A requirement on a
**fully concrete** type (e.g. `require<Eq>(SomeConcrete)`) mentions no param, so it produces
**no predicate at all** — it is silently dropped, relying on use-site discharge in the
generated method bodies. TD5.2 removes that silent drop by emitting a real obligation
(goal=`Trait<ty>`) regardless of concreteness — which is exactly why TD5.2 collides with the
const-ref ICE (W6, `body.rs:730`): a missing concrete impl must diagnose `6-0003` instead of
panicking.

---

## B-build — generated-HIR builder ops (35)

The largest cluster; the shrink target. Becomes a typed generated-HIR **builder effect** scoped
to the goal `G` (TD5e), EXCEPT the four goal-qualified ops that cannot be quoted hygienically
(surprise #3) — those remain typed builder-effect ops even after TD5.4 quotes absorb the rest.

### Generated expressions

| op | dispatch site | category | future effect | rung |
|---|---|---|---|---|
| `builder.bool(x)` | `provider_executor.rs:1669` | B-build | quote literal / typed builder expr | TD5d→TD5e |
| `builder.and(a, b)` | `provider_executor.rs:1675` | B-build | quote `&&` / typed builder expr | TD5d→TD5e |
| `builder.or(a, b)` | `provider_executor.rs:1685` | B-build | quote `\|\|` / typed builder expr | TD5d→TD5e |
| `builder.add(a, b)` | `provider_executor.rs:1680` | B-build | typed builder expr (int add; ABI layout folds) | TD5e |
| `builder.eq(a, b)` | `provider_executor.rs:1702` | B-build | quote `==` / typed builder expr | TD5d→TD5e |
| `builder.lt(a, b)` | `provider_executor.rs:1707` | B-build | quote `<` / typed builder expr | TD5d→TD5e |
| `builder.gt(a, b)` | `provider_executor.rs:1712` | B-build | quote `>` / typed builder expr | TD5d→TD5e |
| `builder.self_ref()` | `provider_executor.rs:1690` | B-build | quote `self` / typed builder expr | TD5d→TD5e |
| `builder.arg_ref(name)` | `provider_executor.rs:1691` | B-build | quote open-name ref / typed builder expr | TD5d→TD5e |
| `builder.field_get(base, field)` | `provider_executor.rs:1695` | B-build | quote `self.${field}` / typed builder expr | TD5d→TD5e |
| `builder.call(recv, m, args..)` | `provider_executor.rs:1739` | B-build | quote method call / typed builder expr | TD5d→TD5e |
| `builder.trait_call(ty, m, args..)` | `provider_executor.rs:1717` | B-build | **typed builder-effect op (CANNOT quote — surprise #3)** | TD5e |
| `builder.trait_const(ty, name)` | `provider_executor.rs:1732` | B-build | **typed builder-effect op (CANNOT quote — surprise #3)** | TD5e |
| `builder.static_call(ty, m, args..)` | `provider_executor.rs:1752` | B-build | **typed builder-effect op (CANNOT quote — surprise #3)** | TD5e |
| `builder.keccak(arg)` | `provider_executor.rs:1820` | B-build | typed builder expr (`core::keccak`) | TD5e |
| `builder.struct_init()` | `provider_executor.rs:1844` | B-build | quote `Self { .. }` seed / typed builder expr | TD5d→TD5e |
| `builder.variant_init(variant)` | `provider_executor.rs:1850` | B-build | quote `Enum::Variant` seed / typed builder expr | TD5d→TD5e |
| `builder.with_field(init, f, v)` | `provider_executor.rs:1859` | B-build | quote init field / typed builder expr | TD5d→TD5e |
| `builder.match_expr(scrut)` | `provider_executor.rs:1900` | B-build | quote `match` seed / typed builder expr | TD5d→TD5e |
| `builder.with_arm(m, pat, body)` | `provider_executor.rs:1907` | B-build | quote arm / typed builder expr | TD5d→TD5e |
| `builder.variant_binder(v, f, prefix)` | `provider_executor.rs:1930` | B-build | quote `group.${field}` / typed builder expr | TD5d→TD5e |
| `builder.tuple_expr()` | `provider_executor.rs:1788` | B-build | typed builder expr (tuple seed) | TD5e |
| `builder.with_elem(tuple, elem)` | `provider_executor.rs:1789` | B-build | typed builder expr (tuple push) | TD5e |
| `builder.str(s)` | `provider_executor.rs:1779` | B-build | quote string literal / typed builder expr | TD5d→TD5e |

### Generated patterns

| op | dispatch site | category | future effect | rung |
|---|---|---|---|---|
| `builder.wildcard_pat()` | `provider_executor.rs:1922` | B-build | quote `_` arm / typed builder pat | TD5d→TD5e |
| `builder.variant_pat(v, prefix)` | `provider_executor.rs:1923` | B-build | quote `${variant}(group)` pat / typed builder pat | TD5d→TD5e |

### Generated types

| op | dispatch site | category | future effect | rung |
|---|---|---|---|---|
| `builder.ty<T>()` | `provider_executor.rs:1990` | B-build | typed builder type (type-as-written) | TD5e |
| `builder.target_ty()` | `provider_executor.rs:1988` | B-build | typed builder type (`Self` w/ args) | TD5e |
| `builder.self_ty()` | `provider_executor.rs:1989` | B-build | typed builder type (`Self`) | TD5e |
| `builder.str_ty(s)` | `provider_executor.rs:1783` | B-build | typed builder type (`String<LEN>`) | TD5e |
| `builder.tuple_ty()` | `provider_executor.rs:1799` | B-build | typed builder type (tuple seed) | TD5e |
| `builder.with_elem_ty(t, e)` | `provider_executor.rs:1800` | B-build | typed builder type (tuple push) | TD5e |
| `builder.trait_assoc_ty(ty, name)` | `provider_executor.rs:1812` | B-build | **typed builder-effect op (CANNOT quote — surprise #3)** | TD5e |

### Generated method signatures

**DEVX-A (DROPPED).** The four signature-dance ops below were removed
(`FCO_METAPROG_DEVX_REVIEW_2026-06-18.md`, Proposal A). For a derive provider the emitted
method's signature *is* the goal trait's declaration of that method, so re-spelling it op-by-op
was pure ceremony. The signature is now **inferred** from the goal trait's method declaration at
`emit_method(name, body)` (`ProviderExecutor::infer_method_sig`): self-ness and argument names from
the declaration; argument/return types from the declaration with the trait's `Self`/own type-params
substituted by `target_ty()` (argument position) / `self_ty()` (return position) — the SAME witness
the dance produced, so the generated impl is byte-identical. These ops are removed from
`RECOGNIZED_BUILDER_OPS` (43 → 39):

| op (REMOVED) | was | now |
|---|---|---|
| `builder.method(name)` | typed builder sig (seed) | inferred from the goal-trait declaration |
| `builder.with_self(sig)` | typed builder sig (`self`) | inferred (declaration's `self` receiver) |
| `builder.with_arg(sig, n, ty)` | typed builder sig (param) | inferred (declaration's params) |
| `builder.returns(sig, ty)` | typed builder sig (return) | inferred (declaration's return type) |

### Emit (generated-item construction) + compile-time string fold

| op | dispatch site | category | future effect | rung |
|---|---|---|---|---|
| `builder.emit_method(name, body)` | `provider_executor.rs` (`eval_builder_method`) | B-build | typed builder emit (method); signature inferred from the goal trait (DEVX-A) | TD5e |
| `builder.emit_const(name, ty, value)` | `provider_executor.rs:1643` | B-build | typed builder emit (const) | TD5e |
| `builder.emit_assoc_ty(name, ty)` | `provider_executor.rs:1635` | B-build | typed builder emit (assoc type) | TD5e |
| `builder.concat(a, b)` | `provider_executor.rs:1773` | B-build | CTFE string fold (pure compile-time op) | TD5e |

**Surprise #3 (goal-qualified ops can't be quoted hygienically).** `trait_call`
(`:1717`), `trait_const` (`:1732`), `static_call` (`:1752`), and `trait_assoc_ty` (`:1812`)
synthesize `<ty as Goal>::item` paths (`GenExpr::TraitCall`/`TraitConst`/`StaticCall`,
`GenTy::Projection`). The quote surface has **no `<_ as _>` form**, so these CANNOT be absorbed
by TD5.4 quotes; they must remain typed builder-effect ops in TD5.5/TD5e. (`static_call`
additionally requires its `ty` to be a `TypeKind::Path` so the callee path is the type-as-written
with the function name appended — `:1758`.)

---

## Q — quote (6)

Becomes typed generated-HIR with hygiene + typed holes (TD5d/TD5.4). Quote forms are recognized
across `eval_expr` (construction + capture), `elab_template_expr`, `elaborate_quote_arms`, and
`elab_arm_items`.

| op | dispatch site | category | future effect | rung |
|---|---|---|---|---|
| `quote { .. }` / `quote(open) { .. }` | `provider_executor.rs:733` (construct), `953`/`1132` (elaborate) | Q | typed generated HIR + hygiene | TD5d |
| `${expr}` expression hole | `provider_executor.rs:776` (capture), `1197` (elaborate) | Q | typed expression hole | TD5d |
| `base.${field}` member-access hole | `provider_executor.rs:781` (capture), `1222` (elaborate) | Q | typed field/binder hole | TD5d |
| `${variant}(group)` pattern hole (arm) | `provider_executor.rs:870` (capture), `1076` (elaborate) | Q | typed pattern hole | TD5d |
| `${arms}` arm splice (standalone) | `provider_executor.rs:1053` (elaborate), `1005` (`elaborate_quote_arms`) | Q | typed arm splice | TD5d |
| empty `quote { }` (arm-fold seed) | `provider_executor.rs:1019` | Q | typed empty arm sequence | TD5d |

The quote elaboration also reuses the comparison/init/method builder primitives
(`elab_template_expr`, `:1132`–`:1337`): inside a quote body the executor recognizes `&&`/`||`/
`==`/`<`/`>` operators, `self`/open names, method calls, `match`, literals, and `self.${field}`.
Those are not separate ops — they map onto the same `GenExpr` layer the `builder.*` ops produce
— but they ARE part of the quote *template vocabulary* and any addition to that vocabulary is a
Q-category change governed by the freeze rule.

---

## CTFE — control flow (9)

Already ordinary Fe semantically (surprise #2). The `for`-loop's old `eval_iterable`
interception of `reflect.fields()` etc. (catalogued under R above) is **GONE** as of TD5c — the
iterables are ordinary method calls returning a `Value::Seq`, and the `for`-loop iterates that
sequence value like any other. What remains is the `Value::Builder`/`Value::Reflect`
method-dispatch tables. Beyond that, there is little to do here; these are listed for
completeness so the freeze covers the executor's full recognized surface.

| op | dispatch site | category | future effect | rung |
|---|---|---|---|---|
| `let pat = init` | `provider_executor.rs:556` | CTFE | ordinary Fe `let` | (interception removal, TD5f) |
| `name = value` (assign / `mut`) | `provider_executor.rs:669` | CTFE | ordinary Fe assignment | TD5f |
| `for pat in iterable { .. }` | `provider_executor.rs` `Stmt::For` | CTFE | ordinary Fe `for` over an ordinary sequence `Value::Seq` (iterable interception removed, TD5c ✔) | TD5f |
| `if cond { .. }` | `provider_executor.rs:660` | CTFE | ordinary Fe `if` | TD5f |
| `else { .. }` | `provider_executor.rs:663` | CTFE | ordinary Fe `else` | TD5f |
| `&&` (cond) | `provider_executor.rs:698` | CTFE | ordinary Fe `&&` | TD5f |
| `\|\|` (cond) | `provider_executor.rs:701` | CTFE | ordinary Fe `\|\|` | TD5f |
| `!x` (unary not) | `provider_executor.rs:729` | CTFE | ordinary Fe `!` | TD5f |
| `return expr` | `provider_executor.rs:617` | CTFE | ordinary Fe `return` | TD5f |

(`while`/`continue`/`break` are explicitly *rejected* at `provider_executor.rs:624` — they are
NOT part of the surface.)

---

## FIN — finish (1)

Becomes effect-handler finalize (TD5e/TD5f).

| op | dispatch site | category | future effect | rung |
|---|---|---|---|---|
| `builder.finish()` | `provider_executor.rs:1657` | FIN | effect-handler finalize / completion check | TD5e/TD5f |

---

## Count reconciliation (the difference IS a finding)

Packet estimate: **~56** (R ~12, B-obl 1, B-build ~33, Q ~7, CTFE "control flow", FIN 1).
Authoritative count: **64** total / **55** excluding CTFE (after DEVX-A; was 68 / 59).

| category | packet | actual | delta | why |
|---|---|---|---|---|
| R | ~12 | 12 | 0 | matches; but actual 12 = 7 method reads + 3 for-iterables + `same_ty`/`same_field` recategorized from B-build |
| B-obl | 1 | 1 | 0 | matches |
| B-build | ~33 | 35 | +2 | the packet's "~33" undercounted the tuple ops (`tuple_expr`/`with_elem`/`tuple_ty`/`with_elem_ty`), `keccak`, `concat`, `str`/`str_ty`, the match-builder ops (`match_expr`/`with_arm`/`variant_binder`), and the pattern ops (`wildcard_pat`/`variant_pat`); `same_ty`/`same_field` are spelled `builder.*` but moved to R. **DEVX-A then dropped the four signature-dance ops (`method`/`with_self`/`with_arg`/`returns`)**, B-build 39 → 35. |
| Q | ~7 | 6 | −1 | the packet listed 5 surface forms plus loose phrasing; the exact distinct quote forms are 6 (quote, expr hole, field hole, pattern hole, arm splice, empty quote). |
| CTFE | (unenumerated) | 9 | +9 | the packet treated control flow as a bucket, not as counted ops; the executor recognizes 9 distinct control-flow constructs (plus 3 explicit *rejections*). |
| FIN | 1 | 1 | 0 | matches |

**Net:** counting only the executor's "command surface" the way the packet did (R + B-obl +
B-build + Q + FIN, no CTFE) gives **55** (was 59 before DEVX-A). Counting the full recognized
surface including control-flow interception gives **64** (was 68). **The TD5.0 baseline numbers
were 45 `builder.*` literals + 7 reflect/field/variant method reads + 3 for-iterables + 6 quote
forms + 9 control-flow constructs**; the 45 + 7 + 3 = 55 "named method/iterable ops" was what the
freeze test pinned at baseline. *(TD5c and DEVX-A have since shrunk the pinned surface to 39 — see
the mechanical pin below.)*

### Mechanical pin (what the freeze test enforces)
The freeze test pins the named ops as string literals (counts are the **current** values; the
TD5.0 baseline was 45 / 7 / 3 = 55, now **39** / 0 = 39 — the executor's only named surface is
`builder.*`):
- `RECOGNIZED_BUILDER_OPS` — the **39** arms of `eval_builder_method` (TD5.0 baseline 45; **TD5c
  removed `same_ty`/`same_field`** — mis-shelved `builder.*`-spelled identity reads — which moved
  onto the typed read-only `ReflectionCompare` table and are resolved in the catch-all, not as
  `("name",` arms (45 → 43); **DEVX-A dropped the four signature-dance ops** `method`/`with_self`/
  `with_arg`/`returns` — the emitted method's signature is inferred from the goal trait's
  declaration at `emit_method(name, body)` (43 → 39)).
- `RECOGNIZED_REFLECT_OPS` — now **EMPTY** (TD5.0 baseline 7). **TD5c removed every non-iterating
  reflection read** from `eval_method_call`: `reflect.*` (`is_struct`/`is_enum`/`target_name`) →
  `ReflectHandle`, then `field.*` (`ty`/`name`) → `FieldHandle` and `variant.*`
  (`is_default`/`precedes`) → `VariantHandle`. The Field/Variant `eval_method_call` arms now just
  consult the handle's read table by name.
- The iterable-ops const (TD5.0 baseline 3 / shrunk to 2) is **DELETED**. The reflection
  iterables (`reflect.fields()`/`reflect.variants()`/`variant.fields()`) are no longer a special
  dispatch surface — they are ordinary method calls returning a `Value::Seq` of handles, so there
  is no list to pin. Instead the `freeze_guard::iterable_reads_are_off_the_executor` test pins
  *structurally* that neither the iterable-expression interception fn nor the iterable-ops const
  ever reappears in the source.

Quote forms and control-flow constructs are AST-shape dispatch (not string literals), so they
cannot be pinned as a string set; they are frozen by *rule* (this doc) and guarded structurally
by the existing `Expr`/`Stmt`/`Pat` exhaustive matches — adding a new quote/control form
requires editing those matches, which is itself a TD5 category decision.

---

## Migration map (ladder ⇄ this table)

- **TD5b** removes `require` (B-obl) from executor ownership → real FCO obligation.
- **TD5c** removes all 12 **R** ops → typed read-only CTFE handles/iterators. **DONE (complete).**
  *Non-iterating reads:* `reflect.*` (`is_struct`/`is_enum`/`target_name`) → `ReflectHandle`;
  `field.*` (`ty`/`name`) → `FieldHandle`; `variant.*` (`is_default`/`precedes`) → `VariantHandle`;
  `same_ty`/`same_field` → the free-standing `ReflectionCompare` table. *Iterables (final slice):*
  `reflect.fields()`/`reflect.variants()`/`variant.fields()` → ordinary method calls returning a
  `Value::Seq` of those same handles (`reflection_sequence`/`variant_field_sequence`); the
  `eval_iterable` interception, the `Iterable` enum, and the iterable-ops const are deleted. All
  off `eval_method_call`/`eval_builder_method` (`RECOGNIZED_REFLECT_OPS` → 0, the iterable-ops
  const gone, `RECOGNIZED_BUILDER_OPS` 45 → 43). **The reflection-read surface is entirely off the
  executor; only `builder.*` ops remain.**
- **DEVX-A** drops the four signature-dance **B-build** ops (`method`/`with_self`/`with_arg`/
  `returns`) → the emitted method's signature is inferred from the goal trait's declaration at
  `emit_method(name, body)` (`ProviderExecutor::infer_method_sig`). A surface *shrink*, not a
  migration off the executor; generated impls are byte-identical (`RECOGNIZED_BUILDER_OPS` 43 → 39).
  See `FCO_METAPROG_DEVX_REVIEW_2026-06-18.md` (Proposal A).
- **TD5d** removes the 6 **Q** forms (and the quote-template vocabulary) → typed generated HIR.
- **TD5e** removes the 35 **B-build** ops + `finish` (FIN) → typed builder effect; the 4
  goal-qualified ops (surprise #3) stay as typed builder-effect ops here.
- **TD5f/g** removes the 9 **CTFE** interception points → provider body runs as ordinary
  effectful compile-time Fe, off the executor.
