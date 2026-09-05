# TD5 derive-provider command surface

This file is the audit inventory for the deliberately bounded command language
executed while expanding a Fe derive provider. It is not a second general Fe
interpreter. The frozen operation-name arrays in
`crates/hir/src/core/lower/provider_executor.rs` are the executable source of
truth; their compile-time cardinality assertion and source-scanning tests make
an undocumented expansion fail CI.

## Security classes

The provider surface has four classes:

1. **Read-only typed handles.** `Reflect<T>`, field, variant, ground type and
   ground generic-argument handles expose exact base-graph facts. Unsupported,
   computed, unresolved or non-ground forms fail closed. These reads do not
   synthesize HIR.
2. **Pure steering.** Booleans, bounded 256-bit natural arithmetic, comparisons,
   non-recursive/effect-free ordinary `const fn` helpers, finite sequences and
   bounded `for` iteration decide which output is requested. Steering shares
   the provider's 100,000-step budget. Natural ranges additionally have a
   4,096-element eager-allocation cap.
3. **Generated IR construction.** `ImplBuilder` creates inert, typed generated
   expression/pattern/type nodes. Integer construction consists only of literal,
   add, subtract, multiply and negate; `float` preserves one parsed floating
   literal as generated HIR without evaluating it in the provider. Quote blocks use executor-assigned local
   slots, so shared expressions cannot capture destination names.
4. **Effects.** `require`, `emit_method`, `emit_const`, `emit_assoc_ty` and
   `finish` append to the sole typed effect trace. Synthesis replays that trace
   through ordinary HIR builders, after which normal type checking applies.

There is no filesystem, environment, network, process, pointer, allocation,
runtime reflection, arbitrary compiler-query or recursive helper capability.
An unrecognized operation, bad operand kind, excessive range, budget exhaustion
or unsupported quote form is a diagnostic, never an ambient fallback.

## Frozen `ImplBuilder` inventory

Effect operations:

- `require`, `emit_method`, `emit_const`, `emit_assoc_ty`, `finish`

Generated expressions:

- literals and logic: `bool`, `int`, `float`, `and`, `or`
- integer arithmetic: `add`, `sub`, `mul`, `neg`
- comparisons: `eq`, `lt`, `gt`
- references and access: `self_ref`, `arg_ref`, `field_get`, `borrow`,
  `borrow_mut`
- calls and constants: `call`, `trait_call`, `trait_const`, `static_call`
- aggregates: `tuple_expr`, `with_elem`, `struct_init`, `variant_init`,
  `with_field`
- matching: `match_expr`, `with_arm`, `variant_binder`
- strings: `str`, `concat`, `keccak`

Generated patterns:

- `wildcard_pat`, `variant_pat`

Generated types:

- `ty`, `target_ty`, `provider_ty`, `self_ty`, `str_ty`, `tuple_ty`, `with_elem_ty`,
  `trait_assoc_ty`

The canonical inventory currently contains 48 operations. Reflection reads are
not string-dispatched `ImplBuilder` commands; typed handle vocabularies own
them, so the bespoke reflection-operation inventory is empty.

`provider_ty()` is the exact provider type selected at the derive request. A
named `using Compile<Program>` selection therefore exposes the ground
`Compile<Program>` tree without inserting a phantom configuration field into
the target. Its full type is part of the memoized expansion key: two distinct
program arguments cannot reuse one generated implementation. Ground
normalization starts from the request module (where `Program` aliases are
declared) while provider code and ordinary `ty<T>()` remain scoped to the
provider module. The configured type must still be finite and ground; the same
base-graph, node, unfold, and execution bounds fail closed.

A concrete type handle and an alias-normalized concrete ground-type handle
also expose `fields()` as an ordinary read-only sequence. This is not a builder
command: the base graph resolves the nominal struct, and each returned
owner-qualified `Field` handle carries its declared type/name plus hygienic
access identity. Consequently
`builder.field_get(builder.field_get(value, outer), inner)` can generate nested
record access without a string path or domain-specific metadata. The initial
surface deliberately accepts concrete non-generic structs only. Generic field
substitution and enum payload reflection fail closed until their occurrence
environments are modeled explicitly.

A field handle additionally exposes the pure binary read
`field.same_name(other)`. Unlike owner-qualified field equality, this compares
only the two authored member spellings. It allows a provider to align two
independently declared records—such as a named metric basis and a sparse
coefficient record—without string extraction, numeric slots, or positional
coupling. It never authorizes access through the other record: generated
`field_get` still consumes the original owner-qualified handle. Wrong operand
kinds and unresolved fields fail closed. Because `fields()` and `same_name`
are typed reflection reads rather than `ImplBuilder` commands, the frozen
46-operation builder inventory does not change.

A variant handle exposes its authored `name()` as the same inert compile-time
string value used by `field.name()`. This permits a fieldless enum to derive a
single parser/printer pair from its nominal declaration, while generated
variant construction and matching still consume the owner-qualified handle.
The name cannot select another declaration or bypass ordinary checking, and it
adds no `ImplBuilder` command.

## Natural iteration

Provider `for` accepts an ordinary `Value::Seq`. Reflection iterators and exact
ground-type traversal already produce such sequences. `start..end`, where both
ends are provider naturals, now produces the same value:

- the range is half-open and must be ascending;
- both bounds must fit `usize`;
- length must not exceed 4,096;
- every materialized element consumes one step from the same 100,000-step
  budget subsequently charged by loop-body execution.

This is intended for bounded plan construction, not open-ended evaluation.

Normalized ground-type handles additionally provide
`normalized_postorder_types()`. It traverses the same finite, base-graph-only
plan as `normalized_preorder_types()`, but visits every type-valued child before
its parent. This lets an ordinary Fe provider express a structural fold with a
value stack; it does not admit recursive executor calls or merged-graph queries.

Compile-time sequences are immutable values with a deliberately small
persistent vocabulary:

- `len()`, `at(index)`, and `last()` read a sequence;
- `append(value)` and `concat(sequence)` return extended sequences;
- `replace(index, value)` returns a sequence with one element changed; and
- `without_last()` returns the prefix before the final element.

Indexes must fit `usize` and be in bounds; `last()`/`without_last()` require a
non-empty sequence. Wrong value kinds and invalid bounds fail closed at the
provider expression. These operations only rearrange bounded compile-time
`Value`s. They emit no HIR, mutate no external state, and remain charged to the
ordinary provider step/command budgets.

## Integer generated expressions

`GenExpr` preserves unsigned literals and the explicit `Add`, `Sub`, `Mul` and
`Neg` operator structure. Synthesis replays these as ordinary Fe HIR. The
provider executor does not assign a signed width or silently coerce a generated
value; the emitted method/constant's ordinary semantic analysis determines
types and rejects mismatches.

Quotes expose precisely the same integer subset (`42`, `+`, `-`, `*`, unary
`-`). Quote-local `let` bindings use hygienic numeric slots. Explicit
`builder.share(expr)` is the domain-neutral sharing primitive for constructed
expression DAGs: it materializes the expression once per emitted member root
as an eagerly evaluated hygienic local and reuses that local at every
occurrence. Its input is deliberately restricted to pure root-scope leaves
(arguments, `self`, constants and safe field projections) composed with the
arithmetic/logic/comparison builders and nested shares. Calls, aggregates,
matches, quote blocks, and arm/local binders fail closed; branch-local sharing
continues to use an ordinary quote-local `let`.

## Explicit borrowing

`builder.borrow(expr)` and `builder.borrow_mut(expr)` preserve explicit `ref`
and `mut` expressions in generated HIR. They are the domain-neutral
counterparts of Fe's required borrow syntax for arguments and receivers, and
ordinary semantic analysis checks them after provider replay. The provider
executor accepts only an existing generated expression handle. Type handles,
reflection handles, strings, and other compile-time values fail closed instead
of being coerced into expressions. Mutable borrows are excluded from
`builder.share` because sharing one mutable capability would change aliasing
semantics.

## Floating generated literals

`builder.float(literal)` preserves the parser's exact `FloatId` and synthesis
replays it as an ordinary Fe float literal. The provider executor neither
converts nor computes with it; ordinary checking at the emitted method or
constant decides its scalar type. Only a direct compile-time float value is
accepted, so integer/string/type operands fail closed. Floating arithmetic is
still expressed with the same generated `add`/`sub`/`mul`/`neg` nodes and is
typed by the generated program. This domain-neutral primitive removes the need
for a provider library to call a private helper merely to obtain `+0.0`.

## Change rule

Any new named operation must:

1. be placed in one security class above;
2. be added to the frozen inventory and cardinality assertion;
3. have positive replay and negative fail-closed tests;
4. update this document in the same commit.

Domain-specific operations and precomputed algebra tables do not belong in this
surface. Algebra libraries should express their decisions through ordinary Fe
const helpers and these domain-neutral construction primitives.

`ImplBuilder` is still an executor-recognized capability rather than an
ordinary, publicly typed Fe builder API: `core::derive` does not yet expose an
opaque generated-expression handle with method signatures for this inventory.
Introducing that honest typed façade requires migrating the complete surface,
not selectively declaring new arithmetic operations, and remains explicit
architectural debt outside this bounded TD5 slice.
