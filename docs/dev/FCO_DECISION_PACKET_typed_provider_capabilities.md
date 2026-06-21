# Architect-decision packet — Typed provider capabilities (P00/P10 = K07/BR2)

> **SUPERSEDED (decided + landed) → `FCO_DERIVE_KIND_FORMS_2026-06-18.md` / `FCO_BRIDGE_BURN_DOWN.md` rows 2–3 / `FCO_MAP.md`.** The typed-capability decision this packet requested was made and shipped: `Evidence`/`ImplBuilder`/`Reflect`/`Derive` are recognized by resolved `core::derive` identity (string-key authority deleted — burn-down rows 2/3, `b82fc43a6` etc.); `Derive` graduated to a real trait. Historical decision packet. SSOT: `FCO_THE_SLIDE_2026-06-19.md`.

**Status: DECISION REQUESTED. Prepared 2026-06-14 by the implementor; no code
written for this — design only.** This is the single highest-leverage decision in
the FCO arc: it is the hard prerequisite for the entire `Derive`-bridge graduation
(K04), for retiring the string-keyed provider authority (BR2) and the
provider-body type/borrowck exemption (BR3), and for the ABI providerization win
(H10). Source inventory: `FCO_BRIDGE_AND_REIFICATION_TARGETS.md` (recommended
next architect-decision packet); staging detail: `FCO_K03_K04_EXECUTION_MAP.md`.

---

## 1. The decision in one line

> How do `Reflect` / `ImplBuilder` / `Evidence` (and later `Derive`) become
> **typed compile-time capabilities** instead of string-recognized names, and how
> much normal type/borrow checking do provider bodies and generated items
> re-enter?

Everything below exists to let you answer that with eyes open. There are **three
sub-decisions** (§5). The implementor has a recommendation for each (§6) but will
not implement until you sign off — per the standing rule "don't solo K03/K04," and
because this slice *deletes* load-bearing authority.

## 2. Why this gates everything downstream

```
typed capabilities (this packet)
  ├─ unblocks K04a  (PrimTy-ize Evidence/ImplBuilder/Reflect, rewire recognition)
  │    └─ K04b (kinds: Derive : (*->Constraint)->Constraint, Evidence : Constraint->*)
  │         └─ K03 (traits as *->Constraint / ConstraintTerm)  ← largest blast radius, last
  ├─ retires BR2 (string-keyed authority) and BR3 (provider-body validation hole)
  └─ unblocks H10 (ABI/static-layout providerization) — the headline M7 reification win
```

K02a (the `Constraint` *kind*) already landed (`804dc959a`); it is **not** the same
as graduating the bridge. Nothing past K04a can proceed until the recognition path
is typed, because adding the builtins as types creates **two recognition paths for
the same names** unless the string path is retired in the same change (§4).

## 3. Current mechanism (verified `file:line`)

**Capability recognition — string-keyed (BR2).**
- `crates/hir/src/core/lower/provider.rs:30-31`
  ```rust
  const REFLECT_KEY: &str = "Reflect";
  const IMPL_BUILDER_KEY: &str = "ImplBuilder";
  ```
  matched at `provider.rs:170-172` by `key_head.data(db).as_str()` against the
  `uses (..)` param head identifier. The code comment is explicit:
  *"The full key/grade capability system is a later milestone."*
- `DERIVE_MARKER = "Derive"`, `DERIVE_FN = "derive"` (`provider.rs:32-35`) — the
  `impl Name: Derive for T` marker and the single `derive` fn are also string-recognized.
- Provider bodies bind these as opaque runtime values `Value::Reflect/Builder/Evidence`
  in the executor (`crates/hir/src/core/lower/provider_executor.rs:301,321,385-391`).

**Provider-body validation exemption — structural (BR3).**
- `crates/hir/src/core/hir_def/item.rs:810` `is_derive_provider_fn` = "parent item
  is a `DeriveProvider`". Recognition here is **clean** (not string-based); the
  *exemption* is the issue. It is consumed at:
  - `crates/hir/src/analysis/semantic/borrowck/check.rs:185` — skip borrowck.
  - `crates/hir/src/analysis/ty/mod.rs:389` and `:718-721` — filtered out of two
    type-analysis passes.
- **Generated items** (the emitted `impl Clone for Point` etc.) already re-enter the
  pipeline; the gap is (a) generated-method-*body* validation and (b) any
  provenance/evidence trail for "this impl came from provider X" (none today).

**effort2's end-state (the port target).** `metaprogramming-effort2` made
`Reflect`/`Evidence`/`ImplBuilder`/`TypeInfo`/`Derive`/`Field` real `PrimTy`
variants with real `HasKind` kinds (`ty_def.rs:1363,2000,2035-2055`):
`Derive : (* -> Constraint) -> Constraint`, `Evidence`/`ImplBuilder : Constraint -> *`.
That layer was dropped in the fco parallel rewrite (the BR0/BR1 string markers are
its scar). **This is a re-port onto fco's substrate, not greenfield** — but
effort2's `analysis/elab/` + `proof_forest` integration is NOT portable (fco
replaced both), so the *recognition rewire* is fco-native work.

## 4. The clash (why string + typed cannot coexist)

Adding `Reflect`/`Evidence`/`ImplBuilder` as `PrimTy` makes **name resolution**
resolve `Reflect<T>` as a prim type. Provider signature validation currently finds
them by string head-ident. With both present, the same `uses (reflect: Reflect<T>)`
is recognized **twice** (once as a prim type, once as `REFLECT_KEY`), and the two
paths can disagree (e.g. a user type also named `Reflect`). So PrimTy-izing the
builtins **requires** rewiring `provider.rs`/`provider_executor.rs` recognition
from string-keys to the resolved prims **in the same change**. That change is K04a,
and it *deletes* `REFLECT_KEY`/`IMPL_BUILDER_KEY` — the load-bearing, irreversible
slice. Hence: architect signoff before it ships.

## 5. The decision surface — three sub-decisions

### D1. The typed shape of a capability obligation (grade / key / scope)
A capability today is just a name. The end state needs at least:
- **key**: which capability (`Reflect`, `ImplBuilder`, `Evidence`, …) — becomes the
  resolved `PrimTy`/type, not a string.
- **grade / mutability**: `ImplBuilder` is `mut` (write authority); `Reflect` is
  read-only. Today encoded ad hoc as `param.is_mut` (`provider.rs:171`). Should
  grade be part of the capability *type* (e.g. `ImplBuilder` is inherently a
  mutable resource) or stay a param annotation?
- **scope/target**: `ImplBuilder<Goal>` / `Evidence<Goal>` are *parameterized by the
  goal being built*. Once K04b lands, `Goal` is a real `Constraint`-kinded
  application; until then it is the string-marker goal. Decision: does K04a carry
  the goal as an opaque type arg (cheap, unblocks ABI) and let K04b make it kinded,
  or wait for the kinded form?

→ **Question for you:** is a capability a *kind-classified type* (`Reflect : * -> *`,
`ImplBuilder : Constraint -> *`) from day one, or a flat `PrimTy` with kinds added
in K04b? (Recommendation §6.)

### D2. How much normal checking do provider bodies re-enter (retire BR3)?
Two sub-questions:
- **Provider body**: typed-in-full, or signature-only? The body is a restricted
  command language (reflection iteration, bool locals, `if`, builder/`quote`
  commands). Full Fe type/borrowck does not obviously apply (quote values are arena
  IDs; `mut ImplBuilder` is a linear-ish resource). Options: (a) keep the body
  exempt but give the *command language* its own checker; (b) type the body under a
  capability-aware environment; (c) signature-only typing + executor-time checks
  (status quo, hardened).
- **Generated items**: today they re-enter trait/type checking but their *method
  bodies* have no dedicated guard fixture (BR3 finding: NO failure-direction
  fixture exists). Decision: is "generated impl with a type-incorrect body fails
  through the **normal** diagnostic path" a guaranteed invariant we pin now?

→ **Question for you:** retire the body exemption (`is_derive_provider_fn` filters at
`ty/mod.rs:389,718-721`, `borrowck/check.rs:185`) in favor of a command-language
checker, or keep it and add a generated-item guard fixture only?

### D3. Provenance-evidence schema for generated impls
The whole FCO thesis is "evidence, not magic." A generated `impl` currently carries
**no evidence** of which provider emitted it or what obligations it discharged.
Decision: do generated impls carry a provenance record (provider id + discharged
requirements + receipt) on the same evidence substrate as const-predicate
discharge (`ty_check/mod.rs:3178-3267` is the existing evidence/receipt schema), so
`fe explain` can show "this `Clone` came from `StableClone`, requiring `Point: Copy`"?

→ **Question for you:** is provenance evidence in-scope for K04a (recommended:
minimal record now), or deferred?

## 6. Implementor recommendation (for each sub-decision)

- **D1:** Introduce the builtins as `PrimTy` in **K04a carrying the goal as an
  opaque type arg**, and add the `HasKind` kinds in **K04b** (so K04a is "typed
  recognition" and K04b is "kinded"). Rationale: K04a's value is *retiring the
  string path*; coupling it to the full kind law enlarges the irreversible slice
  unnecessarily and drags in K03's `ConstraintTerm` (largest blast radius). Grade =
  part of the capability type (`ImplBuilder` is inherently the mutable build
  resource), with `param.is_mut` kept as a redundant assertion during migration.
- **D2:** **Keep the structural exemption for the provider body** (its command
  language is genuinely not ordinary Fe) **but give generated items a hard guarantee
  and a guard fixture now** (BR3): a generated impl whose body is type-incorrect
  must fail through the normal trait/type diagnostic path. This is the cheapest
  honest hardening and needs no kind work. A full command-language checker is a
  separate, later milestone.
- **D3:** **Minimal provenance record in K04a** (provider id + the `require<..>`
  obligations it discharged), rendered by the existing receipt path. Defer richer
  proof transport to the SMT/CTCubFe era.

Net: this makes K04a = "PrimTy-ize the four capability builtins + rewire
recognition string→typed + minimal provenance," with K04b (kinds) and K03
(`ConstraintTerm`) strictly after, exactly as `FCO_K03_K04_EXECUTION_MAP.md` stages
it.

## 7. Blast radius & staging
- New `PrimTy` variants break **every exhaustive `match PrimTy`** (codegen, mir,
  abi) — workspace-wide but mechanical. Triage list to be produced before coding.
- `Reflect`/`Evidence`/`ImplBuilder` becoming real types can **shadow/relocate name
  resolution** for those idents — provider signature validation (`provider.rs`)
  must move in lockstep (this is the §4 clash).
- Do **not** start with `TyData::ConstraintTerm` (K03) — largest blast radius,
  depends on this packet's typed foundation.

## 8. Acceptance criteria for K04a (once decided)
- All existing provider `fe_test` fixtures still green (`derived_eq_default`,
  `derived_clone`, `derived_ord` [new], `quote_provider`, `derived_eip712`) — no
  regression from the string→typed rewire.
- `Reflect<T>` resolves to the builtin type, not a string key; `REFLECT_KEY` /
  `IMPL_BUILDER_KEY` deleted.
- A **negative** fixture: a provider emitting a type-incorrect generated impl fails
  through the normal diagnostic path (the BR3 guard — see the BR3 task).
- (If D3 accepted) `fe explain` shows provider provenance for one generated impl.

## 9. What the implementor will do *without* this decision
Safe, decision-free work proceeding in parallel (does not touch the string path):
std-lib providers on the existing bridge (`StableOrd` [done/landing], ABI
`Encode`/`Decode`/`AbiSize`), the BR3 guard fixture (D2's negative case, which is
useful regardless of D2's outcome), and doc reconciliation. None of these
pre-commit any answer here.
