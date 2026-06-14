# H10 execution map — ABI/static-layout providerization (the #1 reification win)

**Scoping, 2026-06-14. Reification target, not a bridge.** Per the bridge +
reification inventory (`FCO_BRIDGE_AND_REIFICATION_TARGETS.md`, top reification
win #1), the hardcoded Rust ABI/event/error/msg lowering is a
`PROVIDERIZATION_TARGET`: the compiler builds these impls in Rust because Fe
couldn't express them yet; now the typed-provider bridge can. This map scopes the
migration. **Gated on the typed-provider-capabilities decision**
(`FCO_DECISION_PACKET_typed_provider_capabilities.md`): H10 emits generated impls
and consumes capabilities, so it wants the typed recognition + the BR3
generated-item guarantee first. It is *not* gated on K03/K04 kinds (it rides the
existing builder bridge, like StableEq/StableOrd).

## Current mechanism (verified `file:line`) — the parallel Rust provision path

These build HIR impls directly, bypassing `DeferredTask`, producing **no
evidence**, and are not extensible from Fe:

| family | entry | what it synthesizes |
|---|---|---|
| error | `core/lower/error.rs:171` `lower_error_abi_size_impl` (`:365`), `:174` `lower_error_encode_impl` (`:393`) | `AbiSize` (HEAD_SIZE/IS_DYNAMIC) + `Encode` impls for `#[error]` structs |
| msg | `core/lower/msg.rs:132` `lower_msg_variant_abi_size_impl`, `:207` `lower_msg_variant_encode_impl`; `:190` builds `HEAD_SIZE` as a u256 sum, `IS_DYNAMIC` bool-OR | per-variant `AbiSize`/`Encode`/`Decode` |
| event | `core/lower/event.rs:296` `create_topic0_const` (`TOPIC0`), `:400` `lower_emit_method` (`encode_abi_payload`/`encode_event_payload`) | `TOPIC0` const + `emit` method |
| abi consts | `crates/fe/src/abi.rs:~1452` | ABI assoc consts |

The layout facts `HEAD_SIZE` / `IS_DYNAMIC` are **exactly what const predicates can
verify** — that is the FCO connection. Today they are computed and trusted; under
H10 a provider emits them and a const-predicate obligation checks them.

## End state

A Fe-authored `AbiLayout` / `Encode` / `Decode` derive provider (in
`ingots/core_derives` or `ingots/std`) that:
1. Uses `Reflect` to walk fields, `ImplBuilder` to emit the `AbiSize`/`Encode`/`Decode`
   impls (exactly as `StableEq`/`StableOrd` do for their traits).
2. Computes `HEAD_SIZE` / `IS_DYNAMIC` as generated assoc consts.
3. Attaches a **const-predicate obligation** verifying the layout invariant
   (e.g. `HEAD_SIZE == sum(field HEAD_SIZEs)`), so the fact carries evidence
   (`ty_check/mod.rs:1447` discharge path; receipts at `:3178-3267`).
4. The existing `EIP-712` provider (`ingots/std/src/eip712.fe`,
   `derived_eip712.fe`) is the proof the Fe-provider path handles real ABI-shaped
   generation — H10 is "do for ABI/event/error what EIP-712 already does."

## Phasing (smallest, most contract-relevant first)

1. **H10a — `AbiSize`/static-layout provider.** Smallest, highest value: derive
   `HEAD_SIZE`/`IS_DYNAMIC` for structs via a provider, verified by a const
   predicate. No encoding logic yet. Acceptance: a struct's derived `AbiSize`
   matches the current Rust-lowered values on existing ABI fixtures; the layout
   const predicate discharges with a receipt.
2. **H10b — `Encode` provider.** Head/tail encoding for static then dynamic
   fields. Acceptance: differential against the current `lower_error_encode_impl`
   output (the `differential_*` fixtures are the oracle).
3. **H20 — event/error/msg families** on top of H10a/b: `TOPIC0`, `emit`, per-variant
   msg impls. Largest; do after ABI core proves out.

## Why gated on typed capabilities (not just convenient)

H10 providers emit impls and (ideally) attach provenance + layout evidence. Doing
that on the *string-keyed* bridge means the layout facts have no typed capability
trail and the generated impls hit the BR3 validation hole. So the clean sequence
is: typed-provider-capabilities decision → K04a (typed recognition) + BR3 guarantee
→ H10a. If the architect wants H10a *sooner*, it can ride the existing string
bridge as pure std-lib polish (like StableOrd) and be re-pointed at typed
capabilities later — but then its layout facts are trusted, not evidence-backed,
which forfeits half the point.

## Graph nodes
H10/H20 (this map), RT0/RT1 (ABI/static-layout + event/error/msg facts), BR6
(graduate the hardcoded lowering here), S20 (premises/evidence for layout facts),
P50 (generated-impl provenance). Depends on the typed-capabilities decision (P00/P10
= K07/BR2) and the BR3 guarantee.
