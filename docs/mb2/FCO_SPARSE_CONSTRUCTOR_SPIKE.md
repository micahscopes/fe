# FCO sparse-constructor and Schedule32 spike

This spike answers two separate questions that are easy to conflate:

1. Can ordinary Fe and an FCO-derived API hide the recursive
   `Cell<...Cell<Nil>>` representation of conformal values?
2. Does publishing a recursive type through FCO automatically produce a compact
   or shared runtime program?

## Result

The answer to the first question is **yes**. The executable fixture
`sparse_conformal_constructor.fe` defines semantic `point` and `sphere`
operations on a `ConformalModel` trait. Callers use semantic `point` and
`sphere` methods, while concrete ground aliases hide the nested cells behind
ordinary constructors. `DirectCl41` implements that façade normally;
`DerivedCl41` receives the same associated representations and methods from an
FCO provider. The Wasm test executes both paths and compares their checksums.

The flagship direct-lane renderer now applies the same lesson without retaining
the recursive `Cell` representation at runtime. `ConformalPoint` and
`ConformalSphere` records expose only their statically known compact supports;
their field-labelled literals own coefficient ordering at the construction
site. The five provider-derived lanes are collected immediately into one
`ConformalVector`, so the rest of the distance estimator consumes a semantic
vector rather than five unrelated scalar locals.

This façade is deliberately syntax-free and non-magical. The bounded FCO
provider still derives the five direct arithmetic lanes from the canonical
80-candidate support scan. The records only prevent callers from ordering raw
coefficients incorrectly. They deliberately use literals rather than helper
functions: Fe's current Wasm O0 path does not erase `#[inline(always)]` facade
calls. `fco_cga_sparse_facade` pins the resulting browser WGSL to the
pre-façade arithmetic signature (79 multiplications, 82 additions, 13
subtractions, 4 divisions, 3 square roots, and only the vertex/fragment entry
functions), and separately pins the Wasm module to its original six defined
functions.

This is useful, but it is not automatic conformal-algebra derivation. The
provider emits calls to the handwritten semantic constructors. Reflection can
enumerate fields and variants; it cannot discover that the last two point
coordinates must be `(radius2 - 1) / 2` and `(radius2 + 1) / 2`.

The answer to the second question is **no, unless the provider publishes the
compact shape explicitly**. The ignored, hard-bounded Schedule32 measurement
now compares five finite programs:

- a recursive `Schedule<32>` type;
- a 32-iteration loop;
- the same recursive type published as an FCO associated type;
- a handwritten five-add shared DAG;
- that five-add shared DAG emitted as an FCO method body.

Publishing `Plan = Schedule<32>` changes API ownership, not normalized runtime
shape: direct and FCO-published trees have equal RMIR size and call count. An
FCO provider can instead emit a quoted shared-DAG body, and the generated method
executes correctly. Sharing comes from the explicit `x2`, `x4`, `x8`, and
`x16` bindings, not from FCO recognizing common subexpressions. This is a
measured control, not yet a derivation from canonical `Schedule<32>`.

## Exact current FCO boundary

Available now:

- provider execution at compile time;
- reflection over target fields and variants;
- compile-time loops and conditionals over that reflection;
- exact ground nominal constructor/type/literal-const argument inspection;
- bounded preorder inspection of already-materialized ground types;
- expression and method quotation, holes, quote folds, and hygienic untyped
  local `let` bindings;
- associated-type publication with `builder.emit_assoc_ty`;
- generated method bodies and builder expression combinators;
- ordinary recursive type functions such as `Storage<N>` and `Schedule<N>`.

Not supplied automatically:

- conformal coordinate semantics or grade/support rules;
- conversion of an arbitrary value-level support set into a freshly synthesized
  recursive Fe type;
- common-subexpression discovery or DAG scheduling from a recursive type tree;
- semantic normalization of a `Schedule<32>` type-function application inside
  a provider;
- a general typed `Plan` IR with node identity and explicit sharing;
- recursive GPU execution (the finite published body still has to lower to
  legal non-recursive shader control flow).

Provider expansion is upstream of type analysis. It cannot call the existing
semantic type-function normalizer without creating an
`expanded_items -> analysis -> scope_graph -> expanded_items` cycle. The
phase-safe design options and recommended base-graph semantic island are in
`FCO_PLAN_BRIDGE_OPTIONS.md`; a duplicate lowering-time normalizer was rejected.

The important distinction is therefore not “FCO versus ordinary Fe.” FCO is
already capable of publishing either a type-level tree or executable quoted
code. The missing CGA layer is the domain-specific planner that computes support
and deliberately chooses a tree, compact loop, or shared DAG, plus a typed way
to hand that result to the provider. No new type-system feature was needed for
the constructor façade demonstrated here. Its exact current workaround is
revealing: `builder.ty<Storage<5>>()` publication hit a `type_fn_wf` query
cycle, so it publishes concrete ground aliases; free-function quote calls are
unsupported, so it uses `builder.static_call`.

## Reproduction

The constructor equivalence test is a normal test:

```sh
cargo test -p fe-codegen --test sparse_conformal_constructor
```

The Schedule32 comparison is ignored by default because it compiles five Wasm
modules and records timings:

```sh
cargo test -p fe-codegen --test schedule32_strategy_measure -- \
  --ignored --nocapture
```

Both experiments are fixed at small finite sizes (`Storage<5>`, `Storage<4>`,
and `Schedule<32>`); neither test performs an unbounded search.

The flagship sparse façade and its backend-shape gate are reproduced with:

```sh
cargo test -p fe-codegen --test fco_cga_sparse_facade
```
