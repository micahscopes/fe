# Derive Kind — Decision + Feasibility Spike (assoc-type form)

**Date:** 2026-06-17 · **Status:** DECISION (Micah) + empirical spike (run on this branch's
`target/debug/fe`). Supersedes the `(* -> Constraint) -> Constraint` framing in
`FCO_DERIVE_SKETCHES_2026-06-17.md` §B as the **HEADED target**. Design-only; non-blocking on the
live burn-down (TD5). The kinded graduation (K04b/K03) still sequences AFTER the executor de-magic.

## Decision

The HEADED kinded-derive target is the **associated-type form** — `Derive` stays a **first-order
ordinary trait** (`Derive : * -> Constraint`), and the trait being derived rides on an associated
type `Goal : * -> Constraint`:

```fe
trait Derive {                       // Derive : * -> Constraint  (ordinary trait; Self = the provider type)
    type Goal : * -> Constraint      // the trait being derived, named EXPLICITLY (no compiler-secret link)
    const fn derive<T>(ev: own Evidence<Self::Goal<T>>) -> Evidence<Self::Goal<T>>
        uses ( reflect: Reflect<T>, quote: mut Quote, emit: mut Emit )
}

impl Derive for StableEq {           // the PROVIDER implements Derive (allows many providers per trait)
    type Goal = Eq                   // Eq : * -> Constraint, pinned as the goal
    const fn derive<T>(ev: own Evidence<Eq<T>>) -> Evidence<Eq<T>> uses (...) { ... }
}
```

vs. the rejected `Derive : (* -> Constraint) -> Constraint` (goal as a type *parameter*,
`impl Derive<Eq> for StableEq`, `Derive` itself higher-kinded).

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

## Remaining gaps to build (well-scoped; NOT variable-headed solving)

1. **K04b — Constraint-kind the capability constructors.** Declare `Evidence`/`ImplBuilder` with a
   `Constraint`-kinded param (`Evidence : Constraint -> *`) instead of `<G>` (kind `*`). `Constraint`
   is already a legal kind annotation (Probe D), so this is a contained `core::derive` change +
   removing the special-cased kinding. Closes Probe B's `3-0001`.
2. **K03-value — let a trait name be a `* -> Constraint` *value*** usable as an assoc-type value
   (`type Goal = Eq`) and in type-argument position (`Eq<T>`), in ordinary (non-special-cased) code.
   This is Probe E's `2-0006` — the real residual frontier, today bridged by special-casing
   `Derive`/`Evidence`. NARROWER than rung-3/param-head: the GENERIC side (`Self::Goal<T>`) already
   works, so the burden is only "name a concrete trait as a `*->Constraint` value", not solving over
   an abstract head.

Sequencing unchanged: TD5 (executor de-magic) → K04b (cap kinds) → K03-value (trait-as-value) →
retire the `Derive` marker (#7). The abstract `P<T>` head (rung 3) stays SHELVED — the assoc-type
decision does not need it.

## Probes (for re-run)

`/tmp/kindprobe/probe{A,B,C,D,E}.fe` (throwaway; not committed). Re-runnable via
`target/debug/fe check --standalone <file>`. Consider promoting C (positive: generic assoc-type
`Derive` skeleton kind-checks) and E (negative: trait-as-type `2-0006`) to `uitest` fixtures when
K04b/K03-value land, to pin the frontier.
