# Source-invoked structural generation

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
The public milestone still requires its full validation and release gate.
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

# Explicit scalar generation experiment

`generation::generate_scalar_function` evaluates an ordinary checked Fe
provider and fills a function body in a separate, checked standalone source
stage. It is a driver API; the `fe` command does not discover or invoke these
providers automatically.

A provider computes the designated core descriptor:

```fe
use core::meta::FunctionBody

const fn provide() -> FunctionBody<u256> {
    let mut result: u256 = 40
    let slot = mut result
    slot += 2
    FunctionBody { value: result }
}
```

The target source supplies the entire declaration and can include a caller:

```fe
pub const fn answer() -> u256 {}
const fn consume() -> u256 { answer() }
```

After loading the provider file into a `DriverDataBase` initialized with builtin
core/std and resolving `provide` to a `hir::hir_def::Func`, invoke the API:

```rust,ignore
use fe_driver::generation::{
    FunctionTemplate, GenerationBudget, generate_scalar_function,
};

let mut budget = GenerationBudget::new(1, 64 * 1024);
let artifact = generate_scalar_function(
    &provider_db,
    provider_func,
    FunctionTemplate {
        url: "file:///generated/answer.fe".parse()?,
        source: target_source.to_owned(),
        function_name: "answer".to_owned(),
    },
    &mut budget,
)?;
```

`artifact.source()` contains the completed source. Ordinary compiler consumers
use `artifact.database()`, `artifact.file()`, and `artifact.function()`. The
artifact owns its canonical database, and generation performs no filesystem
writes. Integration tests in `tests/scalar_generation.rs` evaluate callers from
that database and check declaration requirements with positive and negative
controls.

The initial entry accepts a safe, top-level, nullary const provider in a
standalone ingot with only builtin dependencies. Root generics, where clauses,
and effects are excluded; ordinary generic helpers retain normal call-site
checking. Descriptor values must use the actual builtin
`core::meta::FunctionBody<bool>` or `FunctionBody<u256>` identity. Type aliases
are accepted, same-shaped user definitions are not.

The selected target must have an explicit return type and an empty body. Only
that body's parsed byte range is replaced, preserving attributes, modifiers,
parameters, labels, generics, requirements, effects, and supporting items. The
target return type must equal the descriptor's scalar type after normalization.
The whole completed target stage passes HIR, semantic borrow, and layout checks.

Provenance retains owned provider and template sources and their byte ranges,
plus the generated body range and request-local invocation number. Output
diagnostics retain that provenance. This stage receipt does not add generated
HIR origins or implicit cross-stage references. The target uses default compiler
options and builtin core/std; it does not inherit the provider's dependencies.

Budgets count each emitted source even when CTFE is cached. Emitted source is
charged before target checking, including attempts rejected by that checking.
The input template is also checked against the source limit before parsing.
These are logical source/function limits, not a bound on all compiler memory or
provider evaluation work. CTFE's existing execution limits still apply.

## Aggregate values and request identity

For tuples and fixed arrays, use an explicit `GenerationSession`:

```rust,ignore
use fe_driver::generation::{GenerationRequest, GenerationSession};

let mut session = GenerationSession::new(
    "schema-stage".to_owned(),
    GenerationBudget::new(8, 64 * 1024),
);
let artifact = session.generate_value_function(
    &provider_db,
    provider_func,
    GenerationRequest {
        key: "constants".to_owned(),
        template,
    },
)?;
```

This entry accepts `FunctionBody<T>` where `T` is recursively `bool`, `u256`, a
tuple (including unit), or a fixed array. For example, a provider can compute
`FunctionBody<([u256; 2], bool)>` for a template returning `([u256; 2], bool)`.
The source body contains only canonical literal syntax. Ordinary target checking
still validates its complete declaration and all callers.

The transport retains a structural type independently of its contents. Empty
`[bool; 0]` and `[u256; 0]` values therefore remain different contracts. User
nominal types, references, pointers and unsupported scalar types are rejected,
even inside zero-length arrays. Target aliases normalize normally, but fitting
integer literals cannot silently narrow the descriptor's element type.

Transport is bounded to 32 levels, 4,096 type nodes, and 4,096 expanded value
occurrences. Expanded array
cost uses saturating arithmetic before the explicit provider evaluation request;
an enclosing empty
array does not materialize its latent element values. Literal rendering checks
the remaining aggregate source budget before each append. These bounds apply to
the transport, not all work in ordinary checking or CTFE.

Every attempted request key is reserved before validation. Reusing a key in the
same session returns `DuplicateRequest`, even if the first attempt failed. Failed
attempts before source emission do not consume the output budget; emitted but
invalid target stages do. A duplicate consumes neither output nor execution.
The attempted-key ledger is not limited by the output budget; callers control
session admission and lifetime.

Receipts and errors retain `RequestIdentity { stage, key }`. The existing
`invocation` field is an emission ordinal only. Reversing request order changes
that ordinal but not the caller-supplied identity. Equal emitted text does not
merge distinct requests. Two fresh sessions may evaluate the same stage/key
against different sources; identity equality is not evidence of equal artifacts
or permission to reuse one. A shared stage key does not make the artifacts in
separate databases mutually visible. The original scalar entry retains `None` for request
identity and continues to reject aggregates.

## Frozen exports and bound calls

`generation::imports` supports one explicit call to a selected function from a
checked generated artifact:

```rust,ignore
use fe_driver::generation::imports::{FrozenArtifact, bind_function};

let frozen = FrozenArtifact::new(artifact);
let selected = frozen.export("apply")?;
let bound = bind_function(template, &selected, &[1, 0], &mut budget)?;
```

The template selects a top-level function with an explicit return type and an
empty body. The index list forwards its parameters in the selected export's
argument order; labels come from the export. For example, `[1, 0]` forwards the
second template parameter first. Repeated indices are allowed for these Copy
scalar values. There is no arbitrary argument-expression string input.

Exports must be public, safe, monomorphic const functions over `bool` and `u256`,
without effects or where clauses. Ordinary View/Own parameter adaptation is
checked by the compiler. Exact scalar types must match after normalization;
references, nominal types and generic interfaces are outside this entry.

The handle retains its particular immutable artifact incarnation, even after the
original wrapper is dropped. Reusing a logical request identity does not rebind
an existing handle. Materialization copies the complete frozen source into a
separate dependency ingot, retaining root paths and private supporting context.
The dependency exposes its whole public surface, not an export whitelist.

The driver constructs the call slot and checks the completed compilation. It
then compares that call's resolved function against the selected materialized
export in the receiving database. Shadowing cannot silently redirect a returned
bound artifact. This check covers the designated direct call; it does not prove
a transitive call graph or establish a general hygiene mechanism.

`BoundFunction` exposes its immutable database, source, selected function and
receipt. The receipt retains the frozen source, original generation provenance,
selected export, completed consumer source and emitted call range. Post-emission
errors retain this receipt after the temporary database is dropped. The operation performs no filesystem
writes and uses builtin dependencies with default compiler options. Custom
dependency closures and persistent serialized handles remain future work.

Each emission charges one function and the bytes of both the frozen package and
completed consumer source, including emissions rejected during checking. These
are logical source limits, not a bound on all compiler allocations. Request
identities continue to name logical work rather than imported declarations.

## End-to-end validator example

Run the complete in-memory example from the repository root:

```sh
cargo run --release -p fe-driver --example generated_validator
```

The Fe provider computes inclusive bounds `(10, 20)` as
`FunctionBody<(u256, u256)>`. Generation fills a private `bounds()` function in
a package with a public `within(value: u256) -> bool` helper. A frozen export
of that helper then supplies the body of `validate(value: u256) -> bool` in a
separate consumer. Ordinary Fe callers evaluate the boundary cases:

```text
below: false
lower: true
inside: true
upper: true
above: false
```

The example needs no manual editing of generated source. The helper reads the
provider-computed bounds from its retained package context, so this application
uses the existing value-generation and bound-call operations. It does not yet
need a general expression builder or literal arguments in the call protocol.
The host supplies explicit templates, selects the export and sequences the two
stages; providers are not automatically discovered.

The example's tests run in the normal Cargo test suite. They compare a provider
edit in a reused database with fresh compilation and verify that a retained old
export keeps its original behavior. This is a focused edit oracle for this
pipeline, not a general incremental correctness or performance claim.

## Nominal identity and frozen package graphs

`generation::imports::packages` composes ordinary checked Fe packages while
preserving the identity of their captured dependency graph:

```rust,ignore
use fe_driver::generation::imports::packages::{
    FrozenPackage, PackageLimits, compose_packages,
};

let records = FrozenPackage::from(frozen_artifact);
let result = compose_packages(
    consumer_source.to_owned(),
    &[("left", &records), ("right", &records)],
    PackageLimits::new(16, 64 * 1024),
)?;
```

The result exposes `database()`, `file()`, `source()` and owned materialization
receipts. It can itself become a dependency of another composition. Clones and
repeated conversions from the same cloned `FrozenArtifact` retain one recipe
identity. Each unique recipe is materialized once per receiving compilation,
so two aliases of that package refer to the same nominal types. Independently
created artifacts stay distinct even when their sources and logical request
identities are equal. A diamond preserves sharing through the captured graph.

For example, a generated package can contain a private computed `limit()`, a
public `Record`, a `make(value) -> Record` constructor and a
`valid(record: Record) -> bool` function. The consumer can pass
`left::make(value: 15)` to `right::valid` when both aliases select the same
package. Separate same-shaped `Record` definitions fail ordinary type checking.
The executable examples are in `tests/frozen_packages.rs`; the underlying
resolver controls are in `tests/nominal_package_identity.rs`.

This entry exposes ordinary package imports. Public visibility and alias
shadowing retain their normal behavior. It does not extend the scalar
`bind_function` contract to nominal interfaces or verify every authored call's
selected target. Nominal compatibility is decided by the existing compiler,
not by a parallel structural comparison in the package layer.

Composition captures immutable source/edge recipes; it does not retain every
intermediate composition's checked database. Generated leaves retain their
original artifact. The returned root owns its fresh checked database, with
builtin core/std and default compiler options. This API constructs finite DAGs;
it does not specify general cyclic dependency or provider-discovery semantics.

Limits count the new root and unique transitive packages, their source bytes,
and each dependency alias once per unique parent. They do not bound all compiler
allocations. Every materialized package is checked. Receipts on output errors
retain sources and dependency URLs after the temporary database is dropped.
Materialization URLs are local to that compilation, not persistent package IDs.
No filesystem or network access is performed.
