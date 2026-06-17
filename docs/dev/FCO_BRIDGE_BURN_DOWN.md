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
| 2 | Provider signatures structurally check **concrete goals** | retire signature exemption for `Evidence<Eq<T>>` / `ImplBuilder<Eq<T>>` via narrow `CapabilityGoal` (no ConstraintTerm, no live head) | NARROW_BRIDGE | **DONE** (`b82fc43a6`) — `Evidence<G>` declared; analysis-layer `CapabilityGoal` + `provider_capability_goals` query de-exempts goal positions; `Evidence<Bogus<T>>`→`2-0002`, unsaturated→`6-0009`, live head→`6-0008`. 12 `derived` + carrier + 4 new fixtures green. **Audit fix (2026-06-17): the RETURN position `-> Evidence<…>` was still exempt (a hole the adversarial audit found); now checked too — `provider_signature_return_goal.fe` proves it.** Body executor still exempt (TD5). |
| 3 | **String-key capability fallback** (`REFLECT_KEY`/`IMPL_BUILDER_KEY`/`EVIDENCE_KEY`) | no production authority by string | REMOVE_BRIDGE | **DONE** — bare head-identifier authority DELETED in BOTH layers. `provider.rs` (expansion): `match key_head.as_str()` fallback removed; capabilities recognized only by canonical `core::derive::*` identity (`path_names_derive_capability`, base-graph-safe). `provider_goal.rs` (analysis): `Evidence`/`ImplBuilder` GOAL POSITIONS recognized by RESOLVED identity (`path_head_resolves_to_capability` → `core` ingot + `derive` module struct), not by `EVIDENCE_KEY`/`IMPL_BUILDER_KEY`. All inline-provider fixtures migrated to `use core::derive::{Evidence,ImplBuilder,Reflect}` (ingots already did). Verification grep is EMPTY (no string-key/`as_str()` authority). Anti-vacuous guards: `provider_capability_keys_on_identity_not_name` (unit, expansion — local-named `Reflect`/`ImplBuilder` denied authority) + `provider_goal_position_by_identity.fe` (cli, analysis — aliased `Evidence`/`ImplBuilder` STILL kind-check the goal by identity). Body executor still exempt (TD5). |
| 4 | **Generated-impl provenance** | derive site → provider id → generated impl id → obligation/evidence | REMOVE_BRIDGE + GUARD_NO_EXPANSION | **DONE** (`6d317d93b`) — `derived_impl_provenance(impl)` reconstructs derive-site→provider→impl→goal from the impl's `Desugared(Derive)` origin + trait_ref via resolved-identity selection. No stored state, no `ImplTrait` schema change. Proof: generated impl→provider by identity, hand-written→`None`, survives re-entry. KNOWN LIMITATION (documented): a `using`-override of a multi-provider goal returns `None` (needs `using`-name-from-AST or recorded provider id — additive follow-up). |
| 5 | **ABI/static-layout providerization** slice (reify-path proof) | a REAL named Fe provider produces a real EVM ABI fact; Rust generators NOT yet removed | REIFY_COMPILER_MAGIC | **DONE** (`c0eaec79b`) — named `StableAbiSize` derives real `core::abi::AbiSize` (HEAD_SIZE sum / IS_DYNAMIC or); facts usable in generic bounds + nested gen-consumes-gen; provenance→`StableAbiSize`; missing-field-bound→`6-0003`. NAMED+fixture-local (multi-backend: not canonical). Test-only/additive. FOLLOW-UP: removing the Rust `lower_{error,msg}_abi_size_impl` generators (the actual magic deletion) is the next, larger slice. |
| 6 | **Constraint aliases used** in real ABI/core fixture | obligation bundles expressed in Fe | EXPRESSIVE_FIXTURE + REIFY | **DONE** (`e3e1838e9`) — `trait StaticAbi: AbiSize + AbiShape {}` + blanket impl (proven #37 pattern); `where T: StaticAbi` expands to both concrete member obligations before the solver (no live P / ConstraintTerm / solver rule). `static_abi_alias.fe`: reads both members' assoc-consts through the one bound (Word 64/2, Triple 96/3, cross-member consistency). Negative: `OnlyAbiSize` → `6-0003` naming `OnlyAbiSize: AbiShape`; `Both` control clean. Fixture-local (EVM-specific, not canonical). |
| TD5 | **Provider body via ordinary CTFE** | de-magic the bespoke executor — `builder.*`/`quote`/`require` become ordinary effectful Fe (`reflect`/`quote` as real effects), not a restricted command language | REIFY_COMPILER_MAGIC | PENDING — the real gate for #7. This is the body exemption noted on #2/#3. Large slice; tracked as TD5 in the dependency graph's TD track. |
| 7 | **Derive marker** graduation | retire the `DERIVE_MARKER` special node AND the special `impl Name: Derive for Tr` parser head (`is_named_derive_provider_head`) + `#[default]` string-detect | REMOVE_BRIDGE | DEFERRED (TD6) — #1/#2/#3/#4 now done; the real remaining gate is **TD5** (while the body runs in the executor, `Derive` still means "special-executor provider", so the marker still has a job). |
| 8 | **Generic<T>** investigation | only if shared derive bodies become a real consumer | — | SHELVED (AH3 trigger). |

## Adversarial audit (2026-06-16) — read-only skeptic, mutation-tested
**All 5 removals (#1–#5a) SURVIVE as genuine features** — none is a relabel, moved-magic, or no-op; the #5 const-ref ICE was confirmed PRE-EXISTING (reproduced on pre-#5 machinery + git ancestry). Method: grep for moved authority, runtime probes, and **local single-line reverts** to catch vacuous tests. Two overstatements found, **both fixed (2026-06-17)**:
- **#2 return-position hole** — the de-exemption covered param + `uses` but not `-> Evidence<…>`; a nonsense return goal compiled silently. FIXED: `provider_capability_goals` now checks the return slot; `provider_signature_return_goal.fe` guards it.
- **#4 provenance test was partially vacuous** — the old hand-written control (`Marker`, no provider) returned `None` two ways, so it passed even with the origin gate deleted. FIXED: added a control that is a hand-written impl of the *provider-backed* `Taggable`; mutation-tested (gate neutralized → test now FAILS, restored).
Cadence: re-run a read-only adversarial audit after every ~2 DONE claims; a broken claim blocks the next feature.

## TD5 ladder (the gate for #7) — RECORDED, DEFERRED until after #6/#5b
Do NOT do TD5 as one rewrite. Turn the bespoke executor into typed effects, retire one at a time; each rung needs a fixture + a deletion/narrowing claim:
- **TD5.0** inventory + freeze the provider command language (`reflect.*`/`quote`/`builder.*`/`require`/`emit_method`/`finish`); freeze rule: no new provider op without a TD5 category.
- **TD5.1** internal `ProviderEffect` IR + dumpable trace (no behavior change; assert trace has `Require`+`EmitMethod`).
- **TD5.2** re-home `builder.require<Trait>` as ordinary obligation emission (first real demagic; ties provider bodies to the obligation queue).
- **TD5.3** reflection = typed read-only compile-time handles (body-level, not just signature).
- **TD5.4** `quote` → generated HIR with hygiene + typed holes (not template strings).
- **TD5.5** `ImplBuilder<G>` = typed generated-HIR builder effect.
- **TD5.6** one tiny provider body runs through ordinary CTFE/effect path (others still on executor).
- **TD5.7** port one real provider (order: marker → Default → Clone → Eq → Ord → AbiSize).
- **TD6/#7** retire the `Derive` marker only once the executor is boring.
First sprint when TD5 starts: TD5.0 doc + freeze, TD5.1 trace, TD5.2 require-as-obligation — then stop.

## Commit rule (burn-down invariant)
No commit may ADD bridge surface unless it also deletes a bridge path, narrows one, reifies compiler magic into Fe, or adds an adversarial guard against bridge expansion. Docs only if they support one of those.

## Pre-existing issues surfaced (not introduced; tracked, not hidden)
- **Concrete missing-`AbiSize` field panics** in the const-ref path (`crates/hir/src/analysis/semantic/lower/body.rs:730`, "const ref should resolve to a semantic instance") instead of emitting `6-0003`. Surfaced by #5; PRE-EXISTING (reproduces with the pre-#5 `derived_abi_shape` pattern; #5 is test-only so cannot have introduced it). A const-eval robustness bug: a derived const initializer referencing a missing impl's assoc const should diagnose, not ICE. Candidate follow-up; out of burn-down scope (touches const-eval, not derive machinery).

## Do NOT
Full `TyData::ConstraintTerm`; live `P<T>` solving; bridge-taxonomy docs without a
deletion/narrowing; provider-signature exemption as invisible "temporary" state;
canonicalize fixture-local providers into std before identity/provenance are solid;
broad EIP-712 before ABI/static-layout.

## Execution order
1 → 2 → 3 → 4 → 5a **DONE + audit-survived**. Now: **#6** (alias-in-ABI fixture) → **#5b**
(retire ONE Rust `lower_{error,msg}_abi_size_impl` generator, differential tests) →
const-ref ICE fix if it blocks deletion → **TD5 ladder** (TD5.0/.1/.2 first sprint) →
**#7** only after TD5. #6 is next because it's cheap+safe, NOT because it unblocks #7.
Keep #5 split: **#5a reify-proof DONE / #5b generator deletion OPEN.** Not "fully
bridge-free": selection/authority/goal-check/provenance de-magicked; provider body
(TD5) + `Derive` marker (#7) remain intentional bridges.
(1–3 touched overlapping `provider.rs`/`derive.rs`, built sequentially via worktree-
isolated agents, each verified + integrated before the next.)
