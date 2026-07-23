# Symbolic algebra atop Fe CTFE and FCO

Status: architecture research, 2026-07-23. This note describes the checked-in
compiler and experiments at `4727577d0`; it does not propose syntax changes.

## Conclusion

Fe can implement a useful **typed, bounded symbolic compiler** now. The most
credible first application is sparse Clifford algebra: construct a semantic
operator expression, compute finite support and coefficients with ordinary Fe
const functions, canonicalize it, materialize an inspectable typed plan, then
stage that plan to straight-line Fe code for Wasm and SPIR-V/WebGPU.

That is not yet a general-purpose computer algebra system in the
SymPy/Symbolics sense. Current Fe has enough machinery for closed, finite
algebras and quasi-static expressions, but lacks the general compile-time
collections, open symbolic values, constructor-pattern rewriting, normalized
plan reflection, and proof infrastructure needed to make arbitrary symbolic
algebra both ergonomic and trustworthy.

The recommended architecture is deliberately layered:

```text
typed user expression
  -> domain analysis (support, grade, scalar domain)
  -> canonical sparse polynomial/multivector
  -> chosen execution plan (tree | compact schedule | shared DAG)
  -> ordinary generated Fe
  -> normal Fe checking and MIR
  -> Wasm or legal non-recursive SPIR-V/WGSL
```

CTFE should compute algebraic facts. Recursive types should carry finite,
inspectable witnesses. FCO should own abstraction and code publication. None
of those three should be mistaken for an optimizer that automatically discovers
sharing.

## Evidence from current Fe

### What is working

1. **Value CTFE can perform real finite algebraic planning.**
   `type_schedule_cl41_pure.fe` computes Clifford signs, scans 50 canonical
   candidates, cancels 18 of them, and proves an exact 32-term survivor
   sequence. The browser Schedule32 source independently scans all 80 ordered
   `S * P * S` triples. Both use Fe const functions rather than a generated
   term table.

2. **Ground recursive type functions materialize typed expression trees.**
   `Schedule<32>` normalizes to a recursive `Add<Term<...>, ...>` type with no
   runtime type-function residue. The normalizer is iterative, has a 4096-step
   ceiling, and only accepts syntactically decreasing recursive calls. Recent
   invariant const parameters make shapes such as
   `Filter<Want, N>` possible while retaining one decreasing subject.

3. **Sparse support and grade pruning are expressible.**
   `sparse_cl41_grade1.fe` intersects candidate and grade masks, proves the
   resulting cardinality is five, computes compact ranks, supplies zero for
   absent blades, and rejects an unsatisfied present-only bound. This is
   functional but verbose and closed over a known support.

   The shared `sparse_clifford_api.fe` prelude removes that closed-world
   limitation for support planning. Its ordinary-Fe `BladeSet` represents up to
   five basis generators (32 blades) in an explicit portable `u32`;
   `support_gp` derives conservative product support for any orthogonal,
   possibly degenerate diagonal metric, while `support_grade` projects any
   support by computed blade grade. Both operations fail closed above the
   declared capacity. Ground results erase to constants: the differential Wasm
   `support_bladeset_ctfe.fe` probe and the canonical Schedule32 source both
   compose that same prelude. The differential Wasm test checks independent
   Rust oracles, including CGA Point*Sphere and a
   degenerate square, and rejects any runtime bitwise support interpreter.
   The canonical `S*P*S` plan now derives its candidate support with two
   `support_gp` applications, projects grade one, statically seals the resulting
   scalar mask, and applies it before its existing coefficient/sign survivor
   predicate. This is reusable support pruning, not coefficient cancellation:
   equal output blades may still cancel only after exact coefficients and signs
   are known.

4. **FCO can publish types and executable code.**
   Providers can inspect exact, already-materialized nominal constructors and
   literal const arguments, walk at most 256 type nodes in preorder, invoke
   bounded pure const helpers, publish associated types, and emit method bodies
   with hygienic untyped local `let` bindings. The shared-local facility is
   sufficient to express a runtime DAG once a planner has chosen it.

5. **Staged output reaches both browser backends through the ordinary
   compiler.** The existing Schedule32 source compiles through Fe MIR and
   Sonatina to Wasm and browser-valid WGSL. Therefore a symbolic layer does not
   need a separate GPU interpreter: it should emit finite, non-recursive Fe and
   let the existing backend path lower it.

### Exact boundaries

1. A provider sees source-level ground nominal types during expansion. It
   cannot semantically normalize `Schedule<32>` and inspect the resulting
   `Add/Term` tree. Provider expansion precedes merged semantic analysis;
   calling the existing normalizer there creates
   `expanded_items -> analysis -> scope_graph -> expanded_items`. See
   `FCO_PLAN_BRIDGE_OPTIONS.md`.

2. Provider ground-type inspection is intentionally syntactic and fail-closed.
   It does not resolve aliases or evaluate computed const arguments. Its
   preorder walk is capped at 256 nodes.

3. The provider command language is not general Fe CTFE. It has fixed
   reflection sequences and `for`, but no arbitrary growable collection,
   hash map, union-find, user-defined mutable graph, or recursion. Pure const
   helpers are non-recursive, capped at 32 calls deep, and currently reject
   array indexing. A provider run is capped at 100,000 evaluation steps and
   10,000 commands/generated expressions.

4. Recursive type functions are a restricted total language, not general term
   rewriting. They match integer arms on one decreasing subject; they do not
   pattern-match arbitrary expression constructors. Ground applications
   normalize; symbolic applications can participate in the dedicated
   induction engine but cannot be stored as open type-level programs.

5. FCO does not perform common-subexpression elimination. The measured direct
   and FCO-published Schedule32 trees have the same RMIR size and call count.
   The tiny shared-DAG control is small because its provider explicitly emits
   named shared values.

These are implementation boundaries, not evidence that a new dependent type
system or closures are required. They do mean that “a general CAS atop current
CTFE” must not be claimed before the missing planning substrate exists.

## Proposed IRs

### 1. Typed semantic expression

Use ordinary nominal types as a closed first IR:

```fe
struct Scalar<const I: usize> {}
struct Input<const I: usize, Domain> {}
struct Sum<A, B> {}
struct Product<A, B> {}
struct Neg<A> {}
struct Reverse<A> {}
struct Grade<const K: usize, A> {}
struct GeometricProduct<Metric, A, B> {}
struct Sandwich<A, X> {}
```

The type parameters retain domain, metric and operation distinctions. The
surface library can expose ordinary functions/traits such as `gp`, `reverse`,
`grade`, and `sandwich`; callers should not construct the nodes directly.
Operator composition can therefore be represented without closures. Closures
would improve ordinary higher-order authoring and tracing, but are not required
for a first explicit combinator API.

Today this IR is useful when it is ground and bounded. A provider cannot yet
walk through a type-function application that produces it, and current
recursive type functions cannot define general constructor-pattern rewrite
rules over it.

### 2. Canonical sparse algebra

For Clifford work, lower semantic expressions to sorted terms:

```text
Term {
  output blade,
  sorted scalar monomial/factor IDs,
  exact coefficient,
}
Canonical = sorted unique keys, zero coefficients removed
```

Represent support separately as a bitset where the algebra dimension permits
it. Grade selection is then a mask intersection; compact rank is the popcount
below a blade. This is already demonstrated for Cl(4,1). For larger algebras,
use a fixed-capacity sorted sequence rather than pretending one machine integer
is universal.

Coefficient policy must be explicit:

- Clifford sign/multiplicity: small signed integer, exact.
- Polynomial coefficients: bounded signed integer or rational
  `(sign, numerator, denominator)` with checked normalization.
- Floating constants: leaves that are not used to establish symbolic equality.

The current packed base-5 `u256` representation is a good closed experiment,
not the public general representation: its capacity and coefficient range are
implicit and domain-specific.

### 3. Execution plan

Keep algebraic normal form distinct from execution shape:

- **Tree:** simplest typed witness; useful for inspection and proofs, currently
  expensive after monomorphization.
- **Compact schedule:** fixed records plus a bounded runtime loop; smallest
  compiler representation, but retains runtime selection/indexing.
- **Shared DAG:** stable node IDs, topological order, and named locals; best
  candidate for shader output when reuse is material.

This separation lets one canonical polynomial feed several interpreters and
makes performance comparisons semantically meaningful.

## Canonicalization and rewriting

### Deterministic normalization first

The first symbolic layer should be a normalizer, not a heuristic simplifier:

1. recursively normalize children;
2. flatten associative `Sum` and scalar-commutative `Product`;
3. sort by a documented total key;
4. combine equal keys in an exact coefficient domain;
5. erase exact zeros;
6. apply domain rules such as Clifford blade multiplication, grade filtering,
   reverse, and involution;
7. assign stable IDs only after canonicalization.

This gives a unique normal form within the deliberately supported theory.
Structural type equality or element-by-element codes can then decide equality
of two normalized closed expressions.

Do not start with a generic `simplify`. SymPy documents both a heuristic
general simplifier and separate functions that promise particular canonical
forms; the latter is the trustworthy model for compiler staging.

### Rewriting second

A bounded rewrite engine can be added over a value-level node arena:

```text
Pattern = Op(tag, child patterns) | Bind(slot) | Const(predicate)
Rule    = (pattern, replacement, side-condition, rule identity)
```

Use explicit prewalk/postwalk/fixpoint combinators, a maximum pass count, a
maximum node count, and deterministic rule order. Record a trace containing
rule identity and before/after node IDs. SymbolicUtils uses similarly explicit
rewrite combinators; this is preferable to hiding strategy inside operator
overloading.

Current Fe cannot implement this general arena ergonomically inside FCO's
restricted provider executor. It can implement closed rewrite families as
ordinary const functions over packed/fixed-capacity data, or type-specific
selection with recursive type functions. A reusable engine needs fixed-capacity
CTFE collections and indexing, or a compiler-owned/domain library interpreter.

## Equality and proof strategy

Fe is not currently a proof assistant, so distinguish four claims:

1. **Definitional equality:** two canonical ground type/value representations
   are identical after compiler normalization.
2. **Checked invariant:** `static_assert` proves cardinalities, masks, exact
   coefficient encodings, and bounded plan properties by CTFE evaluation.
3. **Differential semantic evidence:** execute canonical plan, raw expansion,
   generic Fe interpreter, Wasm, GPU, and independent host oracle and require
   identical results in an exact domain or documented tolerance.
4. **Theorem-level proof:** a proof-producing or externally verified rewrite
   kernel. Fe does not provide this today.

For the first system, every rewrite rule should be a named, reviewed law with
focused property/exhaustive tests over its finite domain. A compact replayable
rewrite trace makes failures diagnosable, but is not itself a formal proof.

A later reflective proof kernel could re-check each rule application, following
the architecture used by proof-by-reflection systems: an untrusted planner
produces a certificate and a small trusted evaluator checks it. That is a
separate milestone, not something current `static_assert` implies.

## DAGs, hash-consing and e-graphs

### Shared DAG

Hash-cons canonical nodes by `(operator, result type/domain, child IDs,
constant payload)`, then topologically emit one Fe `let` per live non-leaf.
Use deterministic open addressing or a sorted lookup and explicit maximum
load/node counts. Extraction should include target-aware costs:

- arithmetic operation weights;
- expensive divisions/square roots;
- live-value pressure;
- shader branch/index penalties;
- reuse count and emitted byte count.

FCO's hygienic local emission is ready for the last stage. The missing pieces
are the reusable node arena/hash-cons planner and, when the input is a
`Schedule<N>` normal form, the base-only semantic normalization bridge.

### E-graphs

Do not make equality saturation the first implementation. `egg` and
Metatheory.jl show why e-graphs are valuable: they retain many equivalent forms
and extract one under a cost model instead of committing to rewrite order.
They also require congruence closure, e-matching, analyses, union-find, and hard
iteration/node/time limits; saturation is not generally guaranteed.

Current provider CTFE has neither the collections nor the resource model for an
honest e-graph. A future bounded e-graph belongs in a compiler/library CTFE
arena with explicit limits, or initially in a host-side research oracle. It
should consume the same typed canonical IR and emit the same checked plan
format, so it can be compared without becoming the semantic authority.

## Staging and target legality

The symbolic engine should never emit recursion for GPU execution. Compile-time
recursion builds a finite plan; staging produces:

- straight-line locals for a DAG;
- a statically bounded loop for a compact schedule; or
- a finite balanced expression tree.

The generated Fe method is then type-checked normally and lowered through MIR.
Backend legality remains the backend's responsibility. This works for Wasm and
SPIR-V/WebGPU and avoids inventing “tail recursion on the GPU,” which WebGPU
does not need for these finite plans.

Every generated artifact should carry provenance:

```text
normalizer version
metric and scalar-domain IDs
source-expression hash
canonical-plan hash and counts
rewrite rule-set hash and trace hash
chosen execution strategy and cost
compiler/Sonatina revisions
```

## NTT and MSM as tests of the same architecture

NTT and multi-scalar multiplication are useful stress tests because they share
the need for exact algebra, static decomposition, parallel scheduling, and
target-aware code generation, while differing sharply in what is known at
compile time.

They should share infrastructure, not be forced into one domain IR:

```text
SymbolicCore
  scalar domains: Semiring, Ring, Field
  actions:        Module/ScalarAction, AdditiveGroup
  expression:     Input, Const, Add, Sub, Mul, Neg, Apply
  structure:      Product, Composition, Permutation, Reduce
  plan:           Stage, Parallel, Sequence, Barrier, BufferView

Clifford layer: basis blades, metric, grade, involutions
NTT layer:      roots of unity, butterflies, permutations, twiddles
MSM layer:      scalar decomposition, windows, buckets, group reductions
```

The symbolic core owns exact laws and types. Domain layers own side conditions.
The backend scheduler owns memory layout, fusion, workgroup size and placement.

### Conal-style NTT

Conal Elliott's FFT formulation is especially well aligned with Fe's existing
`Par`, `Pair`, `Comp`, and `RBin` vocabulary. The paper treats functor
composition as a statically typed alternative to run-time size factoring. Its
composition case has the high-level form:

```text
FFTs over inner structure
  -> transpose
  -> elementwise twiddle
  -> FFTs over outer structure
```

An NTT is the same transform factorization over a finite field, provided the
field and transform size admit the required primitive root of unity. Therefore
the symbolic plan should preserve decomposition, not immediately expand into
indexed butterflies:

```fe
struct Ntt<Field, const N: usize, Direction> {}
struct Factor<Outer, Inner> {}
struct ParallelMap<F, Op> {}
struct Transpose<Outer, Inner> {}
struct Twiddle<Field, Outer, Inner, Direction> {}
struct Compose<A, B> {}
```

Again this is an API sketch using existing nominal types, not proposed syntax.
A ground `Ntt<F, 16, Forward>` can normalize to a chosen radix/composition
shape. CTFE can check:

- `N` and the chosen radices multiply exactly;
- the root has exact order `N` in the configured field;
- each twiddle exponent is correct modulo `N`;
- permutation maps are bijective;
- forward/inverse scaling conventions agree.

Canonical algebraic rewriting includes:

- Cooley-Tukey factorization under its field/root preconditions;
- twiddle exponent normalization;
- identity-stage removal;
- permutation composition;
- inverse/forward cancellation only when normalization/scaling permits it.

Backend optimization is separate:

- choose DIT versus DIF and radix sequence;
- precompute versus generate twiddles;
- fuse adjacent butterfly stages;
- choose in-place versus ping-pong buffers;
- map stages to SIMD, Wasm loops or WebGPU workgroups;
- choose workgroup-local versus storage-buffer traffic.

The distinction is essential. “These two plans compute the same NTT” is an
exact algebraic claim. “This radix/fusion is cheaper on this GPU” is a measured
cost-model decision.

The current Fe type schedule is enough to encode a small static factorization.
The current `std::conal` implementation is not a complete generic FFT/NTT
library: its own module documentation says the deeper combinator-functor
instances and structure-recursing algorithms are later work. A real NTT also
needs a finite-field implementation and arrays/buffer views with proven bounds.

### MSM

For points `P_i` and scalars `s_i`, MSM computes the additive-group expression
`sum_i s_i P_i`. The exact symbolic layer can represent linear combinations and
prove decomposition identities. A Pippenger-style execution plan splits scalar
digits into windows, accumulates equal digits into buckets, reduces buckets,
and combines windows with repeated doubling.

The crucial staging distinction is:

- curve parameters, scalar bit width, point count bounds, coordinate formulas,
  window width, bucket count and reduction topology can be compile-time facts;
- bucket membership normally depends on runtime scalar digits and therefore is
  runtime work, not a CTFE-expanded symbolic term;
- fixed-base or fixed-scalar specializations may move more work to CTFE, but
  must be named separately and assessed for memory and side-channel behavior.

A static plan can look like:

```text
MSM<Curve, Scalar, N>
  -> DecomposeScalars<WindowBits>
  -> Parallel<WindowCount,
       BucketAccumulate<BucketCount, Chunking>>
  -> Parallel<WindowCount, BucketReduce<ReductionTree>>
  -> HornerWindows<WindowBits>
```

Exact rewrites include:

- scalar radix decomposition and recomposition;
- group identity elimination;
- associativity-respecting reduction regrouping;
- signed-digit correction under an explicit encoding;
- projective-to-affine conversion laws under explicit nonzero conditions.

Backend choices include:

- window width and chunk size;
- affine/projective/mixed coordinate formula selection;
- bucket storage layout;
- contention strategy and partial-bucket merge tree;
- CPU versus Worker versus GPU placement;
- batch inversion strategy and memory/recomputation trade-offs.

The planner must not use commutativity or a curve formula unless the relevant
group/field law and formula preconditions are in the domain contract. It must
also retain exceptional-case semantics; a faster incomplete addition formula
is not an algebraically equivalent replacement on arbitrary inputs.

### Shared DAG and cost models

NTT and MSM reuse the same plan/DAG machinery but need different canonical
keys:

- NTT nodes key on operation, field, input lane IDs and twiddle identity.
  Sharing may reuse roots, twiddle powers, address calculations and subtransforms.
- MSM nodes key on curve operation, coordinate representation, point/value
  identity and scalar digit/window. Runtime bucket mutation is an effectful
  schedule node, not a pure expression eligible for unrestricted hash-consing.

The generic cost algebra should accumulate a vector, not one magic integer:

```text
(field adds, field muls, reductions, inversions,
 group adds, group doubles,
 loads, stores, barriers, transfers,
 live bytes, critical depth, emitted bytes)
```

Extraction then applies a target/profile-specific weighting. Algebraic
normalization must not depend on those weights.

For NTT, known factorizations provide the decomposition candidates up front;
the symbolic engine need not discover FFT from distributivity. For MSM, known
window/bucket decompositions likewise provide plan families. This is exactly
the useful middle ground suggested by the CGA work: encode reviewed
decompositions directly, use CTFE to specialize and check their side
conditions, and use the cost model to select among valid shapes.

### CPU, Wasm and WebGPU legality

- CPU/Wasm can execute compact bounded loops and ordinary non-recursive
  functions. SIMD and threads require separate capability checks.
- WGSL forbids direct and indirect recursion, so compile-time recursive
  decompositions must become acyclic functions, loops or straight-line stages.
- NTT maps naturally to a sequence of compute dispatches or fused stage groups
  with explicit barriers and ping-pong buffers.
- MSM bucket accumulation introduces writes and possible contention. A WebGPU
  implementation needs an explicit race-free design; “parallel buckets” alone
  is not a legality proof.
- Real cryptographic fields and curves usually require multi-limb arithmetic
  and carefully bounded reductions. Browser-valid WGSL is not evidence of
  constant-time behavior, subgroup correctness, side-channel resistance, or
  cryptographic suitability.

### Actors and effects

The existing Fe actor direction can make a staged plan explicit rather than
framework-magical:

```text
Wasm planner/orchestrator actor
  owns canonical plan + provenance
  allocates/validates canonical buffers
  sends one transferable input ownership message

WebGPU executor actor
  owns device, pipelines, buffer capabilities
  executes Stage/Barrier graph without intermediate readback
  reports completion/timestamps or final owned output

Worker host actor
  performs CPU/Wasm fallback or independent verification
  is supervised and backpressured by bounded requests
```

Plan effects should state reads, writes, storage class, placement and barrier
requirements. Actor capabilities should prevent a pure rewrite from acquiring
buffer mutation, and prevent two actors from claiming exclusive mutation of
the same resource. NTT stages can transfer buffer ownership once and retain it
through all GPU stages. MSM can partition point/scalar ranges into owned chunks,
produce partial bucket buffers, and reduce them in an explicitly supervised
merge stage.

This is a good application of Fe effects: the algebra stays pure, while
dispatch, mutation, synchronization, transfer and readback are visible in the
plan and actor interfaces.

### Minimal NTT spike

Implement a non-cryptographic 16- or 32-point forward/inverse radix-2 NTT over a
small prime field:

1. express the transform as `RBin<Pair, depth>`/composition plus typed
   butterfly, transpose/permutation and twiddle stages;
2. CTFE-check the primitive root order, all twiddle exponents, permutation
   bijection and inverse round trip;
3. generate three executions from one plan: scalar Fe/Wasm, compact iterative
   Wasm, and WebGPU compute;
4. compare every output against an independent quadratic DFT/NTT oracle;
5. report operation counts, stage depth, memory traffic, artifact size and GPU
   completion timing.

Acceptance is exact field equality for exhaustive small vectors or a documented
large deterministic corpus. It is not a claim about production-size polynomial
commitments, cryptographic parameters or GPU competitiveness.

### Minimal MSM spike

Implement a tiny MSM over a deliberately non-production toy curve/field, for
example 8 or 16 points with bounded scalars:

1. define checked field, curve, point-at-infinity and complete group semantics;
2. derive a fixed-window bucket plan with CTFE-proven digit recomposition,
   bucket/window counts and static resource bounds;
3. run naive double-and-add, serial bucket, Worker-partitioned bucket, and
   race-free WebGPU plans from the same input/curve contract;
4. compare exact affine results, including zero scalars, identity points,
   duplicates, inverses and exceptional additions;
5. record group/field operation counts, bucket occupancy, transfers, memory,
   stage depth and GPU timing.

This spike tests planning and actorized execution only. It must be labelled
non-cryptographic until real field arithmetic, subgroup validation,
constant-time/side-channel analysis, formula completeness, adversarial tests,
and independent implementation review are supplied.

## No-syntax-change API

A realistic high-level Clifford interface is:

```fe
type C = Clifford<Cl41, F32>
type S = SphereSupport
type P = PointSupport

fn invert_point(s: C::Mv<S>, p: C::Mv<P>) -> C::Mv<VectorSupport> {
    C::sandwich(s, p)
}

derive SpecializedKernel<
    expr = Sandwich<SphereInput<0>, PointInput<0>>,
    outputs = Grade<1>,
    strategy = SharedDag,
> for InvertPoint
```

Exact derive argument syntax must follow surfaces Fe already accepts; this is
an API shape, not a syntax claim. An immediately implementable variant puts
configuration in nominal types:

```fe
struct InvertPointSpec {}
impl SymbolicSpec for InvertPointSpec {
    type Expr = Sandwich<Sphere0, Point0>
    type Outputs = Grade1
    type Strategy = SharedDag
}
derive SpecializedKernel for InvertPointSpec using CliffordKernelProvider
```

The derived trait can publish `Input`, `Output`, `CanonicalPlan`, provenance
constants, and an ordinary `eval` method. Semantic point/sphere constructors
hide recursive storage exactly as the existing FCO constructor spike already
demonstrates.

## Resource and diagnostic contract

Every symbolic phase must be total under declared limits:

- maximum semantic-expression nodes;
- maximum expanded monomials before combination;
- maximum support cardinality and coefficient width;
- maximum normalization/rewrite passes;
- maximum DAG nodes and hash-table probes;
- existing type-function unfold fuel;
- existing FCO step, command, helper-depth, and inspection limits.

On exhaustion, report the phase, observed count, limit, expression/operator
path, and useful mitigation (grade restriction, alternate strategy, larger
explicit profile). Never silently fall back from sparse specialization to a
dense expansion or from shared DAG to a runtime selector.

Plan diagnostics should include candidate/survivor/cancelled counts, per-grade
support, term counts per output lane, estimated operation counts, actual
emitted MIR/WGSL counts, and the first mismatching canonical term when an oracle
fails.

## Three minimal spikes

### Spike 1: canonical sparse polynomial kernel

Implement a library-only, fixed-capacity canonicalizer for a small exact scalar
domain. Inputs are explicit symbolic terms; output is a sorted, combined,
zero-elided canonical sequence plus support and grade masks.

Acceptance:

- add/multiply/grade/reverse on Cl(3,0) and Cl(4,1);
- deterministic byte-for-byte plan hash;
- exhaustive equality against a dense/raw oracle for bounded small inputs;
- explicit overflow and resource diagnostics;
- no Python or external generated term table.

This determines whether current value CTFE aggregates/indexing suffice or
whether one small CTFE collection primitive is truly needed.

### Spike 2: one canonical IR, three execution strategies

Feed exactly the same canonical plan into tree, compact schedule, and explicit
shared-DAG emitters. The DAG planner hash-conses repeated scalar products and
emits hygienic locals through FCO.

Acceptance:

- identical Fe/Wasm results for all strategies;
- browser-valid WGSL for compact and DAG;
- recorded CTFE time, HIR/MIR size, Wasm/WGSL bytes, arithmetic counts, and GPU
  timestamp results;
- the Schedule32 full-frame oracle remains exact;
- no strategy-specific semantic table.

If FCO must consume the normalized recursive type, this spike also implements
the narrowly scoped base-only normalization island described in
`FCO_PLAN_BRIDGE_OPTIONS.md`. Otherwise it may consume the explicit canonical
value plan as a temporary, clearly labelled bridge.

### Spike 3: bounded rewrite engine and certificate replay

Add typed patterns and deterministic prewalk/postwalk/fixpoint rewriting over a
small fixed node arena. Include algebra-neutral identities and Clifford laws;
emit a trace; replay each step in a separate checker.

Acceptance:

- rule side conditions and scalar domains are explicit;
- pass/node/step limits fail closed with useful diagnostics;
- replay rejects a mutated rule ID, substitution, or result;
- compare deterministic normalization with bounded equality saturation in a
  host-side oracle on the same tiny expressions;
- decide from measurements whether an Fe-native e-graph is justified.

## Recommendation

Proceed, but name the initial project accurately: **Fe symbolic
specialization**, not “a general CAS.” Build the canonical sparse kernel first,
because it directly improves CGA/QCGA and establishes the required semantics.
Keep canonical algebra, rewrite search, and execution scheduling separate.
Treat FCO as the publication/code-emission layer and recursive types as
inspectable witnesses. Add an e-graph only after deterministic normalization
and shared-DAG staging are measured and trustworthy.

## Local evidence

- `crates/hir/tests/fixtures/type_schedule_cl41_pure.fe`
- `crates/hir/tests/fixtures/fco_cl41_schedule_probe.fe`
- `crates/hir/tests/fixtures/sparse_cl41_grade1.fe`
- `crates/hir/tests/sparse_bitset_support_probe.rs`
- `crates/hir/tests/invariant_const_selector_probe.rs`
- `crates/hir/tests/provider_ground_type_inspection.rs`
- `crates/hir/src/core/lower/provider_executor.rs`
- `crates/hir/src/analysis/ty/type_fn.rs`
- `docs/mb2/FCO_SPARSE_CONSTRUCTOR_SPIKE.md`
- `docs/mb2/FCO_PLAN_BRIDGE_OPTIONS.md`
- `docs/mb2/SCHEDULE_STRATEGY_COMPARISON.md`
- `demos/webgpu-cga-inversion/gen-schedule32/actor-source.fe`

## External design references

- Symbolics.jl separates symbolic IR from target-specific function generation
  and supports sparse output/code generation:
  <https://docs.sciml.ai/Symbolics/stable/manual/build_function/>
- SymbolicUtils exposes explicit prewalk, postwalk, chains and fixpoint rewrite
  strategies:
  <https://docs.sciml.ai/SymbolicUtils/stable/manual/rewrite/>
- SymPy distinguishes heuristic `simplify` from operations with documented
  canonical results:
  <https://docs.sympy.org/latest/tutorials/intro-tutorial/simplification.html>
- `egg` describes equality saturation as retaining equivalent terms and
  extracting by a cost function:
  <https://egraphs-good.github.io/egg/egg/tutorials/_01_background/>
- Metatheory.jl exposes explicit iteration, e-class and timeout limits and
  notes that saturation termination is not generally known:
  <https://juliasymbolics.github.io/Metatheory.jl/dev/egraphs/>
- Lean's metaprogramming architecture is a useful reference for reflected
  object expressions and checked automation, but Fe does not currently supply
  Lean's dependent proof kernel:
  <https://lean-lang.org/papers/tactic.pdf>
- Conal Elliott's local paper motivates typed functor composition and static
  shape factorization for FFT:
  `/workspace/generic-parallel-functional.pdf`
- The Archive of Formal Proofs NTT entry demonstrates that butterfly NTT
  correctness and its finite-field side conditions can be formalized:
  <https://www.isa-afp.org/entries/Number_Theoretic_Transform.html>
- A recent Pippenger MSM description explicitly separates scalar windows,
  bucket accumulation and weighted bucket reduction:
  <https://eprint.iacr.org/2024/1246.pdf>
- The normative WGSL specification rejects cycles among declarations,
  including direct or indirect function recursion:
  <https://gpuweb.github.io/gpuweb/wgsl/>
