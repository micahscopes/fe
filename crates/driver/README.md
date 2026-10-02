# Source-invoked structural generation

Current `#[generate]` providers return checked `Code<Item>` quotes. The compiler
freezes the checked quote and builds ordinary HIR in an output view. The
accessor and ABI examples in `crates/fe/tests/fixtures/structural_generation`
use this path. The old `FunctionDefinition` and `RecordImplementation`
descriptor graph is no longer accepted. See the
[current carrier](GENERATION_CARRIER.md).

The compiler-internal `elaboration::read_back` API freezes an already checked
body on the same carrier. Direct and method calls retain their selected owner, and record
construction and local annotations retain their checked nominal and tuple
types across destination shadowing. Its driver fixtures compare binding and
type meaning after re-emission, including a forged record root that must fail
owner validation. Ordinary method checking owns receiver adjustment and
selection; quotation checks the retained scope and selected call record.
Explicit free-function type arguments retain their checked primitive, tuple and
nominal types through the same signature carrier. Conflicting destination aliases
cannot change their meaning, and a forged nominal argument fails owner validation.
Method-local const parameters map to destination generic binders by index, so
renaming or reversing the destination's parameter names does not swap their uses.
Qualified constants with a primitive receiver retain the checked trait identity
and lookup scope, including a trait private to the source module. This currently
admits traits without additional arguments. Output checks consume ordinary
checking's const and generic records rather than resolving those paths again.
The active read-back acceptance fixtures also compile the copied output through
Sonatina and execute it at O0 and O2. They cover arithmetic, bindings, calls,
records, methods and structured loops, with runtime inputs and conflicting
destination names. Fe execution adapters live beside the authored fixtures;
the structural and execution tests share the same frozen-body transplant.
The compiler-internal `read_back_value` bridge supplies that same checked body
as `Code<Expr>` to ordinary Fe const functions. `core::meta::code_root` returns
a sealed, read-only `CodeNode` cursor. Fe can walk its children, inspect its
operation, compare checked bindings and selected calls, and read literal,
field and array facts. A Fe-authored visitor counts multiplications and loop
forms in the acceptance fixture; the compiler does not calculate those counts
or evaluate the described program. Compound assignments expose their actual
arithmetic operation, and array repeats expose their checked length.
Const parameters and associated constants have distinct cursor kinds; neither
is reported as a function reference.

The cursor refers to existing HIR and ordinary checking results. It stores no
copied syntax, grants no function-body discovery, and cannot be spliced or
published as generated output. The whole `Code<Expr>` value still passes through
Fe and freezes through the existing quotation path. Invalid cursors, child
indices and incompatible queries fail explicitly. Type comparison currently
uses the existing scalar reflection contract (`bool`, `u64`, `u256`), including
aliases; incomplete types and other requested types are unsupported. Empty and
untyped quote templates are not checked-body inputs. Edit-and-restore tests
compare warm and fresh compiler databases for changed operations, literal
values and call selections.

Symbolic annotations, the complete quoted-output corpus and match integration
remain unfinished Q5/Q6 work.

The next section records the original September 21 structural release, before
quotation replaced its output format. Its descriptor limits and examples are
historical; later sections describe subsequent work.

The [generated HIR carrier](GENERATION_CARRIER.md) records the staging boundary
that replaced source insertion.

The reflected-trait extension is described in [REFLECTED_TRAIT_QUERIES.md](REFLECTED_TRAIT_QUERIES.md).
It replaces the ABI provider's primitive dispatch with ordinary `FieldQuery<R>`
families and admits concrete nominal field codecs, while preserving the original
primitive and accessor acceptance outcomes. The historical measurements below
remain the first structural milestone's baseline.


The [release contract and evidence map](STRUCTURAL_GENERATION.md) defines the
fixed applications, supported semantics, exclusions, and completion gate.

`#[generate(provider_path)]` on a concrete, nongeneric record invokes an
ordinary Fe const provider during normal `fe check`, `fe build`, and `fe test`.
The provider receives an owner-bound `core::meta::Record` and returns a
`FunctionDefinition`, a bounded array of optional functions, or a
`RecordImplementation`. Reflection
exposes actual fields and their types; construction checks descriptor authority
before ordinary checking of the generated declarations and their consumers.

Preparation evaluates explicit invocations in the selected ingot and its
dependencies. All providers observe authored inputs, then all declarations are
inserted into an isolated compilation view. Authored files remain unchanged.
Unrelated loaded workspace members are not evaluated.

The accessor example is
`crates/fe/tests/fixtures/structural_generation/accessors.fe`. It uses one Fe
generator with two explicitly scoped selection policies to produce typed getters
for private fields. CLI tests cover project and standalone checking, building,
O0/O2 execution, and rejection of invalid output. Driver tests cover owner
authority, actual heterogeneous field types, edits, and dependency scoping.

`crates/fe/tests/fixtures/structural_generation/abi.fe` uses the same reflection,
type, function, and call syntax to construct `AbiSize` and `Encode<Sol>` impls
for empty, single-field, heterogeneous, and reordered records. Fe computes head
sizes and offsets with `abi_record_layout` and emits calls to existing field
codecs. `IS_DYNAMIC` is emitted as its own associated constant. Raw payload
lengths and complete 256-bit words are compared with fixed expected values,
without decoding them or deriving expectations from generated offsets.

This is a bounded construction surface, currently admitting primitive field
types `bool`, `u64`, and `u256`. Function calls carry up to two type arguments
and four optionally labeled arguments referencing earlier body nodes. Paths
and labels are limited to Fe's 31-byte inline strings. Named
types and calls use explicit `ingot`, `core`, or `std` paths, resolved in the
owner's output scope. These are source syntax, not captured provider bindings
or transported evidence. Ordinary checking enforces visibility and complete
callee requirements even when the emitted function is unused. Raw expression
strings are not admitted.

A record implementation carries its owner's handle, an explicit trait path,
up to two scalar associated constants, and one optional method. The method's
first parameter must carry the same record handle and becomes `own self`.
Ordinary checking validates member signatures, trait requirements and coherence.
`BodyExpr::End` marks unused array capacity; every following slot must also be
`End`, and no reference or result can use those slots. This lets one bounded
generator produce bodies whose output grows with the actual field count.

The ABI fixture admits at most 32 fields of the supported primitive types.
Dynamic records, recursive records, and generic record constructors are excluded.
The fixed public milestone has passed its full validation and release gate;
the linked release contract records the tested checkpoint and results.
Automatic discovery, general reflected equality, and unrestricted evidence
transport are not provided by this interface.

Current limits are 8 parameters, 512 expression slots, 64 optional functions,
and the pre-evaluation allocation bound in `elaboration/project.rs`. Each
function, function batch, or implementation is limited to 64 KiB of generated
source; a prepared project is limited to 1 MiB. Normal CTFE limits apply per
invocation: 1,000,000 steps and recursion depth 64. Tests distinguish step and
recursion exhaustion from successful generation and from a false predicate.

The fixed ABI scaling test prepares fresh databases and runs ordinary output
checking. Its September 18 run recorded the following added source sizes and
preparation times. Times include contention from concurrently running tests;
these are observations, not stable performance thresholds or backend codegen
benchmarks. Every encoder keeps one record receiver and one pointer argument.

| Fields | Added source bytes | Preparation ms |
| --- | ---: | ---: |
| 0 | 270 | 1761 |
| 1 | 552 | 1253 |
| 3 | 1121 | 962 |
| 8 | 2566 | 954 |
| 32 | 9548 | 1031 |

The focused suite also removes and restores the existing `u64` codec and checks
edited versus fresh databases. An unused generated encoder is rejected while
the codec is absent. The CLI suite executes all four fixed payload oracles at
O0/O2 for both project and standalone inputs, alongside the accessor application.

# Core const requirements and evidence

## Ground const where predicates

Ordinary compilation now checks boolean `where` conditions on declarations
without in-scope generic parameters:

```fe
const LIMIT: u256 = 8
const fn allowed(_ size: u256) -> bool { size < LIMIT }

fn operation() where allowed(3), !false {}
```

Every condition is checked at its declaration, even if nobody uses the item.
Only evaluation to `true` succeeds. Type errors, nonconst operations, failed
execution, recursion, and exhausted CTFE limits are errors. Conditions enter
ordinary semantic borrow and layout checking as anonymous const bodies.
They neither add solver assumptions nor filter impl candidates.

Type/trait predicates keep their existing syntax and meaning. Parenthesize a
block condition, as in `where ({ ... })`, to distinguish it from the item's
body. Generic functions, records, enums and inherent methods support the
parameter-dependent requirements described below. Generic trait and impl
conditions remain unsupported, including a trait's implicit `Self` scope.

Generated target templates retain these conditions and receive the same checks.
The existing provider and frozen-export protocols continue to exclude root
requirements, including ground const predicates.


## Generic function const requirements

Top-level generic functions can retain parameter-dependent boolean conditions:

```fe
const fn bounded<const N: usize>() -> u256 where N > 0 { 42 }

const fn forward<const COUNT: usize>() -> u256 where COUNT > 0 {
    bounded<COUNT>()
}

const fn answer() -> u256 { forward<1>() }
```

The declarations check the predicates' types and const-language operations.
Each function use discharges its requirements after inference. `bounded<0>()`
is rejected even though its body does not use `N`. Function values and calls in
constant initializers, array lengths and other anonymous constant bodies use
the same check. A ground condition is still checked on an unused generic
function; parameter-dependent conditions are obligations for its callers.

Concrete conditions use ordinary CTFE and its execution limits. Generic
forwarding currently accepts an identical resolved, typed expression after
scoped substitution. Parameter spelling is irrelevant; declaration identities,
parameter positions, operation identities and arithmetic mode matter. Supported
symbolic expressions are literals, const paths, unary/binary operations, casts
and ordinary const calls. This is exact forwarding, not algebraic implication:
`N > 1` does not automatically establish `N > 0`. Blocks and control flow can
be evaluated with concrete arguments but cannot yet be forwarded symbolically.
Anonymous constants in an ordinary function's signature or executable body can
forward that function's checked conditions. Predicate formation cannot use any
of the enclosing function's conditions, including from constants nested inside
a predicate. This rule is independent of clause order. Nested declarations do
not inherit function premises. Inherent methods follow the same rules as
described below; generic trait methods remain unsupported. Recursive
requirements cannot establish themselves.

For example, a constrained helper can compute an array length in a signature:

```fe
const fn empty_length<const N: usize>() -> usize where N > 0 { 0 }
const fn consume<const COUNT: usize>(
    _ values: [u8; { empty_length<COUNT>() }]
) -> u256 where COUNT > 0 { 42 }
const fn answer() -> u256 { consume<1>([]) }
```

The matching condition permits the signature's helper call. `consume<0>([])`
still fails, even though the helper returns zero for every input. This extends
requirement forwarding, not the supported symbolic array evaluation language.

Generated target templates support the same generic declarations and ordinary
callers. Providers and selected frozen exports keep their existing root
restrictions. This does not add automatic provider discovery or a generic
cross-stage export protocol.

Inference templates and requirement discharge have separate queries so CTFE
can consume completed inference while checking a condition. Raw inference,
semantic templates and CTFE results are not certificates that a program meets
its requirements. Use normal compiler diagnostics, as generation does, before
admitting a program to lowering. The implementation is shared frontend logic;
no backend has a separate const-requirement evaluator.

## Const requirements on generic records

Ordinary generic records can declare boolean const requirements. Every concrete
use must satisfy them, including construction, unused type annotations,
defaults, aliases, fields and trait arguments. Generic uses currently need an
exact matching premise after substitution, just like generic function calls.

```fe
struct Bounded<const N: usize> where N > 0 { value: u256 }

const fn make<const COUNT: usize>() -> Bounded<COUNT> where COUNT > 0 {
    Bounded { value: 42 }
}
const fn answer() -> u256 {
    let item = make<1>()
    item.value
}
```

`Bounded<0>` is rejected even when its value is never constructed. Mentioning
`Bounded<N>` does not implicitly establish `N > 0`: a generic function using
that type must state the matching requirement. A containing record can state
and forward its own requirement into its fields. Predicate formation cannot
assume the record's conditions, including in nested anonymous constants.

Declared type positions and inferred body types use the shared frontend
requirement checker. Inferred types are checked after inference. The generated
consumer tests use ordinary record declarations retained in the source template;
this does not introduce a record-generation descriptor or a new builder API.
Generic trait and impl predicates remain unsupported. Symbolic entailment
remains exact forwarding, and the bounded Lean model does not constitute a
refinement proof of the compiler.

Records with const requirements must currently be fully applied when used as
types. Passing an unapplied constrained constructor through a higher-kinded
parameter is rejected: this increment has no protocol for retaining its
conditions under future applications. Fully applied aliases work; generic
aliases cannot silently acquire or discard the target record's obligations.


## Const requirements on generic enums

Generic enums use the same requirement contract as records. Conditions apply
to the whole type, including unit variants and unused type positions:

```fe
enum Choice<const N: usize> where N > 0 {
    Empty,
    Value(u256),
}
const fn read<const M: usize>(_ item: Choice<M>) -> u256 where M > 0 {
    match item {
        Choice::Empty => 42,
        Choice::Value(value) => value,
    }
}
const fn answer() -> u256 { read(Choice<1>::Value(42)) }
```

Constructing `Choice<0>::Empty` fails despite having no payload. Tuple-variant
constructors must be called directly; storing or passing one as a value receives
an unsupported-use diagnostic.
Matching a valid enum does not grant new const premises: generic consumers
state their conditions explicitly.

All variant payload types must be valid, including unselected variants.
Payloads can forward the enum's explicit conditions into nested constrained
records, enums and anonymous array lengths. Predicate formation cannot use
those conditions to justify itself. Ordinary trait bounds remain available
when forming type-parameter predicates such as `T::ALLOWED`.

As with records, constrained enums must be fully applied; higher-kinded
transport of their conditions remains unsupported. Generated consumer templates
follow the same rules, without adding an enum-generation descriptor. These
checks share the existing frontend discharge queries and do not change
runtime representation or backend lowering.


## Const requirements on inherent methods

Inherent methods and associated functions can require conditions involving
both inherited impl parameters and their own generic parameters:

```fe
struct Window<const N: usize> { value: u256 }
impl<const N: usize> Window<N> {
    const fn take<const M: usize>(self) -> u256 where N > 0, M > N {
        self.value
    }
}
const fn answer() -> u256 { Window<1> { value: 42 }.take<2>() }
```

The checker discharges requirements on the ordinarily resolved method.
Conditions do not select methods, resolve ambiguity, or provide fallback.
Receiver calls and qualified calls use the same requirements, even when a
method body does not use its generic parameters. Exact explicit premises can
be forwarded between free functions and methods, including within signatures.
Predicate formation cannot borrow those premises.

Inherited parameters retain their identity through the method's leading generic
slots; explicit method parameters follow them. Requirement checking reconciles
lexical impl-owned references with method-owned parameters before applying call
arguments. This is a local checking view, not a runtime ABI transformation.

Generic impl-level conditions and generic trait-method conditions remain
unsupported.

## Scoped trait witnesses in ordinary Fe

An authored runtime consumer can already use a supplied trait witness through
the ordinary effect interface:

```fe
fn consume<T>() -> T uses (evidence: Compute<Item = T>) {
    evidence.compute()
}
```

For a trait `Compute` with an associated `Item` and a receiver method returning
it, a caller introduces a concrete provider in a scope:

```fe
with (Compute<Item = u256> = Number { value: 42 }) {
    assert!(consume<u256>() == 42)
}
```

The complete runnable example is
[`scoped_trait_evidence.fe`](../fe/tests/fixtures/fe_test/scoped_trait_evidence.fe).
From the repository root, run:

```sh
cargo run --release --bin fe -- test crates/fe/tests/fixtures/fe_test/scoped_trait_evidence.fe
```

It exercises different associated outputs, forwarding, nested provider
selection and restoration, and provider values through control flow. The
example's providers implement `Copy` for repeated consumption. Removing that
permission does not bypass ordinary move checking.

The `uses` declaration supplies the consumer's trait requirement for its
hidden provider parameter; an ordinary marker struct does not do that.
Association mismatches, missing providers and calls after a provider scope ends
are rejected. Provider-key edits are checked against fresh databases in
`tests/scoped_trait_evidence.rs`.

This is runtime trait-effect witness passing under ordinary coherence. Selecting
a provider value is not selecting an overlapping impl for the same self type.
It is not yet a first-class arbitrary constraint value, a portable selected-impl
certificate or dependent existential package. The const extension below admits
a bounded immutable subset.

## Immutable trait providers during const evaluation

Ordinary const functions can now consume immutable trait providers through the
same `uses` and `with` syntax used at runtime:

```fe
trait Compute: Copy {
    const fn compute(self) -> u256
}
const fn consume() -> u256 uses (evidence: Compute) {
    evidence.compute()
}
```

A caller supplies a concrete provider value with `with (Compute = value)`.
The provider type and trait arguments determine ordinary coherent dispatch.
Generic forwarding, associated-output constraints and nested provider scopes
retain their normal meaning.

The first admitted transport is immutable, direct, by-value trait arguments.
Mutable effect slots, type-keyed storage providers, external address spaces,
external effectful functions and deferred symbolic effectful calls are rejected.
`with` binding expressions and bodies receive const checking even when untaken.
Ordinary ownership checks still apply; repeated consumption requires suitable
ownership permissions such as `Copy`.

This is scoped provider-value transport under ordinary coherence. It does not
convert a `SelectedImplEvidence` handle into a provider, admit alternative impls
for the same complete goal, establish missing premises from arbitrary structs,
or grant reflection/builder capabilities or cross-stage evidence transport.
See `tests/const_trait_evidence.rs` and the executable
`crates/fe/tests/fixtures/fe_test/const_trait_evidence.fe` example.

# Historical structural elaboration integration

The `elaboration` module resolves explicit record invocations and provides an
independent input view. Its compiler-issued `core::meta::Record` arguments let
ordinary Fe const functions use `field_count`, `field`, `field_type`, and
`type_is<T>`. Handles retain the record owner and semantic field identity;
ordinary Fe struct values cannot impersonate them. `type_is` currently admits
bool, u64 and u256 candidates, including aliases. Other candidates error rather
than compare false. Handles are not runtime constants or portable identities.

Fe can construct complete `FunctionDefinition<P, N>` values with structured
names, parameter and return types, and expression references. The construction
boundary inserts them in the authorized owner's module and runs ordinary source
and semantic checking. Optional definition arrays permit Fe-scoped policy
witnesses to select accessor sets. The driver does not implement that selection
policy. Wrong-owner handles, invalid references, malformed names and excessive
output are rejected; invalid generated types and unused body expressions are
checked by the ordinary compiler. Current bounds are eight parameters, 512
expressions per function, 64 optional definitions and 64 KiB of batch output.

`elaboration::prepare` evaluates explicit invocations against authored inputs,
then installs all declarations in one fresh output view before checking their
consumers. It also checks the selected providers' ingots. Generation cannot
observe declarations from its own output phase. The entry is a safe nongeneric
module const function taking one Record, without entry effects or where clauses;
constrained helper calls and scoped witnesses remain ordinary checked Fe calls.
Declared result capacities are checked before evaluation (at most 4096 expanded
descriptor slots per invocation), and project output is limited to 1 MiB.
The authored database is preserved on both success and failure.

Normal `fe check`, `fe build`, and `fe test` execute these invocations and admit
their checked declarations. The two fixed structural release applications and
their O0/O2 runtime execution are covered by `structural_generation_cli`.
The integration tests in `elaboration_inputs.rs` cover
Fe-authored field walks, owner separation, fabricated handles, untaken invalid
source, field-type and policy edits compared with fresh compilation, and const
consumers of generated accessors. The release contract linked at the start of
this document records the completed gate and the supported interface's limits.
