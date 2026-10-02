# Public first-class obligations: bounded Fe evidence flow

Starting checkpoint: mb3 `cdd344592`. The packed physical-layout milestone is
complete. The user has prioritized public FCO functionality ahead of multibackend
restoration. The intent record is `f8aab2e47`. This milestone implements the
bounded public flow described below; it does not restore all mb2 FCO facilities.

## Delivered functionality

Ordinary Fe const functions can obtain `Evidence<Produce<Subject, Output = u64>>`
with `prove`, forward that value, and explicitly open it in another function:

```fe
const fn consume<T, Output>(_ evidence: Evidence<Produce<T, Output = Output>>) -> Output {
    with (evidence) { T::produce() }
}
```

`consume` needs no separate `T: Produce` bound: its evidence argument supplies
that premise inside the opening. Dispatch checks and uses the retained selected
implementation. The output type is bound by the same complete goal and checked
again against the computed result. The
[executable fixture](../fe/tests/fixtures/fe_test/public_obligations.fe) obtains
and forwards evidence, computes a `u64` result, and exposes that result through
an ordinary contract. It requires no custom Rust orchestration or generation CLI.

Introduction currently requires a concrete resolved goal and ordinary unique
lookup. Opening is compile-time only and supports explicit safe nullary const
implementation methods. Scoped competing-implementation introduction, implicit
selection transport and broader method shapes remain followups. This is a
bounded admission policy, not a decision to abandon mb2's scoped selection.

The original acceptance contract and intermediate checkpoints follow. The final
verification receipt is at the end.

## User-facing outcome

Ordinary authored Fe must be able to obtain checked evidence for a trait goal,
forward it to another Fe function, and explicitly consume it there. Consumption
must establish the supported premise without repeating it as a separate caller
requirement. Associated types and method calls must remain tied to the selected
implementation. A returned payload whose type depends on that selection must
retain the dependency or be checked as an independently established concrete type.

The acceptance example will use a producer trait with an associated output type,
a checked introduction, a forwarding function and a consumer. Distinguishable
selections or subjects must not be silently interchangeable. Exact surface syntax
is not frozen yet. Passing evidence as a compile-time value does not by itself
require a runtime dictionary or existential runtime representation.

## Recovered intent and its limits

This is a focused skim of existing history evidence and current source, not a
complete session-history audit. Original external transcripts are not newly read.
The external evidence file preserves their locators and surrounding context.

- External-history evidence X002 (May 21 user message, original JSONL line 343)
  explicitly requests first-class authoring and capability-gated reflection and
  builders. Derive convenience alone is not the intended destination.
- External-history C013-C016 (June 9 scope/effects discussion) motivates scoped
  implementations and a common discharge interface. The report distinguishes
  that intention from a single literal solver/backend implementation.
- `mb2/ingots/core/src/derive.fe:35-52` documents `Evidence<G>` as a
  Constraint-kinded, compiler-bound compile-time witness. Its private field and
  absent constructor prevent ordinary runtime construction. Preserve that
  distinction before proposing a broader evidence value.
- Review report 01, F2/F3, identifies head-only candidate matching and loss of
  selected provenance as defects. Neither defect is a design choice to preserve.
- Reports 25 and 30 recommend retaining the complete goal, selected application,
  instantiated arguments and relevant context. Explicit consumption must not
  discard selection and perform an unconstrained lookup again.
- Report 30's coherent-only starting slice and scoped-opening recommendation are
  later bounded recommendations. They are not evidence that mb2 abandoned scoped
  non-default selection. Its Lean model supports witness/payload dependency, not
  correctness of a compiler implementation or a ratified parser spelling.

History sources live under `/workspace/mb2/docs/critical-review-2026-09-10`,
including `external-history/REPORT.md` and `external-history/EVIDENCE.jsonl`.
Current mb2 source lives at `/workspace/fe-worktrees/mb2`; the evidence repository
under `/workspace/mb2` is not that source checkout.

Before: completed metaprogramming consumers exposed useful checked subsets but
left public obligation introduction and consumption unfinished. Pivot: the user
prioritized full FCO over multibackend work and accepted an introduction,
forwarding, consumption and dependent-result milestone. Consequence: this chunk
is public language integration, not another Rust-driver-only experiment.
Multibackend remains deferred, not abandoned. Missing historical records do not
authorize changing language meaning.

## Reuse and decisions to resolve first

Inspect existing keyed trait-effect witnesses, selected-implementation proof and
method resolution, and scoped const-provider transport together. Reuse their
semantic carriers where they satisfy the contract; a marker wrapper alone does
not open a premise, and a provider object alone is not selected-impl evidence.

Determine and document:

1. Which source operation introduces the witness and proves all prerequisites.
2. How its type represents the complete goal and its selected application.
3. How explicit consumption installs the premise while preserving selection.
4. How associated outputs remain linked through forwarding and return.
5. Which first-slice restrictions are implementation limits, especially around
   scoped competing implementations, rather than final coherence decisions.

Do not invent a global overlap policy to finish the first example. If its public
meaning requires a previously unsettled rule, recover the relevant mb2 rationale
and state the design choice explicitly before implementation.

## Acceptance and completion

- A normal Fe source example checks, builds and executes, with applicable O0/O2
  coverage. No custom Rust orchestration is needed to use its public capability.
- Invalid introduction, missing prerequisites, wrong goal/type arguments,
  mismatched selected associated outputs and unsupported witness escape reject.
  An unused or untaken malformed source path cannot manufacture evidence.
- Forwarding and consumption retain selection; incomplete search or evaluation
  failure never become successful proof. Method-specific requirements remain
  separately checked.
- Definition, supporting-impl, associated-type and provider/context edits and
  restoration match fresh compilation. Existing evidence, scoped-provider,
  generation, ABI/accessor and physical-layout regressions remain preserved.
- Continue semantic modelling where it answers a concrete opening/transport
  question. Existing model results are not implementation proof.
- Keep reviewable local commit boundaries, record costs and limitations, and run
  focused checks plus fmt, news validation, strict clippy and the exact full mb3
  nextest DONE gate once the implementation is ready.

Automatic discovery, unrestricted quotation, general effectful builders,
cross-stage evidence serialization and backend restoration are followups unless
recovered evidence establishes a necessary dependency. Do not substitute only a
new internal handle, repeated marker-wrapper probes or an inventory for the
public introduction-to-consumption flow.

## Source recovery and implementation map

The mb2 executable fixture
`crates/fe/tests/fixtures/fe_test/cascade_greeting_dialects.fe` supplies a concrete
scoped-selection contract: an ordinary default `Greeting for User` returns 1,
`impl Greeting for User as Casual` returns 2 inside `with (Casual)`, the selection
travels through `welcome<T: Greeting>`, and leaving the scope restores the default.
This is historical source evidence, not a newly executed certification of mb2.
It gives scoped non-default selection a concrete target beyond a vague ambition.

Current mb3 has three distinct reuse candidates:

| Component | Existing responsibility | Missing public connection |
| --- | --- | --- |
| `trait_def.rs::SelectedImplEvidence` | Checked selected application, complete goal, associated bindings, destination revalidation | Not a Fe value; borrows one immutable database |
| `effects::EffectWitness` and `ty_check/effect_env.rs` | Keyed witness matching and lexical transport | A provider witness does not automatically establish an arbitrary trait premise |
| `ty_check/env.rs::seed_func_effect_witnesses` and `expr.rs::check_with` | Function-boundary forwarding and scoped installation of effect providers | Evidence opening must preserve proof and selection, not merely insert a provider |

Current `ty_def.rs::Kind` contains Star, Abs and Any, but no Constraint kind.
Consequently the historical `Evidence<G: Constraint>` surface needs a real
constraint-term representation before public evidence can use that spelling.
That prerequisite must remain separate from proof introduction: a well-formed
trait goal is not proof that the goal holds. Introducing a type named Evidence
and inferring its promise from its spelling would repeat the old authority bug.

The next implementation decision is how a constraint term and selected witness
are represented through ordinary source checking and specialization. Keep the
historical `Evidence<Goal>` form as the surface candidate. Inspect mb2's term
lowering and mb3's existing solver identities before adding a second trait-goal
representation. None of these observations licenses treating ordinary coherence
as the final scoped-selection policy, or claiming the public flow implemented.

## Constraint-term implementation checkpoint

The first source foundation is implemented: `Constraint` is a distinct kind,
and concrete `Trait<Subject, Args..., Associated = Type>` applications can fill
constraint-kinded positions. These terms reuse `TraitInstId`; normal trait
reference lowering handles arguments and defaults. Goal formation supplies no
implementation proof. Associated-binding names, multiplicity and kinds are
checked before a term is admitted. Goal-term equality preserves the complete
binding set, rather than using the solver's directional requirement matching.

Seven focused source tests pass, covering kind separation, goal formation without
an implementation, inability to discharge a bound using a goal description,
subject identity through forwarding, binding-set equality in both directions,
non-value-kind rejection, visibility and malformed bindings. The log is
`/workspace/scratch/mb3-constraint-terms-tests.log` (1.16 seconds test execution).
Formatting and release-note validation pass. Full milestone gates remain due.
This is not public evidence: proof introduction and explicit opening remain to
be implemented. Unsaturated trait constructors and general constraint families
are not admitted by this concrete-term lowering path.

## Evidence-introduction implementation checkpoint

`core::obligations::prove<G>()` now introduces an opaque `Evidence<G>` value
for a concrete, resolved goal through ordinary unique implementation lookup.
Source checking and CTFE share `introduce_obligation`, which reuses the selected
implementation proof boundary. The resulting CTFE handle retains the complete
goal, selected implementation, caller scope and proving assumptions. A generic
ordinary Fe const function can forward the value without losing that identity.
The core declaration's private field prevents source construction; the actual
CTFE value is a compiler-issued handle rather than a record containing a marker.

The focused public-obligations suite now has twelve tests. In addition to the
seven constraint tests, it covers introduction and forwarding, source forgery,
missing implementations and prerequisites, mismatched associated bindings,
unused and untaken introduction calls, and warm/fresh supporting-impl and
associated-output edits with restoration. Results are recorded in
`/workspace/scratch/mb3-evidence-introduction-tests.log`.

This checkpoint does not establish premises by consuming evidence. Generic
introduction, scoped non-default introduction, explicit opening, dependent
payload consumption and normal executable acceptance remain unfinished. Ground
ordinary lookup is the current introduction operation's admission rule, not a
final coherence policy. No full milestone gate has been run for this checkpoint.

For opening, the concrete integration boundary is
`semantic/instance/const_ref.rs::semantic_callee_key_with_assumptions`: it currently
resolves trait methods through ordinary lookup. `ImplEnv::witnesses` stores trait
goals, not selected implementations. Opening must carry selected provenance
through this boundary and lexical scope, rather than merely add a goal to the
assumption list and rediscover a possibly different implementation.

## Explicit-opening implementation

The implementation uses `with (evidence) { ... }` in const functions.
It recognizes the core evidence declaration by identity, adds the complete goal
only for that lexical scope, and retains the source witness on trait method
calls. Semantic lowering defers implementation selection for these calls.
CTFE checks the opaque handle, exact goal and selected proof in its retained
context, revalidates at the destination, and uses the existing checked selected
nullary const-method path. It checks the returned value against the instantiated
result type. Runtime lowering rejects an evidence-dependent call that has not
been evaluated at compile time.

The source test now obtains evidence, forwards it through a generic Fe function,
opens it in `consume<T, Output>`, and returns an `Output` whose binding is carried
by that same goal. Eighteen focused tests pass, including lexical premise escape,
missing method requirements, source marker impersonation, unsupported implicit
generic-bound forwarding and inherited-trait dispatch. The log is
`/workspace/scratch/mb3-evidence-opening-tests.log` (2.44 seconds test execution).

First-slice limits remain explicit: safe nullary const methods with explicit
implementation bodies (trait default bodies are excluded), no method-local
generics, const predicates or effects; no runtime opening; no implicit selected
evidence transport through generic bounds or inherited trait selection. Pass
evidence explicitly to another function and open it there. These are admission
limits, not a replacement for mb2's scoped-selection intent.

`crates/fe/tests/fixtures/fe_test/public_obligations.fe` and
`cli_output::public_obligations_check_build_and_execute` are the normal CLI
acceptance example, including a contract returning the computed typed result
and O0/O2 execution. Normal CLI check, contract build and execution at both levels
passed in 4.74 seconds (`/workspace/scratch/mb3-public-obligations-cli.log`).
The expanded focused run passed 62 tests: 21 public-obligation cases plus the
existing selected-method, evidence-boundary, scoped-provider and const-provider
suites (`/workspace/scratch/mb3-obligations-focused-regressions.log`). It includes
warm/fresh body, prerequisite, output and subject edits with restoration, nested
openings, and rejection of an internally substituted implementation even when
ordinary lookup for the retained goal would succeed. The further consumer-module
move/restoration test passed in the full gate, which includes all 22 public-
obligation tests.

The existing `models/EvidenceOpening.lean` model under the critical-review record
was rechecked successfully. Four theorems use no axioms; the goal-erasure
counterexample uses standard `propext` and `Quot.sound`. This supports the
selection/payload contract, not compiler refinement. The actual selection-
substitution regression separately checks that consumption does not ignore its
handle and repeat ordinary lookup.

Implementation costs are explicit: source admission and CTFE introduction both
check a ground goal; opening revalidates the retained proof and separately checks
the selected method. Dispatch executes within the existing CTFE machine, keeping
its recursion and step accounting. The source and semantic call representations
carry an optional witness reference and full goal. No runtime dictionary or new
trait solver is introduced. Scoped competing-implementation introduction and
selection through further generic/inherited calls remain followups, not an
ordinary-coherence language policy.

## Verification receipt

The exact full mb3 gate passed: **3,024 tests passed, zero skipped**, with test
execution taking 1,215.054 seconds. Four test threads were used. This is a test
run duration, not a compiler or runtime performance comparison.

```sh
cargo nextest run --release --workspace --all-features --no-fail-fast --locked --exclude fe-language-server --exclude fe-bench
```

The log is `/workspace/scratch/mb3-public-obligations-full-gate.log`. The run
includes normal CLI check/build/O0/O2 execution, formatting semantic round trips,
all 22 public-obligation cases, and earlier applicable generation, selected
evidence, const-condition, layout, effect, borrow-checking and codegen regressions.
No snapshot updates were required.

Strict workspace clippy uses the installed Rust 1.97.1 toolchain, with all
targets/features and `-D warnings -D clippy::all`; its log is
`/workspace/scratch/mb3-public-obligations-clippy.log`. Formatting and release-note
validation also pass. The model log is
`/workspace/scratch/mb3-evidence-opening-model.log`.

Builds use sccache at `/workspace/.sccache`, `CARGO_INCREMENTAL=0`, two build jobs,
and disk-backed build/scratch directories. No runtime witness serialization,
automatic discovery, general builders or backend restoration is included.
Multibackend restoration remains the subsequent priority.
