# FCO Decision Packet — Provider-Goal Representation (de-magicking provider signatures)

**Status:** OPEN — owner decision requested.
**Author:** implementing agent. **Date:** 2026-06-16.
**Scope:** how to represent the *goal* a derive provider's capability signature mentions
(`Evidence<Eq<T>>`, `ImplBuilder<Eq<T>>`, `Reflect<T>`, `quote`, `require<…>`) so those
signatures can be **type-checked like ordinary Fe**, instead of being exempted.
**Companion docs:** `FCO_ABSTRACT_HEAD_RESEARCH_DOSSIER.md` (the TD/AH split, the solve-line,
Tier A/B/C); `FCO_DERIVE_LITERATURE_AND_RESEARCH.md` (precedent + research backlog).
**Hard rule carried in:** *do not adopt full `TyData::ConstraintTerm` now.* This packet exists to
decide whether a **narrower** carrier (Level 1) can retire the exemption without it.

---

## 0. TL;DR / recommendation

Three levels of how much "constraint-as-a-type-argument" we admit:

| Level | What it is | Live `P`? | Runtime `Evidence<C>`? | New `TyData`? | Blast radius |
|---|---|---|---|---|---|
| **0** | keep the signature exemption (today's bridge) | no | no | no | none (status quo) |
| **1** | narrow compile-time-only `CapabilityGoal` carrier | **no** | **no** | **no** (a sidecar enum, not a `TyData` variant) | provider lowering + capability kind-check only |
| **2** | full `TyData::ConstraintTerm` (constraint is a first-class kind-`Constraint` `TyId`) | enables it | enables it | **yes** | every `TyData` consumer + solver + lowering |

**Recommendation — "Option 2.5":** keep **Level 0** in tree as the bridge, **build Level 1**
to retire the exemption *for concrete goals only*, and **do not build Level 2** absent a separate
owner decision. Level 1 buys ~95% of the de-magicking win (real structural checking of provider
signatures, the abstract-head form named-and-rejected at a typed boundary) at a fraction of the
risk, and it stays strictly on the safe side of the solve-line: every `CapabilityGoal` is
eliminated to a concrete `TraitInstId` / `PredicateListId` **before** the solver runs, and no
`Evidence<C>` value ever exists at runtime.

This packet answers the architect's eight questions, specifies the required fixture, and states
exactly what Level 1 refuses.

---

## 1. The problem, precisely (why the exemption exists)

A derive provider today looks like:

```fe
impl StableEq: Derive for Eq {
    fn derive<T>(uses(reflect: Reflect<T>, builder: mut ImplBuilder<Eq<T>>), ev: Evidence<Eq<T>>) {
        // builds `impl Eq for T` via reflect + quote + builder commands
    }
}
```

The three capability/witness types name a **goal** in type-argument position:

- `Reflect<T>` — argument `T` is an ordinary `*`-kinded type. *(Already fine; see Q2.)*
- `ImplBuilder<Eq<T>>` — argument `Eq<T>` is a **constraint** (`Eq` is `* -> Constraint`).
- `Evidence<Eq<T>>` — same: the witness is *of a constraint*.

For `Evidence`/`ImplBuilder` to be **declared types** that the ordinary checker accepts,
`Eq<T>` must be a `TyId` of kind `Constraint` so it can sit in the type-argument slot of a
constructor `Evidence : Constraint -> *`. **There is no such `TyId` today.** A trait applied to
args is a `TraitInstId`/predicate, never a `TyData`. That is the entire crux.

**How we avoid it now (the bridge, verified in tree):**

1. `Evidence` is **not a declared type at all.** It exists only as `Value::Evidence`
   (`crates/hir/src/core/lower/provider_executor.rs:336`) — a typeless opaque unit bound to the
   provider's ordinary parameters at executor start
   (`provider_executor.rs:393-394`). It is never kinded, never checked.
2. `Reflect` / `ImplBuilder` **are** declared (`ingots/core/src/derive.fe`), but only with a
   plain generic parameter (`struct ImplBuilder<G> { handle: u256 }`). Their type **argument is
   read for capability recognition and then discarded**: `Capability`
   (`crates/hir/src/core/lower/provider.rs:51-57`) carries only the binding *name*, never the goal.
   The recognizer keys on the capability *type's* canonical path
   (`path_names_derive_capability`, K04a), not on `<Eq<T>>`
   (`provider.rs:210-237`). So `ImplBuilder<Eq<T>>`, `ImplBuilder<Whatever>`, and
   `ImplBuilder<Eq>` are today **indistinguishable** to the compiler — the `<…>` is decoration.
3. The provider `derive` fn **body and its `uses` signature are not run through the ordinary
   type checker.** The body executes in the bespoke `ProviderExecutor` (a restricted command
   language, not ordinary CTFE). That is the "exemption."
4. Recognition leans on string constants `DERIVE_MARKER="Derive"`, `REFLECT_KEY="Reflect"`,
   `IMPL_BUILDER_KEY="ImplBuilder"`, `DERIVE_FN="derive"`, `DERIVE_MODULE="derive"`
   (`provider.rs:31-39`); identity recognition (K04a) is primary, the string keys are a
   declared-temporary shim.

**Probe that pins the gate (TD1 spike, this session):** a real provider with a kinded param
`P: * -> Constraint` and signature `Evidence<P<T>>` / `where P<T>` produces
`error[2-0006]: expected type here, but found trait Eq` (a trait cannot occupy a type-argument
slot) and `error[6-0008]: … cannot yet be applied as a trait here`. The same `2-0006` would fire
for the **concrete** `Evidence<Eq<T>>` the moment we stop exempting the signature — because
`Eq<T>` is not a type. De-exempting therefore *requires* a constraint-as-`TyId` carrier, even for
the concrete, no-live-`P` case. **That is the finding this packet resolves.**

Crucially, this is **distinct from the abstract head.** The abstract head is the *solver* seeing
a live variable-headed goal `P<T>`. The carrier here is needed even when `P` never appears —
purely to make the *signature* a checkable type. We can have the carrier and still forbid the
solver from ever seeing a live head.

---

## 2. The architect's eight questions, answered

### Q1 — What, exactly, is needed to de-exempt provider signatures?

To type-check the `uses (..)` clause and witness params as ordinary Fe, each of the following must
become a real, checkable form:

| Surface | Today | To de-exempt it needs |
|---|---|---|
| `Reflect<T>` | declared struct, arg discarded | **nothing new** — `T` is `*`-kinded; just stop discarding the arg and kind-check it as `*` |
| `ImplBuilder<Eq<T>>` | declared struct, arg discarded | a **constraint carrier** for `Eq<T>` + a kind for `ImplBuilder` (`Constraint -> *`) |
| `Evidence<Eq<T>>` | undeclared; `Value::Evidence` only | `Evidence` becomes a **declared** capability type `Constraint -> *` + the same constraint carrier |
| `quote { … }` | special-cased template (`Value::Quote`) | a typed result form; **independent of the carrier** — can stay command-language for now |
| `require<Trait>` in synthesis | lowers to a constraint-application predicate (W-B path) | **already concrete** (W-B landed) — no carrier needed |

So the *only* new thing the de-exemption fundamentally requires is **a way to carry a concrete
constraint (`Eq<T>`) as a kind-`Constraint` type argument** to `Evidence` / `ImplBuilder`. Reflect,
quote, and require do not need it.

### Q2 — Which of these actually need a *constraint* argument (vs an ordinary type)?

Only **`Evidence` and `ImplBuilder`**. Their argument is *a constraint*.

- `Reflect<T>` — `T` is the derive target, an ordinary `*` type. **No constraint carrier.**
- `quote` — produces generated syntax; its checking is about the *template*, not a constraint. **No carrier.**
- `require<Trait>` — already lowered to a concrete predicate via W-B. **No carrier.**

This narrows the entire problem to *two constructors, each taking one constraint argument*.

### Q3 — Can the representation be narrow? (the proposed `CapabilityGoal`)

**Yes.** Proposed compile-time-only carrier (a sidecar enum, **not** a `TyData` variant):

```rust
/// The concrete constraint a provider capability/witness names
/// (`Eq<T>` in `Evidence<Eq<T>>` / `ImplBuilder<Eq<T>>`).
/// Compile-time only; never a runtime value; eliminated to a concrete
/// trait obligation before the solver runs. Never carries a live head.
enum CapabilityGoal<'db> {
    /// A single applied trait: `Eq<T>`  → exactly one obligation.
    ConcreteTrait(TraitInstId<'db>),
    /// A conjunction: `Encode<T> + Decode<T>`  → a fixed obligation list.
    PredicateList(PredicateListId<'db>),
    /// A Tier-A alias expanded at lowering: `Serializable<T>`
    /// → the PredicateListId it expands to, plus the alias path for diagnostics.
    AliasExpanded { alias: PathId<'db>, expanded: PredicateListId<'db> },
}
```

This reuses the **exact machinery W-B/W-C already built** — `lower_hir_constraint_application`
(concrete `Eq<T>` → `TraitInstId`, Self = first arg), `PredicateListId`, canonical-path identity.
`CapabilityGoal` is just the *named home* for "the concrete goal a capability signature mentions,"
stored on the `Capability` enum (which today carries only a binding name). It never introduces a
new kind-`Constraint` `TyId`; the constraint never enters `TyData`; it never reaches the solver as
a head.

`ImplBuilder` and `Evidence` are then recognized exactly as today (K04a canonical-path identity),
but their **type argument is lowered through `lower_hir_constraint_application` into a
`CapabilityGoal`** instead of being discarded — and *that lowering is the kind-check*: it succeeds
only for a concrete constraint, fails (with a typed diagnostic) for anything else.

### Q4 — Where is it legal? (containment)

`CapabilityGoal` is legal **only** in:

1. The `uses (..)` capability clause of a `derive` provider fn (`ImplBuilder<…>`, `Reflect<…>`).
2. The witness parameter position of a `derive` provider fn (`Evidence<…>`).

It is:

- **Compile-time only** — produced during provider lowering, consumed by the kind-check and by
  selection; gone before codegen.
- **Never a runtime value** — there is no `Value` variant for a goal; `Value::Evidence` stays the
  typeless opaque unit it is today (so no `Evidence<C>` dictionary is ever materialized).
- **Never in ordinary type position** — a user writing `let x: Evidence<Eq<T>>` outside a provider
  signature is *not* enabled by this; `Evidence`/`ImplBuilder` remain provider-only capability
  types. (Enforced because the carrier is built only on the provider-lowering path.)
- **Never reachable by the solver as a head** — it is eliminated to `TraitInstId`/`PredicateListId`
  at lowering; the solver only ever sees those concrete forms (the solve-line invariant).

### Q5 — What does it *refuse*? (the negative surface — this is the safety case)

`CapabilityGoal` lowering rejects, each with a typed diagnostic, **before** the solver:

- **A. `Evidence<P<T>>` with live `P`** (a `* -> Constraint` *parameter* head) → the abstract-head
  diagnostic (`6-0008`). `CapabilityGoal` has **no variable-head variant**; a live head cannot be
  constructed. This is the solve-line boundary, now named at a *typed* position rather than slipping
  through an unchecked signature.
- **B. `Evidence<forall T. …>` / quantified constraints** → rejected (Tier C is named-reject; no
  variant exists for it).
- **C. `Evidence<Eq>` (arity/kind mismatch)** → kind error: `Eq` is `* -> Constraint`, not a
  saturated constraint; `lower_hir_constraint_application` yields no `TraitInstId`.
- **D. unexpanded / cyclic aliases** → an alias must expand to a finite concrete `PredicateListId`;
  failure is a typed diagnostic, not a silent admit. (Inherits Tier A's termination requirement.)
- **runtime `Evidence<C>` values** → impossible: no constructor, no `Value` variant. A provider that
  tried to *return* or *store* an `Evidence<C>` as data is rejected because no such runtime type
  exists.

The refusal set is the whole point: de-exempting means these failures now happen at a **declared,
kind-checked boundary** with precise diagnostics, instead of being structurally unrepresentable-yet-
silently-tolerated inside an exempt signature.

### Q6 — What bridge does it retire?

Level 1 retires, specifically:

- ✅ **The signature exemption for the goal argument** — `ImplBuilder<Eq<T>>` / `Evidence<Eq<T>>`
  arguments become *kind-checked*, not discarded. (The provider *body* exemption — the
  command-language executor — is **out of scope** here; that is the separate TD5 "provider bodies
  via ordinary CTFE" track.)
- ✅ **The "argument is decoration" gap** — `ImplBuilder<Eq<T>>` vs `ImplBuilder<Whatever>` vs
  `ImplBuilder<Eq>` become *distinguishable*; the declared goal is checked to be a real concrete
  constraint, and (bonus) can be cross-checked against the selected derive goal.
- ✅ **One special-parse path** — the bespoke acceptance of `Eq<T>`-in-`Evidence` purely because the
  signature isn't checked. It now goes through the *same* `lower_hir_constraint_application` as
  `where Eq<T>` (W-B), so there is one constraint-application lowering, not two.
- ⚠️ **Does NOT retire** the K04a identity recognition (still how `Reflect`/`ImplBuilder`/`Derive`
  are found — that's correct and should stay), nor the `DERIVE_MARKER`/`DERIVE_FN`/`DERIVE_MODULE`
  string shim (separate K04a-C3 cleanup), nor the executor (TD5).

So Level 1 is precisely targeted at the *type-level* magic (constraint-as-decoration), not the
*recognition* or *execution* magic, which have their own tracks.

### Q7 — Smallest fixture that proves it? (full spec in §4)

A single provider whose `ImplBuilder<Eq<T>>` / `Evidence<Eq<T>>` are **structurally checked as a
concrete goal** (positive), plus four negatives proving the boundary holds: live `P` (A),
missing/unresolved trait (B), arity/kind mismatch (C), and runtime-value attempt (D). See §4.

### Q8 — Why is full `TyData::ConstraintTerm` (Level 2) NOT required?

Because everything Level 1 admits is **concrete and first-order**, and everything it produces is a
`TraitInstId`/`PredicateListId` that *already exists* in the IR. `ConstraintTerm` is only required
when a constraint must be a fully general `TyId` that can:

- flow through arbitrary `TyData` consumers (subst, unify, display, codegen) as a kind-`Constraint`
  type — Level 1 never puts it in `TyData`;
- be **abstracted over** (`Evidence<P<T>>`, a live head) — Level 1 forbids this (Q5.A);
- become a **runtime dictionary** (`Evidence<C>` value) — Level 1 has no such value (Q4).

In other words, `ConstraintTerm` pays for generality (live heads, runtime evidence, full type-system
citizenship) that the solve-line *deliberately refuses*. Level 1 carries the constraint only as far
as the capability signature and then collapses it to the concrete obligation the rest of the
compiler already understands. The blast radius difference is the table in §0: a sidecar enum on one
code path vs. a new variant every `TyData` matcher must handle.

If/when a real consumer for the abstract head appears (the AH track's two triggers), *that* is the
moment to weigh Level 2 — and even then `CapabilityGoal` is the natural concrete sub-case it would
generalize, so Level 1 is not throwaway.

---

## 3. The three levels in full

### Level 0 — keep the exemption (status quo bridge)
- **Mechanism:** `Evidence` undeclared (`Value::Evidence` only); `Reflect`/`ImplBuilder` declared
  but args discarded; provider signature + body not ordinary-checked.
- **Pro:** zero change, zero risk, working today (W-B/W-C green).
- **Con:** the type-level magic remains — goal argument is decoration; the abstract-head boundary is
  enforced by *absence of a representation*, not by a *checked rejection*; `Evidence<Eq<T>>` reads
  like a checked type but isn't.
- **Verdict:** correct **bridge**; keep until Level 1 lands. Not the end state.

### Level 1 — narrow `CapabilityGoal` (proposed end state for concrete goals)
- **Mechanism:** §2 Q3. `CapabilityGoal` sidecar enum on `Capability`; `Evidence` becomes a declared
  `Constraint -> *` capability type; capability/witness goal args lowered via
  `lower_hir_constraint_application`; that lowering *is* the kind-check; eliminated to
  `TraitInstId`/`PredicateListId` before the solver.
- **Pro:** retires the type-level exemption for concrete goals; abstract head becomes a *typed*
  named rejection; one constraint-application lowering shared with W-B; no new `TyData`; no live `P`;
  no runtime `Evidence<C>`; reuses landed machinery.
- **Con:** real work (kind `Evidence`/`ImplBuilder`; thread the goal onto `Capability`; lower + check
  the arg; wire the negatives). Touches provider lowering, capability kind-check, `derive.fe`.
- **Verdict:** **recommended build.** Stays strictly inside the solve-line.

### Level 2 — full `TyData::ConstraintTerm` (do NOT build now)
- **Mechanism:** constraint becomes a first-class kind-`Constraint` `TyId` variant in `TyData`.
- **Pro:** the only thing that enables a *live* abstract head and runtime `Evidence<C>` — i.e. the
  AH track, *if* it ever has a consumer.
- **Con:** the "370-commit mess" risk — every `TyData` consumer, the solver, and lowering must learn
  the variant; reintroduces the `ConstraintId`-shaped surface the architect already rejected; opens
  the door to the solver seeing a live head.
- **Verdict:** **gated.** Build only on an explicit owner decision *after* a real abstract-head
  consumer exists (AH3 trigger). Level 1 is its concrete sub-case, not a competitor.

---

## 4. Required fixture (the proof obligation)

Anti-vacuous per the M5 standard: positive proves the intended check *happened*; negatives prove the
forbidden shapes are *rejected at the typed boundary*; the route is observable.

**Positive — `provider_signature_concrete_goal_checked.fe`:**
A derive provider with `uses(reflect: Reflect<T>, builder: mut ImplBuilder<Eq<T>>)` and witness
`ev: Evidence<Eq<T>>` over a concrete goal. Asserts:
1. the capability/witness goal arguments are **lowered and kind-checked** (not discarded);
2. `Eq<T>` resolves to exactly `TraitInstId{Eq, [T]}` (`CapabilityGoal::ConcreteTrait`), Self = `T`;
3. **no live `P`** is ever formed;
4. the provider **body keeps the command-language exemption** (TD5 is out of scope — body still runs
   in the executor);
5. the **generated `impl Eq for T` re-enters ordinary checking** via the W-B `require<…>` path
   (already landed) — end-to-end derive still produces a working `impl` and the exec fixture passes.

**Negative A — live head — `provider_signature_live_head.fe`:**
`fn derive<P: * -> Constraint, T>(…, ev: Evidence<P<T>>)`. Expect the abstract-head diagnostic
(`6-0008`): no `CapabilityGoal` variable-head variant exists; the boundary is named at a *typed*
position. *(This is the architect's explicitly required contrast case.)*

**Negative B — unresolved trait — `provider_signature_missing_trait.fe`:**
`Evidence<MissingTrait<T>>`. Expect a resolution/kind diagnostic — lowering finds no trait, yields no
`TraitInstId`.

**Negative C — arity/kind mismatch — `provider_signature_unsaturated_goal.fe`:**
`Evidence<Eq>` (the `* -> Constraint` head, unsaturated). Expect a kind/arity diagnostic — not a
saturated constraint, so no `CapabilityGoal::ConcreteTrait`.

**Negative D — runtime evidence value — `provider_signature_runtime_evidence.fe`:**
A provider that tries to bind/return an `Evidence<Eq<T>>` as a runtime value (e.g. `let e = ev;`
escaping the command language, or returning it). Expect a compile-time-only diagnostic — there is no
runtime `Evidence<C>` type/value (Q4/Q5). *(If the executor already makes this structurally
impossible, the fixture documents that as the enforcement point rather than adding a new diag.)*

Each negative must prove the **forbidden solver shape did not slip through** (no live `P` reached the
solver; no silent `Kind::Any`); the positive must prove the **concrete obligation was produced** and
the derive still works.

---

## 5. What I recommend doing now

1. **Adopt Option 2.5** (this packet's recommendation): Level 0 stays as the bridge; Level 1 is the
   sanctioned build for retiring the concrete-goal exemption; Level 2 stays gated.
2. **Do not block forward progress on this decision.** Per the architect, while the decision is
   pending the sanctioned forward work is **Tier A constraint aliases** (W-C is already landed). Tier
   A needs no carrier decision (it expands to `PredicateListId` at lowering) and its
   `AliasExpanded` shape is *exactly* the third `CapabilityGoal` variant — so Tier A is both useful
   now and de-risks Level 1.
3. On approval of Option 2.5, implement Level 1 behind the fixture in §4, via a worktree-isolated
   agent (per the "keep going via agents" directive), verified before integration — same cadence as
   W-B/W-C.

**Decision requested:** approve Option 2.5 (build Level 1, gate Level 2), or redirect.
