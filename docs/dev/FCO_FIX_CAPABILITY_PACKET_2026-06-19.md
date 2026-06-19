# `Fix` / `Fix<T>` override-capability — design packet (rung-3 stage-2)

**Date:** 2026-06-19 · **Status:** LIVE Half-B appendix to `FCO_THE_SLIDE_2026-06-19.md` (the SSOT). Per the governing principle, decisions A–D are DEFERRED-TUNABLE defaults (not gates); the SOUNDNESS OBLIGATIONS (unforgeable `Fix`, gate actually blocks, non-ambient propagation, attenuation lattice, MIR re-resolution determinism) are MANDATORY. NOT byte-identical (step-3 new behavior).
HEAD `e84f30b4a`. Source: read-only design agent (opus), grounded in code.

## Frame: two shadowings (don't conflate)
- **Already exists (lexical value scan):** `EffectEnv::lookup_precise` (`effect_env.rs:162-226`) iterates frames
  innermost-first and `break`s on first keyed match — innermost-wins over runtime *values* in the frame stack.
  A `with`-value is never in the impl table, so the solver can't select it.
- **Stage-2 adds (solver-level provision override):** an inner scope selecting a different *impl/`ImplementorId`*
  than its parent for the same canonical goal. THIS creates the money hole. **`Fix<T>` gates this one.**
- Existing `EffectBarrier`/`Barrier` (`effect_env.rs:139-145`, `effects/model.rs:172-205`) is the closest
  "block this key in scope" primitive — a reuse candidate for enforcing absolute `fixed`.

## The model
- **`Fix<T: Constraint>` / `Fix`** = capability types cloned from `Evidence` (`ingots/core/src/derive.fe:50-52`):
  private field, **no public constructor**, NOT in prelude, recognized by **resolved `core::*` identity** (not
  name — reuse `scope_is_derive_capability`, `provider_goal.rs:373-394`). `Fix : Constraint -> *` — same kind
  as `Evidence`, reuses the constraint-indexed-capability machinery as-is.
- **Unforgeability IS the money-soundness guarantee.** No expression mints a `Fix`; the only producer is the
  compiler at the root; the only propagation is explicit grant → the set of scopes that can override a `fixed`
  provision is closed + statically auditable. (Construction blocked: private field/no ctor/not prelude.
  Type-confusion blocked: resolved-identity, incl. `IngotKind::Core`. Capture blocked: not in scope unless granted.)
- **"Minted at root"** maps onto the existing `ProviderSource::RootProvider` anchor (`core/semantic/mod.rs:1667`,
  `provider.rs:75-131`; "entry roots have no caller to supply effects", `root_effects.rs:231`) — compiler-seeded
  like `seed_func_effect_witnesses` (`env.rs:1108-1180`), never an `Expr`.
- **Attenuation (monotone-down only):** `Fix ⊒ Fix<T>` ∀T; `Fix<T> ⊒ Fix<T>`; never widen (`Fix<T> ⋢ Fix`, `⋢ Fix<U>`).
  A holder of `Fix` mints narrow `Fix<T>` to delegate down; grant typeck verifies granting ⊒ granted.
- **The gate rule:** a `fixed` provision for `T` may be overridden in scope `S` **iff** `S` holds `Fix<T>` or `Fix`;
  else `fixed` is absolute (plain `with` over a `fixed` trait = hard error). Enforced at `check_with`
  (`expr.rs:1157`) + the cut-1 verify leg `trait_effect_goal_satisfiability_in_scope` (`expr.rs:2331`, scope already threaded).

## Soundness tie to cut-1 (+ the one new obligation)
- 3.2 provenance (`ResolvedTraitMethod.implementor` → `ImplEnv.selected_implementor`), 3.1 `scope` carry, 3.3
  `check_reresolution_determinism` (`trait_def.rs:234`, hard-fail `classify.rs:2309`) are the rails: a scoped override
  records the *overridden* `ImplementorId`; MIR re-resolution that ignored it mismatches → hard fail. The 3.3
  doc comment literally says it "locks the invariant before rung 3.4 broadens 'provision'" — stage-2 IS that broadening.
- **NEW LOAD-BEARING INVARIANT:** the `fix` override MUST write its selected `ImplementorId` into the callee
  instance's `ImplEnv` identically to the non-override path → then 3.3's assertion covers stage-2 for free
  (the "selection-input reconstructible at MIR" obligation). Fallback (heavier): serialize cap-presence into `stable_key.rs`.

## Genuinely new (vs reuse)
New: (1) mint-at-root mechanism; (2) the `fix` verb (consumes `Fix<T>` to authorize a `fixed` override; `with` never
checks authority); (3) the `fixed` marker/tier (none exists today, plan `:60-62`). Everything else reuses
`Evidence`/`ImplBuilder` + `ProvisionEnv` + cut-1 rails + the barrier primitive.

## Skeptic leaks (must close)
1. **Forgeable lookalike** — must use full resolved-identity (incl. `IngotKind::Core`); name-only check = money hole.
2. **AMBIENT CAPTURE (most dangerous)** — if `Fix` is seeded into the ambiently-walked `EffectEnv` frame, every
   nested scope inherits it (opposite of "root-local/private"). Must propagate non-ambiently (demanded/threaded,
   or a frame excluded from the ambient walk).
3. **Attenuation widening** — grant typeck must verify granting ⊒ granted; unit-test the lattice.
4. **`fix` escaping its scope** — override lifetime = the frame; barrier-check provision-carrying values at frame exit.
5. **Generic-context `Fix`** — "was the override authorized" becomes an instantiation property the determinism
   assertion can't certify. v1: **restrict `Fix<T>` to monomorphic/root contexts** until proven or cap-presence serialized.
6. **Coherence** — the override must REPLACE the in-scope candidate set (→ `Selection::Unique`), not ADD (→ `Ambiguous`).
7. **Salsa-key shatter** — apply the gate at the verify-leg site (where `scope` is readable), never inside the tracked solve.

## Decisions needed from Micah (each with the packet's recommended default)
- **(A)** master `Fix` + narrow `Fix<T>` + monotone-down attenuation — *rec A2* (narrow-only A1 = smaller v1).
- **(B)** "the root" — *rec: contract root* (reuse `RootProvider` anchor). (ingot root too coarse; privileged module = new concept.)
- **(C)** `fixed` default-on for canonical vs per-trait `#[fixed]` marker — *rec leans default-on*; **third option flagged:
  property-derived** (reachable from `core::Money`/`Consensus` marker) = safety without C1's global ergonomic cost. **Meatiest call.**
- **(D)** surface — *rec: keep `uses (_: Fix<T>)`; distinct `fix` verb (not overloaded `with`) so every money-swap is a greppable token.*
- **Cross-cutting:** confirm the "write `ImplementorId` into `ImplEnv`" discharge (4.2); restrict generic-context `Fix` in v1; enforce non-ambient propagation.
