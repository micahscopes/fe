# Moving Rust desugaring into Fe metaprogramming — prioritized menu

> **HISTORICAL/REFERENCE (deletion-target menu) → `FCO_BRIDGE_BURN_DOWN.md` / `FCO_MAP.md` / SSOT `FCO_THE_SLIDE_2026-06-19.md`.** This surface-area-ranked menu of Rust-desugar → Fe-provider targets feeds the slide's deletion ladder + the burn-down board; the live tracking of which targets shipped is the board. Kept as the dated prioritized menu.

**Date:** 2026-06-18 · **Status:** creative design study (read-only), load-bearing feasibility claims
spot-verified. Ranked by **surface area removed** (compiler special-paths deleted), not LOC.

## The proven pattern

"Compiler-internal desugaring → std provider, scheduled by the existing expansion machinery" is **shipped
and tested**: the Rust `AbiSize` generators were deleted and replaced by `StableAbiSize`
(`ingots/std/src/abi.fe:120-165`), scheduled by `schedule_error_abi_size`/`schedule_msg_variant_abi_size`
(`expansion.rs:384/484`), byte-identical. The hole through every stratum (name-res, `all_items`, ty_check,
MIR see generated impls as ordinary items) is already drilled. New leverage: `ConstraintTerm` (honest
`Evidence<Eq<T>>`) + the frozen, inventoried provider surface (`TD5_PROVIDER_COMMAND_SURFACE.md`).

## Menu

### #1 — `#[error]` → `StableError` provider (HIGHEST LEVERAGE; mostly doable now)
Deletes `error.rs` `create_selector_const` (:262) + `lower_error_encode_impl` (:369) + `lower_error_struct`
(~400 of 464 LOC) — the SELECTOR compile-time keccak, the `Encode<Sol>` impl, the hand-built
`encode_to_ptr`. Most **security-critical** desugaring (wrong selector = silent ABI mismatch),
per-contract-frequent, and **half-done already** (AbiSize carved out in #5b). **Verified low-new-capability:**
the `keccak`/`concat`/`str`/`emit_const`/`trait_const` ops are all in the frozen surface, and the
`keccak((Name "(" SOL_TYPE,... ")"))` fold is *already shipped* in `StableEip712` (`eip712.fe:260-280`).
Two honest gaps: a small `selector(keccak(..))` top-4-bytes helper (or express via `quote`), and the
`encode_to_ptr` running-pointer fold (the fiddliest — may want a `payload`-style method override like
`StableAbiSize` already uses). **Doable now for SELECTOR + the impl skeleton; encode body is the one
fiddly piece.**

### #2 — `#[event]` → `StableEvent` provider
Deletes `event.rs` (~557 LOC): `create_topic0_const` (TOPIC0 = the *same* keccak fold as #1) + the
indexed/data emit body. **Gate: one new reflection read** — `field.has_attr("indexed")` (a small
`FieldHandle` extension, TD5c pattern). Do right after #1 to reuse the keccak/SOL_TYPE machinery.

### #3 — `#[msg]` variants (`Encode`/`Decode`/`MsgVariant`) → providers
Deletes the bulk of `msg.rs` (733 LOC). Encode reuses #1. **Decode is harder** (positional ABI reads
need a `<ty as Trait>::method` quote form not present today — TD5 surprise #3) + a `#[selector=…]`
per-variant attribute read. Sequence last. NOTE: the `recv`/contract-dispatch half is **NOT** a
providerization target — genuine control-flow lowering; keep in Rust.

### #4 — `derive` canonical lookup → real `Derive` trait + executor deletion (#7)
The destination, biggest blast radius. Kills the string-marker (BR0) + string-keyed authority (BR2) +
the bespoke executor. **Gated on trait-constructor-as-value (IN FLIGHT — `FCO_TRAIT_CTOR_VALUE_2026-06-18.md`)
+ typed provider capabilities.** Not the next step; the endpoint.

### #5 — operators (`==`→`Eq::eq`, `+`→`Add::add`) → KEEP in Rust
Lowers via a 15-entry static table (`core_requirements.rs:58-75`) — declarative data, **not** hand-rolled
codegen. There's almost no special *path* to delete. Fails the surface-area test. **Leave it.** (Not
everything is a providerization target — the lens says so.)

## Highest-leverage move: #1

Most compiler magic deleted for the least new capability: the engine already did the AbiSize half of this
exact struct, the scheduling glue exists and is tested, and the keccak fold is already shipped in EIP-712.
Only genuinely new: a small `selector(keccak(...))` helper. The textbook continuation of a pattern proven
twice, on a struct the compiler *already partially providerizes*. **Independent of the trait-ctor/#7 work —
a parallel track.**

## Honest gates (what's NOT doable yet)
- #4/#7: trait-constructor-as-value (in flight) + typed-capabilities packet. Biggest blast radius.
- #2/#3: per-field/variant attribute reflection (`#[indexed]`, `#[selector]`) — a small `FieldHandle`/
  `VariantHandle` read-table extension (freeze-rule governed).
- #3 Decode: the `<ty as Trait>::method` quote form (TD5.4/5e).
- `require<concrete>` is silently dropped today (TD5 surprise #1) — generated bodies must discharge
  concrete-field obligations at the use site until TD5.2 lands.

## The ambitious endgame
`#[error]`/`#[event]`/`#[msg]`/`derive` ALL become ordinary stdlib derives (`#[derive(ErrorVariant)]`
etc., Form 2 real traits + `impl Derive<…>` providers), run not by the 131KB `provider_executor.rs` but as
**ordinary compile-time effectful Fe** (TD5 endpoint). Then the only compiler magic left across the whole
ABI/event/error/derive surface is: attribute parsing, the keccak/log **intrinsics** (genuine kernel
primitives), and the obligation solver. Every *shape* decision lives in `ingots/std`. Sequence to get
there: ConstraintTerm ✅ → trait-ctor-as-value (in flight) → typed capabilities → TD5 quote/builder→typed
effects → small reflection-attr + `<_ as _>` quote extensions. Converts ~1754 LOC of hand-rolled Rust
lowering + a 131KB executor into readable std Fe — the largest surface-area reduction in the compiler.

Targets: `crates/hir/src/core/lower/{error.rs,event.rs,msg.rs}`; glue `expansion.rs:384-540`; Fe templates
`ingots/std/src/{eip712.fe:251-351,abi.fe:120-165}`; capability ground truth `TD5_PROVIDER_COMMAND_SURFACE.md`.
