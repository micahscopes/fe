# Constraint-kind revival plan (K02 — re-port from effort2)

**Scope of K02 (graph node) as of 2026-06-14.** The `Constraint` kind was built
on `metaprogramming-effort2` and never carried into `first-class-obligations`
(parallel branches; port regression — see `FCO_CONSOLIDATION_MAP.md`). This is
the re-port plan. Conclusion up front: **the `Constraint` *kind itself* is cheap
to revive; the *metaprogramming-over-Constraint* layer is a re-integration, not a
cherry-pick** (fco deliberately rebuilt that area, so it is forward work, not a
regression to undo).

## The effort2 commit span (base..metaprogramming-effort2)

Two layers:

**Layer 1 — the kind system (re-portable):**
| commit | date | what | size |
|---|---|---|---|
| `1184fbbf0` | 05-29 | Wire Constraint kind bounds through HIR | 9 files / +136 |
| `e98cec695` | 05-29 | Infer local generic parameter kinds | 3 files / +433 |
| `7caf6343f` | 05-29 | Lower constraint generic arguments by expected kind (incl. `PrimTy` kinds) | 16 files / +442 |
| `f6affe4f3` | 05-30 | Add Derive constraint scaffold | 7 files / +114 |
| `1d009d576` | 05-29 | Update tree-sitter Constraint kind grammar | tree-sitter |

**Layer 2 — metaprogramming as kinded constraints (re-integration, NOT port):**
`9135f5f88` (ImplBuilder provider method hooks), `b096ccb11` (validate evidence
provider signatures), `286f9d14a` (lower compiler `uses` as capability
constraints), `cede0607d` (harden compile-time-only metaprogramming types). These
ran through effort2's `crates/hir/src/analysis/elab/` (`provider_execution.rs`,
`builder.rs`, `capability.rs`, `coherence.rs`) + `proof_forest.rs` — the
architecture fco **deleted**: fco has **no `analysis/elab/`**; provider execution
moved to `core/lower/provider_executor.rs` + the obligation queue, and the
CI gate forbids CTFE in `proof_forest.rs`. So Layer 2 cannot be ported; it must
be re-wired onto fco's substrate (this is K03/K04 = the BR0/BR1 graduation).

## Phasing

### K02a — the `Constraint` kind core (cheap, mostly mechanical) ← do first

1. **Enum + traversal** (`ty_def.rs`): re-add `Kind::Constraint` (and effort2's
   `Placeholder(String)` if K02b is wanted). `1184fbbf0`'s diff applies
   *near-verbatim* — fco's `applicable_kind` match is still
   `Kind::Star | Kind::Abs | Kind::Any`, exactly effort2's pre-image. Adds:
   `does_match` arm, `Display` arm, `Kind::Star | Kind::Constraint => return None`.
2. **Exhaustive-match triage:** adding the variant breaks every exhaustive
   `match Kind`. There are **~34 `Kind::Star` sites across 10 files**
   (`canonical.rs`, `const_ty.rs`, `layout_holes.rs`, `scratch.rs`, `ty_lower.rs`,
   `ty_def.rs`, `effects/match_.rs`, `ty_check/mod.rs`, `ty_check/pat.rs`,
   `core/semantic/mod.rs`). Most want `Constraint` to behave like `Star` (a
   nullary kind) — mechanical; a few (kind application / arrow) need thought.
3. **Grammar:** re-port `KindBoundConstraint` (bare-ident kind), `KindBoundPath`
   (`A<B>` path kind), `WhereConstraintPredicate` from `1184fbbf0`'s
   `parser/{syntax_kind,ast/param,parser/param}.rs` + regen tree-sitter
   (`1d009d576`). fco's parser is +12 lines vs effort2 and diverged a month, so
   this is a *guided re-apply*, not `git cherry-pick`.
4. **HIR lowering:** `core/hir_def/params.rs` + `core/lower/params.rs` +
   `ty_lower.rs` `Constraint` arm.

**Shippable intermediate (resolves BR7 + OD1/OD2):** after step 3, `* ->
Constraint` / `A<B> -> *` **parse**. Lower them either to `Kind::Constraint` /
named-kind, or — if semantics aren't wired yet — to a **named "planned, not yet
fully supported" diagnostic** (never `Kind::Any` accept-and-ignore). That single
increment flips BR7 from AT_RISK and satisfies OD1/OD2, independent of K03/K04.

### K02b — kind inference (`e98cec695`, +433) — decide port-vs-defer

effort2's `Placeholder` variant existed *"until the compiler has real kind
inference,"* and `e98cec695` is that inference. Optional for a first revival:
re-add `Placeholder` + minimal inference, or defer and require explicit kinds.
Decision point, not a blocker for K02a.

### K03 / K04 — traits as `* -> Constraint`, Derive graduation (re-integration)

The `PrimTy` kinds (`Derive : (* -> Constraint) -> Constraint`,
`Evidence`/`ImplBuilder : Constraint -> *`, `ty_def.rs:2035-2055` in `7caf6343f`)
and `ConstraintTerm` were wired through effort2's `analysis/elab` + proof_forest.
On fco these become: register `Derive`/`Evidence`/`ImplBuilder`/`Reflect`/`Field`
as real kinded `PrimTy` builtins (replacing the BR0/BR1 string markers), and
re-wire `core/lower/provider_executor.rs` + the obligation queue to use them. This
is genuinely new integration against fco's substrate — *not* a regression to
undo. Gated (per K-spine) on K07 provider bridge-drift cleanup.

## Recommendation

Land **K02a** as the first PR (Constraint kind enum + match-arm triage + grammar
re-port + tree-sitter regen + the named-diagnostic intermediate). It is the
cheap, high-signal step: it un-regresses BR7, satisfies OD0/OD1/OD2, and makes
`* -> Constraint` a real parsed kind again — turning the north star from "tracked"
into "the kind exists." K02b and K03/K04 follow as separate, larger increments.

Open confirm before starting K02a: whether to re-port `Placeholder` now (couples
K02a to K02b) or add `Constraint` alone and require explicit kinds (smaller).

## Status: K02a LANDED (2026-06-14, commit 804dc959a)

The `Constraint` kind core is re-ported and regression-clean. `* -> Constraint`
parses and lowers to `Kind::Constraint` (`Kind = Star | Constraint | Abs | Any`).
Tests: parser `generic_param_with_constraint_kind_bound`, lowering
`lowers_constraint_kind_bound`; fe-hir lib 116/0, m5 40/0. The `Kind::Star` match
sites needed no triage (they use wildcards) — only the four exhaustive sites
(applicable_kind, lower_kind, lower_hir_kind_local, KindBound::pretty_print) got
a `Constraint` arm.

Remaining (separate increments): **K02b** kind inference (`e98cec695`, +
`Placeholder`); **KindBoundPath** (`A<B> -> *`, a later effort2 commit) — BR7's
path-half; **K03/K04** the kinded `PrimTy` builtins + provider re-integration;
**tree-sitter** grammar (`1d009d576`) — needs the tree-sitter CLI (not hand-edit
`parser.c`). Note: `tree_sitter_parse_strict` is pre-existing-red on this branch
(26 base fixtures, 97.7%), unrelated to K02a (confirmed via stash).
