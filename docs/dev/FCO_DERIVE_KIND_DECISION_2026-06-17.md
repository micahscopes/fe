# Derive Kind — Decision + Feasibility Spike (assoc-type form)

**Date:** 2026-06-17 · **Status:** DECISION (Micah) + empirical spike (run on this branch's
`target/debug/fe`). Supersedes the `(* -> Constraint) -> Constraint` framing in
`FCO_DERIVE_SKETCHES_2026-06-17.md` §B as the **HEADED target**. Design-only; non-blocking on the
live burn-down (TD5).

> **⚠️ SUPERSEDED (2026-06-21) — see `FCO_THE_SLIDE_2026-06-19.md` "KEYSTONE INSIGHT".** The framing below ("ordinary CTFE" / "executor → CTFE de-magic" / "provider bodies become ordinary effectful CTFE") describes engine **fusion** and is superseded. The settled decision is **stage, don't fuse** (twice-measured): the executor is a **quasiquoter backend** (GenExpr→HIR, not a value-evaluator) run as a **downstream salsa query** producing a real `impl`; it is NOT folded into the CTFE value-evaluator (CTFE-inside-the-solver = Salsa-cycle ICE, a measured dead-end). Near-term the **cascade SELECTS** among existing impls; the keystone later **RUNS** a deriver to **GENERATE** one — distinct steps.

The kinded graduation (K04b/K03) still sequences AFTER the executor de-magic.

## Decision — TWO-STAGE target (Micah, 2026-06-17/18): assoc-type INTERMEDIATE → param-head END

Both forms are targets, staged. The associated-type form is the **intermediate milestone** (ship it
first — the spike shows it's close); the higher-kinded `(* -> Constraint) -> Constraint` form is the
**end target** ("the more elegant one… push through to it").

**Stage 1 — INTERMEDIATE: assoc-type form.** `Derive` is a **first-order ordinary trait**
(`Derive : * -> Constraint`); the derived trait rides on an associated type `Goal : * -> Constraint`;
the provider implements it (allows many providers per trait):

```fe
trait Derive {                       // Derive : * -> Constraint  (ordinary trait; Self = the provider type)
    type Goal : * -> Constraint      // the trait being derived, named EXPLICITLY (no compiler-secret link)
    const fn derive<T>(ev: own Evidence<Self::Goal<T>>) -> Evidence<Self::Goal<T>>
        uses ( reflect: Reflect<T>, quote: mut Quote, emit: mut Emit )
}
impl Derive for StableEq { type Goal = Eq; const fn derive<T>(ev: own Evidence<Eq<T>>) -> ... { ... } }
```

**Stage 2 — END: param-head form.** `Derive` becomes higher-kinded
(`Derive : (* -> Constraint) -> Constraint`); the goal moves from an associated type to a **trait
parameter**; the provider implements `Derive<Eq>`:

```fe
trait Derive<P> where P : * -> Constraint {          // Derive : (* -> Constraint) -> Constraint
    const fn derive<T>(ev: own Evidence<P<T>>) -> Evidence<P<T>>
        uses ( reflect: Reflect<T>, quote: mut Quote, emit: mut Emit )
}
impl Derive<Eq> for StableEq { const fn derive<T>(ev: own Evidence<Eq<T>>) -> ... { ... } }  // P := Eq pinned at impl
```

**Crucial — Stage 2 is the "True Derive" track, NOT the shelved abstract head.** `P` is pinned to a
**concrete** trait (`Eq`) at the impl boundary, so `Evidence<P<T>>` substitutes to `Evidence<Eq<T>>`
*before* solving — the solver never sees a live `P`. This is substitution + kind-check (TD1/TD2/TD3),
NOT variable-headed solving (the `6-0008` frontier that the abstract head `trait Derive<P>` with `P`
reaching the solver needs — that stays SHELVED). Stage 1 is the natural stepping stone: `type Goal = Eq`
(explicit assoc binding) is morally the spelled-out version of `Derive<Eq>` (positional binding), and
the bridge between them is exactly the substitution machinery (TD2).

## Why (scrutiny of the original sketches)

The pre-fable original (May-22 architect/Codex session, `architect-upload-bundle-2026-05-22/
session-log/codex-session-019e4df9-readable.md`) laid out a **ladder**, and `* -> Constraint` is
always the kind of the **trait** (`Eq`/`Hash`/`Abi`), never of `Derive`:

| rung | original form | `Derive`'s kind | architect's verdict |
|---|---|---|---|
| 1 | `trait Derive where Self: * -> *` (L708) | `(*->*) -> Constraint` | "EqDerive is the deriving object, not Eq" |
| 2 | marker: `impl Derive for EqDerive` (L724) | **`* -> Constraint`** | "feasible today, but **duplicates trait identity**" |
| 3 | `trait Derive<P> where P: * -> Constraint` (L764) | `(*->C)->C`, **P abstract** | abstract head — needs substitution-on-instantiation (**shelved**) |
| 4 | `impl Derive for Eq` (L777/813/838) | `(*->C)->C`, concrete | "the **beautiful** version" (L847) |

Findings: (1) Micah's `Derive : * -> Constraint` memory = rung 2 (the marker compromise ≈ today's
bridge), flagged for *identity duplication*. (2) The "beautiful" rung-4 shape `impl Derive for Eq`
makes the **trait** the implementer, so it allows only ONE derive per trait — but we ship MANY
providers per trait (canonical + `using StableX`). So rung 4's literal shape can't express our world;
the workable principled form is `impl Derive<Eq> for StableEq`. (3) **The assoc-type form is rung 2,
modernized**: name the goal as `type Goal` so the link is first-class (kills the duplication wart),
keep `Derive` first-order. Two of our own docs (`FCO_K03_K04_EXECUTION_MAP.md:112`,
`FCO_CONSOLIDATION_MAP.md:972`) already wrote "OD3 (`Derive : * -> Constraint`)" as shorthand —
contradicting `(*->C)->C` elsewhere in the same files (doc drift; this decision resolves it toward
`* -> Constraint`).

## Feasibility spike (empirical, `target/debug/fe check --standalone`, 2026-06-17)

| probe | form | result |
|---|---|---|
| A | `type Goal: * -> Constraint` (assoc-type kind bound) | **clean (exit 0)** — already accepted |
| D | `struct Box<G: Constraint>` (Constraint-kinded type param) | **clean** — legal kind annotation today |
| B | `Evidence<Self::Goal<T>>`, `Evidence` declared `<G>` (kind `*`) | `3-0001`: `Self::Goal<T>` **correctly computed kind `Constraint`**; only `Evidence`'s param kind mismatched |
| C | same, capability declared `<G: Constraint>` | **clean** — the **whole generic `Derive` trait declaration kind-checks** |
| E | concrete impl: `type Goal = Eq`, `Eq<T>` in type-arg position | `2-0006` "found trait `Eq`, expected type" |

**Decisive contrast:** a *param* head `P<T>` hits **`6-0008` "constraint-constructor parameter is
not yet a supported trait head"** — the unimplemented variable-headed-solving frontier (the dossier's
"major extension", shelved). A *projection* head **`Self::Goal<T>` kind-checks fine today** (Probe B
reached `3-0001`, i.e. it assigned `Constraint` kind, not `6-0008`). **The assoc-type form dodges the
hardest, explicitly-parked feature.** The generic `Derive` skeleton is essentially free on today's
machinery (Probe C clean).

## Remaining gaps to build (well-scoped; NO variable-headed solving in either stage)

**Stage 1 (assoc-type / intermediate) gaps:**
1. **K04b — Constraint-kind the capability constructors.** Declare `Evidence`/`ImplBuilder` with a
   `Constraint`-kinded param (`Evidence : Constraint -> *`) instead of `<G>` (kind `*`). `Constraint`
   is already a legal kind annotation (Probe D), so this is a contained `core::derive` change +
   removing the special-cased kinding. Closes Probe B's `3-0001`.
2. **K03-value — let a trait name be a `* -> Constraint` *value*** usable as an assoc-type value
   (`type Goal = Eq`) and in type-argument position (`Eq<T>`), in ordinary (non-special-cased) code.
   This is Probe E's `2-0006` — the real residual frontier, today bridged by special-casing
   `Derive`/`Evidence`. NARROWER than the param-head: the GENERIC side (`Self::Goal<T>`) already works.
   - **COUPLING (the one real risk):** K04b and K03-value are coupled — you cannot make
     `Evidence : Constraint -> *` without `Eq<T>` simultaneously being a real `Constraint` value, or
     the existing `Evidence<Eq<T>>` bridge providers break. Land them together; keep all `derived_*`
     fixtures green.

**Stage 2 (param-head / end) delta over Stage 1:**
3. **TD1 — kind-check `P<T>` in the `Derive<P>` decl** (`P : * -> Constraint` as a trait-level
   generic param; apply it as `P<T>` in the generic declaration without goal-lowering it — the
   declaration-site analogue of Probe C's clean projection-head check).
4. **TD2 — substitution-on-instantiation** (`P := Eq` at the `impl Derive<Eq>` boundary, so the body
   normalizes to concrete `Evidence<Eq<T>>`). This is the bridge from Stage 1's `type Goal = Eq`.
5. **trait-constructor-as-generic-argument** (`Derive<Eq>` — passing `Eq`, a `*->Constraint` thing,
   in trait generic-arg position). Stage 1's K03-value (trait-as-value) is the stepping stone; this
   extends it to argument position. The architect (May-22 L784) and cubical roadmap (§5.1.4) flag
   this as the "real type-system feature"; Stage 1 de-risks it incrementally.
6. **TD3 — `Evidence<P<T>>` / `where P<T>` normalize to concrete** post-substitution.

The abstract `P<T>` head (live `P` reaching the solver, `trait Derive<P>` used generically) stays
SHELVED — **neither stage needs it** (Stage 2 pins `P:=Eq` concrete before solving).

## Sequencing (OPEN — being mapped)

> **⚠️ SUPERSEDED (2026-06-21) — see `FCO_THE_SLIDE_2026-06-19.md` "KEYSTONE INSIGHT".** The framing below ("ordinary CTFE" / "executor → CTFE de-magic" / "provider bodies become ordinary effectful CTFE") describes engine **fusion** and is superseded. The settled decision is **stage, don't fuse** (twice-measured): the executor is a **quasiquoter backend** (GenExpr→HIR, not a value-evaluator) run as a **downstream salsa query** producing a real `impl`; it is NOT folded into the CTFE value-evaluator (CTFE-inside-the-solver = Salsa-cycle ICE, a measured dead-end). Near-term the **cascade SELECTS** among existing impls; the keystone later **RUNS** a deriver to **GENERATE** one — distinct steps.

Convention has been TD5 (executor de-magic) → K04b → K03-value → retire `Derive` marker (#7). Whether
the Stage-1 kind/signature work can land in parallel with / ahead of TD5 (the signature kinds and the
body de-magic are different phases) vs. is hard-gated by it (retiring the special-casing touches the
marker recognition = #7/TD5 territory) is being mapped against the code — see the dispatched plan.

## Probes (for re-run)

`/tmp/kindprobe/probe{A,B,C,D,E}.fe` (throwaway; not committed). Re-runnable via
`target/debug/fe check --standalone <file>`. Consider promoting C (positive: generic assoc-type
`Derive` skeleton kind-checks) and E (negative: trait-as-type `2-0006`) to `uitest` fixtures when
K04b/K03-value land, to pin the frontier.
