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
| 1 | Provider selection by **resolved goal identity** | delete canonical-path/string matching as final semantics → resolve goal+head to `Trait` def, compare defs | REMOVE_BRIDGE | **IN PROGRESS** — behavior landed (W-C, `8c6cf81d0`), but impl still canonical-path + last-segment string fallback; code self-flags removal target. Hardening dispatched. |
| 2 | Provider signatures structurally check **concrete goals** | retire signature exemption for `Evidence<Eq<T>>` / `ImplBuilder<Eq<T>>` via narrow `CapabilityGoal` (no ConstraintTerm, no live head) | NARROW_BRIDGE | **READY / GO** — spike VIABLE, plan `FCO_LEVEL1_BUILD_PLAN_provider_goal.md`, carrier invariant locked (`bd10ec966`). |
| 3 | **String-key capability fallback** (`REFLECT_KEY`/`IMPL_BUILDER_KEY`) | no production authority by string; remaining = labeled compat shim w/ removal target | REMOVE_BRIDGE | PENDING — quarantined by K04a-C3 but the `match key_head.as_str()` fallback still present in `provider.rs`. |
| 4 | **Generated-impl provenance** | derive site → provider id → generated impl id → obligation/evidence | REMOVE_BRIDGE + GUARD_NO_EXPANSION | PENDING — origin chain (derive site → impl) exists; provider-id link missing (provider is transient at synthesis). Map done. |
| 5 | **ABI/static-layout providerization** slice | one old Rust-resident generator (e.g. `AbiShape<T>`: IS_DYNAMIC/HEAD_WORDS) produced by a Fe provider | REIFY_COMPILER_MAGIC | PENDING — after P50 (so generated ABI impls aren't another opaque bridge). |
| 6 | **Constraint aliases used** in real ABI/core fixture | obligation bundles (`constraint StaticAbi<T> = AbiShape<T> + AbiSize<T>`) expressed in Fe | EXPRESSIVE_FIXTURE + REIFY | PENDING — safe alias form landed (`6183f29c1`); now USE it in #5. |
| 7 | **Derive marker** graduation | after #1+#2+#4 are boring, retire `DERIVE_MARKER` special node | REMOVE_BRIDGE | DEFERRED (TD6) — gated on 1/2/4. |
| 8 | **Generic<T>** investigation | only if shared derive bodies become a real consumer | — | SHELVED (AH3 trigger). |

## Do NOT
Full `TyData::ConstraintTerm`; live `P<T>` solving; bridge-taxonomy docs without a
deletion/narrowing; provider-signature exemption as invisible "temporary" state;
canonicalize fixture-local providers into std before identity/provenance are solid;
broad EIP-712 before ABI/static-layout.

## Execution order
1 → 2 → 3 → 4 → 5 → 6. (1–3 touch overlapping `provider.rs`/`derive.rs`, so build
sequentially via worktree-isolated agents, each verified + integrated before the next.)
