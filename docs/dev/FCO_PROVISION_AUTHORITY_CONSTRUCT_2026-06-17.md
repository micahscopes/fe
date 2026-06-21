# Provision Authority — the Language Construct (design-wizard, fable-grounded)

> **HISTORICAL (authority-construct exploration; folded) → `FCO_FIX_UNIVERSAL_IMPL_DESIGN_2026-06-21.md` §4 + `FCO_MAP.md`.** The authority *construct* explored here is now the ratified `Fix`/establishment model's deferred `Authority`/`grant`/`RootProvider` policy layer (Tier 3, grant-as-data, no reified `Mint` tower per the cliff law). Historical construct exploration. SSOT: `FCO_THE_SLIDE_2026-06-19.md`.

**Date:** 2026-06-17 · **Status:** language-design exploration (self-review methodology; fable-corpus
grounded). Design-only, read-only, NON-BLOCKING. Designs the *construct* (vs the A/B/C/D policy menu
in `FCO_AUTHORITY_GATED_OVERRIDE_2026-06-17.md`). Load-bearing code claims spot-verified ✓.

## DEVX CONSTRAINT (Micah, 2026-06-17 — governs all surface design)
The *mechanism* below (consumer-side demand that a layout choice is consistent + from a trusted
scope) is ratified-good. The **surface is NOT**: `@uniform` / `#[witness_uniform]` / "provenance" /
"witness" / "canonical" / "@rooted" are **PL/type-theory jargon and must NOT leak into the devx**.
Audience = smart-contract devs, not PL theorists. Redesign the surface to: (1) **invisible-by-default**
— the everyday dev writes NOTHING; std sets up storage-shaped traits so they're consistent
automatically; (2) **plain-language errors** carry the whole devx (e.g. "`Balance` is stored two
different ways in this contract — in `get` and in `pay()`; a contract must store a type one way
everywhere"); (3) **domain-natural opt-in** for the rare customization, reading like a *setting*
(e.g. a contract-level "this contract's storage encoding is X"), never a `where`-clause qualifier;
(4) any std-author marker uses domain words ("per-contract" / "storage-layout" / "consistent"), not
"witness/uniform". The `@uniform`-style machinery stays strictly UNDER THE HOOD. This is a surface/
naming redesign over the (kept) mechanism — tracked for a devx pass, not yet spelled.

## The decisive reframe
A programmer must be able to say four distinct things: **S**eal (provider: "no closer shadow"),
**O**verride-authority ("I may introduce a competitor here"), **P**rovenance-demand (consumer: "the
witness that discharges THIS goal must be from-above and the SAME everywhere this type's layout is
computed"), **A**ttenuation (delegate a narrower authority down). **The money bug is a P failure** —
`StorageMap::get`/`set` independently resolve `K::write_key` (✓ `StorageKey` user-extensible,
`storage_map.fe:8-11`; slot derivation routes through `write_key`); nothing demands they agree. So
S/O alone (provider-side) can't fix it and re-create the foreign-foreign contradiction. **The
construct must give the CONSUMER P, and express S/O/A as cases of P.** The rule rides the dangerous
operation, not a remembered annotation (Move "abilities are structural").

## Three constructs (shapes of mechanism, not policies)
1. **Provision regions** — region-rank qualifier on the `impl` site; seal = a barrier at a closed
   region (reuses the existing `Barrier` primitive ✓ `effect_env.rs:200`/`effects/model.rs:173`).
   "A place you stand." Clean for owned types; contradictory for foreign-foreign layout traits;
   gives no consumer P. → good as a cheap *floor*, not the core.
2. **Provision capabilities** — override needs an unforgeable `OverrideAuthority<I>` grant, threaded
   in `uses`, attenuatable. "A value that flows." Beautiful for S/O/A and **right for CONTROL**
   overrides (mock injection, plugin Display). **Fatal for consensus:** a lexical token guarantees
   the *block*, not whole-contract agreement → catches 4/5 misuses, MISSES the forgotten-override
   (override `set` not `get`) — the money one. Wrong primitive for consensus uniformity.
3. **Use-site provenance demand (RECOMMENDED CORE).** The consumer that does the dangerous op
   demands a property of the resolved witness:
   ```fe
   #[witness_uniform]                       // trait default: every bound is implicitly @uniform
   trait StorageKey { fn write_key(ptr: u256, self) -> u256 }

   fn storagemap_slot<K>(key: K, salt: u256) -> u256
       where K: StorageKey @uniform         // P: witness-provenance qualifier on the bound
   { ... }
   ```
   `@uniform` (permit override, demand agreement) / `@rooted` (from at-or-above a root) / `@canonical`
   (the unique witness) are predicates over the witness's provenance (`ScopeId` + content-addressed
   `TraitInstId`), **checked AFTER the goal eliminates to a concrete `TraitInstId` — solve-line
   invariant preserved, no live `P`.** Catches the money bug **by name at monomorphization**: if an
   app shadows `StorageKey for MyId` in `set`'s scope but not `get`'s, the two occurrences resolve
   different EvidenceIds → `@uniform` fails ("witness for StorageKey<MyId> not uniform: get resolves
   companion, set resolves with-block app::pay"). **S/O/A all fall out of P** (seal = trait defaults
   `@rooted layout`; override = introduce a witness at-or-above the demanded root; attenuation =
   obligation propagation, which effort2 already has). Backend-neutral: the kernel is "witness
   identity uniformity across a computation" — never mentions storage; works for wasm `MemoryLayout`,
   spirv encoding; the root + the marked-trait set live in each backend's std.

## Recommendation
**Construct-3 as the core construct; Construct-1 barrier as the cheap first-ship floor; Construct-2
capabilities reserved for CONTROL (non-consensus) overrides.** Sequence: barrier-floor → provenance-
demand → witness-capture-in-type-identity (the OCaml-generative-functor long-term form; `@uniform`
is its gradual on-ramp). Grounded in: Christoph "authority visible in signatures" (the op announces
its own demand), Sean "dangerous ops not terse" + "kill darlings" (P subsumes 3 mechanisms), Yoshi
(rides `ImplementorId`/`TraitInstId`, no new `TyData` — consistent with D7 projection), Move, ocap.

## Mechanism / construct / deferred-policy split (honors "mechanism not policy")
- **FIXED CORE MECHANISM:** (1) witness provenance on every solution (`TraitGoalSolution.implementor:
  ImplementorId`, `ImplementorOrigin = Hir|VirtualContract(✓ mod.rs:443)|Assumption` ✓ — *add* the
  originating `ScopeId`/scope-chain); (2) an (empty-by-default) provenance-predicate **seam** at the
  discharge chokepoint; (3) the solve-line invariant. Carries DATA + seam, NO policy.
- **LANGUAGE CONSTRUCT (Fe source, not core, not config):** the `@uniform/@rooted/@canonical`
  qualifier + `#[witness_uniform]` default. Compiles to the seam's predicate. Uniform across backends.
- **DEFERRED POLICY (NOT in core):** which traits are layout-canonical (per-backend std, property-
  derived incl `StorageKey`); the coherence root (contract/module/circuit, per-backend std); sealed/
  open default (derived from the property); witness-capture (post-ladder).
- **Crucial:** moving floor→demand→capture changes only the construct/policy, NEVER the core.

## Forward-compat for ProvisionEnv v0 (the ONLY thing touching current work)
v0 = `ProvisionEnv { scope: ScopeId, assumptions } + .solve_cx() (single collapse) + an EMPTY
check_provenance seam`. ~30-45 LOC, debt-negative. Refinements:
- **R1 (mandatory = GAP 1):** retain `ScopeId`, do NOT collapse to `Ingot` (✓ today discarded at
  `trait_resolution/mod.rs:117` — the scope already flows IN, just dropped one line later). Without
  it "from above" has no referent. Cheap now, expensive to retrofit.
- **R2 (new):** thread the migrated `process_trait_obligation` solution through
  `check_provenance(solution, env) -> Result<(), …>` that returns `Ok(())` for everything in v0
  (~5 lines, byte-identical). **Reserves the chokepoint** so `@uniform` later is a body change, not a
  re-plumb.
- **R3:** do NOT fold `effect_env` in v0 (A4 may be undecidable without re-architecting the proof
  forest). Construct-3's provenance lives on `TraitGoalSolution` (witness + scope), NOT `effect_env`
  — so the recommended construct does NOT block on the hard "one resolver" fold. (Happy result.)

## Gut-questions for Micah (deferred; not now-blocking)
1. Rule lives provider / token / **consumer**? (rec consumer — only it catches forgotten-override.)
2. Primary qualifier `@uniform` (permit-override-demand-agreement) vs `@canonical` (the binary you
   rejected)? (rec `@uniform` — honors "sometimes allow overrides.")
3. Coherence root = **deployed contract** (✓ `VirtualContract` anchor)? generalizes to wasm/spirv
   layout-owner in std.
4. Protected set property-derived + name `StorageKey`? (rec yes — enumerated Ord/Hash/ABI misses it.)
5. Capabilities (Construct-2) for the *control* cases — one-mechanism-for-both, or split
   demand(consensus)/token(control)?

## Spikes / FV
- **SPIKE-1 (GATING):** does witness provenance (`ScopeId`+`TraitInstId` EvidenceId) survive
  monomorphization, so `@uniform` is an EvidenceId-equality check at mono? Probe: shadow
  `StorageKey for MyId` in one caller of `StorageMap<MyId>`, confirm two distinct `TraitInstId`s
  observable at the slot site. If it fails, Construct-3 → barrier-floor and witness-capture moves up.
- SPIKE-2: is folding `effect_env` into the proof forest decidable without re-architecting it?
  (Construct-3 does NOT block on it.) FV-1: attenuation/qualifier-composition is monotone (Lean).
  FV-2: `BTreeMap<MyKey>`/`StorageMap<MyKey>` transitive `@uniform`-drag sizing (the A2 cost).
