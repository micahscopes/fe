# FCO Burn-down #5b — Build Plan: delete a Rust AbiSize generator, Fe provider produces it

> **HISTORICAL (DONE) → `FCO_BRIDGE_BURN_DOWN.md` (board) / `FCO_MAP.md`.** This #5b plan SHIPPED: both AbiSize generators deleted byte-identical (`43ebe7efb`, `fbfbba0ef`); see the burn-down board's reconciliation note (2026-06-17). Kept as the dated feasibility/build record. SSOT: `FCO_THE_SLIDE_2026-06-19.md`.

Feasibility spike, 2026-06-17. Investigation + build plan ONLY (no production code
landed). Companion to the #5a reify-proof (DONE, `c0eaec79b`) and the H10 ABI
providerization map (`docs/dev/FCO_H10_ABI_PROVIDERIZATION_MAP.md`, phase H20).

---

## VERDICT: FEASIBLE-WITH-GAPS

"A Fe provider replaces the Rust `lower_error_abi_size_impl` AbiSize generator
with byte-identical codegen, explicitly invoked (not canonicalized), for the
`#[error]` type family."

- **Feasible:** every moving part already exists and is data-driven. The provider
  engine (`ProviderExecutor::run` + `synthesize_provider_impl`) takes plain data
  (`TargetReflection`, `self_ty`, `target_name`, `DeriveGenerics`, `trait_ref`),
  not an AST derive site. The error/msg desugared structs are present as **base
  items** by the time the expansion stage runs, so the compiler can schedule a
  synthetic request against them. The named-provider (`using`) route is the
  explicit, non-canonical selection the multi-backend stance requires, and it does
  NOT require the provider to live in a `CoreDerives` ingot.
- **With-gaps (all surmountable, none a blocker):**
  1. **Staging move.** Today the AbiSize impl is built in *base lowering* (inside
     `lower_error_struct`); the provider engine runs in the *post-lowering
     expansion stage* (`expanded_items_impl`). #5b must move AbiSize generation
     from base → expansion. Decoupled from `Encode` (which stays in base); both
     impls are only resolved at ty_check, post-merge, so the move is sound — but it
     is the real engineering work and the staging risk to validate.
  2. **Desugared-struct exclusion.** The expansion stage currently *refuses* to
     treat `#[error]`/`#[event]` desugared structs as derive targets
     (`expansion.rs:173`, `report_derive_on_desugared_struct`). #5b must add a new,
     **compiler-internal** scheduling path for them (not user `#[derive]`).
  3. **Provider home.** The #5a `StableAbiSize` is fixture-local. For production it
     must move to the `core` ingot (NOT `core_derives`) so it is visible to every
     error/msg-bearing ingot via `Named` selection while staying off the canonical
     surface.
  4. **One narrow byte-identical risk — tuple FIELDS.** The Rust generator inlines
     a special-cased sum for `Tuple` fields that differs in *form* (not value, in
     all known cases) from the provider's uniform `<Field as AbiSize>::HEAD_SIZE`.
     No error/msg field is a tuple today, so this is latent, not hit — but it must
     be guarded (see Gap Analysis G4).

No PAUSE condition is hit (see Multi-backend stance). Recommend proceeding with
**error** as the first slice.

---

## 1. The feasibility gate — can the compiler invoke a Fe provider for a compiler-generated type?

**Yes.** Two routes; Route A is recommended.

### How it works today (the two stages)

- **Base lowering** (`base_scope_graph_impl`, `crates/hir/src/core/lower/mod.rs:146`):
  AST → HIR items + base scope graph. `lower_error_struct`
  (`error.rs:63`, called from `item.rs:495`) and `lower_msg_as_mod`
  (`msg.rs:45`, called from `item.rs:310`) run **here**, synthesizing the struct
  AND its `AbiSize`/`Encode`/`Decode` impls inline via `HirBuilder`. So the
  error/msg structs (and the to-be-deleted AbiSize impls) are **base items**.
- **Post-lowering expansion** (`expanded_items_impl`, `expansion.rs:82`): a
  separate salsa-tracked query that reads `base_scope_graph_impl`, walks
  `base.items_dfs(db)`, collects `DeriveRequest`s from user `#[derive]`/`derive`
  sites, runs `execute_requests` (`derive.rs:673`), and produces generated impls
  that `scope_graph_impl` merges into the base graph. **This is where the provider
  engine runs.** Because it reads the base graph, the error/msg structs are
  already visible to it as targets.

The engine is fully data-driven and AST-free at its core:
- `ProviderExecutor::run(db, provider, reflection, target_ty, target_name)`
  (`provider_executor.rs:385`) — needs only a `&ValidatedProvider`, a
  `&TargetReflection` (plain data: `TargetShape::Struct { fields: Vec<ReflectedField{variant,index,name,ty}> }`,
  `provider.rs:940`), the self type, and the bare name.
- `synthesize_provider_impl(builder, target_name, self_ty, generics, reflection, trait_ref, output)`
  (`provider_synthesis.rs:40`) — replays `ProviderOutput` commands into a real
  `impl Trait for Target`. No AST is consulted.

`execute_requests` (`derive.rs:684-760`) selects the provider
(`provider::select_provider`), runs the executor, and calls
`synthesize_provider_impl`. The `DeriveRequest` is the only thing tying this to a
"user derive site," and its AST-derived fields are non-essential for an internal
caller: `trait_name`/`trait_path` (we supply `AbiSize` / its core path),
`selection` (we supply `ProviderSelection::Named(StableAbiSize path)`),
`primary_range`/`selection_range` (diagnostics spans — we point at the desugared
origin), `desugared` (a `DeriveDesugared` origin — see note in Route A).

### Route A (RECOMMENDED) — synthetic request in the expansion stage

In `expanded_items_impl`'s first walk (`expansion.rs:135`), where desugared error
structs currently route to `report_derive_on_desugared_struct` (`:173`), add an
internal branch: for a `HirOrigin::Desugared(DesugaredOrigin::Error(_))` struct,
build a `TargetReflection` from the struct's HIR fields (exactly like
`lower_struct_derives`, `derive.rs:442-457`, reading `struct_.fields(db)` instead
of an AST) and `schedule` a synthetic `DeriveRequest` with
`selection: ProviderSelection::Named(<core StableAbiSize path>)`. The existing
`execute_requests`/`synthesize_provider_impl` path then generates the impl as a
sibling of the struct in the merged graph — identical mechanism to a user
`derive AbiSize for E using StableAbiSize`, just compiler-scheduled.

- **Insertion point:** `crates/hir/src/core/lower/expansion.rs`, the
  `ItemKind::Struct` arm, `HirOrigin::Desugared(...Error...)` case at `:173`
  (today calls `report_derive_on_desugared_struct`). Add the schedule there.
- **Reflection builder:** new helper paralleling `derive::lower_struct_derives`
  (`derive.rs:410`) but field-sourced from HIR, not AST. `DeriveGenerics` is
  trivial for error structs (no generics) — `impl_params` empty,
  `self_ty_args = GenericArgListId::none`.
- **`desugared` origin:** reuse the struct's own `ErrorDesugared` origin (cast to a
  `DeriveDesugared`-compatible origin), or add a small `DeriveDesugared` variant
  for compiler-internal AbiSize so diagnostics/provenance attribute correctly. The
  #5a provenance path already attributes generated impls to `StableAbiSize`.
- **Deletion:** remove the `lower_error_abi_size_impl(...)` call at
  `error.rs:171` and the function (`error.rs:365-391`). `Encode` generation
  (`error.rs:174`) stays in base lowering, untouched.

### Route B (NOT recommended) — direct executor call during base lowering

Call `ProviderExecutor::run` + `synthesize_provider_impl` directly inside
`lower_error_struct`, replacing `lower_error_abi_size_impl`. **Rejected because:**
- It would emit the generated AbiSize impl into the **base** graph, while user
  derives emit into the **expansion** graph — two divergent codegen paths for the
  same machinery (the opposite of the burn-down's "one chokepoint" goal).
- It re-introduces a stratification hazard: provider selection
  (`visible_providers`) reads cross-ingot base graphs; doing that mid-base-lowering
  risks a query cycle the expansion stage was specifically designed
  (`expansion.rs:13-19`) to avoid.
- It duplicates `DeriveGenerics`/scope-shim setup that the expansion stage already
  provides.

**Conclusion (gate):** clean via Route A. The engine does not assume a user derive
site once you supply the `TargetReflection` + `Named` selection; the only real work
is moving AbiSize generation to the stage where the engine already lives.

---

## 2. Byte-identical gap analysis

What `lower_error_abi_size_impl` (and the shared `create_head_size_assoc_const` /
`create_is_dynamic_assoc_const` / `create_payload_size_func`) produces, vs what the
#5a `StableAbiSize` provider produces.

### The two computation strategies

| Const | Rust generator | `StableAbiSize` provider |
|---|---|---|
| `HEAD_SIZE` | 0-seeded SUM over fields of `build_head_size_expr(field_ty)` (`msg.rs:529-537,548`) | first-field-seeded SUM over fields of `builder.trait_const(field.ty(),"HEAD_SIZE")` = `<F as AbiSize>::HEAD_SIZE` (`derived_abi_size.fe:51-58`) |
| `IS_DYNAMIC` | `false`-seeded OR over fields of `build_is_dynamic_expr(field_ty)` (`msg.rs:457-465,476`) | `false`-seeded OR over fields of `builder.trait_const(field.ty(),"IS_DYNAMIC")` (`derived_abi_size.fe:44,59`) |
| `payload_size` | inherited from the **defaulted trait method** in both cases (the Rust `create_payload_size_func` is a re-emission of the same default — see G5) | inherited default (`abi.fe` `AbiSize::payload_size`) |

### Per-field-typekind comparison — `build_head_size_expr`/`build_is_dynamic_expr`

The Rust generator special-cases by `TypeKind`
(`msg.rs:548-586` head; `:476-517` dynamic):

| Field `TypeKind` | Rust generator emits | Provider emits | Equivalent? |
|---|---|---|---|
| **Path** (`u256`, `bool`, `DynString`, `Bytes`, a struct) | `Field::HEAD_SIZE` (plain path push, `msg.rs:556-563`) | `<Field as AbiSize>::HEAD_SIZE` (QualifiedType, `goal_item_path` `provider_synthesis.rs:499`) | **YES** — both resolve to the same assoc const via inherent/trait resolution. Proven green by #5a (`derived_abi_size.fe`: Point/Mixed/Outer/WithDynamic). |
| **Array** `[T; N]` | `<[T;N] as AbiSize>::HEAD_SIZE` via `abi_size_assoc_expr` (`msg.rs:583`) | `<[T;N] as AbiSize>::HEAD_SIZE` via `trait_const` | **YES — IDENTICAL by construction.** Both route through the **same** core `impl<T,const N> AbiSize for [T;N]` (`abi.fe:936`, `HEAD_SIZE = (N as u256) * abi_field_head_size<T>()`). This is the one real msg field-shape gap from #5a's tested set, and it is covered for free. |
| **Mode** (`mut T`/`ref T`) | recurse into inner (`msg.rs:579`) | `<mut T as AbiSize>::HEAD_SIZE` (uniform) | **NEEDS CHECK / likely N/A.** No error/msg FIELD is a mode type today. If one ever is, the provider relies on an `AbiSize` impl for the mode type (or mode being transparent at resolution); the Rust version peels it. Guard: differential test + reject-or-peel in the provider. **Latent, not hit.** |
| **Tuple** `(A, B)` | inline recursive SUM of element `HEAD_SIZE` (`msg.rs:565-578`), i.e. raw per-element `HEAD_SIZE` | `<(A,B) as AbiSize>::HEAD_SIZE` = `abi_field_head_size<A>() + abi_field_head_size<B>()` (`abi.fe:1048`) | **DIVERGENT IN FORM; value-equal in known cases; NOT HIT.** `abi_field_head_size<T>` returns **32 when `T::IS_DYNAMIC`** (`abi.fe:258`) whereas the Rust tuple branch sums raw `HEAD_SIZE`. For every scalar/dynamic leaf in core, `HEAD_SIZE == 32 == abi_field_head_size`, so values coincide — but a hypothetical nested static-array-in-tuple could differ. **No error/msg field is a tuple today** (tuples appear only in test `args:` harness calls, never in `#[error]`/msg-variant field declarations). See G4. |
| **Ptr / Never** | `0` (`msg.rs:584`) | would need an `AbiSize` impl for ptr/never | **N/A** — not valid ABI field types; never appear in error/msg fields. |

### Findings

- **Error family: NO gaps.** Every `#[error]` struct in the entire repo
  (`std::evm::Panic` = `{code: u256}`; the 3 test fixtures = `u256`/`DynString`
  fields only) is pure **Path** fields. #5a already proves Path + dynamic
  (`DynString`) end-to-end green. The provider matches byte-identically.
- **Msg family: one extra construct, already covered.** msg variant fields add
  **Array** types (`msg_array_arg.fe` `[u256;8]`; `msg_nested_array_arg.fe`
  `[u256;2]`, `[[u256;2];2]`, `[u256;38]`). Both Rust and provider route through the
  identical core `[T;N]` impl — equivalent by construction.
- **The Rust per-typekind recursion is largely redundant** with the leaf `AbiSize`
  impls that already exist in core (`[T;N]` `abi.fe:936`, tuples `abi.fe:1027+`).
  The provider's uniform `<Field as AbiSize>::HEAD_SIZE` delegation is *cleaner* and
  produces the same values — this is itself an argument the deletion is a genuine
  simplification, not moved magic.

### Can the Fe provider/command-language express each construct?

Yes for all hit cases. The provider needs only `reflect.fields()`,
`field.ty()`, `builder.trait_const(ty, name)`, `builder.add`, `builder.or`,
`builder.require<AbiSize>`, `builder.emit_const` — all present and exercised by
`derived_abi_size.fe`. No new command-language primitive is required for error or
for the currently-existing msg field shapes.

---

## 3. Smallest / best-covered first slice: **error**

Pick **`lower_error_abi_size_impl`** (`error.rs:365`), justified:

| Criterion | error | msg-variant |
|---|---|---|
| Generator size | one impl, two consts, one payload func | same shape, but per-variant inside a desugared **mod** |
| Field-type surface | **pure Path** (u256, DynString) — 0 special cases hit | Path **+ Array** (incl. nested arrays) |
| Target structure | a single top-level struct | structs nested in a desugared `mod`; the expansion scheduler walks top-level base items, so msg-variant structs need mod-descent scoping |
| #5a coverage | 100% (Path + dynamic already green) | needs the array path validated end-to-end |
| Real production targets | exactly one (`std::evm::Panic`) + 3 fixtures | dozens of msg fixtures |
| Blast radius of regression | minimal | larger |

Error is strictly smaller, has zero unhit field-typekinds, and its single real
target (`Panic`) plus 3 fixtures form a tight gate. Do error first; msg-variant is
the natural #5c (its only new construct, Array, is already proven equivalent here).

---

## Ordered build plan (error slice)

1. **Promote the provider to `core`.** Move `StableAbiSize` from the fixture into
   the `core` ingot (e.g. `ingots/core/src/abi.fe` or a sibling), `pub`, named
   `StableAbiSize`. NOT `core_derives` (that would make it canonical-selectable —
   forbidden by multi-backend). Keep the #5a body verbatim. Add a unit assertion
   that `select_provider(.., Named(StableAbiSize), goal=AbiSize)` resolves from a
   downstream module (`provider.rs` test pattern at `:1500+`).
2. **Add the HIR-sourced reflection helper.** A `reflect_struct_from_hir(struct_)`
   building `TargetReflection { shape: Struct { fields } }` from `struct_.fields(db)`
   (mirror `derive.rs:442-457`), reusable by both the user path and the new internal
   path.
3. **Wire the internal scheduler.** In `expansion.rs:173` (the
   `DesugaredOrigin::Error` arm), instead of only reporting, build the reflection +
   schedule a synthetic `DeriveRequest{ trait=AbiSize, selection=Named(core
   StableAbiSize path), desugared=<error origin> }`. Keep the existing
   `report_derive_on_desugared_struct` for a *user* `#[derive]` on the error struct
   (still an error). Ensure the generated impl hangs off the error struct's lexical
   parent scope (the `groups`/shim mechanism already does this).
4. **Delete the Rust generator.** Remove `error.rs:171` call + the
   `lower_error_abi_size_impl` fn (`:365-391`). Leave `lower_error_encode_impl`
   (`:174`/`:393`) and the shared `create_*` helpers (still used by msg until #5c).
5. **Run the differential gate (below). Must stay byte-identical green.**
6. **Provenance/diagnostic polish.** Confirm `fe explain` attributes the generated
   `AbiSize for Panic` impl to `StableAbiSize` (G2 of the design-wizard verdict);
   confirm a missing-field-`AbiSize` still diagnoses `6-0003` (the #42 fix), now via
   the provider's `require<AbiSize>` path rather than the Rust generator.

### Differential-test set (the gate)

Existing fixtures whose encode/decode runtime behavior must remain byte-identical
green (these read HEAD_SIZE/IS_DYNAMIC transitively through encode/decode/revert):

- `crates/fe/tests/fixtures/fe_test_runner/custom_error_revert.fe` — `InsufficientBalance{balance:u256, required:u256}` (HEAD_SIZE 64, static). PRIMARY error gate.
- `crates/fe/tests/fixtures/fe_test/abi_dynamic_payload.fe` — `DynamicPayloadError{first:DynString, marker:u256, second:DynString}` (dynamic OR fold; HEAD_SIZE 96). Dynamic error gate.
- `crates/fe/tests/fixtures/fe_test/option_ok_or.fe` — `MyError{code:u256}` (HEAD_SIZE 32). Single-field gate.
- `crates/fe/tests/fixtures/fe_test_runner/unwrap_non_encodable_error.fe` — error encodability path.
- `ingots/std/src/evm/panic.fe` (`Panic{code:u256}`) — exercised by any std panic path; the one real production error target.
- `crates/fe/tests/fixtures/fe_test/derived_abi_size.fe` — the #5a provider fixture; must stay green (the provider is now also core-resident).
- UI-test negatives that must keep diagnosing (not ICE): `crates/uitest/fixtures/ty_check/abi_size_concrete_missing_field.fe`, `derived_abi_size_missing_field_bound.fe` (the `6-0003` path / #42 fix).

Recommended differential method: run `fe test` over the above on `master`-HEAD
(Rust generator) and on the #5b branch (provider), assert identical pass + identical
runtime assertions. The fixtures already assert concrete sizes (e.g.
`custom_error_revert` reverts with the encoded selector+payload; `abi_dynamic_payload`
asserts byte offsets), so a value drift fails them.

NOTE: serialize all cargo builds in this env (OOM/freeze under concurrent target-dir
contention) — one build at a time, timeout 1500s+.

---

## 4. Multi-backend stance + PAUSE check

**No PAUSE condition is hit.** The replacement keeps `AbiSize` derivation
EVM-explicit and NAMED:

- The provider is selected via `ProviderSelection::Named(StableAbiSize)`
  (`select_provider`, `provider.rs:895`), which filters `visible_providers` by
  `name` and does **NOT** require `IngotKind::CoreDerives`
  (`validated_providers_in_ingot` `:599` has no such filter; only the *canonical*
  `core_providers` `:647` filters to CoreDerives). So the provider lives in `core`
  (IngotKind::Core, `ingot.rs:30`), visible everywhere, yet stays OUT of the
  canonical auto-selected surface.
- The compiler invokes it *explicitly* for the error/msg type-family (Route A
  schedules a `Named` request); it is never the `Canonical` provider for `AbiSize`.
  There is no canonical `AbiSize` provider, exactly as #5a and the H10 map intend
  (head/tail word layout is EVM-specific; wasm/spirv get their own).
- The fold primitives (`add`, `or`, `trait_const`) are backend-agnostic infra; only
  the *selection* of this provider for EVM error/msg types is EVM-specific, and that
  selection is compiler-driven and explicit.

This matches the design-wizard verdict (`/workspace/fe-design-wizard-kinded-derive-verdict-2026-06-15.md`,
decision F: "`derive Tr for Type using Provider` … ship as-is") and the H10 map's
multi-backend note (it explicitly says AbiSize is NOT canonicalized into
`core_derives`). **No EVM trait is canonicalized into agnostic core; no architect
escalation needed.**

The autonomy protocol's PAUSE triggers (ConstraintTerm / live-P / public-syntax /
evidence-schema / bridge-deletion-without-replacement) are NOT engaged: this is a
bridge deletion WITH a replacement (the named core provider), using only the
existing builder-command surface, no new public syntax, no ConstraintTerm.

---

## 5. Open questions / single biggest risk

**Biggest risk: the base→expansion staging move, not the codegen equivalence.**
Today the AbiSize impl is a *base* item; #5b makes it an *expansion-generated*
item. Anything that reads the base scope graph and expects to find an `AbiSize`
impl for an error type *before* the merge would break. The expansion design
(`expansion.rs:9-19`) asserts merged-graph consumers see generated items like
hand-written ones, and ty_check/MIR run post-merge — but this must be validated:
the `Encode` impl (staying in base) emits `Self::HEAD_SIZE` reads in `encode_to_ptr`
(`error.rs` via `build_head_size_body_expr`); confirm those resolve fine when the
AbiSize impl is now expansion-resident (expected: yes, resolution is post-merge at
ty_check). **This is the one thing a probe build should confirm** before landing —
recommend a single serialized build running the differential gate, specifically
`custom_error_revert.fe` (the impl is deleted from base; the encode path must still
find HEAD_SIZE).

Open questions:
- **Diagnostic span quality** for a compiler-scheduled request: the synthetic
  `DeriveRequest` has no user `using` site, so `primary_range`/`selection_range`
  must point at the error struct's desugared origin. Confirm provider-failure
  diagnostics (e.g. a future missing-`AbiSize`-on-field) still land on a sensible
  span. The #42 fix's `6-0003` path is the reference.
- **`DeriveDesugared` origin reuse vs a new variant** for compiler-internal AbiSize
  — does provenance (`fe explain`) want to distinguish "compiler auto-derived
  AbiSize" from "user `derive … using StableAbiSize`"? Minor; either works.
- **Evidence/const-predicate layer (H10 step 3)** — out of #5b scope. #5b rides the
  existing builder bridge (string-keyed → K04a typed recognition already landed);
  attaching a const-predicate that *verifies* `HEAD_SIZE == sum(field HEAD_SIZEs)`
  is a later slice. #5b's facts are provider-generated but still trusted, same as
  #5a. Flag for the architect if evidence-backing is wanted at deletion time
  (that would expand scope).
- **msg-variant follow-up (#5c)**: the msg structs live inside a desugared `mod`;
  the expansion scheduler walks base items including mod members
  (`items_dfs`), so the Array path (already proven equivalent here) plus
  mod-scoped impl placement is the only extra work. Sequence error → msg.
