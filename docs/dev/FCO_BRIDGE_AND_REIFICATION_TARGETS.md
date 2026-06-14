# FCO Bridge + Reification Target Inventory

**Repo-grounded audit, 2026-06-14, branch `first-class-obligations`.** Audit + docs
pass only — no compiler behavior changed. Every status/classification cites a
verified `file:line` read in this repo. This is the companion overlay to:

- `docs/dev/fco_dependency_graph_v0.json` (canonical node graph — node IDs reused here)
- `docs/dev/FCO_CONSOLIDATION_MAP.md` (living map, Bridge Placeholder Audit BR0–BR13)
- `docs/dev/fco_bridge_reification_targets.json` (machine-readable form of this doc)
- `docs/dev/fco_bridge_reification_targets.mmd` (mermaid, colored by `migration_class`)

## The throughline this audit serves

> Replace Rust-style macro magic → typed Fe-authored providers → backed by
> obligations/evidence → generalized into scoped provisions → graduated into
> Constraint kinds / kinded obligations → used for CTCubFe, SMT, Sonatina v2,
> proof transport.

**The distinction the whole pass turns on:** *implementation status ≠
architectural finality.* A target can be `COMPLETE_AND_TESTED` **as a bridge** and
still be a `GRADUATION_TARGET`. "Works" ≠ "final."

Two kinds of non-final:
- A **bridge** = "we built a temporary stand-in because the final abstraction
  doesn't exist yet" (e.g. the `Derive` string marker until Constraint kinds).
- A **reification target** = "we built this in the compiler because Fe couldn't
  express it yet; now Fe is becoming able to" (e.g. hardcoded ABI lowering → Fe
  providers). Not a shim — a candidate for lifting into typed compile-time Fe.

---

## Inventory table

`mc` = migration_class. `bs` = bridge_status. Full per-target detail (why_it_exists,
risk_if_fossilized, repo_evidence, open_questions, recommended_next_action,
owner_decision_needed) is in `fco_bridge_reification_targets.json`.

| id | title | status | mc | bs | graph node | key evidence |
|---|---|---|---|---|---|---|
| TGT-DERIVE-MARKER | Derive compiler-known marker | COMPLETE_AND_TESTED | BRIDGE | GRADUATION_TARGET | BR0/K04 | provider.rs:33,93-110; item.rs:810 |
| TGT-EVIDENCE-IMPLBUILDER-MARKERS | Evidence/ImplBuilder opaque Value markers | COMPLETE_AND_TESTED | BRIDGE | BRIDGE_INTENTIONAL | BR1 | provider_executor.rs:301,385-391 |
| TGT-PROVIDER-AUTHORITY-STRINGKEY | capability recognition: resolved-identity (K04a-C2/C3) + quarantined string-key shim | PARTIAL | PROVISIONIZATION_TARGET | BRIDGE_AT_RISK | BR2/K07/P* | core::derive::{Reflect,ImplBuilder}; provider.rs path_names_derive_capability; std migrated to identity; shim now fixture-only (removal target). body-exempt unchanged |
| TGT-GENERATED-ITEM-VALIDATION | generated-item validation + provenance gap | PARTIAL | PROVIDERIZATION_TARGET | BRIDGE_AT_RISK | BR3/H50/P50 | ty_check.rs:242-289; provider.rs:176-190 |
| TGT-REFLECTION-BINDING | reflection handle-based (method-name strings) | COMPLETE_AND_TESTED | BRIDGE | BRIDGE_INTENTIONAL | BR4 | provider_executor.rs:568-588,1483-1494,2127 |
| TGT-QUOTE-FRAGMENT-LIMITS | quote hygiene / fragment limits | COMPLETE_AND_TESTED | DIAGNOSTIC_GUARD | DIAGNOSTIC_GUARD | BR5 | diagnostics.rs:694,863; derive.rs:129,736 |
| TGT-HARDCODED-ABI-EVENT-ERROR-MSG | ABI/event/error/msg Rust lowering | COMPLETE_AND_TESTED | PROVIDERIZATION_TARGET | BRIDGE_INTENTIONAL | BR6/H* | error.rs:365; msg.rs:443,524; event.rs; eip712.fe:248 |
| TGT-CONSTRAINT-KIND | Kind::Constraint (K02a landed) | COMPLETE_AND_TESTED | NATIVE | FINAL | K02/OD2 | ty_def.rs:1278-1292,904 |
| TGT-TRAITS-AS-STAR-TO-CONSTRAINT | traits as `* -> Constraint` (ConstraintTerm) | ABSENT | BRIDGE | GRADUATION_TARGET | K03 | no TyData::ConstraintTerm; no constraint.rs |
| TGT-EVIDENCE-OVER-REAL-CONSTRAINTS | Evidence&lt;Goal&gt; as kinded application | ABSENT | BRIDGE | GRADUATION_TARGET | K04/OD3 | provider.rs:30-31; provider_executor.rs:385-391 |
| TGT-KIND-ANY-PLANNED-FORM-TRAP | Kind::Any accept-and-ignore hazard | PARTIAL | BRIDGE | BRIDGE_AT_RISK | BR7/K01/OD1 | ty_def.rs:906,1857; params.rs (KindBound=Mono\|Abs) |
| TGT-DIAGNOSTIC-TAXONOMY-DRIFT | code-as-SSOT, doc lag, 8-0086 reserved | PARTIAL | DIAGNOSTIC_GUARD | DIAGNOSTIC_GUARD | BR8/D00 | diagnostics.rs:814,863,207,1074,941; common/diagnostics.rs:211-223 |
| TGT-EVIDENCE-SCHEMA-SEPARATION | schema vs rendering separation | COMPLETE_AND_TESTED | EVIDENCE_CONSUMER | FINAL | BR9/R* | ty_check/mod.rs:3178-3267 |
| TGT-PREMISE-MODEL | premise model (CTFE empty / Assumption populated) | COMPLETE_AND_TESTED | NATIVE | FINAL | BR10/S20 | ty_check/mod.rs:1447,1534-1542,3196-3211 |
| TGT-MERGE-RUNTIME-CLASS | merge_runtime_class MIR patch | COMPLETE_BUT_UNDERTESTED | KERNEL_PRIMITIVE | BRIDGE_INTENTIONAL | BR11/RT* | mir/runtime/lower/infer.rs:1132-1211,1328 |
| TGT-GLOBAL-COHERENCE-CHECKING | global coherence (5-0001) bridge | COMPLETE_AND_TESTED | PROVISIONIZATION_TARGET | BRIDGE_AT_RISK | BR13/PS1 | diagnostics.rs:886; trait_lower.rs:37-40 |
| TGT-CONDITIONAL-BLANKET-PROVISIONS | conditional blanket impls as provisions | PARTIAL | PROVISIONIZATION_TARGET | BRIDGE_AT_RISK | PS2/STCLONE | clone.fe:7-8; trait_def.rs:238-313 |
| TGT-METHOD-RESOLUTION-CANDIDATE-GATING | method gating by provision conditions | PARTIAL | PROVISIONIZATION_TARGET | BRIDGE_AT_RISK | PS3/STCLONE | method_selection.rs (working tree); trait_def.rs:238-313 |
| TGT-SCOPE-TIERS-PROVISION-ENV | scope-tiered ProvisionEnv | ABSENT | PROVISIONIZATION_TARGET | GRADUATION_TARGET | PS1/PV00 | effect_env.rs; trait_def.rs:317 |
| TGT-CANONICAL-ONLY-TRAITS | canonical-only traits safety valve | ABSENT | PROVISIONIZATION_TARGET | GRADUATION_TARGET | PS5 | ops.fe:126 (Ord, unrestricted) |
| TGT-WITNESS-CAPTURE-TYPE-IDENTITY | witness capture in type identity | ABSENT | PROVISIONIZATION_TARGET | GRADUATION_TARGET | PS6 | ty_def.rs (identity witness-blind) |
| TGT-ADT-WF-CONST-PREDICATE-DISCHARGE | ADT/sig/WF discharge | COMPLETE_AND_TESTED | NATIVE | FINAL | W11/W12 | callable.rs:800-803; mod.rs:1447 |
| TGT-ASSUMPTION-ROUTE-EXACT-MATCH | assumption-route exact match + evidence | COMPLETE_AND_TESTED | NATIVE | FINAL | A10/A11 | mod.rs:1534-1542 |
| TGT-IMPL-PREDICATE-GATE-NOT-SELECT | gate-not-select (trait + method-call) | COMPLETE_AND_TESTED | NATIVE | FINAL | I20/I40 | commits 0ebd3be8a, b3de1b218 |
| TGT-METHOD-CONFORMANCE-CONST-PREDICATE | M0 method conformance (6-0016) | COMPLETE_AND_TESTED | NATIVE | FINAL | MC00/BR8 | method_cmp.rs:1050-1086; diagnostics.rs:1074 |
| TGT-RECEIPT-RENDERING | LSP hover reads evidence | COMPLETE_AND_TESTED | EVIDENCE_CONSUMER | FINAL | R10/BR9 | commit 41e4c2db2; mod.rs:3178-3267 |
| TGT-BACKEND-PLATFORM-FACTS | platform facts as assoc-const obligations | COMPLETE_AND_TESTED | NATIVE | FINAL | B20/B50 | mod.rs:1447; platform-fact fixtures |
| TGT-ARITHMETIC-UNCHECKED | unchecked arithmetic vs proof evidence | NOT_EXPRESSIBLE_YET | KERNEL_PRIMITIVE | BRIDGE_INTENTIONAL | S50/S60/RT50 | mod.rs:3196-3211 (reserved premises slot) |
| TGT-CTCUBFE-FORMS | CTCubFe forms 0-6 / law / proof / transport | BLOCKED_BY_DESIGN | REIFICATION_TARGET | GRADUATION_TARGET | C0-C6/S40 | C1 prerequisites landed (consolidation map) |

---

## Verified diagnostic-code map (code is SSOT; codes computed from pass + local_code)

`DiagnosticPass` discriminants (`crates/common/src/diagnostics.rs:211-223`):
`Parse=1, MsgLower=9, EventLower=10, ErrorLower=16, DeriveLower=13,
NameResolution=2, TypeDefinition=3, ImplTraitDefinition=5, TraitSatisfaction=6,
TyCheck=8`. Global code = `<pass>-<local_code:04>`.

| code | meaning | source (verified) |
|---|---|---|
| `8-0085` | const predicate formed, evaluated **false** | `ty/diagnostics.rs:814` (`WhereConstPredicateFailed => 85`, TyCheck) |
| `8-0084` | quote outside provider | `ty/diagnostics.rs:863` (`QuoteOutsideProvider => 84`, TyCheck) |
| `3-0025` | CTFE fault (div-by-zero) | `ty/diagnostics.rs:207` (`ConstEvalDivisionByZero => 25`, TypeDefinition) |
| `2-0002` | name not found / formation | NameResolution `NotFound => 2` |
| `2-0010` | no method named | `name_resolution/diagnostics.rs:180` (`MethodNotFound => 10`) |
| `5-0001` | conflicting trait impl / overlap | `ty/diagnostics.rs:886` (`ConflictTraitImpl => 1`, ImplTraitDefinition) |
| `6-0003` | trait bound not satisfied | `ty/diagnostics.rs:941` (`TraitBoundNotSat => 3`, TraitSatisfaction) |
| `6-0016` | method const-predicate mismatch (M0) | `ty/diagnostics.rs:1074` (`MethodConstPredicateMismatch => 16`, TraitSatisfaction via ImplDiag voucher `diagnostics.rs:4583`) |
| `13-00xx` | provider ran and failed | `core/lower/derive.rs:129,736` (`DeriveErrorKind::ProviderFailed`, DeriveLower=13) |
| `8-0086` | recognized-but-not-yet-expressible | **NO `=> 86` arm exists** — doc-reserved only (today such forms surface as `2-0002`) |

This confirms BR8's "code-as-SSOT, doc lags": `6-0016` and `8-0084` are **live**
(an earlier map called them future); `8-0086` is genuinely unimplemented.

---

## Answers to the 12 audit questions

### 1. Which bridges are intentional and healthy?
- **TGT-EVIDENCE-IMPLBUILDER-MARKERS** (BR1) — opaque markers suffice for the
  executor; graduates cleanly with K04.
- **TGT-REFLECTION-BINDING** (BR4) — already *handle-based* (typed `FieldKey`,
  `Value::Reflect`), only the method **vocabulary** is string-matched, under a
  budget. The architect's "ident-string reflection" framing was wrong; corrected.
- **TGT-QUOTE-FRAGMENT-LIMITS** (BR5) — restricted fragment, named diagnostics
  (`8-0084`, `13-00xx`), 5 snapshot guards. *Expr fragment extended 2026-06-14
  (`757d66b69`): added `||`/`<`/`>` to the prior `&&`/`==` to enable ordered
  std-lib providers (StableOrd).*
- **TGT-HARDCODED-ABI-EVENT-ERROR-MSG** (BR6) — intentional until typed providers
  exist; EIP-712 already proves the Fe-provider path.
- **TGT-MERGE-RUNTIME-CLASS** (BR11) — pragmatic, contained, do-not-rewrite.
- **TGT-ARITHMETIC-UNCHECKED** — the *honest* bridge: a runtime fact with no
  compile-time discharge; replacing it would be unsound.

### 2. Which are at risk of fossilizing?
The `BRIDGE_AT_RISK` set (ranked in the report below):
**TGT-PROVIDER-AUTHORITY-STRINGKEY** (BR2), **TGT-GENERATED-ITEM-VALIDATION**
(BR3), **TGT-GLOBAL-COHERENCE-CHECKING** (BR13), **TGT-KIND-ANY-PLANNED-FORM-TRAP**
(BR7), **TGT-CONDITIONAL-BLANKET-PROVISIONS** (PS2),
**TGT-METHOD-RESOLUTION-CANDIDATE-GATING** (PS3).

### 3. Which old Rust/compiler implementations are providerization targets?
- **TGT-HARDCODED-ABI-EVENT-ERROR-MSG** (BR6 → H10/H20/H30) — ABI/static-layout,
  then event/error/msg; layout consts (`HEAD_SIZE`/`IS_DYNAMIC`) verified by
  const predicates. EIP-712 is the existing template (`eip712.fe:248`).
- **TGT-GENERATED-ITEM-VALIDATION** (BR3 → H50/P50) — generated impls re-enter
  the pipeline already; the gap is body validation + provenance evidence.

### 4. Which old resolution mechanisms are provisionization targets?
- **TGT-PROVIDER-AUTHORITY-STRINGKEY** (string-keyed capability env → P10/P20/P30).
- **TGT-GLOBAL-COHERENCE-CHECKING** (global trait env + 5-0001 checker → companion
  provisions, coherence-by-placement).
- **TGT-CONDITIONAL-BLANKET-PROVISIONS** / **TGT-METHOD-RESOLUTION-CANDIDATE-GATING**
  (blanket guards + method candidate assembly → PS2/PS3).
- **TGT-SCOPE-TIERS-PROVISION-ENV** (the umbrella PS1/PV cluster — the ~6
  pathways collapse here).
- **TGT-CANONICAL-ONLY-TRAITS** / **TGT-WITNESS-CAPTURE-TYPE-IDENTITY** (PS5/PS6).

### 5. Which should stay compiler primitives but get Fe-facing specs/evidence?
- **TGT-MERGE-RUNTIME-CLASS** (KERNEL_PRIMITIVE; gains a resource spec only when
  Sonatina v2 exists).
- **TGT-ARITHMETIC-UNCHECKED** (KERNEL_PRIMITIVE; gains a premises-backed
  check-elision contract via SMT/resource backends).

### 6. Which planned syntaxes need diagnostic guards now?
- **TGT-KIND-ANY-PLANNED-FORM-TRAP**: `A<B> -> *` / `* -> A<B>` (path kind forms)
  do **not** parse and have **no** named rejection. `* -> Constraint` is now
  *supported* (K02a) so it no longer needs a guard. K01 for the path forms is
  **grammar recognition + a named diagnostic** (because they don't parse), not
  mere diagnostic routing. Per steering, do not force `8-0086`.

### 7. Which nodes should block Derive bridge graduation (K04)?
- **TGT-PROVIDER-AUTHORITY-STRINGKEY** (K07/BR2) — **hard prerequisite**: PrimTy-izing
  the capability builtins clashes with string-key recognition unless authority is
  retired first (FCO_K03_K04_EXECUTION_MAP.md, Clash 1).
- **TGT-GENERATED-ITEM-VALIDATION** (BR3) — the body-exemption half of the same drift.
- (K02 is already satisfied — `Kind::Constraint` landed.)

### 8. Which should block scoped ProvisionEnv (PS1)?
None *block* it — but PS1 should be **co-designed with**, not sequenced after,
**TGT-CONDITIONAL-BLANKET-PROVISIONS** and **TGT-METHOD-RESOLUTION-CANDIDATE-GATING**
(the interim gating already landed in the working tree is a partial PS2/PS3) and
must carry **TGT-CANONICAL-ONLY-TRAITS** (PS5) as a safety valve before any local
shadowing ships. The recommendation is to **grow PS1 from the keyed `EffectEnv`**
(`effect_env.rs`), not from effort2's flat envs.

### 9. Which are safe std-lib polish on the current bridge?
- New derive providers authored on the *existing* engine: `StableClone` (now
  graduated to a passing fixture), `StableOrd`, the ABI `Encode`/`Decode`/`AbiSize`
  family. These ride the existing bridge and are independent of K02–K04 and of
  ProvisionEnv. (Source: `FCO_DERIVE_STDLIB_CONTEXT.md` gap list.)
- Richer `quote` fragments (TGT-QUOTE-FRAGMENT-LIMITS) — feature scope, not soundness.

### 10. Which require architect decisions before coding?
- **TGT-PROVIDER-AUTHORITY-STRINGKEY** — capability typing design (P00/P10).
- **TGT-SCOPE-TIERS-PROVISION-ENV** — the ProvisionEnv architecture.
- **TGT-TRAITS-AS-STAR-TO-CONSTRAINT** — K03 representation (thin `ConstraintId`
  vs project over `PredicateListId`).
- **TGT-CANONICAL-ONLY-TRAITS** — canonicity policy (which traits, until when).
- **TGT-ARITHMETIC-UNCHECKED** / SMT trust model (long-term, U70).

### 11. Which failures/pressure tests should be PRESERVED as fixtures (not fixed around)?
- **`docs/dev/repro_stable_clone_blanket_ambiguity.fe`** — keep the quarantined
  repro as documentation of the PS1/PS2 finding even though `derived_clone.fe` now
  passes; it pins *why* the gating exists.
- **`crates/fe/tests/fixtures/fe_test/derived_clone.fe`** — the graduated repro;
  this is the regression guard for method-candidate gating. Keep it.
- **BR3 gap (TGT-GENERATED-ITEM-VALIDATION)** — *add* a
  generated-impl-fails-through-normal-trait-diagnostics fixture (currently NONE):
  it preserves the gap as a pressure test rather than letting it be silently closed.
- **TGT-KIND-ANY-PLANNED-FORM-TRAP** — add a fixture pinning the named rejection of
  `A<B> -> *` (currently NONE).
- **The 5 `quote_*` snapshots** — preserve as the quote-fragment guard.
- **`assumption_route_mismatch_is_rejected`** — keep (no-false-discharge invariant).

### 12. Which graph nodes are stale because the repo has already advanced?
See finding #7 below — this is one of the most valuable outputs.

---

## Required final report

### 1. Target count by `migration_class` (29 targets)

| migration_class | count |
|---|---|
| NATIVE | 7 |
| PROVISIONIZATION_TARGET | 7 |
| BRIDGE | 6 |
| EVIDENCE_CONSUMER | 2 |
| PROVIDERIZATION_TARGET | 2 |
| DIAGNOSTIC_GUARD | 2 |
| KERNEL_PRIMITIVE | 2 |
| REIFICATION_TARGET (umbrella) | 1 |

### 2. Count by `bridge_status`

| bridge_status | count |
|---|---|
| FINAL | 9 |
| GRADUATION_TARGET | 7 |
| BRIDGE_AT_RISK | 6 |
| BRIDGE_INTENTIONAL | 5 |
| DIAGNOSTIC_GUARD | 2 |
| UNKNOWN_NEEDS_AUDIT | 0 |

Count by status: COMPLETE_AND_TESTED ×15, ABSENT ×5, PARTIAL ×5,
COMPLETE_BUT_UNDERTESTED ×2, NOT_EXPRESSIBLE_YET ×1, BLOCKED_BY_DESIGN ×1.

### 3. Top 10 fossilization risks (ranked)

1. **TGT-PROVIDER-AUTHORITY-STRINGKEY** (BR2) — string-keyed authority + a
   permanent type/borrowck hole for provider bodies; **blocks all of K04**.
   Interim escape guard now landed (`provider_authority_outside_provider`).
   **K04a (resolved-type capability recognition) is the architect-selected fix —
   in progress.** *Highest leverage.*
2. **TGT-GLOBAL-COHERENCE-CHECKING** (BR13) — Rust-style global coherence quietly
   becoming the permanent model; the ~6 resolution pathways never collapse.
3. **TGT-GENERATED-ITEM-VALIDATION** (BR3) — body exemption + zero provenance
   evidence; **NONE** guard fixture for the failure direction.
4. **TGT-KIND-ANY-PLANNED-FORM-TRAP** (BR7) — `Kind::Any` silently swallows
   `A<B> -> *` the moment surface syntax is added; **NONE** guard fixture.
5. **TGT-TRAITS-AS-STAR-TO-CONSTRAINT** (K03) — the kinded-application telos
   (K05/K06, CTCubFe) cannot exist without it; largest blast radius, so easy to
   defer indefinitely.
6. **TGT-CONDITIONAL-BLANKET-PROVISIONS** (PS2) — without a general model the
   gating is per-call-site duplicated policy.
7. **TGT-METHOD-RESOLUTION-CANDIDATE-GATING** (PS3) — the interim fix is in the
   *working tree, uncommitted*; risk it stays a point-fix not a ProvisionEnv.
8. **TGT-HARDCODED-ABI-EVENT-ERROR-MSG** (BR6) — two parallel provision paths
   persist; layout facts stay unverified.
9. **TGT-DERIVE-MARKER** (BR0) — string marker pins Derive/Evidence semantics to
   executor behavior, not a kind law.
10. **TGT-DIAGNOSTIC-TAXONOMY-DRIFT** (BR8) — taxonomy lives in snapshots+docs,
    not a registry; continues to drift.

### 4. Top 10 reification / providerization wins

1. **ABI/static-layout provider rewrite** (TGT-HARDCODED-ABI-EVENT-ERROR-MSG →
   H10) — small, contract-relevant, uses assoc-const facts + where predicates,
   produces receipts. The headline M7 win.
2. **Typed provider capabilities** (TGT-PROVIDER-AUTHORITY-STRINGKEY → P20) —
   unlocks #1 and all of K04.
3. **Event/error/msg provider rewrite** (→ H20) — follows ABI.
4. **Generated-impl provenance evidence** (TGT-GENERATED-ITEM-VALIDATION → P50).
5. **Global coherence → companion provisions** (TGT-GLOBAL-COHERENCE-CHECKING →
   PS1) — coherence-by-placement.
6. **Scoped ProvisionEnv** (TGT-SCOPE-TIERS-PROVISION-ENV → PS1) — collapses ~6
   pathways into one scope-indexed env grown from `EffectEnv`.
7. **Derive bridge graduation** (TGT-DERIVE-MARKER → K04) — marker → kinded construct.
8. **Traits as `* -> Constraint`** (TGT-TRAITS-AS-STAR-TO-CONSTRAINT → K03).
9. **EIP-712 enrichment via layout evidence** (→ H30) — already a Fe provider.
10. **std-lib derive providers** (StableOrd, ABI family) on the existing bridge —
    cheapest, demonstrates the engine's reach.

### 5. Nodes safe to do NOW (no architect decision; ride the existing bridge)

- **std-lib polish**: `StableOrd`, ABI `Encode`/`Decode`/`AbiSize` derive providers
  (existing engine). `StableClone` already graduated.
- **Commit the working-tree PS2/PS3 method-candidate gating fix** (it is finished,
  with `derived_clone.fe` as its passing fixture).
- **Add the BR3 guard fixture** (generated-impl-fails-through-normal-trait-diagnostics).
- ~~**Add the BR7 guard fixture** (named rejection of `A<B> -> *`)~~ — **LANDED**
  as a *tripwire* (`crates/uitest/fixtures/ty/def/kind_path_form_trap.fe`): K01 is
  not done, so it pins the current **loud** rejection (`1-0001` parse + `2-0007`
  "expected trait, found type") rather than a named diagnostic. Its value is that
  the trap (silent `Kind::Any` fold) is **not** sprung today; if a grammar change
  ever makes a path-in-kind-position parse and silently become `Kind::Any`, the
  errors vanish and the snapshot fails.
- **W12** (record evidence at WF positions) — small follow-up.
- **fe explain seed (R2)** — consumer extension on the existing schema.
- **Reconcile the taxonomy doc** to the live codes (no code change).

### 6. Nodes blocked by architect decisions

- **TGT-PROVIDER-AUTHORITY-STRINGKEY** (capability typing design = P00/P10).
- **TGT-SCOPE-TIERS-PROVISION-ENV** (ProvisionEnv architecture).
- **TGT-TRAITS-AS-STAR-TO-CONSTRAINT** (K03 representation).
- **TGT-EVIDENCE-OVER-REAL-CONSTRAINTS** (K04a signoff — shares the capability decision).
- **TGT-CANONICAL-ONLY-TRAITS** (canonicity policy).
- **TGT-ARITHMETIC-UNCHECKED** / **TGT-CTCUBFE-FORMS** (SMT/resource trust model;
  per-Form packets — long-term).

### 7. Nodes where the repo CONTRADICTS the old docs (highest-value finding)

These are stale because the repo has advanced. **Reconciling updates applied to
`fco_dependency_graph_v0.json` are listed at the end of this section.**

- **STCLONE / PS3 — the StableClone fix has LANDED (working tree).** The
  consolidation map (and the quarantined repro) describe `p.clone()` as ambiguous.
  In the current working tree it is **fixed**: `method_selection.rs` uses
  `impls_for_ty_with_constraints` with a gate-not-erase fallback (uncommitted diff
  verified), `trait_def.rs:238-313` folds the guard predicate to the concrete goal
  (`Point: Copy`) and drops UnSat candidates, and **`crates/fe/tests/fixtures/fe_test/derived_clone.fe`
  is now a PASSING fixture under the exit-0 glob**. The graph's PS3 note already
  says "PARTIALLY LANDED"; it is now effectively LANDED for the method-resolution
  path (pending commit). *No graph status field needed changing — PS* nodes carry
  no `status` field — but this is the single biggest repo-vs-doc delta.*

- **Method-conformance (M0 / 6-0016) has LANDED.** `FCO_CONSOLIDATION_MAP.md`
  candidate **#3** is labeled `M6_OR_POST` and says `compare_constraints` is
  "trait-only today; an impl method can silently weaken/strengthen const
  predicates with no error." The repo contradicts this: `compare_const_predicates`
  (`method_cmp.rs:1050-1086`) compares by exact normalized-term identity and emits
  `6-0016` (`diagnostics.rs:1074`), landed commit `468bb69b7`. *The map's BR8/MC
  notes already acknowledge `6-0016` is live; candidate #3's table row is the
  stale text.* (Doc-only; no graph node carried a wrong status.)

- **Gate-2 tail (concrete method-call gating) has LANDED.** Earlier scorecards
  call it "quarantined." Commit `0ebd3be8a` wired `gate_concrete_method_selection`
  re-raising the selection as a `GenericConfirmation` obligation through the same
  discharge path. *The map's M5 scorecard already notes this; flagged for
  completeness.*

- **`Kind::Constraint` exists (K02a).** Already reflected in the graph (`K02` note,
  `OD2`, `BR7` "PARTIALLY RESOLVED"). Verified at `ty_def.rs:1278-1292`. No change
  needed — listed so the audit is explicit that the kind, not the graduation, is done.

**Graph fix applied:** `BR2` (`fco_dependency_graph_v0.json`) carried
`"status"`-free but its prose `current_mechanism` cited `analysis/ty/mod.rs:386-389,721`.
The second exemption site is at **`:718-721`** (the `diags` filter), not `:721`
alone — corrected the citation to `:386-389,718-721` for accuracy. No status field
on BR* nodes was wrong; the PS/STCLONE/MC deltas above are doc-text staleness in
`FCO_CONSOLIDATION_MAP.md` (candidate #3 row), not graph-node status errors, so
per the brief ("only where the repo plainly contradicts an existing node's
**status**") the graph JSON needed only the citation correction. The `.mmd` is
regenerated/disposable and was left to the existing generator.

### 8. Nodes that should become fixtures

| target | fixture action | exists? |
|---|---|---|
| TGT-METHOD-RESOLUTION-CANDIDATE-GATING | keep `derived_clone.fe` (graduated PASS) | yes (working tree) |
| TGT-CONDITIONAL-BLANKET-PROVISIONS | keep `repro_stable_clone_blanket_ambiguity.fe` as doc-repro | yes |
| TGT-GENERATED-ITEM-VALIDATION | **ADD** generated-impl-fails-through-normal-trait-diagnostics | **NONE** |
| TGT-KIND-ANY-PLANNED-FORM-TRAP | tripwire pinning loud rejection of path-in-kind-position | yes (`kind_path_form_trap`; tripwire, not named-rejection — K01 not done) |
| TGT-QUOTE-FRAGMENT-LIMITS | keep 5 `quote_*` snapshots | yes |
| TGT-ASSUMPTION-ROUTE-EXACT-MATCH | keep `assumption_route_mismatch_is_rejected` | yes |
| TGT-PROVIDER-AUTHORITY-STRINGKEY | interim authority/escape guard | yes (`provider_authority_outside_provider`) |

### 9. Recommended next implementation node

**Commit the already-landed PS2/PS3 method-candidate gating fix** (it is finished
in the working tree with `derived_clone.fe` as its passing regression fixture),
**then add the BR3 guard fixture**
(generated-impl-fails-through-normal-trait-diagnostics).

Rationale: the gating fix is the cheapest *real* provisionization advance, is
already written and tested, closes the StableClone finding, and needs no architect
decision. The BR3 fixture is a zero-design hardening that preserves a real gap as a
pressure test. Both are safe, immediate, and on the conventional end of the arc the
steer asks us to prioritize. (The std-lib `StableOrd`/ABI providers are an equally
safe parallel track if a second implementor is free.)

### 10. Recommended next architect-decision packet

**Typed provider capabilities (P00/P10) — the K07/BR2 prerequisite for K04.**

This single decision unblocks the most: it is the hard prerequisite for the entire
Derive-bridge graduation (K04), for retiring the string-keyed authority and the
provider-body type/borrowck exemption (BR2/BR3), and for the ABI providerization
win (H10). The packet should answer: the typed shape of a capability obligation
(grade/key/scope), whether provider bodies are typed in full or signature-only, and
the provenance-evidence schema for generated impls. Per `FCO_K03_K04_EXECUTION_MAP.md`,
PrimTy-izing `Reflect`/`Evidence`/`ImplBuilder` clashes with string-key recognition
unless this is resolved first — so it gates the whole K-spine graduation, not just
one node.

(The *second* packet, once capabilities are decided, is the **scoped ProvisionEnv
(PS1)** grown from `EffectEnv`, carrying the PS5 canonical-only safety valve.)

---

## Steering compliance notes

- **StableClone** classified exactly as steered: provider-bridge pressure test =
  **PASS**; the issue was conditional-blanket / method-resolution candidate gating
  (PS1/PS2/PS3), **not** std-lib polish, **not** a Clone special-case. The
  `Copy`-blanket is not weakened. Verified the implementor's fix is in the working
  tree (`impls_for_ty_with_constraints` + gate-not-erase fallback) and the repro
  has graduated to a passing fixture.
- **K02/K03/K04 split** preserved: `Kind::Constraint` (K02a, `804dc959a`) =
  LANDED/NATIVE; traits-as-`*->Constraint` (K03) = ABSENT; `Evidence` over real
  constraints (K04) = ABSENT; Derive graduation = ABSENT/gated on K07. K02a is
  **not** claimed as bridge graduation.
- **K01 / kind diagnostics**: for the path forms (`A<B> -> *`) K01 is framed as
  **grammar recognition + named diagnostic** (they don't parse), not just routing;
  no recommendation to implement before parity/grammar behavior are known (both now
  established).
- **8-0086**: not forced; current `2-0002` named/no-ICE behavior left as the
  acceptable default until the taxonomy decides suppress-vs-wrap.
- **No git commit performed.** Files written to disk only; the implementor reviews
  and commits. The only graph edit was a citation accuracy fix (BR2 `:718-721`).
