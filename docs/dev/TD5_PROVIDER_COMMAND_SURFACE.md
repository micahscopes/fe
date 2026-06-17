# TD5.0 — Provider command-surface inventory (FROZEN)

**Status:** TD5.0 deliverable. This is the authoritative, exhaustive enumeration of every
operation the bespoke provider-body executor
(`crates/hir/src/core/lower/provider_executor.rs`) recognizes. It is the table the design
packet (`TD5_PROVIDER_BODY_EFFECTS.md`) deferred to TD5.0.

**Freeze rule (TD5.0 deletion claim):** *no new provider-body operation may be added to the
executor without (a) a TD5 category decision and (b) — for quote forms — an entry in the
quote-fragment spec.* The freeze is enforced by the unit tests
`recognized_*_ops_are_frozen` in `provider_executor.rs`'s `#[cfg(test)]` module, which pin the
exact set of recognized op-name string literals against the canonical
`RECOGNIZED_BUILDER_OPS` / `RECOGNIZED_REFLECT_OPS` / `RECOGNIZED_ITERABLE_OPS` consts. Adding
an op without updating both the dispatch and the canonical list fails the test. **Update the
lists only as part of a TD5 rung** (and update this doc in the same change).

## Counts (AUTHORITATIVE — see "Count reconciliation" below)

| category | count | what it is |
|---|---|---|
| **R** — reflection reads | 12 | read-only queries on `reflect` / `field` / `variant` handles (method dispatch + `for`-loop iterables) |
| **B-obl** — obligation | 1 | `builder.require<Trait>(ty)` |
| **B-build** — generated-HIR builder ops | 39 | the `builder.*` expression/type/sig/pattern/emit cluster (excludes `require` and `finish`) |
| **Q** — quote | 6 | `quote{}` + the five hole/splice forms |
| **CTFE** — control flow | 9 | `let`/`mut`(assign)/`for`/`if`/`else`/`&&`/`||`/`!`/`return` interception points |
| **FIN** — finish | 1 | `builder.finish()` |
| **total** | **68** | |

The packet estimated **~56**. The authoritative count is **68** (or **59** if you exclude the
9 CTFE control-flow interception points, which the packet did not enumerate as "ops"). The
difference is explained in "Count reconciliation".

Dispatch sites are in three functions:
- `eval_method_call` (`provider_executor.rs:1497`) — receiver-typed method dispatch
  (`Value::Builder` delegates to `eval_builder_method`; `Value::Reflect`/`Value::Field`/
  `Value::Variant` handled inline).
- `eval_builder_method` (`provider_executor.rs:1568`) — the big `(method, args)` match for
  all `builder.*` ops. **This is the main dispatch site; the freeze comment lives here.**
- `eval_iterable` (`provider_executor.rs:2182`) — the `for`-loop interception that
  pattern-matches `reflect.fields()` / `reflect.variants()` / `variant.fields()` *instead of
  evaluating the call* (surprise #2).

---

## R — reflection reads (12)

These become a typed **read-only CTFE capability** (TD5c): field/variant handles get a typed
CTFE representation and `reflect.*` reads stop being executor-intercepted strings.

**TD5c status — first slice DONE (the three `reflect.*` scalar reads).** `reflect.is_struct()`,
`reflect.is_enum()`, and `reflect.target_name()` no longer have bespoke arms in
`eval_method_call`. `Value::Reflect` now carries a typed read-only `ReflectHandle` that owns its
own scalar-read property table (`is_struct`/`is_enum`/`target_name`); the executor consults the
handle *by name* and no longer knows those names. They are **removed from
`RECOGNIZED_REFLECT_OPS`** (7 → 4) — they are off the recognized string-keyed executor surface
(total named method/iterable surface 54 → 51). The executor's ambient `target_name` field was
deleted (the read lives on the handle). No live `P`/`ConstraintTerm`/schema change/public syntax:
reflection stays read-only CTFE. **Still executor-owned (later TD5c slices):** the
`field.*`/`variant.*` reads (`ty`/`name`/`is_default`/`precedes`), the three `for`-iterables
(`fields`/`variants`/`variant.fields`), and the mis-shelved `same_ty`/`same_field`.

| op | dispatch site | category | future effect | rung |
|---|---|---|---|---|
| `reflect.is_struct()` | `ReflectHandle::scalar_reads` (MIGRATED off executor) | R | typed read-only `ReflectHandle` scalar read (bool) | TD5c ✔ |
| `reflect.is_enum()` | `ReflectHandle::scalar_reads` (MIGRATED off executor) | R | typed read-only `ReflectHandle` scalar read (bool) | TD5c ✔ |
| `reflect.target_name()` | `ReflectHandle::scalar_reads` (MIGRATED off executor) | R | typed read-only `ReflectHandle` scalar read (string) | TD5c ✔ |
| `field.ty()` | `provider_executor.rs:1524` | R | typed read-only CTFE handle (type) | TD5c |
| `field.name()` | `provider_executor.rs:1530` | R | typed read-only CTFE handle (string) | TD5c |
| `variant.is_default()` | `provider_executor.rs:1543` | R | typed read-only CTFE handle (bool) | TD5c |
| `variant.precedes(other)` | `provider_executor.rs:1549` | R | typed read-only CTFE handle (bool; decl-order index compare) | TD5c |
| `reflect.fields()` (for-iterable) | `provider_executor.rs:2197` | R | typed read-only CTFE iterator over struct fields | TD5c |
| `reflect.variants()` (for-iterable) | `provider_executor.rs:2198` | R | typed read-only CTFE iterator over variants | TD5c |
| `variant.fields()` (for-iterable) | `provider_executor.rs:2199` | R | typed read-only CTFE iterator over variant fields | TD5c |
| `builder.same_ty(a, b)` | `provider_executor.rs:1826` | R | typed read-only CTFE type-identity compare (bool) | TD5c |
| `builder.same_field(a, b)` | `provider_executor.rs:1835` | R | typed read-only CTFE field-identity compare (bool) | TD5c |

**Note on placement:** `same_ty`/`same_field` are spelled as `builder.*` methods but are pure
reflection-style *reads* (they compute a `Value::Bool` from reflected handles and emit no
command/expr). They are catalogued as **R** because that is what they *become* — read-only CTFE
comparisons — not as B-build. The freeze test still pins them in `RECOGNIZED_BUILDER_OPS`
(their literal lives in `eval_builder_method`); the doc category is the migration target.

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

## B-build — generated-HIR builder ops (39)

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

| op | dispatch site | category | future effect | rung |
|---|---|---|---|---|
| `builder.method(name)` | `provider_executor.rs:1950` | B-build | typed builder sig (seed) | TD5e |
| `builder.with_self(sig)` | `provider_executor.rs:1960` | B-build | typed builder sig (`self`) | TD5e |
| `builder.with_arg(sig, n, ty)` | `provider_executor.rs:1967` | B-build | typed builder sig (param) | TD5e |
| `builder.returns(sig, ty)` | `provider_executor.rs:1978` | B-build | typed builder sig (return) | TD5e |

### Emit (generated-item construction) + compile-time string fold

| op | dispatch site | category | future effect | rung |
|---|---|---|---|---|
| `builder.emit_method(sig, body)` | `provider_executor.rs:1606` | B-build | typed builder emit (method) | TD5e |
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

Already ordinary Fe semantically (surprise #2). The only genuinely magic narrowness is the
`for`-loop's `eval_iterable` interception of `reflect.fields()` etc. (catalogued under R above)
and the `Value::Builder`/`Value::Reflect` method-dispatch tables. Beyond removing the
interception, there is little to do here; these are listed for completeness so the freeze
covers the executor's full recognized surface.

| op | dispatch site | category | future effect | rung |
|---|---|---|---|---|
| `let pat = init` | `provider_executor.rs:556` | CTFE | ordinary Fe `let` | (interception removal, TD5f) |
| `name = value` (assign / `mut`) | `provider_executor.rs:669` | CTFE | ordinary Fe assignment | TD5f |
| `for pat in iterable { .. }` | `provider_executor.rs:567` | CTFE | ordinary Fe `for` (over typed CTFE iterators) | TD5c/TD5f |
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
Authoritative count: **68** total / **59** excluding CTFE.

| category | packet | actual | delta | why |
|---|---|---|---|---|
| R | ~12 | 12 | 0 | matches; but actual 12 = 7 method reads + 3 for-iterables + `same_ty`/`same_field` recategorized from B-build |
| B-obl | 1 | 1 | 0 | matches |
| B-build | ~33 | 39 | +6 | the packet's "~33" undercounted the tuple ops (`tuple_expr`/`with_elem`/`tuple_ty`/`with_elem_ty`), `keccak`, `concat`, `str`/`str_ty`, the match-builder ops (`match_expr`/`with_arm`/`variant_binder`), and the pattern ops (`wildcard_pat`/`variant_pat`). Note `same_ty`/`same_field` are spelled `builder.*` but moved to R, which offsets some of the growth. |
| Q | ~7 | 6 | −1 | the packet listed 5 surface forms plus loose phrasing; the exact distinct quote forms are 6 (quote, expr hole, field hole, pattern hole, arm splice, empty quote). |
| CTFE | (unenumerated) | 9 | +9 | the packet treated control flow as a bucket, not as counted ops; the executor recognizes 9 distinct control-flow constructs (plus 3 explicit *rejections*). |
| FIN | 1 | 1 | 0 | matches |

**Net:** counting only the executor's "command surface" the way the packet did (R + B-obl +
B-build + Q + FIN, no CTFE) gives **59** vs the estimated ~56 — close, with B-build the main
undercount. Counting the full recognized surface including control-flow interception gives
**68**. **The authoritative numbers are 45 `builder.*` literals + 7 reflect/field/variant method
reads + 3 for-iterables + 6 quote forms + 9 control-flow constructs.** The 45 + 7 + 3 = 55
"named method/iterable ops" is what the freeze test pins.

### Mechanical pin (what the freeze test enforces)
The freeze test pins the named ops as string literals (counts are the **current** values; the
TD5.0 baseline was 45 / 7 / 3 = 55):
- `RECOGNIZED_BUILDER_OPS` — the 45 arms of `eval_builder_method`.
- `RECOGNIZED_REFLECT_OPS` — the **4** reflect/field/variant method reads in `eval_method_call`
  (TD5.0 baseline 7; **TD5c removed the three `reflect.*` scalar reads** `is_struct`/`is_enum`/
  `target_name`, which migrated onto the typed `ReflectHandle` and are no longer string-keyed
  executor arms).
- `RECOGNIZED_ITERABLE_OPS` — the 3 `for`-loop iterable method names in `eval_iterable`.

Quote forms and control-flow constructs are AST-shape dispatch (not string literals), so they
cannot be pinned as a string set; they are frozen by *rule* (this doc) and guarded structurally
by the existing `Expr`/`Stmt`/`Pat` exhaustive matches — adding a new quote/control form
requires editing those matches, which is itself a TD5 category decision.

---

## Migration map (ladder ⇄ this table)

- **TD5b** removes `require` (B-obl) from executor ownership → real FCO obligation.
- **TD5c** removes all 12 **R** ops → typed read-only CTFE handles/iterators. *Slice 1 DONE:* the
  three `reflect.*` scalar reads (`is_struct`/`is_enum`/`target_name`) now live on the typed
  `ReflectHandle` (off `eval_method_call`, off `RECOGNIZED_REFLECT_OPS`). Remaining: `field.*`/
  `variant.*` reads, the three `for`-iterables, and `same_ty`/`same_field`.
- **TD5d** removes the 6 **Q** forms (and the quote-template vocabulary) → typed generated HIR.
- **TD5e** removes the 39 **B-build** ops + `finish` (FIN) → typed builder effect; the 4
  goal-qualified ops (surprise #3) stay as typed builder-effect ops here.
- **TD5f/g** removes the 9 **CTFE** interception points → provider body runs as ordinary
  effectful compile-time Fe, off the executor.
