# "Higher-kinded effects" — named, mapped, and ranked (design sketch)

> **HISTORICAL (design sketch; grounding for the cliff law + `Fix`/`Evidence` shape) → `FCO_THE_SLIDE_2026-06-19.md` (cliff law) / `FCO_FIX_UNIVERSAL_IMPL_DESIGN_2026-06-21.md` §1.1 / `FCO_MAP.md`.** The constraint-indexed-capability shape (`Fix : Constraint -> *`, concrete-saturated only) and the soundness cliff are now folded into the SSOT/decision-ledger; the speculative HKT-effect ranking beyond that core is research-only. Kept as the dated design sketch.

**Date:** 2026-06-18 · **Status:** design wizard sketch (fable-grounded; load-bearing claims spot-verified)
+ surface-area-as-bug-substrate overlay. Speculative beyond the grounded core; soundness cliff marked
throughout. No build implication — this is *shape + soundness map*, not a plan.

## Naming: NOT "higher-kinded effects"

The phrase collides with two real PL concepts and leaks jargon:
- **higher-*order* effects** = effect *operations that take computations* (`catch`/`local`/`bracket`,
  scoped handlers) — Fe explicitly REFUSES control effects (next-push-plan:204; DEEP-LORE #5/#6: effect
  set small + fixed, not user-extensible). The name must not imply Fe is growing these.
- **kind-polymorphic effects** = effects quantified over a kind variable — Fe has none, wants none.

The concept actually in play: **a capability whose parameter is itself higher-kinded** — `Derive<P: * -> Constraint>`,
and the companion capability constructors `Evidence : Constraint -> *`, `ImplBuilder : Constraint -> *`
(`ingots/core/src/derive.fe:31,48` — both literally `<G: Constraint>`; `Reflect<T> : * -> *`).
**Name (analysis-internal): constraint-indexed capabilities** (broadly, constructor-indexed). **Surface
rule: "kind" never appears in a keyword, diagnostic, or user doc** — like `Vec<T>` never makes anyone say
"type constructor." Kill the darling phrase.

## It is the SAFE HALF of "obligations one level up"

Fable's spine (next-push-plan:174-184; obligations-review transcript L1509/L1512): two kind-arrow positions —
- **domain `F: A<B> -> *`** carries an *obligation* (applying F demands evidence) — concrete, eliminable
  before the solver.
- **codomain `G: * -> A<B>`** carries *evidence* (`∀T. G<T>: A<B>`, a constructor-attached axiom) — a new
  solver rule, "universally quantified evidence touches coherence," Fable ranked it LAST.

> **Constraint-indexed capabilities = the bounded-DOMAIN half, held strictly concrete-before-the-solver.
> The codomain (evidence-carrying) half IS the abstract-head cliff (`FCO_ABSTRACT_HEAD_RESEARCH_DOSSIER`).**

## Derive ⋈ effects (grounded)

- `Derive : * -> (* -> Constraint) -> Constraint` (Form 2) is already a constraint-indexed capability whose
  provider body is effect-world code (`uses (reflect, builder)`, `core_derives/src/lib.fe:26-31`).
- `Evidence : Constraint -> *` **is** a higher-kinded capability constructor — and post-`ConstraintTerm`
  it's an *honest* ordinary generic application (the carrier's "recognized specially" comments retire).
- The cascade rides the obligation pathway: `require<Eq>(field.ty())` → `ProviderEffect::Require` → ordinary
  checking (TD5.2). **demand = obligation**, for a higher-kinded capability.
- `P` is pinned `:= Eq` at the impl boundary → the solver only ever sees concrete `Eq<FieldTy>`. Solve-line holds.
- Under provision scoping: a constraint-indexed capability is just a **constructor-indexed provision**; the
  one resolver (`demand → scope-chain → evidence`) resolves on the *saturated* concrete key and needn't know
  it's higher-kinded.
- **The trap (surface-area-as-bug):** making `Evidence` honestly `Constraint`-kinded *removes the kind-error*
  that used to reject the abstract head (`Evidence<P<T>>` now kind-checks silently) — so the carrier survives
  as a diagnostic pass to keep rejecting it. **A more honest kind structure can disarm a guardrail; the
  rejection must MOVE, not vanish** (else a silent-acceptance bug grows in the seam).

## Use-cases — ranked by surface-area (collapse vs new) × soundness × demand

The surface-area lens and the soundness map **agree**: seam-collapsers are sound + demanded; pure-new-surface
is the cliff + demand-empty.

| rank | idea | surface area | soundness | demand | verdict |
|---|---|---|---|---|---|
| **1** | **Constraint aliases** (`Codec<T,A> = Encode<A>+Decode<A>+AbiSize`) | **COLLAPSES** the `: AbiSize`/`<A: Abi>` conjunction tax (verified real: `abi.fe:198,207,243,250`) | concrete (elaborator-expanded; never a solver head) | **real, in-tree** | **BUILD — the actionable win** ("ship-equivalent", dossier Tier A) |
| **2** | **Backend/`Platform` index** (`<A: Abi>` generalized; per-backend assoc-consts) | **COLLAPSES** per-backend duplication (write-once-retarget) | concrete (`A` pinned per backend) | real, roadmapped ([[fe-multi-backend-planned]]) | grounded; the most *valuable* near-term consumer (loosely HKE — `Abi: *`, type-indexed) |
| 3 | container effects (`Traverse<F: * -> *>`, `derive Functor`) | concrete form collapses traversal dup; **convenient form ADDS a solver rule** | domain=concrete; **codomain `∀T.F<T>:Eq` = CLIFF** | needs `Generic<T>` (absent) first | high interest, low near-term plausibility |
| 4 | proof/audit transport over `* -> Constraint` | erased/Prop form neutral; live-`P` form = new surface | erased sound; **live `P` = CLIFF** | empty today | speculative |
| 5 | the provision resolver itself as constraint-indexed | collapses (it IS the unify) | **money-safety** (canonical-no-shadow), not a solver cliff | real (= the north star) | this is provision scoping, not HKE per se |
| **6** | quantified caps (`∀T. P<T> =>`), live layout ctors | **PURE NEW SURFACE** | **CLIFF** (variable-headed solving, anti-proven) | empty | **REJECT — named-reject; lift per concrete shape** |

The abstract head is the archetype the lens condemns: maximally expressive, demand-empty, crosses the cliff —
**pure new surface for bugs+misunderstandings with no seam collapsed.** Ranks 1–2 are the inverse: they
*delete* surface (restatement tax, backend dup) and stay concrete.

## Honesty pass
- **Grounded/landed:** `Derive<P>` + `Evidence`/`ImplBuilder : Constraint -> *` + the cascade-as-obligation.
- **Designed, not built (build-worthy):** constraint aliases (Rank 1). Partial/roadmapped: backend index (Rank 2).
- **Speculative / crosses the cliff:** container *codomain* evidence (3), live-`P` proof transport (4), quantified (6).
- **Demand-empty today** (dossier's own skepticism, applied to these ideas): 4, 5-as-HKE, 6. Only **Rank 1 + 2**
  warrant a contract dev's attention now.

## Coda
"The constraint-constructor solver was never the lever." The phrase "higher-kinded effects" makes everyone
picture an abstract solver reasoning about a live `P` — that piece stays shelved. What's real: a kind
(`Constraint`, shipped) + an honest constructor-indexed capability (`Derive`/`Evidence`, shipped) + a
transparent abbreviation (aliases — cheap, build) + a backend index (grounded) + a refusal (the
codomain/quantified cliff — keep). Call them **constraint-indexed capabilities**, never surface "kind", and
keep the codomain half **rejected-not-faked** until the kind checker (the weakest link) can keep its promises.
