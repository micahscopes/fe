# A typed sparse GA compiler for one WebGPU invocation

Status: active design plus structural G2/G3 vertical slices, 2026-08-13.

This document specifies the fuller geometric-algebra implementation behind the
Phase 7 gallery goal. It is deliberately honest about three different things:

1. the Fe code that exists and executes now;
2. the reusable expression compiler we are building toward; and
3. a possible later workgroup implementation that uses several GPU lanes.

The primary target is a sparse, straight-line program executed by **one shader
invocation**. In WebGPU, an invocation is already one GPU lane. It cannot spawn
threads. Compile-time sparsification can expose independent instructions for
the device compiler to issue concurrently, but that is instruction-level
parallelism within a lane, not dynamically created shader invocations.

## Executive design

The intended user-facing shape is a static Fe program:

```fe
type Expr = Grade<
    Sum<
        Geometric<Sphere, Point>,
        Outer<Reverse<Tangent>, Plane>,
    >,
    2,
>

type Program = GaProgram<Expr, Cl41, Strict>
derive EvaluateGa for Inputs using CompileGa<Program>
```

The exact spelling of the final derive façade is not committed yet. The
essential properties are:

- the expression, metric, supports, and numerical policy are static Fe types;
- runtime values are only the input coefficients;
- CTFE/FCO expands the expression into a finite typed plan;
- the emitted evaluator contains ordinary scalar/vector Fe arithmetic, not a
  runtime GA interpreter;
- absent blades, zero pairings, plan scans, node tags, and scheduling branches
  do not reach Wasm or WGSL; and
- the same plan can have semantic, strict, balanced, Wasm, and GPU interpreters
  without making any generated artifact the correctness oracle.

The implementation pipeline is:

```text
static expression + metric + leaf supports + numeric policy
    |
    v
conservative blade-support interpretation
    |
    v
exact sparse terms, with coefficients and source identities
    |
    v
policy-governed normalization
    |
    v
hash-consed pure operation DAG
    |
    v
one-invocation schedule (topological order + reduction trees)
    |
    v
FCO-emitted ordinary Fe SSA
    |
    +----------> Wasm
    +----------> SPIR-V / browser WGSL
    +----------> native differential execution
```

Support, exact algebra, numerical transformation, and machine schedule are
separate layers. Conflating them is the easiest way to obtain fast but
incorrect code.

## What exists now

### Reusable libraries

`ingots/sparse_clifford` already provides:

- `BladeSet`, currently bounded to dimension five / 32 blades;
- diagonal-metric conservative geometric-product support;
- grade projection, blade grade, compact rank, and sparse value storage;
- metric-independent wedge signs and a bounded sparse candidate plan;
- dense Fuchs--Thery-style support recurrence for cross-checking; and
- several real CGA/QCGA plans that erase to straight-line shader arithmetic.

`ingots/ga_expr` now adds:

- `Input<Slot, Support>` leaves;
- `Sum`, `Difference`, `Neg`, `Geometric`, `Outer`, `LeftContraction`,
  `RightContraction`, `ScalarProduct`, `Grade`, `Reverse`, `PoincareDual`, and
  `Regressive` expression nodes;
- `DiagonalMetric<Dimension, NonzeroSquares>`;
- compositional `GaSupport<Metric>` interpretation over those nodes;
- explicit `Strict` and `AlgebraicBalanced` numerical policies;
- `GaProgram<Expression, Metric, Policy>` as a static compilation request; and
- `TermZero`, `TermAdd`, and `BilinearTerm` as the initial exact-plan witness
  vocabulary.

Support analysis is compositional today. It is conservative: it removes
structural zeros and impossible blades, but does not claim cancellation between
runtime coefficients.

### Specialized executable vertical slice

`crates/codegen/tests/fixtures/ga_expr_fco` expresses
`(a ^ b) + (a ^ b)` for sparse PGA(2,0,1) line inputs. In Fe it:

1. infers output support `{e01, e02, e12}` from the expression type;
2. computes the six nonzero ordered products at CTFE;
3. materializes an exact ground type witness carrying input slots, output
   blades, magnitude two, and signs;
4. statically requires that witness to equal an independently spelled expected
   type; and
5. has an FCO provider walk the normalized type and emit three branch-free
   methods using `builder.share` for the six products.

The integration gate executes 2,005 directed and deterministic-random cases in
Wasmtime. It compares the results bit-for-bit with an independently authored
Rust interpreter for the declared emitted association, and separately compares
them within a documented tolerance to a dense source-tree interpretation. The
same Fe implementation compiles to browser-valid WGSL with:

- only the fullscreen vertex and fragment functions;
- six surviving GA multiplications plus one display-scale multiplication; and
- no runtime loop, branch, switch, plan table, or host algebra import.

This is a real end-to-end proof of the staging route. It is **not yet** a
general lowering from every arbitrary expression node to exact terms. The
prototype's exact plan constructor is specialized to the twice-wedge example.

### Structural expression-lowering slice

`crates/codegen/tests/fixtures/ga_expr_generic_fco` is the first genuinely
structural G2/G3 slice. One unchanged Fe provider lowers two different ground
trees:

```fe
Sum<Outer<LineA, LineB>, Outer<LineA, LineB>>
Neg<Outer<Sum<LineA, LineB>, Difference<LineA, LineB>>>
```

The expression is ordinary Fe type data in a zero-sized `GaProgram` marker
field on the reflected input record. The provider finds that field, traverses
the expression in normalized postorder, and uses persistent compile-time
sequences as a value stack. Each stack entry carries conservative support plus
eight generated scalar components. `Input`, `Sum`, `Difference`, `Neg`, and
`Outer` are recognized by nominal constructor identity; no complete expression
name or tree shape appears in the provider.

The independent gate executes 1,004 inputs for each tree in Wasmtime and
compares bit-for-bit with separately authored Rust tree interpreters. It also
requires browser-valid WGSL with no runtime loop, branch, or switch and no host
algebra import. This closes the specific objection that the earlier
`(a ^ b) + (a ^ b)` proof could merely be a bespoke candidate table.

Its boundary is explicit:

- three dimensions and two packed `{e0,e1,e2}` inputs;
- strict tree-order evaluation only;
- `Input`/`Sum`/`Difference`/`Neg`/`Outer` only;
- a dense eight-lane compile-time intermediate (absent runtime terms are still
  omitted through support-aware emission); and
- a zero-sized marker field because derive-provider goal arguments are not yet
  exposed as ground reflection handles.

General diagonal metric products, the remaining node vocabulary, compact typed
outputs, stable expression DAG sharing, and deriving input slots/support from
the reflected record are still required before this is the final GA façade.

## Semantic model

### Blades and multivectors

For dimension `n`, a basis blade is represented by an `n`-bit generator mask.
A sparse support is a set of those blade masks. A runtime multivector leaf has:

```text
Leaf {
    semantic input slot,
    scalar domain,
    statically known possible blade set,
    compact runtime coefficient storage,
}
```

Input slots are not DOM IDs, event IDs, or arbitrary author bookkeeping. They
are stable identities used to distinguish coefficient leaves while forming
monomial keys and shared loads. The final façade should derive slots from the
reflected input record and its fields, so ordinary users do not number them.

### Metrics

The first complete target is an orthogonal/diagonal metric:

```text
Metric {
    dimension,
    zero-square generator mask,
    negative-square generator mask,
}
```

The existing support layer only needs the nonzero-square mask; exact planning
also needs each square's sign. Degenerate metrics such as PGA are first-class.

A general symmetric metric is a later extension. Multiplying two basis blades
under an off-diagonal metric can expand into several blades, so `left XOR
right` is no longer the exact product. It must use an exact contraction
recurrence or a statically derived change of basis. The API must never accept a
general matrix and quietly apply diagonal support rules.

### Operators

The core operator set has unambiguous semantics:

- `Sum`, `Difference`, `Neg`: coefficient-wise linear operations;
- `Geometric<A,B>`: the metric geometric product;
- `Outer<A,B>`: grade-additive exterior product;
- `LeftContraction<A,B>`: keep grade `grade(B) - grade(A)` where defined;
- `RightContraction<A,B>`: keep grade `grade(A) - grade(B)` where defined;
- `ScalarProduct<A,B>`: keep grade zero of the geometric product;
- `Grade<A,K>`: grade projection;
- `Reverse<A>`: multiply grade `k` by `(-1)^(k(k-1)/2)`;
- `PoincareDual<A>`: complement blades using orientation, independent of
  metric invertibility; and
- `Regressive<A,B>`: dual of the wedge of duals, with a pinned orientation
  convention.

The implementation should also add `GradeInvolution` and
`CliffordConjugate`. A generic operation named only `Inner` should be avoided:
GA libraries disagree about which contraction or grade selection it denotes.

Metric dual and Poincare dual must be separate operations. The former may be
undefined for degenerate metrics; the latter is useful precisely in PGA.

## Compile-time intermediate representations

### 1. Support IR

Support interpretation is an abstract interpreter over expression types:

```text
support(Sum(a,b))       = support(a) union support(b)
support(Grade(k,a))     = grade_filter(k, support(a))
support(Outer(a,b))     = disjoint blade products
support(GP(a,b))        = metric-admissible blade products
support(Reverse(a))     = support(a)
```

It deliberately does not erase a blade because two symbolic terms might
cancel at runtime. Unknown support becomes an explicit dense support, not an
implicit fallback.

The current `u32` representation is good for the real Cl(4,1) gallery work,
but the general representation should be a fixed-capacity sequence of words:

```fe
struct BladeBits<const Words: usize> { ... }
```

Capacity is part of the program type and compilation budget. Exceeding it is a
diagnostic, never truncation.

### 2. Exact sparse-term IR

The general exact term should be conceptually:

```text
Term {
    output_blade,
    coefficient: ExactScalar,
    factors: ordered small sequence of LeafCoefficientId,
    source_path: stable expression-node path,
}
```

`ExactScalar` initially supports checked signed integers. Rational constants
can use normalized sign/numerator/denominator triples. Runtime `f32` constants
remain leaves unless the selected numeric policy explicitly permits folding
them; floating values must not establish symbolic cancellation.

Term keys sort by output blade and factor sequence. Coefficients can be
combined only when the exact factor keys match. Every arithmetic operation is
checked against fixed compile-time capacity and magnitude limits.

### 3. Expression/DAG IR

Term normal form and execution form are different. The execution DAG uses
nodes such as:

```text
Load(leaf coefficient)
Const(exact scalar lowered to runtime scalar)
Neg(node)
Mul(left, right)
Add(left, right)
Convert(node, scalar type)
```

Hash-cons pure nodes by `(operation, scalar type, child IDs, constant payload)`.
Commutative child sorting is policy-dependent: it is permitted for exact
domains and `AlgebraicBalanced`, but not as a floating-point transformation in
`Strict`.

The hash-consing implementation should be ordinary bounded Fe CTFE data—a
fixed node arena and deterministic open-addressed map or sorted lookup. It must
record capacity, collisions/probes, and failed insertions. FCO's current
`builder.share` is the emission primitive, not an automatic CSE pass.

### 4. Schedule IR

A one-invocation schedule contains:

```text
Schedule {
    topologically ordered live nodes,
    output root for each surviving blade,
    maximum live-value estimate,
    dependency depth,
    operation counts,
    numeric policy,
}
```

Scheduling should use a deterministic cost function. A first implementation
can use dependency level plus a Sethi--Ullman-style live-value estimate:

1. issue already-shared/load nodes early enough for reuse;
2. interleave independent output trees where doing so does not increase peak
   live values excessively;
3. balance associative exact/algebraic reductions;
4. retain authored/source order in `Strict`; and
5. topologically emit each live node once.

The goal is not to encode GPU vendor scheduling in Fe. The goal is to present
a compact dependency DAG with useful independent chains and tolerable register
pressure. Actual superscalar issue is the shader compiler/device's job.

## Numerical contracts

### `Strict`

`Strict` preserves the authored expression tree's scalar evaluation order.
Allowed transformations are:

- removal of algebraically structural zero operations that perform no runtime
  scalar operation in the language semantics (for example, an absent blade or
  disjoint-support rejection, not rewriting a runtime `0.0 * x`);
- compile-time blade/sign routing;
- sharing of a pure subexpression so the same runtime result is reused; and
- dead-output pruning.

It does not reassociate additions, distribute multiplication, merge repeated
floating terms into a coefficient, cancel `x - x`, or assume finite/non-NaN
inputs. Signed zero and NaN behavior remain part of the contract.

### `AlgebraicBalanced`

`AlgebraicBalanced` interprets scalar operations under a declared real-algebra
model for planning. It may:

- flatten and reorder sums/products;
- collect equal monomials into exact coefficients;
- remove exact coefficient zero;
- balance reduction trees; and
- select equivalent factorizations under a deterministic cost model.

The generated runtime arithmetic is still `f32`/`f64`, so the result can differ
from `Strict` in rounding, overflow timing, NaNs, infinities, and signed zero.
Tests must compare both the emitted policy and a dense semantic reference; they
must not use `AlgebraicBalanced` where strict reproducibility is required.

A future `FastApprox<ErrorPolicy>` should only be introduced with a concrete
error/finite-input contract. It must not be an unlabeled optimizer mode.

Exact finite-field and integer scalar domains can use aggressive canonical
normalization without the floating caveat, subject to overflow/modulus rules.

## FCO/CTFE phase boundary

The provider executor runs before the merged semantic graph exists. It cannot
freely invoke trait solving or arbitrary full-program type normalization
without creating a generation/analysis cycle.

The currently implemented narrow bridge can normalize and traverse a **closed,
ground recursive type plan** in preorder or postorder using base-graph facts,
including the local
`AlgebraicTwiceWedgePlan6` fixture and imported ground plans already exercised
elsewhere. It remains deliberately bounded:

- normalized preorder/postorder traversal is capped at 256 nodes;
- recursive type unfolding is capped at 4,096 steps;
- a provider run has a 100,000-step / 10,000-command budget; and
- const-helper support inside provider execution is narrower than ordinary Fe
  CTFE and fails closed on unsupported constructs.

It cannot use generated impls, general associated-type solving, or an open
symbolic type program. Persistent bounded compile-time sequences now make
structural folds over that closed tree possible inside Fe, but do not broaden
which graph can be observed. Therefore the robust architecture remains
two-stage:

1. ordinary Fe CTFE materializes a closed, finite plan witness; and
2. FCO consumes that witness and publishes ordinary executable Fe.

Longer term, the base-only semantic-island design in
`FCO_PLAN_BRIDGE_OPTIONS.md` can broaden normalization without admitting a
merged-analysis cycle. A duplicate hidden Rust GA normalizer is not the answer:
the algebra and plan should remain inspectable Fe definitions.

## “Parallel within one invocation” precisely

| Scope | Available parallelism | Needed representation | This project |
|---|---|---|---|
| One invocation/lane | Independent instruction chains, hardware ILP, compiler-chosen vectorization | Shared straight-line SSA DAG | Primary target |
| One subgroup | Several invocations with subgroup operations | Subgroup intrinsics and uniform control | Future, capability-gated |
| One workgroup | Several invocations, workgroup memory, barriers, partition/reduction plan | Compute entry, shared arena, lane mapping | Separate later track |
| One dispatch | Many independent workgroups | Global indexing and buffers | Ordinary WebGPU execution |

There is no standard mechanism for one invocation to create more invocations.
Packing four independent coefficients into a vector can be useful, but that is
SIMD-shaped arithmetic in one lane, not four independently synchronized
threads. A vector emitter should be benchmarked and chosen by cost policy; it
must not be assumed faster on every WebGPU backend.

The optional workgroup compiler would lower a large expression DAG into tasks,
assign tasks to statically existing invocation IDs, store dependencies in
workgroup memory, and place barriers between dependency levels. It has a very
different cost model: occupancy, barrier count, shared-memory traffic, and
uniformity. It should consume the same exact plan but have a distinct
`WorkgroupSchedule<Lanes>` type and tests. It must never be presented as the
single-invocation implementation.

## Correctness strategy

Correctness is a matrix, not one golden artifact.

### Compile-time/type gates

- expected support sets for basis, sparse, dense, degenerate, and grade cases;
- exact normalized term sequences, coefficients, signs, output blades, and
  source identities;
- overflow/capacity rejection;
- metric dimension and scalar-domain mismatch rejection; and
- deterministic plan identity across irrelevant source/name changes.

### Independent semantic gates

- an ordinary Fe dense multivector interpreter;
- the existing dense Fuchs--Thery recurrence where applicable;
- an independently authored Rust oracle;
- exhaustive small-dimension basis products;
- generated bounded expression trees over randomized sparse supports;
- directed cancellation, degeneracy, dual/orientation, and grade cases; and
- adversarial scalar values: `+0`, `-0`, subnormal, infinities, NaNs, minimum,
  maximum, and values around rounding/overflow boundaries.

Exact domains require exact equality. `Strict` floating tests require bitwise
equality to the strict interpreter. `AlgebraicBalanced` requires bitwise
equality to the declared schedule interpreter and a separately stated
semantic/error relation to the dense expression.

### Backend execution gates

- zero-import Wasmtime execution;
- native execution against the same tapes where supported;
- SPIR-V generation and browser-default Naga validation;
- actual WebGPU execution on the software/reference path plus browser E2E for
  selected kernels; and
- parity between scalar and any future vector/workgroup emitter.

### Shape/performance gates

- survivor operation counts;
- load/use and explicit share counts;
- no runtime plan loops/branches/tables in straight-line mode;
- dependency depth and estimated maximum live values;
- WGSL function count after required inlining;
- compile analysis/FCO/backend timings;
- generated MIR/WGSL/Wasm sizes; and
- real GPU timing distributions after warmup.

These are regression properties. None substitutes for semantic execution.
Mutation tests should deliberately flip one sign, candidate, grade rule, or
metric square and prove the semantic gates fail.

## Implementation milestones

### G0 — existing algebra foundations (complete)

- bounded sparse supports and diagonal product/grade operations;
- sparse plan witnesses and provider-emitted straight-line arithmetic;
- dense independent recurrences and CGA/QCGA examples; and
- explicit FCO shared-expression emission.

### G1 — typed expression/support layer (first slice complete)

- static metric/expression/numeric-policy vocabulary;
- compositional support for the core operators;
- exact typed term vocabulary; and
- one executable CTFE/FCO/Wasm/WGSL vertical slice with independent semantics.

### G2 — general exact binary operators

- derive term plans for arbitrary sparse leaves under `Sum`, `Difference`,
  `Neg`, `Geometric`, `Outer`, contractions, grade, reverse, and dual;
- add metric square signs and exact checked coefficients;
- derive leaf/component slots from reflection; and
- remove the example-specific exact-plan constructor.

Acceptance: exhaustive dimensions 0--5 against dense Fe and Rust oracles, with
strict bit-parity where promised, and no runtime planning in WGSL.

### G3 — arbitrary finite expression trees

- recursively interpret any ground composition of supported nodes;
- retain source identities/grouping for `Strict`;
- normalize exact/algebraic terms for `AlgebraicBalanced`;
- add explicit capacity/fuel diagnostics containing expression path and stage;
  and
- property-generate trees, supports, metrics, and policies.

Acceptance: a new composed expression needs only a type alias/input record and
no provider/compiler changes.

### G4 — shared DAG and scheduler

- bounded Fe CTFE node arena and deterministic hash-consing;
- policy-aware reduction construction;
- dependency/live-value analysis and deterministic topological scheduling;
- FCO emission with one share per reused pure node; and
- scalar versus vector schedule measurement.

Acceptance: independently repeated leaves/products load/compute once, semantic
gates remain green, and adversarial expressions stay within published compile
and register-pressure budgets.

### G5 — gallery integration

- replace bespoke PGA/CGA/QCGA plan providers with the common expression
  compiler incrementally;
- preserve or improve their semantic and GPU gates;
- provide a small GA playground showing authored expression, inferred support,
  exact plan, and generated WGSL through the Fe `SourceInspector`; and
- use the common compiler in the buttery 3D distance-estimator example.

Acceptance: each migrated demo deletes more bespoke planning code than it
adds, and application behavior remains Fe-owned.

### G6 — broader metrics and scalar domains

- general symmetric metric expansion with exact finite planning;
- exact rational/modular coefficient domains;
- orientation-safe metric/Poincare duals; and
- capacity-scaled support storage beyond dimension five.

### G7 — optional workgroup scheduler

- compute-stage typed entry and invocation identity;
- workgroup storage and barrier lowering in Fe/WebGPU;
- task partitioning by dependency level or output blade;
- uniformity, occupancy, and shared-memory gates; and
- comparison against the single-invocation compiler on expressions large
  enough to amortize coordination.

This milestone is not required to call the one-invocation implementation
complete.

## Gallery/API outcome

The success condition is compact semantic Fe such as:

```fe
type DistanceExpr = ScalarProduct<
    Grade<Geometric<Reverse<Rotor>, Point>, 1>,
    Surface,
>
```

expanding to a powerful, inspected, browser-valid kernel without a handwritten
term table, generated `.fe` file, Rust algebra generator, JSON plan, runtime
manifest, or JavaScript math shim. The host may upload coefficients and launch
WebGPU; it must not know which blades survive or how the algebra is scheduled.

That is the same composting direction as the rest of the gallery: readable Fe
owns the semantics, Fe CTFE/FCO owns specialization, compiler-generated
artifacts carry the result, and Rust remains an independent oracle/toolchain
rather than shipped application logic.
