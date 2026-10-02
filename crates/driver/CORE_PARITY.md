# Core semantic parity reconstruction

Starting checkpoint: `f5edde55f`. The active program implements all four core
chunks recommended in the September 21 review, with multibackend restoration
deferred. Libraries supply observable acceptance cases, not a separate library
restoration campaign. The review packet and raw assessment are preserved under
`/workspace/scratch/mb2-mb3-expressiveness-2026-09-21`.

## Completion boundaries

1. Selected evidence through ordinary runtime calls and generic helpers, including
   named alternatives, associated outputs, nested restoration, inherited/default
   calls, complete applicable requirements and canonical establishment protection.
2. Pure computed structural types and generic associated families, including
   substitution, defaults, bounds, confluence and a scan executable at two sizes.
3. General checked generic/enum derive authoring with complete signatures,
   requirements, binding and hygienic quotation or equivalent expressiveness.
4. Authenticated structural reflection, configured providers, generated associated
   types, structural folds, enum parsing and composed elaboration where needed.

Valid reviewed programs rejected by a bounded implementation remain parity debt.
Syntax changes must preserve meaning through generic and generated clients.
Historical defects and incomplete experiments are recorded separately from
intended capabilities. Whole-program positive/negative cases, ordinary CLI
execution, incremental checks, costs and clean extractable commits close each
chunk; test counts or a model alone do not.

## Current requirement and generation limits

Ground `where` conditions run by CTFE. A symbolic condition is forwarded only
when the caller has the same condition after substitution: `N > 1` does not
currently establish `N > 0`. Conditions on ordinary generic functions,
records, enums, trait methods, and generic inherent `impl` blocks have checked
consumers. Trait-method calls substitute the trait's parameters and can forward
the identical condition from a generic caller. Trait-declaration conditions
use their declared bounds, are checked at impl headers and concrete uses, and
can be forwarded from a matching generic premise. A trait-impl method may
repeat its trait method's condition after parameter rebasing, but cannot add
an unpromised condition. Inherent impl-header conditions are checked at method
calls, associated-constant expressions, and constant patterns, including
before direct CTFE. A ground condition on an item nested in a generic item is
accepted; a condition that captures the outer generic parameter still gets
an explicit 8-0092 limit diagnostic.

The mb2 bare const-where fixture now passes type checking, and its pretty-print
fixture preserves bare comparisons and the braced multiline form.

Trait-impl conditions now leave proof search as obligations and are checked
after inference, including before CTFE executes an affected body. Trait and
const obligations share the checker's deferred queue during inference, and
function and trait-impl call conditions share one post-inference discharge
collection. Type-position checks and symbolic nested-item capture still use
separate handling. These are limits of the current checker, not general
logical claims about const predicates.

The `Constraint` kind marker resolves to the designated core item after name
lookup. A local trait with the same spelling remains an ordinary trait bound;
an imported alias of the core marker retains the kind meaning. This corrects
a spelling-based bug also present in mb2. Other names used in kind positions
receive a diagnostic rather than silently becoming an unconstrained kind.

## Historical generated descriptor checkpoint

The following records the pre-quotation carrier. The current output is checked
`Code<Item>` quotation; the `BodyExpr` descriptor graph and its decoder have
been removed. The provider entry remains nongeneric.

`#[generate]` requires a nongeneric provider entry, but the target record may
have generic parameters and requirements. Its generated functions have a
bounded expression vocabulary with `Add` as the only
arithmetic operator, and generated impls carry at most one `own self` method.
The descriptors cannot spell general generic types or complete method
signatures. Explicit `ingot`, `core` and `std` paths in generated declarations
bind to the provider or built-in ingot before target checking, so target-local
aliases cannot capture them. General free-name capture remains unimplemented.
Generated functions, function arrays, and trait impls now enter the output
scope graph as HIR, including declarations on const-generic records. Function
and impl identity are stable across an unrelated sibling insertion. Function
identity also survives a provider output edit that preserves the site and
ordinal. The const-generic fixture exercises the generated argument bodies and
owner bounds.

Generated associated constants can now use the same bounded `BodyExpr` language
as generated functions. The ABI fixture emits `HEAD_SIZE` as a sum of calls to
the field codecs, and a generic-record fixture checks that different field
types produce different sizes. Calls still require explicit rooted paths, so
this does not yet establish general lexical capture or quotation. Providers
also see only authored inputs in a stage, so one generated record codec cannot
be queried while generating another in that same stage. That needs a defined
composed stage. Quotation is
planned after the `HirBuilder` carrier, to author these checked descriptions
from Fe syntax rather than growing `core::meta` into another evaluator.

## Selected-output investigation

The first control program uses one subject and two explicit trait arguments:
`Produce<Number>` returns `u64`, while `Produce<Flag>` returns `bool`. Evidence
passes through two ordinary const helpers whose output parameter is constrained
by the same complete goal. This executes on the existing compiler when using
unqualified method dispatch and an explicitly qualified associated return type.

This control establishes only that explicit output equalities can carry these
payloads. The trait argument is part of the goal, so it does not demonstrate two
competing implementations for the same goal. It is not the proposed replacement
for named selection, runtime helper transport or a general dependent package.

Using the qualified call `<T as Produce<M>>::produce()` inside the opening
exposed a lost lexical premise: the checker queued an obligation, then solved it
after restoring the outer assumptions. Commit `160b47555` retains use-site
assumptions on each obligation, including across requeueing, and restores the
surrounding checker context after processing it. The regression checks both the
qualified call through two helpers and rejection of a subsequent out-of-scope
call. All 37 focused public/scoped/const evidence tests pass, including a warm
edit to the invalid scope and restoration. The log is
`/workspace/scratch/mb3-selected-output-probe/scoped-regressions.log`.
This is a checked prerequisite, not a completed chunk; the full chunk gate
remains due after runtime selection integration.

The selected-application boundary is `semantic/instance/template.rs::ImplEnv`.
It now retains the chosen implementation, full goal and proving context in
instance identity. Consumption checks the original context and requested goal;
matching declared requirements carry the choice through generic helpers.
Unrelated functions do not inherit ambient dispatch choices. Semantic and runtime
method resolution consume the same checked application. Stable symbol identity
includes the selection without changing the empty-selection spelling.

Historical canonical policy is declaration-owned: current mb2's `is_single_impl`
reads `#[fixed]` on the trait. Its preceding comments retain an older hardcoded
policy description. Reconstruction must use live behavior and recovered intent,
not reintroduce the superseded name allowlist from those comments.

Named declaration reconstruction admits `impl Trait for Type as Name`, retaining
the name through parsing, HIR, formatting and method-symbol identity. Explicit
candidate proving can consume it; ordinary trait solving and method lookup must
exclude it even when no anonymous implementation exists. Different alternatives
can expose different associated outputs. Each selected application must establish
its own prerequisites. Conflicting anonymous implementations, conflicting copies
of the same name, and all conflicting implementations of `#[fixed]` traits remain
errors. Core ABI traits and the standard storage-key trait retain that attribute.
The attribute checker recognizes `#[fixed]` only on traits and diagnoses malformed,
duplicate, or misplaced uses. The broader mb2 authority layer (`PermitAuthority<G>`,
`ImplPermit<T>`, and `impl_permit()`) is still absent, so this attribute does not
provide a public permission mechanism for controlled implementation.

Source-level `with(Name)` now opens concrete named applications. Historical
aliases occupy a separate fallback
namespace: mb2's `cascade_with_alias_value_precision.fe` requires an ordinary
value binding to win over an equally spelled alias. Opening preserves that rule,
proves the complete selected application and retains its identity through helpers.
Selection markers are erased before lowering runtime values. Nested scopes restore
the previous choice. Deferred obligations retain their selected applications as
well as their premises, so ground selected goals do not weaken ordinary proving.

Named implementations require explicit selection, even when there is only one
applicable implementation. Micah confirmed this rule on 2026-09-22: callers
write `with (Only)` to use `impl Trait for Type as Only`. This follows mb2's
written design, although its live `TraitSolveCx::default_tier_selection` can
fall back to a sole named implementation. Code relying on that fallback must
add an explicit selection or leave the implementation unnamed. Elision can be
considered later without changing the meaning of existing explicit selections.
Generated default tiers and prerequisite-aware applicability remain separate
compatibility work.

The qualified goal form `with (<Subject as Compute>)` now explicitly selects
the sole matching unnamed implementation. It can override a named choice in an
inner scope, then restore the outer choice. A generic goal with a known impl
checks; a goal with only named implementations is rejected. Both forms use the
same checked selected-application record. The derived default versus
hand-written override case awaits the generation work.

The declaration regressions pass alongside all public evidence tests (48 tests),
including prerequisite edits and restoration. Formatting round-trips the new
syntax. Logs are `named-regressions.log` and `named-format.log` in the probe
directory below. These checks do not close the runtime-selection chunk.
Strict workspace clippy (`--workspace --all-targets --all-features -- -D warnings`)
passes; `named-clippy-ci.log` records it. The Makefile's additional print-output
lint rejects an existing validator example (`named-clippy.log`).

The source/runtime acceptance programs are
`tests/fixtures/selected_helpers.fe` and `tests/fixtures/selected_outputs.fe`.
The former transports receiver calls and arguments through two generic helpers,
checks nested restoration and keeps unrelated calls on ordinary dispatch. The
latter uses one subject and one trait with distinct `u256` and `bool` outputs,
passing them through the same two generic helpers and returning the typed payload.
It uses an explicit output parameter, not a replacement nominal subject or an
algorithm specialized separately for each output. Fe test and UI fixtures now
exercise restoration, wrong selected outputs, escape under the default
projection, missing independent requirements, duplicate named alternatives and
both associated-equality premise orders. The retired Rust-only selected-method
evaluator and its API-specific edit tests were removed. Scoped evidence and
public evidence tests retain warm/fresh edit coverage for the language path.
That Rust API could request a generic named implementation directly; Fe-level
`with (Name)` still rejects such a request unless the implementation application
is concrete. This remains an expressiveness gap, not a parity claim.
`crates/fe/tests/selected_runtime.rs` checks and builds ordinary non-const versions
and executes them at O0 and O2 through the CLI.

This is a vertical slice within chunk 1. Named opening currently requires a
concrete implementation application and concrete associated bindings. Generic
named families, remaining method requirement forms,
public runtime evidence values, and the historical default-tier compatibility
cases remain work in this same chunk. The public value-based evidence opener is
still const-only. No completion claim for chunk 1 or the four-chunk goal follows
from these fixtures.

Verification for the concrete lexical/runtime slice: 67 focused evidence and
selection tests pass, as do formatting, news validation and strict workspace
clippy. The exact full command was:

```
cargo nextest run --release --workspace --all-features --no-fail-fast --locked --exclude fe-language-server --exclude fe-bench
```

It ran 3,037 tests: 3,035 passed, including the O0/O2 runtime regression, and two
diagnostic snapshots failed solely because the three earlier `#[fixed]` ABI
annotations moved source locations by three lines. After reviewing and updating
only those locations, both affected tests pass using the same release test
binary. `lexical-nextest.log`, `lexical-snapshot-verified.log`,
`lexical-regressions.log` and `lexical-clippy.log` retain the receipts. This is a
full run plus targeted snapshot revalidation, not a claim of a second full run.

The historical `cascade_greeting_dialects.fe` and `cascade_nested_shadowing.fe`
programs also execute successfully with this compiler at O0 and O2. Only comment
lines were removed from the scratch copies; their declarations, calls and
assertions are unchanged. `historical-runtime.log` records all four runs.

An O0 emitted-IR probe checks specialization growth: one selected call produces
8 functions and 2,044 text bytes; 32 calls with the same selection still produce
8 functions (6,002 bytes); 32 calls alternating two selections produce 11
functions (8,972 bytes). The extra three are the second selected method and its
two generic helpers. Repeated call sites therefore share instances in this
workload. Text size includes symbol names and repeated caller instructions; this
is not a runtime-speed or general compilation-cost claim. Sources, IR and
`selection-specialization-cost.json` retain the measurements.

At `a2ff74586`, two follow-on probes discriminated remaining chunk 1 work. In
`inherited_runtime.fe`, a `Child: Parent` requirement loses the selected `Parent`
application across two helpers, so a default method returned the ordinary result.
The control `default_runtime.fe`, using direct `Parent` bounds, passes. Retention
needed to account for implied requirements using the existing bound-expansion service.
`projection_runtime.fe` replaces the explicit output parameter with
`<T as Compute>::Output`; the selected Boolean client was rejected.

The follow-on implementation shares one callable requirement calculation between
checking and specialization. It retains choices needed by transitive parent
bounds and a default method's enclosing trait. The inherited fixture exercises
two helper layers, nested restoration, defaults, independent parent/child choices
and warm/fresh body edits. Selected associated bindings also enter instance
normalization, and matching treats selected evidence as the more specific side.
The output fixture now includes ordinary projection-returning helpers.

Negative cases require checking a child's implied requirements against selected
parents: a Boolean parent cannot satisfy a numeric parent-output requirement.
Proving a selected parent also must not establish a missing child implementation.

Lexical applicability is fixed after inference, before generic substitution. This
preserves the mb2 distinction in `ty_check/mod.rs::discharge_from_scoped_provision`
and `semantic/instance/const_ref.rs::scoped_provision_implementors`: only choices
matching the checked goal were committed. A concrete `Subject` choice inside a
generic body does not retroactively select an unrelated rigid `T` after
specialization. Caller-supplied selection through that body's declared `T` bound
still applies. The output fixture tests both paths and their different types.

The same lexical rule applies inside a bounded helper. A caller's selection
reaches method calls and constants through the helper's own type parameters
and bounds (`T::compute`, `T::ID`). A concrete path such as
`Subject::compute()` or `Subject::ID` keeps the ordinary implementation even
when the selected goal becomes equal after substituting `T = Subject`.
Thus `with (Flag) { bounded<Subject>() }` evaluates the concrete call plus one
to 43, and a differing selected associated output cannot make call lowering
panic. This is a deliberate change from mb2's probable by-goal inheritance.

A related ordinary-language correction compares all trait arguments when using
contextual associated equalities. Previously, bounds for `Family<u256>` and
`Family<bool>` on the same subject could supply each other's output types. The
regression checks both premise orders and warm/fresh invalid edits and restoration.
The implementation keeps the existing canonical scratch-query boundary.

This follow-on checkpoint passes all 71 focused evidence/selection tests,
including ordinary CLI execution at O0/O2, strict workspace clippy, formatting
and news validation. The exact full gate above passes all 3,040 tests with none
skipped (`selection-applicability-nextest.log`, 655.062 seconds of test execution).
Focused and clippy receipts are `selection-applicability-regressions.log` and
`selection-applicability-clippy.log`. The direct CLI counterexamples now diagnose
the inherited output conflict and the wrong trait argument; the rigid-parameter
escape control executes its original numeric result without a lowering panic.

The ordinary associated-equality fix is an independent extraction boundary. The
selected inherited-call/output change follows the named-selection commits. These
are verified increments within the full four-part program, not closure of its
remaining admission and fixture-coverage work.

Probe sources and logs: `/workspace/scratch/mb3-selected-output-probe`.

## Projection substitution prerequisite

The computed-family tranche starts with a correction to ordinary substitution.
When a projection's trait arguments changed, two specialized binder paths copied
its associated bindings unchanged. They now use the shared type traversal, which
substitutes both. The regression checks nested bindings, scoped and unscoped
substitution, and another declaration's parameter sharing the same numeric index.
It fails before the correction and passes afterward.

This independent fix passes all 94 HIR unit tests, strict workspace clippy,
formatting, news validation and the exact full release gate (3,041 passed, zero
skipped, 577.281 seconds). Receipts are in
`/workspace/scratch/mb3-computed-types/binder-{before,after,clippy,nextest}.log`.
The gate compiled this correction before the subsequent family scope work began;
it is not validation of that unfinished implementation.

## Generic associated families

A trait can now describe an associated type with its own parameters, such as
`type Pair<T> = (T, bool)`. Implementations can inherit that default or override
it. An ordinary generic function can return `F::Pair<T>` and call a trait method
that constructs that result. The CLI execution regression in
`crates/fe/tests/selected_runtime.rs` checks defaults, overrides, generic result
forwarding and nested families through check, build and O0/O2 execution.

Each family parameter belongs to its declaring associated member. Substitution
retains enclosing arguments separately, including arguments absent from the
result body. Nested families therefore cannot capture another member's parameter
merely because it has the same numeric index. An mb2 comparison exposed exactly
that capture defect; the mb3 regression preserves the intended `(u256, bool)`
result instead of mb2's erroneous `(bool, bool)`.

Declared parameter requirements are checked at formation sites and made available
while checking the corresponding body. Explicit call arguments retain their
requirements until inference finishes, even if normalization erases the parameter
from the result. Defaults are checked without needing an output bound or caller;
inherited defaults are also validated with the concrete implementation context.
Output-bound declarations are checked for resolvable names and satisfied trait
parameter requirements, using the family's declared premises. Promised family
output bounds require completed proofs. Guarded partial families
and binder-erasing associated equality bindings are diagnosed.

Normalization errors are reported at definitions instead of allowing invalid
types to suppress every diagnostic. Cycle recovery preserves originating errors
through applications and family construction. Tests cover direct, growing,
mutual and inherited recursion, valid overrides, finite nesting, and reused versus
fresh databases across edits and restoration. These checks do not prohibit all
compiler query cycles.

Declaration validation also catches previously ignored output bounds on ordinary
associated types. An earlier check wrongly rejected the mainline recursive
trait-header fixture and added `Item = Self::Item` to make it pass. The original
fixture is restored. When an associated output promises to implement its own
trait, the trait's parameter constraints that mention `Self` are deferred until
an implementation supplies the output. Constraints independent of `Self` still
must hold at the declaration. A separate negative fixture checks that an
unrelated missing premise is not deferred. Implementations still check their
concrete associated output against its declared bound.

This is the associated-family portion of chunk 2. Pure computed type functions,
their confluence witnesses and the scan programs remain outstanding. This family
increment passes all 3,072 tests in the exact full release gate (zero skipped,
601.110 seconds), strict workspace Clippy, formatting and news validation. The
final historical recheck accepts 12 and rejects 19 standalone cases as intended,
without parser failures or panics. The CLI witness checks execution at O0/O2;
HIR regressions cover query reuse, edits and restoration. Receipts are retained
in `/workspace/scratch/mb3-computed-types/family-extraction-receipt.json` and
`family-verified-full-gate.log` in the same directory.

A controlled comparison of
ordinary trait proofs, without associated families, found that both the pre-family
compiler at `a90fe7f36` and this implementation exceeded an eight-second limit for
a recursively duplicated `(T, T)` argument. Both rejected the singly nested
control in about 3.8 seconds. This identifies a pre-existing resource limitation,
not a general complexity guarantee. Logs and detailed reproduction notes are
retained in
`/workspace/scratch/mb3-computed-types/HANDOFF.md`.


## Computed structural types

A recursive type function can compute a type from ordinary type arguments and a
final `usize` constant. For example:

```fe
struct Pair<A, B> {}
recursive type fn Tree<T, const N: usize>() -> (*) {
    match N {
        0 => T
        _ => Pair<T, Tree<T, {N - 1}>>
    }
}
fn unwrap(x: Tree<bool, 2>) -> Pair<bool, Pair<bool, bool>> { x }
```

The result is an ordinary compiler type used in signatures and trait selection.
This does not require generated source or a separate driver stage. Ordinary const
functions can compute integer payloads inside the resulting types. The declaration
checker validates every arm, checks that recursive subjects decrease, and retains
invariant arguments with their declaration owners. It uses ordinary type checking,
scoped substitution and const evaluation.

The scan fixture `crates/fe/tests/fixtures/fe_test/computed_scan.fe` uses one generic
algorithm at four and eight leaves. It checks every exclusive prefix against a
flat running accumulator, as well as totals. Normal CLI checking, compilation and
O0/O2 execution pass. These are sequential EVM executions. Explicit and default
associated-family confluence examples from mb2 also pass unchanged.

Formation checks preserve declared requirements before expansion erases an
argument. `Tree<Bad, N>` cannot bypass a declared `Bad: Marker` requirement merely
because the computed result omits `Bad`. Both concrete and generic missing-premise
cases have regressions, with edits, restoration and fresh-database comparison.
Another regression prevents an alias from replacing a checked decreasing call
with a nondecreasing one while preserving the number of recursive occurrences.
The corresponding missing-premise and alias examples were accepted by the mb2
binary used for comparison; their acceptance is not a compatibility requirement.

Qualified projection bodies are preserved, including generic callers and changes
to an implementation's associated result. Equivalent `T::Out` and
`<T as Item>::Out` spellings behave alike. The historical compiler accepted the
qualified version but rejected the bare spelling. This does not establish all
possible selected-evidence or symbolic normalization contexts.

Generic trait arguments no longer hide the implementing block's own associated
type equations during method conformance. A generic `impl<A, P: T<A>> T<A> for
Wrap<P>` now checks and executes when its output depends on `P::Out`; the
category-encoding fixtures also execute a composed value, reverse gradient,
and compile-time cost interpretation. A method with a genuinely wrong return
type still reports 6-0008. `Self::Dom` in a subtrait resolves through declared
supertrait premises, while two inherited `Dom` candidates remain ambiguous.
Nested qualified type arguments such as `Wrapped<<T as Model>::Point>` parse
without changing left-shift expressions. Two `16-0004` borrow-checking
reproductions also fail on mainline and remain outside the passing fixture gate.

Computed applications are not admitted in trait implementation headers. The
check sees deferred types, including aliases, nested applications and trait
arguments. This preserves mb2's explicit separation between computed types and
implementation identity. An executable probe demonstrated why the boundary is
needed: two headers spelling `Tree<1>` and its expanded `Pair<bool, bool>` passed
source checking but caused runtime dispatch to panic. The structural type remains
a valid implementation target. This restriction does not prohibit computed types
in associated outputs or function signatures.

Generic stored fields normalize when their enclosing type is instantiated.
The CLI regression passes one generic constructor through both scalar and nested
computed field shapes, with O0/O2 execution. This improves on mb2's coarse
stored-position restriction without establishing every symbolic layout position.
Ground computed inherent-method targets also retain their existing behavior:
the expanded type finds the method, and defining it again on the expanded type
reports a conflict. The trait-header restriction does not apply to inherent impls.

Normalization now uses one root-local reducer for computed-type unfolding,
associated families, and projections. Its cache records each result and depth
cost under the const-evaluation policy, so a cached deep result cannot bypass
the caller's remaining depth. Work is capped at 65,536 visited nodes per
active reduction. Independent sibling requests start fresh budgets. The old
separate tracked computed-result query and duplicate computed-only limit cause
are gone; 3-0050 now names the shared type-normalization limit. Exhaustion
reports an error rather than establishing an arbitrary result type.
Computed functions require all arguments, preserving the distinction between
constant slots and ordinary higher-kinded type parameters. A depth-4,095 phantom
`Node` chain checks with the rebuilt release compiler; the depth-4,096
counterpart reports the named shared limit in a UI fixture. A depth-1,024
chain also checks in debug mode on the default Rust stack after type-depth and
well-formedness checking became iterative. The limits do not promise
interactive performance or bound total compiler work across all roots. Earlier
measurements and source/binary identities are in
`/workspace/scratch/mb3-computed-types/depth-cost/`.

Const payloads may call ordinary const functions. CTFE has step and call-depth
limits, but those limits alone do not contain compiler query cycles: a payload
can call `core::size_of<Tree<0>>()` while `Tree` is being validated, which asks
the normalizer for the same definition. Definition validation now detects that
re-entry before unfolding and reports an invalid recursive type function at the
definition. A paired fixture permits a payload to inspect a different computed
type. The check is keyed by database and definition and scoped to the current
query thread. The single reducer has no normalization-value cycle recovery;
definition checking retains a named-error cycle fallback. The containment and
worklist contract is recorded in `docs/architecture/type-reduction.md`. This
guard does not establish every possible CTFE dependency path on its own.

Symbolic membership induction now checks the historical generic `Reduce`
example: a whole shape family follows from its component premise without
repeating a bound on each computed shape. It reuses checked arm templates,
scoped substitution and strict ordinary trait solving. Every arm must prove the
same unary trait, with a fresh rigid recursive hypothesis in a step arm. This
establishes membership, not associated bindings or a selected implementation.
The admitted class has a symbolic size, type invariants and at most one recursive
call per arm. Checked symbolic size expressions such as `N - 1` support
formation of recursive arms with constrained constructors. Syntax, kind and
decreasing-recursion checks precede induction; arm formation and membership
require complete solver answers. Size-dependent payloads cannot borrow a premise
about one size to prove smaller sizes. General method use still requires its
ordinary explicit bound; ground dispatch resolves the actual implementation.

Both left and right shape families pass warm/fresh edits and restoration.
Missing component, constructor and base premises, diagonal-only constructors,
and size-dependent premises are rejected. A CLI consumer checks, builds and
executes at O0/O2 for two sizes with independent scalar expectations. This
follow-on passed 19 focused tests, the CLI consumer, strict workspace Clippy,
and the exact full workspace gate: 3,104 tests passed, none skipped
(766.256 seconds). Historical comparisons are
recorded in `/workspace/scratch/mb3-computed-types/induction-history/`.

A ground cross-check now covers sizes 0 through 8: each symbolic
`RPow<Pair, N>` admission is paired with a separately written nested `Comp`
alias and its leaf count. Checked symbolic const computations may supply the
induction subject when their term still contains parameters; a source
occurrence alone does not make a ground or deferred term symbolic. The CLI
execution and formatting round trip passed before the term-only guard was
tightened; the focused induction acceptance and rejection tests passed on the
refined guard. The final milestone gate remains due.

This computed-type increment passed the exact full workspace nextest gate:
3,097 tests passed, none skipped (704.191 seconds). Strict workspace Clippy,
focused regressions, formatting and news validation also passed.
It does not complete chunk 2 or the remaining derive and reflection chunks.

## Impl const conditions in proof results

Trait proof search now selects an implementation from its header and trait
premises, then returns every selected implementation's const conditions as
residual checks. A false condition or a fault does not make a structurally
matching implementation disappear. The checker reports the failed condition at
the use, with the authored predicate as the secondary location. This is a
deliberate semantic correction from the earlier mb3 solver hook: for
`Wrapped<0>: Positive` with `where N > 0`, structural search can succeed while
the use is rejected because the condition is false.

The current use-site paths cover trait method calls, generic trait obligations,
associated outputs in type positions, and selected evidence, including conditions
from nested prerequisite proofs. The target-search cache now keys on the origin
ingot rather than the caller's lexical scope; condition discharge retains the
caller's scope and premises. The query-order and edit fixtures check that the
caller's premises do not leak between uses. RD74 permits pure, fuel-bounded type
normalization during matching, including computed types and const-generic
arithmetic. A normalization fault or exhausted budget must be reported as an
error, never converted to a candidate non-match. Impl const-where conditions
remain residual obligations, evaluated by the checker after proof search.
This narrows RD60's original blanket evaluation ban. The CI boundary scans
the solver entry points and condition-discharge calls across type analysis,
while allowing normalization and shared unification. It rejects the old solver
hook and an indirect discharge helper. Matching-fault regressions require an
error rather than a non-match or fallback selection: an associated type that
exhausts the reduction budget, a ground const division by zero in an associated
type, and a faulting impl header next to a viable alternative.

Residual transport now includes trait-declaration conditions, and the checker
uses one live deferred queue for trait and const obligations. PR 6 still needs
the remaining symbolic nested-item condition decision and the milestone-wide
gate. The RD74 boundary and matching-fault regressions are in place.

## Associated constant evaluation

Ordinary associated constants are checked at declaration for type errors and
calls that cannot run in a const context. Their values are executed only when
a use demands them. Thus an unused value that would exhaust the CTFE step
budget is accepted, while an unused call to an ordinary function is rejected.
Reference cycles are checked from resolved const dependencies without running
the values. Compiler-generated associated constants that must materialize
remain eagerly evaluated.

This follows mb2's lazy execution for unused values while retaining mainline's
declaration-time const-language check. It deliberately differs from the eager
fault reporting introduced by mainline PR #1556 (RD77).
When a demanded ordinary associated value fails during CTFE, the diagnostic
marks the associated-constant use and points to the failing expression in its
body. The CTFE callee trace supplies the use, so unrelated const diagnostics
keep their existing locations.
Within a selected impl's associated-constant body, a reference written as
`Self::N` keeps that impl's selection after `Self` becomes concrete. A reference
written as `Subject::N` uses ordinary lookup, matching the lexical selection
rule for concrete paths.
