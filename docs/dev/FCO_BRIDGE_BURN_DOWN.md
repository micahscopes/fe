# FCO Bridge Burn-Down Board

Architect directive 2026-06-16: stop normalizing "temporary bridge = good enough."
Every iteration must **REMOVE_BRIDGE / NARROW_BRIDGE / REIFY_COMPILER_MAGIC /
GUARD_NO_EXPANSION / EXPRESSIVE_FIXTURE**. Docs only if they directly support
deleting/narrowing a bridge. This board is the tracking SSOT for the phase — keep it
short; do not grow a sprawling map.

Standing authorization (see memory `fco-autonomy-protocol`): GO without asking on
identity-migration / provenance / providerization / bridge-guard fixtures / solver-
stays-concrete. PAUSE only for full `ConstraintTerm`, live `P<T>` to the solver,
public syntax beyond approved surface, evidence-schema compat, bridge deletion
without a passing replacement, or scoped-provision/canonical-policy change.

## Board

| # | Bridge | Target | Class | Status |
|---|---|---|---|---|
| 1 | Provider selection by **resolved goal identity** | delete canonical-path/string matching as final semantics → resolve goal+head to `Trait` def, compare defs | REMOVE_BRIDGE | **DONE** (`bf8e23441`) — `goal_matches_provider` keys on `goal_def == head_def` via stratification-safe `resolve_trait_def` (base graphs only). 2 residual COMPAT SHIMs labeled w/ removal targets (bare-ident derive-prelude; named-provider-local-head) → feed #7. Anti-vacuous unit guards + behavior fixtures green. |
| 2 | Provider signatures structurally check **concrete goals** | retire signature exemption for `Evidence<Eq<T>>` / `ImplBuilder<Eq<T>>` via narrow `CapabilityGoal` (no ConstraintTerm, no live head) | NARROW_BRIDGE | **DONE** (`b82fc43a6`) — `Evidence<G>` declared; analysis-layer `CapabilityGoal` + `provider_capability_goals` query de-exempts goal positions; `Evidence<Bogus<T>>`→`2-0002`, unsaturated→`6-0009`, live head→`6-0008`. 12 `derived` + carrier + 4 new fixtures green. Body executor still exempt (TD5). |
| 3 | **String-key capability fallback** (`REFLECT_KEY`/`IMPL_BUILDER_KEY`/`EVIDENCE_KEY`) | no production authority by string | REMOVE_BRIDGE | **DONE** — bare head-identifier authority DELETED in BOTH layers. `provider.rs` (expansion): `match key_head.as_str()` fallback removed; capabilities recognized only by canonical `core::derive::*` identity (`path_names_derive_capability`, base-graph-safe). `provider_goal.rs` (analysis): `Evidence`/`ImplBuilder` GOAL POSITIONS recognized by RESOLVED identity (`path_head_resolves_to_capability` → `core` ingot + `derive` module struct), not by `EVIDENCE_KEY`/`IMPL_BUILDER_KEY`. All inline-provider fixtures migrated to `use core::derive::{Evidence,ImplBuilder,Reflect}` (ingots already did). Verification grep is EMPTY (no string-key/`as_str()` authority). Anti-vacuous guards: `provider_capability_keys_on_identity_not_name` (unit, expansion — local-named `Reflect`/`ImplBuilder` denied authority) + `provider_goal_position_by_identity.fe` (cli, analysis — aliased `Evidence`/`ImplBuilder` STILL kind-check the goal by identity). Body executor still exempt (TD5). |
| 4 | **Generated-impl provenance** | derive site → provider id → generated impl id → obligation/evidence | REMOVE_BRIDGE + GUARD_NO_EXPANSION | **DONE** (`6d317d93b`) — `derived_impl_provenance(impl)` reconstructs derive-site→provider→impl→goal from the impl's `Desugared(Derive)` origin + trait_ref via resolved-identity selection. No stored state, no `ImplTrait` schema change. Proof: generated impl→provider by identity, hand-written→`None`, survives re-entry. KNOWN LIMITATION (documented): a `using`-override of a multi-provider goal returns `None` (needs `using`-name-from-AST or recorded provider id — additive follow-up). |
| 5 | **ABI/static-layout providerization** slice (reify-path proof) | a REAL named Fe provider produces a real EVM ABI fact; Rust generators NOT yet removed | REIFY_COMPILER_MAGIC | **DONE** (`c0eaec79b`) — named `StableAbiSize` derives real `core::abi::AbiSize` (HEAD_SIZE sum / IS_DYNAMIC or); facts usable in generic bounds + nested gen-consumes-gen; provenance→`StableAbiSize`; missing-field-bound→`6-0003`. NAMED+fixture-local (multi-backend: not canonical). Test-only/additive. FOLLOW-UP: removing the Rust `lower_{error,msg}_abi_size_impl` generators (the actual magic deletion) is the next, larger slice. |
| 6 | **Constraint aliases used** in real ABI/core fixture | obligation bundles (`constraint StaticAbi<T> = AbiShape<T> + AbiSize<T>`) expressed in Fe | EXPRESSIVE_FIXTURE + REIFY | PENDING — safe alias form landed (`6183f29c1`); now USE it in #5. |
| 7 | **Derive marker** graduation | after #1+#2+#4 are boring, retire `DERIVE_MARKER` special node | REMOVE_BRIDGE | DEFERRED (TD6) — gated on 1/2/4. |
| 8 | **Generic<T>** investigation | only if shared derive bodies become a real consumer | — | SHELVED (AH3 trigger). |

## Pre-existing issues surfaced (not introduced; tracked, not hidden)
- **Concrete missing-`AbiSize` field panics** in the const-ref path (`crates/hir/src/analysis/semantic/lower/body.rs:730`, "const ref should resolve to a semantic instance") instead of emitting `6-0003`. Surfaced by #5; PRE-EXISTING (reproduces with the pre-#5 `derived_abi_shape` pattern; #5 is test-only so cannot have introduced it). A const-eval robustness bug: a derived const initializer referencing a missing impl's assoc const should diagnose, not ICE. Candidate follow-up; out of burn-down scope (touches const-eval, not derive machinery).

## Do NOT
Full `TyData::ConstraintTerm`; live `P<T>` solving; bridge-taxonomy docs without a
deletion/narrowing; provider-signature exemption as invisible "temporary" state;
canonicalize fixture-local providers into std before identity/provenance are solid;
broad EIP-712 before ABI/static-layout.

## Execution order
1 → 2 → 3 → 4 → 5 → 6. (1–3 touch overlapping `provider.rs`/`derive.rs`, so build
sequentially via worktree-isolated agents, each verified + integrated before the next.)
