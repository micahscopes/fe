# Concrete-only `ConstraintTerm` — Measured Cost/Benefit + Decision Packet

**Date:** 2026-06-18 · **Status:** costing complete (2 read-only agents, load-bearing claims
spot-verified by the integrator). **DECISION: GO (Micah, 2026-06-18)** — pause lifted; this reverses the
2026-06-15 D7/wiring-party "no `ConstraintTerm` / projection" verdict. Supersedes the assoc-type/param
workaround plan (carrier dissolves; both derive forms become ordinary applications). Build in flight:
R1 (inert variant, agent `a52877336cf08569c`).

## What's being costed

A **concrete-only `TyData::ConstraintTerm`**: a `Constraint`-kinded type-term that wraps a resolved
`TraitInstId` (the `Eq<T>` in `Evidence<Eq<T>>`), used as an **inert leaf** — head ALWAYS a resolved
trait (no variable/abstract head), never produced/reduced by type functions. This makes a constraint a
first-class *term* (not just a first-class *obligation*), so `Evidence<Eq<T>>` becomes an ordinary
generic application and the bespoke "carrier" dissolves.

## The verdict this revisits

2026-06-15 (D7/wiring-party, architect now gone) killed `ConstraintTerm` on two grounds:
(a) **"largest blast radius in the compiler"** and (b) **"no demand."** Both are now contestable:
(a) is **refuted by measurement** (below); (b) Micah's "first-class constraints for real / zero-shim /
support both derive forms" **is** the demand. Soundness was never the objection — and isn't one.

## COST (measured on this tree; spot-verified ✓)

`TyData` has **9 variants** (`ty_def.rs:1059`); `ConstraintTerm` would be the **first ever of kind
`Constraint`** (today even `QualifiedTy` is `Kind::Star`).

- **16 compile-forced match arms**: **3 trivial** + **13 real**, of which **~9 are 1–3 line copies of
  the existing `QualifiedTy` arm** (fold, walk, pretty-print, `as_scope`, `name_span`, `ty_depth`,
  visibility, groundness, canonicalize).
- **2 genuinely substantive arms**: unification (`unify.rs:129` — equal iff inner `TraitInstId`s unify,
  mirrors the `AssocTy` arm; the one load-bearing semantic arm) and stable-identity
  (`stable_key.rs:468` — deterministic encoding, helper already exists).
- **2 structural recursors**: fold (`fold.rs:57`), walk (`visitor.rs:78`) — free-var/flags collectors
  ride these automatically (no own arms).
- **Pre-built, $0**: `Kind::Constraint` already exists (`ty_def.rs:1278`); `does_match`/`Display` already
  handle it; `applicable_ty` already returns `None` for `Kind::Constraint` (`ty_def.rs:904` — a
  constraint leaf correctly takes no args). Interning is salsa-automatic. **No** new `InvalidCause`, free-var,
  or flags code.
- **MIR/codegen/layout/ABI**: entirely trivial — a constraint never reaches runtime, so every runtime
  match already absorbs it via `_`/`else`.
- **Re-port baseline**: effort2 built exactly this (`TyData::ConstraintTerm`, `ty_def.rs:1161`, comment
  *"the `Eq<T>` in `Evidence<Eq<T>>`"*). The CONCRETE typing-only path was functional; **~80–110 LOC
  liftable**; leave behind the **~250 LOC abstract-head scaffold** (`ConstraintHeadKind::GenericParam`,
  `ConstraintApplicationId`, the interned-head `==` unify, the `GenericParam` lowering).

**Net new engineering ≈ 2 substantive arms + 2 recursors + ~9 one-liners (~80–110 LOC).** The "every
`match TyData` breaks" scare is false for an inert concrete leaf.

## BENEFIT (deletion ledger; spot-verified ✓)

| Target | file:line | what happens |
|---|---|---|
| Provider-goal **carrier** | `provider_goal.rs` (**509 LOC**, confirmed) | **~250–300 code LOC deletable** — the position-scoped intercept exists only because `Eq<T>` can't travel the `*`-kinded walk; with `ConstraintTerm` it's an ordinary type application. ~50 LOC (capability-identity recognition, BR2) relocates into normal lowering. |
| Carrier invocation | `mod.rs:736-744` | ~19 LOC deletable (the `is_derive_provider_fn` goal-extract block). |
| `lower_hir_constraint_application` | `trait_lower.rs:337` (~100 LOC) | **Stays** — shared by `where Eq<T>` and the leaf; `ConstraintTerm` *calls* it to fill its leaf. |
| `2-0006` rejection | `ty_lower.rs:141` | Stays; **gains a peer branch**: expected-kind `Constraint` + trait head → lower to `ConstraintTerm` instead of erroring. The `*`-position rejection stays correct. |
| `Evidence`/`ImplBuilder` | `ingots/core/src/derive.fe` | Become **honest** ordinary `Constraint -> *` generics — the "recognized by the carrier, not the ordinary walk" comments retire. No `PrimTy` change (current branch already has them as core structs). |
| `Derive` string-marker | `provider.rs:31`, parser `item.rs:895` | **NOT** retired by this — that's BR0/#7, **TD5-gated**, separate. |

**Net ≈ −170 to −210 LOC**, replacing a bespoke position-scoped intercept with one uniform
`Constraint`-kinded type application. **Bridges dissolved:** BR0 (largely — `Eq<T>` becomes a real term,
not a token), BR1 (`Evidence`/`ImplBuilder` honest), BR7 partial (`Kind::Constraint` gets an inhabitant).
Untouched: BR2/BR3 (provider body/authority — TD5-gated).

## SOUNDNESS (keystone, verified ✓)

The solver entry `is_goal_satisfiable(..., goal: TraitInstId)` (`trait_resolution/mod.rs:269`) takes a
`TraitInstId` — **a `ConstraintTerm` can never *be* a goal.** It is a typing-only representation, inert
in the solver (exactly as effort2's was — `trait_predicates` filters to `Trait` only; non-trait kinds
fall to `residual_constraints`, never discharged). Therefore it **cannot touch the solve-line invariant
or the Lean-proven assumptions** (coherence-as-function, fixed-Γ, full concreteness, first-order). The
only thing that *would* breach them is the **abstract head** (a variable head reaching the solver) —
which a concrete-only leaf **excludes by construction** (wrap a `TraitInstId` directly; never carry a
`GenericParam` head). Sound, scoped to the inert-leaf use (no type-fn production).

## Stale docs corrected (found during costing)

- `FCO_CONSOLIDATION_MAP.md:901-904` — "fco reduced `Kind` to `Star|Abs|Any`, deleted `Constraint`":
  **STALE.** Live `Kind = Star | Constraint | Abs | Any` (`ty_def.rs:1278`). Only `Placeholder` was dropped.
- `FCO_CONSOLIDATION_MAP.md:892-898` — implies `Evidence/ImplBuilder/Reflect/Derive` are still deleted
  `PrimTy` builtins: **STALE.** They're ordinary `core::derive` structs now, already `Constraint`-kinded.
- The 2026-06-15 verdict's "largest blast radius": **not supported** by the measured 16-arm/~100-LOC figure.

## RECOMMENDATION — GO (Micah's call)

The two pillars the verdict stood on are gone: the cost is **small and net-LOC-negative**, and the
demand is **real** (your own "first-class for real / zero-shim / support both forms"). It's **sound**
(inert leaf, can't be a goal), has a **re-port baseline** (effort2), **dissolves the carrier + BR0/BR1**,
and makes `Evidence`/`ImplBuilder` honest generics. It is the genuine "first-class *constraints*" the
project name promises (we already have first-class *obligations*).

**Caveats / gates:** (1) it's a representation-schema commitment (reverses an architect-ratified verdict)
— Micah's decision, not autonomous. (2) Scope strictly to the **concrete inert leaf**; the abstract-head
scaffold stays shelved (excluded by construction). (3) Sequence after / alongside the in-flight Stage-1
work; the `Derive` marker (#7) is still TD5-gated and independent. (4) Both derive surface forms
(`type Goal = Eq` and `Derive<Eq>`) become ordinary applications once this lands — "support both" falls out.

Suggested build order if GO: variant + constructor + `Kind::Constraint` arm → fold/walk/print one-liners
→ unify + stable-identity (the 2 real arms) → `ty_lower` arg-position lowering (expected-kind `Constraint`
→ `ConstraintTerm`) → delete the carrier → migrate `Evidence<Eq<T>>` to ordinary lowering → full gate
byte-identical. Each a checkpoint; STOP-on-wall.
