# TD5 — Provider body: bespoke executor → ordinary effectful CTFE (DESIGN PACKET)

> **CORRECTED 2026-06-19 → `FCO_THE_SLIDE_2026-06-19.md` CORRECTION 2.** The endgame is NOT "executor → ordinary effectful CTFE" / engine fusion: the executor is a *quasiquoter / backend*, not a value evaluator (measured). The expansion↔type-check stratum collapses by STAGING (the x-0…x-4 ladder), not fusion. The command-surface inventory, the TD5.0/.2 ladder, and the NO-SHIM-FOR-SHIM ratchet rule here remain BINDING.

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

## NO SHIM-FOR-SHIM (the ratchet rule) — architect 2026-06-17
**We do not replace an opaque shim with another shim.** A `ProviderEffect` layer is acceptable ONLY as
a temporary transition that *immediately deletes/narrows* executor-owned behavior. Hard rule:

> Do NOT land a `ProviderEffect` layer unless the same commit — or the immediately following commit —
> migrates at least one executor operation OUT of bespoke handling. A trace-only `ProviderEffect`
> commit is allowed ONLY if it is (1) explicitly diagnostic/temporary, (2) has a deletion target,
> (3) is followed by a concrete opcode migration (preferably `builder.require`), and (4) the board
> says **"observability only," NOT "bridge removed."**

**TD5 progress is NOT "we wrapped the executor in a nicer enum." TD5 progress is "one fewer thing
requires the executor."** Every rung below is named by what it DELETES. Bad: `ProviderEffect` exists but
the executor still owns everything → that's laundering the bridge; reject it.

## The ladder — DELETION milestones (each rung removes/narrows executor ownership)
- **TD5a — inventory + freeze.** Output: the §command-surface table with exact `file:line` + future
  effect + first fixture + deletion target per op; freeze rule (no new opcode without a category).
  Deletion claim: no NEW hidden opcodes can be added.
- **TD5b — `require` no longer executor-owned (the first REAL migration).** `builder.require<Trait>(ty)`
  emits an ordinary provider-origin FCO obligation (goal=`Trait<ty>`, subject, provider id, derive site,
  reflected-field origin, evidence/provenance linked) instead of being a bespoke executor opcode that
  *silently drops concrete requirements* (surprise #1). **Deletion: the executor no longer owns
  generated trait requirements.** The `ProviderEffect` trace (observability) is allowed here ONLY paired
  with this migration — not as a standalone "TD5 progress" commit. (Depends on the W6 const-ref ICE fix.)
  Acceptance: StableEq still works; a missing field bound fails through NORMAL obligation diagnostics;
  provenance can say "StableEq required Eq<FieldTy> because field `x` was reflected from Point"; with the
  old executor-side `require` handling disabled, the new path still carries the obligation.
- **TD5c — reflection no longer string/executor-owned.** Field/variant handles get a typed CTFE
  representation. Deletion: `reflect.*` reads stop being executor-intercepted strings.
- **TD5d — quote no longer template-executor-owned.** `quote` elaborates to typed generated HIR with
  hygiene + typed holes. Deletion: quote stops being a template interpreted by the executor.
- **TD5e — builder emit no longer executor-owned.** Generated-item construction (`emit_*`/`method`/…) is
  a typed build effect scoped to the goal G. Deletion: generated-HIR construction leaves the executor.
- **TD5f — one provider body runs OUTSIDE the bespoke executor.** Deletion: the executor path is unused
  for one provider (the marker/Default pilot).
- **TD5g — all canonical providers off the executor.** Deletion: the executor is removed/quarantined.
- **#7 — retire the `Derive` marker** only once the executor is boring (has no job).

Porting order for TD5f/g: marker → Default → Clone → Eq → Ord → AbiSize (each adds one effect family:
Default=struct-init, Clone=reflect+call, Eq=reflect+require+quote-conjunction, Ord=match/branch,
AbiSize=assoc-const folds).

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

## Highest-leverage first DELETION
**TD5b — migrate `builder.require` out of executor ownership.** It is already obligation-shaped, so it's
the cleanest opcode to re-home into the real FCO obligation/evidence machinery — and doing so DELETES
the executor's silent-drop of concrete requirements (surprise #1). The `ProviderEffect` trace is the
*scaffolding* that makes this safe (a strangler-fig contract), but it lands **paired with** the require
migration, not as a standalone "progress" commit. The measure of done is "require no longer needs the
executor," not "a trace exists."

## Smallest first PR (TD5a + TD5b, then STOP)
1. **TD5a** — complete this packet's command table (exact `file:line` per op) + add the freeze rule to
   the repo. Deletion claim: no new hidden opcodes.
2. **W6 prerequisite** — fix the const-ref ICE (`body.rs:730`, task #42) so a missing concrete `require`
   diagnoses `6-0003` instead of panicking. (TD5b walks straight into it — surprise #1.)
3. **TD5b** — `builder.require<Trait>(ty)` emits an ordinary provider-origin FCO obligation; the
   executor's bespoke `require` handling is **removed** (not wrapped). Paired observability: a
   `ProviderEffect`/trace seam, labeled "observability only." Fixtures: missing concrete member →
   `6-0003`; provenance says "provider required Eq<FieldTy> because field x"; **with executor-side
   `require` disabled, the obligation still flows.** STOP after this rung; reassess.

A `ProviderEffect` enum that merely re-wraps identical behavior, a trace-only commit with no migration,
or new docs that don't delete/narrow executor ownership **do NOT count as TD5 progress.**

## Sequencing
Architect-confirmed: **#6 → #5b → (W6 ICE fix) → TD5.0/.1/.2 → … → #7.** TD5 is not started until #6/#5b
are done. This packet is a *living* plan — #5b (retiring a real Rust AbiSize generator) may refine how
provider bodies should emit, so revise TD5.2/TD5.5 after #5b lands.
