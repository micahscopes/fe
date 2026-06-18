# Metaprogramming authoring DEVX — review + alternative-interface proposals

**Date:** 2026-06-18 · **Status:** design-wizard review (read-only; fable-log + surface-lore grounded;
load-bearing claims spot-verified). Recommends A+B+C-narrow for the authoring-DEVX slice.

## Settled intent (not re-litigated)

`fe-obligations-next-push-plan-2026-06-09.md:186-190`: the `builder.*` vocabulary is a **COMPILATION
TARGET, not a user API** — "spend nothing on builder ergonomics"; **quasi-quote is the primary authoring
surface**; quote blocks elaborate to the same pipeline. So the question is *which quote-centric slice buys
the most authoring ergonomics per engine cost, without losing auditability* — not whether to do it.

## Preserve (load-bearing, non-negotiable)
1. **Evidence not magic** — generated code re-enters the full pipeline; malformed output = compile error at
   the derive site, never a deployed vuln.
2. **Provider = complete spec of its output** — the single best property; the reason quotes win.
3. **Authority = visible capability** — splicing needs `uses (builder: mut ImplBuilder<G>)`; making code
   *exist* is never terse (Christoph/Sean).
4. **Hygiene, no escape hatch** — names resolve in the provider's scope; no Julia/Elixir escape idiom.

## Pain (cited in real provider code)
1. **The signature dance** — `method/with_self/with_arg/returns/emit_method` (5 lines of pure ceremony
   re-spelling the goal trait's own signature) appears 8× (`lib.fe:43-47,105-109,128-132,149-152`,
   `abi.fe:157-160`, `eip712.fe:289-296,343-346`).
2. **EIP-712 is builder-trees, not quotes** (`eip712.fe:251-351`; **verified 12 `builder.*`, 0 `quote`**).
   The M4 quote-showcase provider is unreadable builder-tree assembly — it stopped being a spec of its
   output. Strongest single argument in the review.
3. **Two dialects** — bodies can be `quote{}` (lib.fe) OR `builder.*` trees (abi/eip712); newcomers learn two.
4. **Hand-rolled O(n²) dedup** — `same_ty`/first-by-type scan with two bool flags (`eip712.fe:311-329`).
5. **Control-flow gaps** — no `<ty as Trait>::item` form in quotes (TD5 surprise #3) — THIS is *why*
   EIP-712 can't be quotes; no `while`/`continue`/`break`; int literals rejected in quote bodies.
6. **Fold identity seeds** the author must know (`quote { true }`, `<()>::HEAD_SIZE` 0-seed) — honest +
   MIR-folded, but a learnability tax.

## Proposals (ranked by payoff × feasibility)

**A — `emit_method(name, body)` sig inference (RANK 1; ship first; near-free).** Infer the method signature
from the goal trait's declaration; drop the 4-op sig dance. Hits all 8 sites. **SHRINKS the frozen surface
(drops `method`/`with_self`/`with_arg`/`returns`)** — a surface-area win, freeze-rule-welcome. Cost: LOW
(the goal `G` is already carried by `provider_goal.rs`; verify the method sig is reachable at emit).

**B — `<ty as Trait>::item` quote form (RANK 2; unblocks EIP-712 + Decode menu #3).** Add the missing quote
production so EIP-712 (and AbiSize's `trait_const` folds, and `#[msg]` Decode — explicitly blocked on this,
`FCO_STDLIB_DESUGAR_PROVIDERIZATION_2026-06-18.md:35-38`) can be written as quotes. After A+B there is ONE
body dialect. Cost: MEDIUM (parser + elaboration; a Q-category freeze change). **Reverses a prior
darling-kill** (the surface study deliberately excluded `<_ as _>`) — recommend on EIP-712 evidence, but
**discuss before shipping** (a real decision, not auto).

**C-narrow — `first_by_type()` + targeted sequence reads (RANK 3; cheap).** Kill the worst hand-rolled loop
(Pain #4) with a read-table extension on the field-sequence handle (the TD5c pattern, freeze-governed).

**C-general — `fold`/`map` with closures (RANK 4; deferred).** "A derive is a fold over fields" made literal.
Needs first-class CTFE closures (the executor recognizes none today) — defer until closures land for other
reasons. Keep the fold SEED visible as the first arg (auditability) — never a bare `map().join`.

**D — declarative `fieldwise` (KILL).** Re-introduces compiler-blessed generation strategies — exactly the
road the project explicitly *didn't* take (`DERIVE_WITHOUT_MACROS.md:103-109`); re-magics the surface,
breaks "provider = spec". Only salvage: `fieldwise` as a pure-Fe std combinator built ON quotes (= C).

## Recommendation: ship A + B + C-narrow

Collapses the two-dialect problem to one (`quote`), turns the EIP-712 disaster back into a one-screen spec
(the whole point of the quasiquote arc), kills the worst loop — all as *shrinks or small additions* to the
frozen surface. Darlings to kill: the sig-builder chain (A); `trait_call`/`trait_const`/`static_call` as
author-facing ops (survive as elaboration internals once B lands); declarative `#[derive]`-strategy (D).

## Convergence (why this isn't a new track)
A and B ARE the shape TD5d/e should take ("quote/emit as real effects", `TD5_PROVIDER_COMMAND_SURFACE.md`):
A shrinks the builder surface, B grows the quote surface to subsume the builder goal-qualified ops. And B
directly unblocks the providerization menu (#3 Decode). So the DEVX work and the executor-de-magic (TD5)
are the same work seen from the authoring side. Constraints honored: no PL-jargon (`fold`/`map`/`fieldwise`
are JS/TS/Python-native, never "catamorphism"); auditability *strengthened* (A+B make the provider read
like its output again; seeds stay visible; `--show-derived` unchanged); authority stays a visible capability.
