# HKT Derive — the three kind forms, demonstrated + their tradeoffs

**Date:** 2026-06-18 · **Status:** design demonstration, empirically grounded against the live
compiler (HEAD post-TD5-iter). **DECISION (Micah, 2026-06-18): FORM 2** — `Derive` kind
`* -> (* -> Constraint) -> Constraint` (provider as `Self`, goal as a param). Fixtures pin all
three: `crates/fe/tests/fixtures/cli_output/single_files/derive_kind_form{1,2,3}_*.fe`.

## The three forms

How you'd write a provider that derives `Eq`, and what kind `Derive` therefore has:

| form | impl you write | `Self` is | `Derive`'s kind | multi-provider (`using`)? |
|---|---|---|---|---|
| **1** self-as-trait | `impl Derive for Eq` (`trait Derive where Self: * -> Constraint`) | the **trait** `Eq` (`* -> Constraint`) | `(* -> Constraint) -> Constraint` | ❌ one derive per trait |
| **2** param **(CHOSEN)** | `impl Derive<Eq> for StableEq` (`trait Derive<P: * -> Constraint>`) | the **provider** `StableEq` (`*`) | `* -> (* -> Constraint) -> Constraint` | ✅ |
| **3** assoc | `impl Derive for StableEq { type Goal = Eq }` (`trait Derive { type Goal: * -> Constraint }`) | the **provider** `StableEq` (`*`) | `* -> Constraint` | ✅ |

Note the `(* -> Constraint) -> Constraint` kind belongs **only to form 1** (the trait is `Self`).
The earlier decision doc labeled form 2 that way — **imprecise**: form 2 has an extra leading `*`
(the provider `Self`), so it's `* -> (* -> Constraint) -> Constraint`.

## Empirical behavior today (all error — same root gap)

Run against the live compiler (`fe check --standalone`); each fixture pins its diagnostic:

(The fixtures name the demonstration trait **`Derive_`** — trailing `_` — to keep it distinct from the
real `Derive`, which today is a string-marker, not a real trait. The kind-form table above uses the
conceptual `Derive`.)

- **Form 1:** `impl Derive_ for Eq` → `error[2-0006]` "found trait `Eq`" — `Self` can't be a trait.
- **Form 2:** `impl Derive_<Eq> for StableEqK` → `error[2-0006]` "found trait `Eq`" — bare trait as a
  generic **argument**. (The trait *declaration* `trait Derive_<P: * -> Constraint> { .. Evidence<P<T>> }`
  already kind-checks — the only gap is the arg.)
- **Form 3:** `type Goal = Eq` → `error[2-0006]` "found trait `Eq`" (+ the method sig cascades, as
  `Self::Goal` becomes `invalid(NotAType(Eq))`) — bare trait as an associated-type **value**.

**The unifying gap:** all three need a **bare trait constructor (`Eq`, unsaturated, kind
`* -> Constraint`) usable as a first-class value** — in arg position (form 2), assoc-value position
(form 3), or as `Self` (form 1). `ConstraintTerm` (landed R1–R3) covers a **saturated** `Eq<T>`
(kind `Constraint`); none of these is saturated. So the next feature that lights up the chosen
form is **trait-constructor-as-value** (the `* -> Constraint` thing itself), NOT more `ConstraintTerm`.

## Tradeoffs

- **Form 1** — cleanest kind, but **single-provider only** (kills `using StableEq` named alternates,
  which Fe ships) and the most exotic (`Self : * -> Constraint`, `impl <trait> for Eq`, `Self<T>`).
- **Form 2 (CHOSEN)** — provider is an ordinary `Self` type → multi-provider ✓; the goal is **visible
  in the head** (`Derive<Eq>`); the declaration already kind-checks; one clean gap (trait-ctor-as-arg).
- **Form 3** — also multi-provider, **simplest kind** (`Derive` stays first-order), but the goal is
  **implicit** (hidden under `type Goal`) and it costs an extra associated type + projection machinery.

## Why Form 2

Multi-provider is load-bearing (`derive Eq for Point using StableEq`, std `using StableAbiSize`,
user `using AlwaysTagged`), so Form 1 is out. Between 2 and 3, Form 2 keeps the derived trait
**visible at the impl head** (`impl Derive<Eq> for StableEq`) rather than buried in an associated
type, at the cost of one extra arrow in the kind. Micah's call: **Form 2.**

## What it takes to land Form 2 (the chosen gap)

Trait-constructor-as-value in **argument position**: when an expected param kind is `* -> Constraint`
and the arg path resolves to a (concrete) trait, carry that trait constructor as a `* -> Constraint`
value rather than erroring `2-0006`. This is the unsaturated sibling of the saturated `ConstraintTerm`
work. Plus the provider engine + `Derive`-as-real-trait (#7/TD5-gated). Sequence: after TD5 → #7.
