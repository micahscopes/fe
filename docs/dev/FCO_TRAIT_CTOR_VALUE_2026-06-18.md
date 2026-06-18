# Trait-Constructor-as-Value — design + rung plan (Form 2 enablement)

**Date:** 2026-06-18 · **Status: R1–R3 LANDED + independently verified — Form 2 is GREEN.** R1 inert
variant `863db9b83`; R2+R3 produce+reduce `df2c37570` (only `derive_kind_form2_param.snap` changed,
`2-0006`→clean; cold gate fe-hir 132/0, cli_output 360/0). The unsaturated sibling of `ConstraintTerm`;
flipped the chosen **Form 2** derive-kind to clean = decision-doc Stage 2 (TD-arg + TD2/TD3).
**Implementation note:** the `2-0006` source was `path_resolver.rs` (trait-path generic-arg pre-validation),
NOT `ty_error.rs:261` as scoped — the impl agent instrumented to find the real emitter and guarded it there.
The R3 reduction is in `app_in_mode` (the single fold-routed site), reusing `complete_explicit_args` so
`Eq<T=Self>` saturates. This is **job-1 of #7** (makes `impl Derive<Eq> for StableEq` a real impl);
job-2 (executor→CTFE, TD5) + marker deletion remain.

## What & why

`ConstraintTerm(TraitInstId)` (landed R1–R3) represents a **saturated** constraint `Eq<T>` (kind
`Constraint`). Form 2 needs the **unsaturated** form: a bare trait constructor `Eq` (kind
`* -> Constraint`) as a first-class value, so `impl Derive_<Eq> for StableEqK` lowers (today `2-0006`
"found trait `Eq`"). Representation: a new inert leaf **`TyData::TraitCtor(Trait)`** that **reduces to a
`ConstraintTerm` on application**.

## Shim-free (verified)

Real type-system mechanism, no recognizer/executor/string-key:
- a real `TyData` variant with a real kind (`* -> Constraint`);
- **kind-directed lowering** (bare trait in a `* -> Constraint`-expected arg → `TraitCtor`; in a `*`
  position it still correctly `2-0006`s — discipline in the ordinary kind-checker);
- **one reduction site**: `TyApp(TraitCtor(Eq), T) → ConstraintTerm(Eq<T>)` lives in `TyId::app_in_mode`
  (`ty_def.rs:678`). Verified: `fold_ty_app` defaults to `TyId::app` (`fold.rs:46`) and `super_fold_with`
  routes every `TyApp` through it, so **all** substitution/normalize/lowering paths re-enter that one
  site. The reduction is the *uniform* constructor-application path, not a special case. Purely additive
  (no deletions claimed).

## Crux claims (spot-verified ✓)

- `fold_ty_app` → `TyId::app` (`fold.rs:46`); `super_fold_with` `TyApp` arm routes through it (`fold.rs:58-62`).
- `app_in_mode` is the single application site (`ty_def.rs:678`), gated by `applicable_ty`.
- `applicable_ty` already accepts a `* -> Constraint` leaf (one `*` arg) and returns `None` for a
  saturated `Constraint` (`ty_def.rs:916-927`) — so a `TraitCtor` takes exactly its subject, then stops.
- `Eq<T = Self>` (`ops.fe:119`) — defaulted param ⇒ explicit-subject arity 1; the reduction must use
  default-completion (`complete_explicit_args`, `trait_lower.rs:403`), NOT hard-code arity.
- The Form-2 `2-0006` is emitted by the diagnostic visitor `ty_error.rs:261` (`visit_path`), which does
  NOT thread expected-kind today — so R2 must add a kind-directed guard there (the messy sub-part).

## Lowering needs expected-kind threading (unlike ConstraintTerm R2)

R2 (ConstraintTerm) produced unconditionally from a *saturated* app (unambiguous). A bare `Eq` is
**ambiguous**: `*` position → `2-0006`; `* -> Constraint` position → `TraitCtor`. So lowering must consult
the expected param kind. Site: `lower_trait_ref_impl_inner` (`trait_lower.rs:241`); the expected kind is
locally available (`trait_params[i].kind(db)`, used at `:284`) but consulted *after* lowering — plus the
`ty_error.rs:261` diagnostic guard.

## Blast radius (honest; purely additive)

~16 compile-forced match arms (mostly 1–3 line copies of the `ConstraintTerm`/`QualifiedTy` arm: kind,
fold, walk, unify, stable-key, print×2, scope, span, visibility, const_ty×2, term×2, ty_depth,
function_symbols). Pre-built: `Kind::Abs`/`Constraint`, `does_match`, `Display`, `applicable_ty`,
interning. **~90–155 LOC, purely additive — NO deletions claimed.** (Comparable to ConstraintTerm's
measured ~+130; the win is surface-area / first-class representation, not LOC.)

## Rung plan (ConstraintTerm cadence; each a checkpoint, STOP-on-wall)

- **R1 — inert variant, byte-identical.** `TyData::TraitCtor(Trait)` + constructor + ~16 arms + unit
  test; produced nowhere. Gate: full suite byte-identical. Risk: low. (In flight.)
- **R2 — produce in arg position (TD-arg), expected-kind-directed.** `trait_lower.rs:241` + the
  `ty_error.rs:261` kind-directed diagnostic guard. Gate: Form-2 head no longer `2-0006` (may surface a
  transient `6-0007/8` until R3). Risk: medium (expected-kind threading into the diagnostic visitor).
- **R3 — application/normalization (TD2/TD3) + flip Form 2.** Add the `TraitCtor`-saturation reduction
  to `app_in_mode` (reusing default-completion). `P<T>` reduces to `ConstraintTerm(Eq<T>)`; `compare_ty`
  matches. Gate: `derive_kind_form2_param.snap` flips to clean; suite otherwise byte-identical.
  **Riskiest** — a new reduction in the globally-observed, salsa-interned `app`; STOP if any unrelated
  snapshot moves.
- **R4 (optional, separate) — Form 3.** Reuse R1–R3 + an assoc-type-value lowering branch. Flips
  `derive_kind_form3_assoc`. Independent of Form 2.

## What flips

Form 2 → green (target). Form 3 → ~1 extra small rung (same feature, assoc-value position; cascade
clears once the binding lowers). Form 1 → stays red (needs `impl <trait> for Eq` / Self-as-trait — the
deep shelved variant; out of scope). Advances **job-1 of #7** (makes `impl Derive<Eq>` a real
identity-recognized impl); the executor (TD5) + marker deletion (#7) remain the other half.
