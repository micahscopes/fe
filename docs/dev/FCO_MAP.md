# FCO_MAP — the whole thing on a page

**2026-06-21 · entry point.** Read this first. It is the navigation map over the FCO doc pile; it
carries no decisions of its own. Authority: `FCO_THE_SLIDE_2026-06-19.md` (SSOT) is right about
anything this map gets wrong.

---

## The converged spine (what every doc should now reflect)

- **ONE operation:** establish an impl — hand-written OR generated — both become one `ImplTrait` HIR
  node via the same constructor and converge at `lowered_implementor` (`ImplementorOrigin::Hir`). The
  executor's `GenExpr`/`ProviderOutput` is a transient intermediate, never persisted; the only
  difference hand-vs-generated is the `HirOrigin` provenance tag (`raw` vs `desugared`).
- **ONE resolver:** the provision walk + proof forest. Scope is resolved at the **verify-leg** (and the
  discharge seam), OUTSIDE the tracked solve — so `scope` NEVER enters the salsa solver key (the
  cache-safety linchpin). Downstream is record-driven (`ImplEnv.selected_implementor`), never a re-walk.
- **ONE authority:** `Fix<G>` — unforgeable, linear (`own`), single-use. BORN at root, SPENT to
  establish one consensus-critical impl, GONE after. It gates impl **CREATION**, never the **USE** of
  the implemented functions. The **gate** (`does_impl_trait_conflict`, live as `5-0001`) COUNTS impls;
  `Fix` is the scarce authority the gate honors for a sanctioned canonical impl/override. Cosmetic
  goals (`Eq`/`Ord`/`Clone`…) need no `Fix`; canonical goals (ABI / storage layout) require it.
- **ONE identity fix (the keystone):** content-keyed stable identity for generated impls (today they
  get a positional expansion `TrackedItemId` + `HirOrigin::desugared`, unstable vs hand-written
  source-AST anchoring). It is the linchpin for soundness AND tooling (LSP / tracing / debug parity)
  AND the deletions. **CONTAINED** — one `TrackedItemVariant` arm + ~7 redirects. Independent of `Fix`.
- **Canonical policy lives in Fe** (per-platform mint), not Rust. The proof obligations are mandatory;
  the *policy* (the canonical/`fixed` SET, the `fix` surface, the coherence root) is a deferred-tunable
  default — default the POLICY, never the PROOF.
- **Honest economics:** net-additive Rust (~+18k engine); deletable-when-done ~1.2–2.5k. The payoff is
  **surface-area eliminated** (special paths, seams, six→one resolver) and **per-feature cost ≈ 0**,
  NOT a net-LOC win. Judge burndown by surface area, not LOC delta.

The cliff law that keeps every step sound: pin the constraint head **concrete** before the solver
(`Eq<T>`, `Derive<Eq>`, `Fix<Consensus>` = ✅; `P<T>` with `P` free = 🚫). Quantify over Constraint only
at the **kind** level (∀, instantiate-only), never at the **solver** level (∃/search).

## Current state (2026-06-21)

- **CASCADE** (coherent cascading shadowing): the selection seam already exists
  (`discharge_from_scoped_provision`). C1 + much of C3 landed; the cascade dream fixtures are
  **4/7 green** (drive-green spine C3b → C3c-2-wire → C3c-3-flip; canonical money-floor stays
  exactly-one). See `dream_fixtures/MANIFEST.md`.
- **HKT-derive**: Form 2 chosen + landed; the **20 derive-kind fixtures green**; `ConstraintTerm`,
  `TraitCtor`, `Derive`-as-real-trait all shipped (`DERIVE_MARKER` deleted).
- **Design ratified**: the `Fix`/establishment model is settled
  (`FCO_FIX_UNIVERSAL_IMPL_DESIGN_2026-06-21.md`, with two corrections folded: two-layer enforcement;
  `Fix` stays an ordinary `own` value, NOT a capability/provider binding).

## The finite remaining path

1. **T1.1 — recognizer collapse.** One predicate recognizes the `Fix`/`Evidence`/`ImplBuilder`/
   `Reflect`/`Derive` family by resolved `core::derive` identity. Byte-identical. Includes the
   `Some`-branch determinism cleanup.
2. **T1.2 — the `Fix` floor (byte-identical).** Wire `goal_is_canonical` LIVE so canonical goals stay
   exactly-one and `5-0001` fires; remove the `allow(dead_code)`. The smallest, money-risk-closing rung.
3. **T2 — the keystone (parallel long-pole).** Content-keyed stable identity for generated impls. The
   one open soundness frontier (downstream-constructed `Body`/`TrackedItemId` identity stability —
   acyclicity is PROVEN; byte/id-identity is runtime-only today). Runway on the shelf:
   `CTFE_DERIVE_PHASE_BOUNDARY.md` Option B (post-lowering derive-expansion salsa query). DEAD-END to
   avoid: never run the deriver inside `is_query_satisfiable` (Salsa-cycle ICE) — run it OUTSIDE, feed
   the impl IN ("quasiquoter backend," never "CTFE provider").
4. **T3 — the money rail + deletions.** Half-B: fix-consumption + the deferred `Authority`/`grant` +
   per-platform mint + non-ambient propagation → **delete the global coherence checker LAST**.

Then: visceral fixtures (derive-via-`uses`/`with`, the remaining cascade fixtures, derive-grammar
retirement); the **N-way / abstract-head** (`P : * -> Constraint`) work stays DEFERRED behind the cliff
law (charter: `FCO_ABSTRACT_HEAD_RESEARCH_DOSSIER.md`).

## Workstream reconciliation (for the orchestrator — recommend, not applied)

DONE (close the board items): K02 Constraint-kind revival; `ConstraintTerm` R1–R3; `TraitCtor` R1–R3;
Form-2 derive-kind decision; `Derive`-bridge graduation / `DERIVE_MARKER` deletion; both AbiSize
generators deleted (#5b/#5c); the #5a reify-proof; provider-capability authority by resolved identity
(burn-down rows 1–3); generated-impl provenance (row 4); ProvisionEnv v0 / rung-3 (read-wrapper);
cascade C1; M5 obligation/CTFE+assumption-route substrate (the old consolidation-map gates 1–7/10).

LIVE (keep open): T1.1 recognizer-collapse · T1.2 `Fix`-floor · T2 keystone · T3 money-rail + checker
deletion · remaining cascade fixtures (3/7) · deferred surface/policy tuning (canonical set, `fix`/
`global` spelling) · DEFERRED abstract-head/N-way.

## LIVE docs — one-line index (what to read for what)

- `FCO_THE_SLIDE_2026-06-19.md` — **SSOT.** The plan, the spine, the cliff law, and the ratified
  Fix/establishment model. Start here after this map.
- `FCO_FIX_UNIVERSAL_IMPL_DESIGN_2026-06-21.md` — **decision ledger.** SETTLED/OPEN/REFUTED tags, the
  six-lens adversarial pass, the D1–D9 open decisions.
- `FCO_FIX_CAPABILITY_PACKET_2026-06-19.md` — Half-B appendix: the FV soundness obligations for `Fix`.
- `FCO_BRIDGE_BURN_DOWN.md` — **burndown board.** What bridge each increment removes/narrows + status.
- `PROVISION_SCOPING_SYNTHESIS_2026-06-17.md` — ratified provision-scoping decisions (§4 anchors the slide).
- `FCO_ABSTRACT_HEAD_RESEARCH_DOSSIER.md` — charter for the DEFERRED abstract-head / N-way program.
- `FCO_DERIVE_KIND_FORMS_2026-06-18.md` — why the `Derive` kind is Form 2 (decision record, authoritative).
- `dream_fixtures/MANIFEST.md` — executable spec for the cascade (the 7 fixtures + drive-green spine).
- `fco_dependency_graph_v0.json` — the draft dependency DAG (status-light; audit against repo).

## The historical pile

Every other `FCO_*` doc is SUPERSEDED or HISTORICAL and carries a top banner pointing here / to the
SSOT. They are kept as the dated record (decision lineage, sizing spikes, the BR0–BR13 inventory, the
M5 consolidation map, the rung-3 spikes, the policy/surface explorations) — read them for *origin and
file:line locus*, never for current sequencing.
