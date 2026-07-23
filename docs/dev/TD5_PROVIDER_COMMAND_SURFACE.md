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
   add, subtract, multiply and negate. Quote blocks use executor-assigned local
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

- literals and logic: `bool`, `int`, `and`, `or`
- integer arithmetic: `add`, `sub`, `mul`, `neg`
- comparisons: `eq`, `lt`, `gt`
- references and access: `self_ref`, `arg_ref`, `field_get`
- calls and constants: `call`, `trait_call`, `trait_const`, `static_call`
- aggregates: `tuple_expr`, `with_elem`, `struct_init`, `variant_init`,
  `with_field`
- matching: `match_expr`, `with_arm`, `variant_binder`
- strings: `str`, `concat`, `keccak`

Generated patterns:

- `wildcard_pat`, `variant_pat`

Generated types:

- `ty`, `target_ty`, `self_ty`, `str_ty`, `tuple_ty`, `with_elem_ty`,
  `trait_assoc_ty`

The canonical inventory currently contains 43 operations. Reflection reads are
not string-dispatched `ImplBuilder` commands; typed handle vocabularies own
them, so the bespoke reflection-operation inventory is empty.

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

## Integer generated expressions

`GenExpr` preserves unsigned literals and the explicit `Add`, `Sub`, `Mul` and
`Neg` operator structure. Synthesis replays these as ordinary Fe HIR. The
provider executor does not assign a signed width or silently coerce a generated
value; the emitted method/constant's ordinary semantic analysis determines
types and rejects mismatches.

Quotes expose precisely the same integer subset (`42`, `+`, `-`, `*`, unary
`-`). Quote-local `let` bindings use hygienic numeric slots and are the sharing
mechanism for direct arithmetic DAGs.

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
