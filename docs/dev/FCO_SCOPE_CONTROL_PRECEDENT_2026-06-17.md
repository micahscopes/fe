# Scope-Control Precedent + the `global` Keyword Verdict

> **HISTORICAL (precedent research + keyword verdict; surface deferred-tunable) → `FCO_MAP.md` / SSOT `FCO_THE_SLIDE_2026-06-19.md`.** Per the governing principle (`fco-guts-over-sugar`), the `global`/keyword surface is a deferred-tunable default, not a gate. Kept as the dated precedent research + keyword-pair decision record.

**Date:** 2026-06-17 · **Status:** precedent research (design-wizard, web-verified). Design-only,
NON-BLOCKING. Refines `FCO_AUTHORITY_GATED_OVERRIDE` / `FCO_PROVISION_AUTHORITY_CONSTRUCT`.

## DECISION (Micah, 2026-06-17) — the keyword pair
- **`fixed`** = the surface keyword for the top/non-overridable tier (the sealed, one-true-witness,
  consistent-per-deployment state). Decided now. (Internal name stays `global ≡ canonical`; not surfaced.)
- **`fix`** (verb) = a FUTURE, permissioned override: "override a `fixed` provision **in the context of a
  held capability**." This is the capability-gated *authorized override* (the wizard's Construct-2 /
  ocap-attenuation case) given a clean, jargon-free surface — the `fixed`(adjective, sealed) / `fix`(verb,
  authorized-override) symmetry. **DEFERRED — do not build now; "for now `fixed` is great."** Captures
  Micah's "overridable but gated by who/capability" with capabilities as the gate.

## Precedent for nuanced scope control (the family Fe is joining)
- **Coq/Rocq — closest direct precedent.** Instance locality `local` / `global` / `#[export]` +
  per-instance `priority`. CRITICAL: `global` (active on any transitive `Require`, even un-`Import`ed)
  was judged a **footgun and DEMOTED from the default → `#[export]` (follow-the-import) is the default
  since 8.18** (PR #16258; implicit-global warning #13562). Among languages solving *exactly* Fe's
  problem, `global` is the *discouraged* setting.
- **Lean 4** — same `local`/`scoped`/`global` trichotomy + priority. Twice-reinvented ⇒ stable design point.
- **Scala 3** — givens are **import-scoped** (`import p.given`, not wildcard) — a deliberate fix of
  Scala 2's #1 implicits complaint. The TreeSet/`Ordering` incoherence bug (#11507/#11987) = **our money
  bug without money** → confirms Ord/Hash/`StorageKey` are the protected set.
- **OCaml functors** — witness lives **in type identity**; *generative* functor = fresh witness per
  application ⇒ structurally prevents the Scala bug. = our "witness-capture later"; generative ≈
  **fresh-per-deployment** = contract-scoped canonical.
- **Haskell** — orphans-as-warning is **too weak** (silent incoherence); named/local-instance proposals
  never landed ⇒ retrofitting scope onto global-coherence is very hard → **get it right pre-1.0.** The
  anti-pattern Fe is escaping.
- **Rust** — strict coherence + orphan = the newtype tax; what Fe drops as *default*, keeps as *opt-in*.
- **Agda** instance args (in-scope-set = instance-set, scope-chain). **Effekt/Unison** lexical
  capabilities (the `with` tier). **E/Agoric ocap** attenuation (authority narrows downward) = our "A"
  clause; Cosmos's runtime-ocap *failure* corroborates "enforce statically, lexical token ≠ whole-unit."

## The convergent sweet spot (strong validation of the FCO direction)
Coq-8.18, Lean, Scala-3, Agda independently landed on: **"visibility/witness follows the import +
lexical scope graph, innermost-first, with an explicit opt-in for the few things that must be coherent."**
Three of them got there by *correcting* an earlier always-global default. **Fe is designing toward that
consensus from the start.** Design-space axes: (1) scope-ladder granularity, (2) how a use-site finds a
witness [scope-chain ✓], (3) where witness identity lives [in-type = sound asymptote, heavy ⇒ sequence
last], (4) coherence enforcement [opt-in/per-property ✓].

**ACTIONABLE design constraint (folds into provision scoping):** the **module/ingot tier must FOLLOW
THE IMPORT GRAPH** (Coq `#[export]` / Lean `scoped` / Scala `import p.given`) — **NOT be ambiently-on**
(Coq `global`, the demoted footgun). This answers the synthesis open sub-Q1 ("does `use module::*` bring
provisions?") with a precedent-backed: provisions come in *with imports*, not ambiently.

**Genuinely novel (no single-language precedent):** unifying generative-functor witness-identity +
ocap attenuation for *consensus* — Fe would be first. Corroborates the named spikes (SPIKE-1 witness-
provenance-through-mono; effect_env-fold decidability) in `FCO_PROVISION_AUTHORITY_CONSTRUCT`.

## VERDICT: do NOT ship `global` as the surface keyword
`global` is the one common candidate that **mis-promises both load-bearing properties**:
1. Mainstream `global` (Python/JS/C) = "global *variable/state*" (negative baggage) and is mechanically
   INVERTED: Python `global` reaches *up* from a narrow scope; Fe's reaches *down* to forbid. Mainstream
   `global` = "visible everywhere **unless shadowed**" — Fe's whole point is **non-shadowable**. It
   promises the opposite of intent.
2. Coq/Lean `global` = the **demoted footgun** — borrowing it imports that baggage.
3. **"global to WHAT":** Fe's coherence root is the **deployed unit** (contract / layout-owning module /
   circuit), not the universe. `global` overpromises (universe), breaks "override from above" (nothing is
   above global), and is literally wrong on wasm/spirv.

**Recommendation — surface keyword `fixed`** (primary): leads with the property a contract dev cares about
(this witness is *fixed*, can't be quietly swapped), no global-state stink, backend-neutral, clean override
story ("it's fixed; to change it, re-fix at a higher scope; you can't locally shadow a fixed thing").
Runners-up: **`shared`** (best at "same one for everyone"), **`pinned`** (vivid). `sealed` = Rust precedent
+ matches the prevention-floor but is PL-ish/closed-set-connoting; `contract-wide` = most honest but
EVM-leaks (wrong multibackend — exactly why a *property* word beats a *root* word). **Keep `global ≡
canonical` as the INTERNAL/semantic name; do not surface it.** Surface keyword is ultimately a Micah-gut
taste call (rec `fixed`); worth a newcomer mental-model test (`fixed` vs `global` vs `shared`).

## Net
- Direction VALIDATED (4 ecosystems converged on it; Fe designs toward the sweet spot from the start).
- ACTIONABLE: module tier follows the import graph, not ambient (answers open sub-Q1).
- `global` keyword REJECTED for the surface → **`fixed`** (rec) / `shared` / `pinned`; `global≡canonical`
  stays internal-only.
- The two novel pillars need the already-named spikes; feasibility unconfirmed, shape confirmed.
