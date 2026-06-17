# TD5 — Provider body: bespoke executor → ordinary effectful CTFE (DESIGN PACKET)

**Status:** PLAN ONLY — do NOT implement TD5 yet. TD5 is the real gate for retiring the `Derive`
marker (#7); it stays OFF the critical path until after #6 / #5b. This packet makes the first sprint
(TD5.0/.1/.2) executable on demand.

**Provenance note:** the original full packet (a 56-row per-command table with exact `file:line` for
each opcode) was produced by a planning agent but **lost to git-worktree auto-cleanup** (uncommitted in
a worktree that was removed when the agent returned — see memory `fe-worktree-builds-share-target-lock`).
This reconstruction preserves the agent's executive summary + findings verbatim; the exhaustive
per-command table is re-derived as **TD5.0 itself** (its first deliverable), so nothing load-bearing is
lost — only re-walked.

## The goal
Provider bodies (`builder.method`/`quote`/`require`/`emit_*`/`finish`, `reflect.*`) run in a bespoke
command-language executor (`crates/hir/src/core/lower/provider_executor.rs`), not ordinary Fe. While
that's true, `impl X: Derive for Tr` still *means* "run this body in the special executor", so the
`Derive` marker (#7) cannot retire. TD5 turns the executor into a **typed effect handler**, then
retires one effect family at a time until provider bodies are ordinary effectful compile-time Fe.

## Command surface (TD5.0 will produce the exact table; here are the categories + counts)
**~56 distinct executor-recognized operations**, in 6 categories:
- **R — reflection reads (~12):** `reflect.fields()`/`is_struct()`/`is_enum()`/`variants()`,
  `field.ty()`/`field.name()`, variant/field iteration. → become a typed **read-only** CTFE capability.
- **B-obl — `builder.require<Trait>(ty)` (1):** the obligation op. → ordinary obligation emission (TD5.2).
- **B-build — generated-HIR builder ops (~33, the LARGEST cluster, the shrink target):**
  `method`/`with_self`/`with_arg`/`returns`/`emit_method`/`emit_const`/`emit_assoc_ty`/`struct_init`/
  `with_field`/`variant_init`/`add`/`or`/`bool`/`ty<…>`/`target_ty`/`self_ty`/`trait_call`/`trait_const`/
  `static_call`/`trait_assoc_ty`/… → typed generated-HIR **builder effect** (TD5.5).
- **Q — quote (~7):** `quote { … }`, `quote(open) { … }`, holes `${…}`, `self.${field}`, `${variant}(grp)`
  arm/pattern holes. → typed generated-HIR with hygiene + typed holes (TD5.4).
- **CTFE — control flow:** `let`/`mut`/`for`/`if`/`else`/`&&`/`return`. → **already ordinary Fe
  semantically** (see surprise #2); little to do beyond removing the interception.
- **FIN — `builder.finish()` (1):** completion/validation. → effect-handler finalize.

**Freeze rule:** no new provider-body operation may be added without (a) a TD5 category and (b) — for
quote forms — an entry in the quote-fragment spec. This keeps the surface from growing while it shrinks.

## The ladder (each rung = a burn-down row with a fixture + a deletion/narrowing claim)
- **TD5.0** inventory + freeze (the table above, with exact `file:line` + future-effect + first-fixture
  + deletion-target per op). Doc + freeze rule.
- **TD5.1** internal `ProviderEffect` IR + a dumpable, **asserted** trace. No behavior change.
  Enum ≈ `ReflectFields | Require{goal,subject} | Quote{…} | BuilderMethod{…} | EmitConst{…} | … | Finish`.
  Test: a provider's trace contains the expected `Require` + `EmitMethod/EmitConst`.
- **TD5.2** re-home `builder.require<Trait>` as **ordinary obligation emission** (first real demagic):
  emit a typed obligation carrying goal + subject + provider id + derive site + field origin into the
  FCO obligation/evidence queue (the design-wizard G2 "reserve `premises`" slot). Missing-member failure
  then routes through normal obligation diagnostics; provenance gains the require chain.
- **TD5.3** reflection → typed read-only CTFE capability (body-level, handles not strings).
- **TD5.4** quote → typed generated-HIR with hygiene + typed holes (NOT template strings).
- **TD5.5** `ImplBuilder<G>` → typed generated-HIR builder effect **scoped to the goal G**.
- **TD5.6** one tiny provider body runs through the ordinary CTFE/effect path (others stay on executor).
- **TD5.7** port one real provider; order: marker → Default → Clone → Eq → Ord → AbiSize (each adds one
  effect family: Default=struct-init, Clone=reflect+call, Eq=reflect+require+quote-conjunction,
  Ord=match/branch, AbiSize=assoc-const folds).
- **TD6 / #7** retire the `Derive` marker only once the executor is boring.

## Top wrenches + their guards
- **W6 — the const-ref ICE is a TD5.2 PREREQUISITE, not a surprise.** TD5.2 (concrete `require` as a
  real obligation) and the AbiSize port both walk straight into the tracked panic at
  `crates/hir/src/analysis/semantic/lower/body.rs:730` (a derived const initializer referencing a
  missing impl's assoc const ICEs instead of `6-0003`). **Fix the ICE before/with TD5.2** (task #42).
- **W3 — builder authority scoping.** `ImplBuilder<Eq<T>>` must be typed so it cannot emit an `AbiSize`
  member. Today this is implicit (one provider ↔ one `trait_ref`); TD5.5 must make it a typed property.
  Guard: a fixture where a builder for goal `Eq` tries to emit an unrelated member → rejected.
- **W4 — the solve-line.** Body-level `require` must eliminate to a concrete `TraitInstId` via the same
  `lower_hir_constraint_application` the signature goal uses — **no live `P` to the solver, ever.**
- (Plus: phase separation — no runtime `Reflect`/`ImplBuilder`/`Evidence` values; provenance must
  survive the migration; quote hygiene — no capture-by-spelling.)

## Surprises (preserved from the planner — these change the plan)
1. **`builder.require` is NOT a real obligation today** — it's a *where-clause synthesizer* that
   **silently drops fully-concrete requirements** (`requirement_where_clause`,
   `provider_synthesis.rs:134`/`:156`: only emits a `WherePredicate` when the required type mentions a
   generic param; a concrete `require` type produces nothing, relying on use-site discharge). That
   silent drop is exactly what TD5.2 removes — and is why TD5.2 collides with the const-ref ICE (W6).
2. **The control-flow interpreter is already ordinary Fe** (`let`/`for`/`if`/`&&`/`return`). The
   genuinely magic narrowness: the `for`-loop's `eval_iterable` **intercepts** `reflect.fields()`
   (it pattern-matches the receiver instead of evaluating the call), plus the whole `Value::Builder`
   method-dispatch table. So TD5 is narrower than "rewrite the interpreter."
3. **Goal-qualified builder ops can't be quoted hygienically.** `trait_call`/`trait_const`/`static_call`/
   `trait_assoc_ty` synthesize `<ty as Goal>::item` paths; the quote surface has **no `<_ as _>` form**,
   so unlike comparison/init primitives these **cannot** be absorbed by TD5.4 quotes — they must remain
   typed builder-effect ops in TD5.5.

## Highest-leverage first step
**TD5.1:** insert one `record(ProviderEffect)` journaling seam into `eval_builder_method` /
`eval_method_call` / quote elaboration, with an asserted, dumpable trace. Zero behavior change; gives
every later rung a stable strangler-fig contract to narrow against; is the prerequisite for the TD5.2
deletion.

## Smallest first PR (the TD5.0/.1/.2 sprint, then STOP)
1. **TD5.0** — this packet's table completed with exact `file:line` per op + freeze rule (add to repo).
2. **TD5.1** — `ProviderEffect` enum + `record()` seam + one trace-assertion test (`StableEq` trace =
   ReflectFields, Require(Eq<FieldTy>), Quote, EmitMethod, Finish). No behavior change.
3. **TD5.2** — (after the W6 const-ref ICE fix) `require` emits a typed obligation; fixtures: missing
   concrete member now `6-0003` (not ICE, not silent), provenance shows "provider required Eq<FieldTy>
   because field x". STOP after this rung; reassess.

## Sequencing
Architect-confirmed: **#6 → #5b → (W6 ICE fix) → TD5.0/.1/.2 → … → #7.** TD5 is not started until #6/#5b
are done. This packet is a *living* plan — #5b (retiring a real Rust AbiSize generator) may refine how
provider bodies should emit, so revise TD5.2/TD5.5 after #5b lands.
