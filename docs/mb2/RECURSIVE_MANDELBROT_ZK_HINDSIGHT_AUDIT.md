# MB2 recursive Mandelbrot ZK proof: architecture, hindsight, and WebGPU consolidation audit

Date: 2026-09-02

Repository snapshot: `/workspace/fe-worktrees/mb2`, branch `mb2`, HEAD
`4ee5d00f7` (`test(proof): gate committed recursive chunks`). The working tree
also contains an active, uncommitted WebGPU interaction-trace increment. This
document records an audit of that snapshot. It is a companion to, not a
replacement for, the authoritative burn-down in
`docs/mb2/FE_WEB_ROLLCALL_CAMPAIGN_SSOT.md`.

The WebGPU capability comparison uses
`/workspace/fe-worktrees/quilting-webgpu-api`, branch
`codex/quilting-webgpu-api`, committed HEAD `1b24aeb37`, plus its explicitly
uncommitted mixed-storage compiler slice. That branch is WIP and based on the
older MB2 commit `2d028eac9`, so none of its proposed APIs should be treated as
settled.

## Executive judgment

The proof work is real Fe work. The Mandelbrot recurrence, high-precision
fixed arithmetic, AIR, BabyBear and quartic field operations, Poseidon2,
Merkle trees, Fiat-Shamir transcript, FRI schedule, canonical receipt,
security-sized scalar prover and verifier, recursive interval semantics,
recursive merge relation, and almost all WebGPU proof scheduling are authored
in Fe. Rust supplies compiler implementation and independent oracles. The
browser JavaScript supplies standards mechanics. Neither Rust nor JavaScript
contains a second shipped proof algorithm.

The project has crossed several hard boundaries:

- A complete one-transition, 114-query BabyBear receipt is generated and
  verified by separate zero-import Fe Wasm artifacts. The prover generated a
  canonical 948,808-byte receipt in 539.32 seconds; the fresh verifier accepted
  it and rejected the typed mutation matrix in 21.05 seconds.
- The production AIR was reduced from maximum expression degree 19 and an
  invalid 73,709 composition bound to degree 2 and a valid 4,095 bound for the
  4,096-row trace. The current shape is 691 constraints with family degrees
  `[2, 2, 1, 1]`.
- The production-sized thirteen-round FRI actor runs in browser WebGPU. A
  Chrome readback matched an independent Plonky3 recurrence across the complete
  1,688,956-byte immutable buffer receipt. A broader 49-pass software-WebGPU
  gate matched all AIR LDE values, composition lanes, commitments, transcript,
  FRI layers, and openings against direct-DFT and Plonky3 models.
- The recursive semantic contract, fixed-size boundary commitments, private
  verified-leaf authority, adjacent merge, 423-product and 583-assertion merge
  relation, Poseidon quadratic relations, and a staged security-verifier task
  relation exist. The newly focused gate
  `recursive_committed_chunk_preserves_certified_boundaries` passes in release:
  1 passed, 13 filtered, 121.67 seconds.

The central unfinished fact is equally clear: there is no recursive
cryptographic parent receipt yet. The current merge can combine two values that
were admitted by the private verified-leaf boundary, and the verifier and merge
relations can be written and replayed as traces, but the complete child
verification traces are not yet authenticated inside a parent STARK. The SSOT
identifies two fixed stages plus all 114 query relations per child as still
unbound. Until those relations are committed, proved, and folded into a parent
receipt, “recursive Mandelbrot proof” describes the architecture and semantic
relation, not a generated recursive proof.

The present direction is not a dead end. The work has deliberately built the
right reusable substrate: one Fe denotation, multiple interpreters for value,
witness, constraints, degree, scalar placement, WebGPU placement, and replay.
The strongest hindsight lesson is about staging that substrate. Type-level
structure should derive compact plan shape and correctness evidence. Runtime
Fe loops and typed memory should execute large policy-sized plans. Fully
specializing every receipt field, FRI query, Merkle path, or relation row into
the compiler graph caused 15 to 20 MB Wasm modules, multi-gigabyte compiler
peaks, and hour-scale gates without increasing proof assurance.

The WIP Quilting WebGPU capability work should be integrated, but selectively.
Its `GpuLayout` evidence and intended resource custody, access, residency,
initialization, recovery, and effect model line up directly with proof needs.
Its current compiler implementation is not yet broad or reconciled enough for
wholesale import, and its stale branch base makes a merge especially risky.
The right move is to land its small, independently gated capability slices on
current MB2, then migrate one proof buffer boundary at a time while retaining
the existing exactness receipts and the portable eight-binding limit.

## Sources and audit boundary

The principal intent and status sources were:

- `FE_NATIVE_GALLERY_PLAN.md`, especially the proof capstone requirements,
  Fe-native ownership rules, and the explicit separation of semantic topology
  from placement topology.
- `docs/mb2/FE_WEB_ROLLCALL_CAMPAIGN_SSOT.md`, especially G-RECEIPT,
  G-RECURSE, the WebGPU proof track, the browser gate, and immediate burn-down.
- `docs/mb2/MANDELBROT_BOUNDED_PROOF_SPEC.md`, which fixes the claim semantics,
  numeric model, transcript order, and the distinction between finite bounded
  survival or escape and Mandelbrot membership.
- Fe packages under `demos/capstones/mandelbrot-proof/`, plus reusable proof
  packages under `ingots/`.
- Rust compiler gates under `crates/codegen/tests/`, including the 6,314-line
  `mandelbrot_recursive_fixed_oracle.rs` and browser/WebGPU fixtures.
- Recent proof and WebGPU history through `4ee5d00f7`.
- `docs/mb2/FE_WEBGPU_CAPABILITY_SURFACE.md`, the committed `GpuLayout` slice,
  and the uncommitted mixed-resource compiler slice in
  `/workspace/fe-worktrees/quilting-webgpu-api`.

No expensive build was run for this audit. Existing measured gates and source
evidence are reported as such. The active MB2 worktree was not modified.

## Intended architecture and actual dataflow

The project’s intended invariant is more specific than “write it in Fe.” One
ordinary typed Fe denotation fixes meaning. FCO and CTFE derive exact static
structure. Multiple interpreters analyze or place that same structure. The
compiler lowers the result. Fixed host adapters realize browser or GPU
standards, and independent models try to falsify the result.

The current proof dataflow is:

```text
high-precision public claim + exact orbit boundary
    |
    | Fe `Fixed<L>` and exact recurrence
    v
one-transition sparse witness, 4,096 rows at L = 4
    |
    | shared semantic task and quadratic plans
    +--> wide value interpreter and independent integer checks
    +--> six-column sparse witness plus 192-column planned base row
    +--> constraint interpreter, 691 constraints, max degree 2
    +--> degree, liveness, and placement interpreters
    v
base LDE, LD01 commitment
    |
    | transcript derives quartic interaction challenges
    v
152-column interaction trace, interaction LDE, LD02 commitment
    |
    | public relation + four zerofier families
    v
quartic composition codeword, BC02 commitment
    |
    | Fe-derived FRI schedule and security profile
    v
13 FRI rounds + 114 transcript queries + canonical Merkle multipaths
    |
    | reflection-derived staged canonical codec
    v
948,808-byte security-sized leaf receipt
    |
    | separate Fe verifier mints private authority
    v
VerifiedRecursiveInterval<L>
    |
    | adjacent statement/boundary equality + exact merge relation
    v
merged committed interval
    |
    | still missing: parent proof authenticating both child verifier traces
    v
future compact recursive parent receipt
```

There are currently three related but distinct meanings of “recursive”:

1. `recursive/src/lib.fe` supplies field-neutral exact chunk semantics over
   const-generic 13-bit limb vectors. It can replay a chunk and merge adjacent
   certified intervals.
2. `recursive-baby-bear` commits a statement and its endpoint boundaries to
   fixed-size typed Poseidon2 digests. `recursive-verifier-baby-bear` protects
   admitted leaves behind the private `VerifiedRecursiveInterval<L>`
   constructor and permits only exact adjacent merges.
3. `recursive-air-baby-bear` and `recursive-verifier-air-baby-bear` express the
   merge and child-verifier work as quadratic relations and typed task traces.
   This is the circuit material needed for recursive proving, but it is not yet
   committed and proved as a parent receipt.

Keeping those levels distinct is essential. A semantic accumulator is not a
cryptographic accumulator. A private constructor prevents accidental use by
ordinary Fe code, but it does not make a proof recursively succinct. A replayed
relation trace is not authenticated until the parent proof commits and opens
it under its own soundness argument.

## What is actually Fe-authored

| Layer | Fe owns | Non-Fe owns | Audit judgment |
|---|---|---|---|
| Claim and arithmetic | `HighPrecisionEscapeClaim<L>`, exact limb arithmetic, rounding, normalization, chunk replay, boundary equality | Rust bigint independently reconstructs values | Genuine Fe semantics, independently checked |
| AIR | sparse task topology, witness rows, control rows, copy buses, quadratic DAGs, constraints, degree interpretation | Rust independently enumerates rows and evaluates mutations | Genuine Fe relation, no host constraint table |
| Field and hashes | BabyBear, quartic extension, Grain-derived Poseidon2 constants, permutation, Merkle topology | Plonky3 and BigUint are oracles | Genuine Fe crypto, independent constants and value checks |
| Transcript and FRI | nominal domains, ordering, schedule, query policy, fold chain, request sets, paths | Rust independently derives transcript and Plonky3 recurrence | Genuine Fe protocol ownership |
| Receipt | nominal carriers, capacities, staged canonical codec, verifier | host copies opaque bytes only | Genuine Fe schema and verification |
| Recursive layer | boundary digests, private admitted-leaf type, adjacent merge, merge relation, verifier task relation | Rust drives independent mutation and execution gates | Real partial recursion substrate, parent proof missing |
| WebGPU schedule | Fe actor passes, typed workgroup/dispatch/cycle/taper/cooperative policies, FRI and Merkle placement | compiler emits WGSL and physical plan; browser submits commands | Application policy remains Fe-owned |
| Browser runtime | Fe actors own input, scheduling, retry, cancellation policy where wired | JS obtains device, allocates/binds buffers, submits passes, observes device loss, bridges events | Standards adapter is legitimate, but the JSON manifest remains a temporary architectural seam |
| Compiler | Fe source is type checked and specialized; providers generate evidence | Rust implements HIR/MIR, Salsa queries, Sonatina lowering, SPIR-V/WGSL/Wasm emission | Toolchain, not a second application implementation |

The anti-sham conclusion is therefore favorable but qualified. The host does
not contain Mandelbrot, Poseidon, FRI, or receipt logic. It does carry a
compiler-generated JSON description of passes, resources, and layouts. That is
not application authoring, but it is still an extra runtime protocol format and
conflicts with the final gallery goal of eliminating runtime render manifests.
It should be composted after the typed physical plan and resource capability
surface are stable.

## Independent oracle boundaries

The test strategy is unusually strong because it does not confuse backend
parity with independence.

### Independent semantic truth

- Rust `BigUint` or integer models reconstruct exact fixed-point arithmetic,
  signed normalization, carries, rounding, task rows, and recursive endpoints.
- Plonky3 supplies an independent BabyBear/Poseidon2/Merkle and FRI value
  implementation.
- Direct DFT and u64 field models check NTT and LDE independently of the Fe
  transform implementation.
- Mutation gates alter semantic fields, paths, roots, challenges, transcript
  bindings, row lanes, and relation nodes. They check rejection, not merely
  byte equality.
- Browser buffer comparison inspects the complete device result rather than
  trusting a green completion pixel. This already found a real structured-loop
  SPIR-V bug that the liveness signal could not reveal.

### Cross-backend parity, useful but not independent

- The same Fe denotation lowered to Wasm and WebGPU is excellent evidence that
  the compiler preserves semantics across backends.
- It is not an independent cryptographic oracle because both paths share Fe
  source and compiler analysis.
- Chrome, llvmpipe, SwiftShader, and native wgpu exercise different executors,
  but only a separately implemented value model establishes proof exactness.

### Oracle hygiene to preserve

Do not generate the Rust oracle’s constants, layouts, or expected values from
the same Fe provider being tested. Shared protocol specifications can guide
both implementations, but independent spelling is a feature. The 6,314-line
recursive Rust oracle is large partly because it is a true second model. Its
size is an iteration problem, not evidence of a shipped shim.

## Status by architectural gate

This section summarizes evidence; the SSOT remains the authoritative checklist.

### Done

- Exact high-precision field-neutral chunk semantics using `Fixed<L>` with
  13-bit limbs.
- Fixed-size, nominally domain-separated statement and boundary digests.
- A private verified-leaf authority whose only constructors are actual receipt
  verification and valid adjacent merge.
- Exact recursive merge relation with integer, modular-wrap, coherent-rewire,
  and boundary mutation gates.
- Complete one-transition sparse BabyBear AIR at L4, 4,096 base rows, 8,192 LDE
  points, 691 constraints, and degree-2 composition within the trace bound.
- Grain-derived Poseidon2, typed Merkle trees, quartic transcript challenges,
  production query policy, thirteen-round FRI, canonical multipaths, staged
  receipt codec, and separate scalar prover/verifier artifacts.
- Security-sized 114-query scalar receipt generation and verification.
- Production FRI WebGPU actor with typed cycle, taper, and cooperative pacing.
- Independent Chrome buffer exactness for the complete production-sized FRI
  recurrence on a deterministic synthetic composition codeword.
- Software-WebGPU exactness for the broader 49-pass AIR-to-openings replay.
- Focused release gate for committed recursive chunk boundaries: 1 passed, 13
  filtered, 121.67 seconds.

### Partial and active

- Production WebGPU AIR input is being connected to the previously gated FRI
  producer. The current uncommitted MB2 slice extends the base-trace graph from
  base commitment and challenge derivation into product, rounding, linear, and
  boundary interaction locals. It widens the focused actor from 36 to 41 passes
  while retaining seven resources.
- The new focused `mandelbrot_proof_interaction_bus_webgpu_ingot` tests one
  product-denominator placement separately. This is a good response to shader
  and compile-scale risk, but it has not yet produced a full interaction LDE,
  root, composition, or receipt.
- The production security verifier has a 120-task semantic plan: six fixed
  stages plus 114 query checks. Scalar evaluation, memory placement, and replay
  share the plan. The parent relation still does not authenticate the internal
  `FriAuthentication`, `AirRequestSet`, or 114 query relations for both
  children.
- Browser FRI opening geometry is derived and structurally gated, but the last
  physical Chrome attempt lost the external WebGPU instance during readback.
  That physical exactness gate must be rerun after browser restart.
- Cooperative batching prevents one unbounded submission train, but cold
  Chrome still showed multi-second queue waits and a 3.37-second main-thread
  tick gap. Responsiveness remains a measurement and tuning item.

### Missing

- A parent STARK receipt proving two child verification traces and the exact
  adjacent merge relation.
- A balanced Fe task reduction that generates leaves and recursively merges
  them with progress, cancellation, backpressure, and bounded residency.
- A multi-transition leaf policy and measurements that choose useful chunk
  size. The current production leaf proves one transition. `leaves` in the
  committed interval counts proof leaves, not Mandelbrot iterations.
- Full high-precision GPU arithmetic and proof placement for multiple
  `Fixed<L>` tiers. BabyBear is the proof field, not the numeric precision
  ceiling, but the production specialization is L4.
- The interactive point or disc picker, finite survival or escape claim UI,
  WebGPU proof generation, Fe-Wasm verification, and revm-Wasm verification in
  one browser flow.
- The parent verifier-cost evidence needed to show recursive verification is
  cheaper than replaying the child computations.
- Final removal of the JSON render manifest and runtime proof resource graph.
- Reproducible publication of required local Sonatina changes and an exact Fe
  dependency pin.

## Complexity hotspots and compromises

### 1. The proof API is spread across too many layers for a reader

The proof and supporting ingots contain roughly 49,000 lines of Fe. The largest
packages mix application relation, generic proof protocol, carrier schemas,
scalar placement, verification, and WebGPU placement. This breadth was useful
for discovering the right abstractions, but it hides the simple denotation the
project wants to showcase.

The issue is not that code lives in ingots. Reusable code belongs there. The
issue is that a reader cannot yet follow a short path from:

```text
orbit recurrence -> chunk relation -> proof policy -> generated proof -> verify
```

without crossing a large number of low-level packages. After the recursive
parent works, establish a deliberately small capstone facade. It should name
the numeric model, claim, chunk policy, proof profile, and backend policy, with
the existing ingots remaining the implementation library.

### 2. Full static expansion exceeded the useful compiler envelope

The project found this boundary empirically:

- early combined prover/verifier attempts approached 14.8 GiB;
- a split prover still produced 19,525 semantic specializations and 10,793
  runtime functions;
- policy-sized receipt paths produced aggregate results with more than 100,000
  flattened lanes;
- fresh 114-query artifacts reached roughly 15.4 MB for the prover and 14.6 MB
  for the verifier;
- one fresh prover compile took 2,109.22 seconds and peaked at 13,288,420 KiB;
- the broad recursive Rust gate became an hour-scale operational hazard.

Several compiler fixes were real and general: selected-entry graph analysis,
prepared-body caching, typed arena provenance, address-carried aggregates,
bulk `memory.copy`, consuming lowered bodies, and mutable aggregate-fact
invalidation. They should remain. But the main proof-level lesson is to avoid
turning a 114-query execution into 114 separately specialized programs.

Use FCO/CTFE to derive:

- capacities and domain sizes;
- nominal task kinds and legal transitions;
- plan digest and static safety evidence;
- offsets, liveness intervals, and workgroup geometry;
- compact tables when a table is truly data.

Use ordinary Fe loops and typed memory to execute:

- all security queries;
- all Merkle path elements;
- all FRI rounds under a compact round descriptor;
- all streamed relation rows;
- canonical encoding and decoding.

This retains compile-time derivation from first principles while preventing
the type checker and backend from materializing every element as unique control
flow.

### 3. The independent recursive oracle is monolithic

`crates/codegen/tests/mandelbrot_recursive_fixed_oracle.rs` is 6,314 lines and
contains independent arithmetic, AIR, field, NTT/LDE, Poseidon/Merkle, receipt,
recursive, and mutation models. Its independence is valuable. Its shape is not
an ideal edit loop.

Refactor test support, without sharing Fe-derived expected values, into:

- `orbit_oracle` for fixed arithmetic and chunk boundaries;
- `air_oracle` for rows and residuals;
- `baby_bear_oracle` for field, NTT/LDE, and extension arithmetic;
- `transcript_oracle` for Poseidon, Merkle, transcript, and FRI;
- `recursive_oracle` for child traces, merge, and parent mutation matrices;
- a content-addressed artifact corpus with provenance and exact source/profile
  digests.

Run narrow gates while editing. Run one process-isolated full release gate at
the DONE boundary. The current focused committed-chunk test is the correct
direction. The former all-in-one function should not silently remain as a
non-test body with unclear status; either make it an explicit opt-in final gate
or split and delete it after equivalent coverage is documented.

### 4. Old BN254 and new BabyBear generations coexist

The original Q12/BN254 packages remain useful protocol-shape and oracle
material, but the production GPU direction is BabyBear. Some older packages
still contain explicit field-index matches and the capstone README describes
earlier pending states. This creates conceptual ambiguity and invites readers
to mistake historical scaffolding for current authoring style.

Do not delete independent evidence prematurely. Instead:

- label BN254 packages as historical/reference protocol-shape gates;
- keep only reusable field-generic algorithms in shared public ingots;
- route production examples through BabyBear facades;
- update the capstone README from the SSOT only after the recursive parent
  receipt exists;
- remove duplicate exports or tables once no gate depends on them.

### 5. Raw typed tapes still leak physical detail into Fe code

`region_layout` is a strong improvement: reflection derives canonical regions
inside a u32 tape and callers name `Region<Space, T>` instead of maintaining
offset tables. The WebGPU proof code nevertheless carries long const-generic
signatures for several `StorageBuffer<u32, N>` resources, validity tapes, and
workspace capacities. The active interaction slice shows both the success and
the remaining friction: semantic rows are nominal, but the physical API still
passes base, challenge, interaction, and validity word buffers individually.

This is where the Quilting capability work is relevant. It should add typed
resource role and custody around the existing region views, not replace all
u32 tapes with naive struct arrays.

### 6. The JSON physical manifest is a legitimate scaffold, not the final form

The browser runtime is generic. It creates devices, buffers, pipelines and bind
groups; runs pass cycles, repeats, tapers, and cooperative batches; reads back
declared buffers; and reports device loss. It does not know FRI or Mandelbrot.
That keeps it on the right side of the anti-sham boundary.

However, the compiler currently serializes the physical resource and pass plan
as JSON that the runtime parses. The final goal explicitly rejects a runtime
render manifest and extra JSON semantics. The consolidation target is a
content-addressed compiler artifact whose schema is fixed by the platform
adapter, for example a compact binary or generated ES module containing only
physical facts and digests. Authors must never spell or maintain it. The
runtime must not infer application policy from it.

### 7. `valid` flags mix untrusted carriers and admitted values

Many canonical carriers include `valid: bool` to fail closed across decoding,
allocation, and GPU boundaries. That is appropriate at untrusted boundaries.
Inside the verifier, private constructors and nominal typestate can reduce
repeated flags:

```text
Decoded<T> -> Validated<T> -> Authenticated<T> -> VerifiedInterval<L>
```

Do not remove failure receipts where shader or host boundaries require them.
Convert them once into non-forgeable Fe authority and keep the trusted
interior API smaller.

### 8. Claim language and chunk accounting need sharper public names

The bounded proof specification is correct: finite failure to escape is not
Mandelbrot membership. The future UI should say `EscapesBy<N>` or
`SurvivesThrough<N>`, with the exact fixed-point model and rounding visible.
Avoid “converges” or “is in the set.”

The committed recursive interval records both iteration endpoints and a
`leaves` count. Those are different scales. The UI, metrics, and parent policy
should expose:

- iteration span;
- transitions per leaf;
- number of leaf proofs;
- merge depth;
- proof bytes and prover work at each level.

Early escape also shortens an evaluated chunk. The chunk policy should make
terminal shortening explicit so balanced merge scheduling does not assume all
leaves cover the requested nominal span.

## What hindsight says should be standardized

Several abstractions have now survived enough independent use to deserve
careful extraction from the capstone.

### Stable candidates

1. **Canonical structural streams.** `CanonicalWords`, growing Fe writers,
   staged decoders, and nominal domain commitments should become the standard
   way to transport typed values without handwritten field tables.

2. **Typed logical regions.** `RegionSchema`, `Region<Space, T>`, checked
   load/store, and arena liveness should form a reusable logical-memory layer.

3. **One plan, many interpreters.** `QuadraticPlan`, streamed relation replay,
   field value, witness, residual, degree, and placement interpreters are the
   clearest current example of idiomatic Fe metaprogramming. Package the pattern
   and document the distinction between semantic DAG and schedule DAG.

4. **Ordered dependency structures.** `FriSchedule`, factor trees, and typed
   Merkle topology should share a small vocabulary for shape, placement,
   liveness, and content digest. FFT factorizations may reassociate where laws
   permit. Merkle and transcript structures must preserve exact order while
   allowing multiple physical placements.

5. **Proof profiles as typed policy.** Field, trace domain, LDE factor, soundness
   target, maximum composed proofs, query count, and receipt capacities should
   be one nominal profile interpreted into arithmetic checks, schedule,
   transcript binding, codec capacities, and UI facts.

6. **Scoped asynchronous work.** `Pending`, `Suspend`, structured tasks, worker
   outcomes, backpressure, cancellation, and GPU completion should converge on
   one runtime control-effect spine. A proof job is an especially good consumer
   because it has progress, device residency, cancellation, and recovery.

### Candidates that need another consumer first

- A generic recursive-STARK framework should not be extracted until the first
  parent receipt verifies. Its current interfaces may still be shaped by one
  verifier.
- A generic GPU proof arena should wait until both the proof and Quilting atlas
  exercise resource custody and recovery.
- Device-tuned Bush, DIT, DIF, fused workgroup, and subgroup schedules should
  remain named placement policies over the same denotation. Do not standardize
  an optimization that has not crossed exactness and hardware measurement
  gates.

## Fe-idiomatic simplification after correctness

The safest post-correctness facade would look conceptually like this:

```fe
type MandelbrotL4 = MandelbrotFixed<4, Radix13, FloorRounding>
type Leaf = OrbitChunk<MandelbrotL4, 64>
type Profile = StarkProfile<BabyBear, Quartic, Trace4096, Security100>
type Schedule = DerivedProofSchedule<Leaf, Profile>

let claim = survives_through(point, bound)
let job = prove_recursive<Schedule>(claim)
let result = await job
verify<Schedule>(result.receipt)
```

This is an authoring goal, not a proposal to hide obligations. Each alias must
still expand to inspectable facts:

- numeric range and rounding rules;
- leaf transition count and terminal behavior;
- field and extension;
- AIR constraint count and degree bound;
- transcript domains and query policy;
- receipt schema and canonical digest;
- scalar, WebGPU, or verifier placement;
- exact resource and work estimates.

The provider should produce those facts from ordinary Fe types and denotations.
The source viewer can show semantic Fe, generated evidence, placement, emitted
WGSL/Wasm, and oracle receipts as separate lenses. Concise source must not make
the proof opaque.

## Quilting WebGPU capability consolidation

### What exists on the WIP branch

The branch has two committed increments:

- `f9abe8d8d`: a detailed typed-capability architecture.
- `1b24aeb37`: FCO-derived portable storage layout evidence and an independent
  Wasm oracle.

The committed layout slice derives size, alignment, stride, field offsets, and
field kinds for the first word-aligned storage profile. Its oracle checks a
three-word record at size 12, alignment 4, offsets 0/4/8, and stride 12. A
negative fixture rejects unsupported `bool` layout rather than inventing a
representation.

The uncommitted compiler slice broadens storage resource elements from u32-only
or flat u32 records to u32, i32, f32, and flat records of those scalars. It adds
a mixed `f32/u32/f32` fixture and adjusts Wasm and WebGPU resource lowering.

The design document goes further than the code. It proposes:

- `GpuLayout<Space, T>` evidence;
- higher-kinded resource families parameterized independently by kind, access,
  residency, initialization, recovery, and visibility;
- actor-owned logical custody distinct from physical GPU identity;
- typed allocate, upload, map, submit, completion, and recovery effects;
- reuse of `Pending`, `Suspend`, tasks, cancellation, and generation checks;
- immutable content-addressed assets;
- typed pass graphs interpreted by one fixed host executor;
- scoped lease, borrow, mutation, and close authority.

### Concrete seams with MB2 proof work

#### `GpuLayout` and `RegionSchema` are complementary

They answer different questions:

- `GpuLayout<StorageLayout, T>` describes the physical byte layout and stride
  of one GPU element.
- `RegionSchema` describes the logical, canonical word regions inside a typed
  tape or arena.

Compose them. Do not choose one. A future view could carry:

```text
GpuRegion<Scope, Access, PhysicalLayout, LogicalSpace, T>
```

where the resource family proves that `T` is host-shareable and the region
proves that this actor stage may address its canonical subrange.

#### Proof resources map naturally to the proposed policy axes

- public input and immutable parameters: immutable or content-addressed;
- base and interaction codewords: actor-resident, read-write during creation,
  read-only after commitment;
- NTT and Merkle scratch: job-transient, reusable by liveness;
- canonical receipt: actor-resident until readback, then typed readback;
- fixed Poseidon parameters: generated once or content-addressed, regenerate on
  device loss;
- recursive child receipts: externally supplied immutable inputs with digest
  verification;
- progress and completion: typed task outcomes, not polled numeric flags.

#### Resource custody completes the proof-job actor

The recursive prover needs one logical job that owns buffers across many
dispatches, yields cooperatively, reports progress, accepts cancellation, and
can either regenerate or fail honestly after device loss. This is exactly the
Quilting design’s separation between logical lease and physical generation.
The same infrastructure can later drive the gallery’s rendering actors.

#### Content-addressed artifacts can replace generated static tables and JSON

Large fixed data, compiler-produced WGSL, physical plan bytes, and retained
oracle fixtures should be named by type, schema, digest, and provenance. This
fits both the proof and Quilting work and avoids source-generation shims.

### Gaps and migration risks

1. **Stale base.** The Quilting branch is based near `2d028eac9`, while MB2 has
   since landed significant proof, actor, readback, and compiler changes. A
   branch merge or rebase risks silently mixing unrelated compiler work.

2. **Committed evidence exceeds current compiler support.** The design says
   nested records and fixed arrays recurse. The uncommitted compiler
   `resource_element()` accepts only scalars or one flat nominal record whose
   fields are scalars. The provider and compiler must be reconciled before
   nested layout is claimed.

3. **Layout is only the first 4-byte profile.** The committed provider assumes
   word-aligned 32-bit lanes. That is fine for the first slice but not yet a
   general WebGPU layout system.

4. **Two authorities can drift.** FCO derives layout evidence, while bundle and
   lowering code also derive resource fields. The final compiler must consume
   or independently reconcile the same evidence and fail on disagreement.
   Merely testing one 12-byte record is not enough.

5. **Proof binding pressure is real.** Existing actors deliberately reuse
   buffers and logical regions to stay at seven resources plus one trap under
   WebGPU’s portable eight-storage-binding minimum. A naive one-resource-per-
   nominal-value API would make the proof nonportable.

6. **Typed struct buffers can cost more than u32 tapes.** Large proof matrices
   are naturally columnar or packed field words. Replacing every tape with an
   array of wide structs can worsen stride, read patterns, aliasing, and
   binding pressure. Apply rich records first to control, metadata, receipts,
   and small resource boundaries. Keep typed regions over packed word arenas
   where that is the correct physical representation.

7. **Manifest wording conflicts with the gallery endpoint.** The Quilting doc
   currently permits extending the compiler-derived resource manifest. MB2’s
   final goal removes runtime render manifests. Use the typed resource model,
   but target a fixed content-addressed physical plan artifact rather than
   entrenching JSON.

8. **Lifecycle APIs are design, not implementation.** Leases, borrows,
   recovery, and resource-bearing task custody are not landed. Proof code must
   not program against speculative names until the smallest lifecycle slice is
   independently gated.

### Recommended import sequence

1. **Stabilize the WIP branch in its own small commits.** Finish or split the
   mixed-scalar compiler change, add negative and WGSL/Naga gates, and document
   its exact supported shape. Do not present nested records or arrays as
   compiler-supported until they are reconciled.

2. **Reimplement or cherry-pick the committed `GpuLayout` slice onto current
   MB2.** Avoid a whole-branch merge. Resolve conflicts against current actor,
   readback, and WebGPU lowering deliberately.

3. **Add a provider/compiler reconciliation gate.** For each admitted layout,
   compare Fe FCO evidence, compiler layout, emitted WGSL, bundle physical
   facts, and an independent standards oracle. Include a rejection where they
   disagree.

4. **Compose layout with `region_layout` in one proof-neutral fixture.** Prove
   that a typed physical record can contain or point to canonical logical
   regions without hand-maintained offsets.

5. **Migrate one small proof boundary.** Good first candidates are the
   interaction challenge block, a root/transcript metadata record, or the final
   readback receipt. They exercise mixed scalar or nominal layout without
   disturbing the large columnar arenas.

6. **Introduce access and residency evidence while preserving physical
   aliasing.** Prove that several logical regions may share one physical
   resource under liveness analysis and still count as one browser binding.

7. **Connect GPU completion to the existing task/effect spine.** A focused
   proof job should submit one typed pass group, receive typed completion,
   cancel, reject stale generation results, and recover or fail according to
   Fe policy.

8. **Move the full proof actor.** Once exact resource count, buffer digests,
   mutation behavior, and Chrome execution match, migrate the FRI producer and
   then the recursive leaf/merge queue.

9. **Replace the runtime JSON manifest.** Only after the compiler-owned physical
   plan has a stable schema and digest should the fixed runtime consume a
   non-JSON artifact. Keep a debug projection for inspection, but do not make
   it the execution contract.

## Recommended proof work sequence

The shortest route to a truthful recursive browser demo is:

### A. Finish the active WebGPU leaf producer

1. Land the current interaction-local slice in a clean, focused increment.
2. Complete interaction trace accumulation, LDE, LD02 commitment, public-bound
   composition, and BC02 commitment on the same production data.
3. Feed that real composition into the already exact thirteen-round FRI actor.
4. Execute one physical Chrome exactness receipt after restarting the lost
   browser instance. Compare all selected buffers against the independent
   model, not a completion color.
5. Encode the canonical leaf receipt in Fe and verify it through the existing
   Fe-Wasm verifier.

This closes a browser-generated nonrecursive leaf proof. It is the required
base case for recursion.

### B. Authenticate the child verifier trace

1. Keep the 120-task semantic verifier plan as the authority.
2. Fill the two missing fixed stage relations:
   `FriAuthentication` and `AirRequestSet`.
3. Fill one generic query relation and interpret it for all 114 queries through
   value-level loops and typed memory, rather than 114 specializations.
4. Commit each child verifier trace and prove its streamed relation.
5. Bind child receipt digests, admitted interval digests, and the exact merge
   relation into one parent statement.

### C. Emit the first parent receipt

1. Prove two one-transition child receipts.
2. Verify both inside the parent relation.
3. Prove exact statement equality, shared-boundary equality, interval order,
   leaf-count addition, and endpoint retention.
4. Emit one compact parent receipt.
5. Verify it with a separate Fe verifier and reject mutations to either child,
   the shared boundary, interval order, leaf count, merge relation, and parent
   transcript.
6. Measure parent prover time, parent proof size, and parent verification work
   against replaying both children.

### D. Generalize to useful chunking

1. Choose a small multi-transition leaf size from measured GPU occupancy,
   proof overhead, and recursion cost. Do not assume one proof recursion per
   Mandelbrot iteration.
2. Derive the leaf trace and chunk schedule from the numeric and proof profile.
3. Schedule independent leaves in parallel and sibling merges as a balanced
   ordered reduction.
4. Treat early escape as an explicit terminal leaf shape.
5. Extend from L4 to higher precision by changing the const-generic limb count
   and measuring the convolution, trace, and resource scaling.

### E. Add the browser experience

1. Fe click selects a private point. Fe drag derives a public disk.
2. The user selects `EscapesBy<N>` or `SurvivesThrough<N>` under the explicit
   fixed numeric model.
3. A Fe task owns leaf generation, merge queue, progress, cancellation,
   backpressure, and device recovery.
4. WebGPU produces leaf and parent proof data.
5. Fe-Wasm verifies the canonical parent receipt.
6. The same verifier is exercised through revm-Wasm if the contract slice is
   ready.
7. The UI reports precision limbs, iterations, chunk size, leaves, merge depth,
   proof bytes, generation time, verifier time, and bounded claim language.

## Invariants not to trade away

- Never describe bounded survival as Mandelbrot membership.
- Never let the host choose proof queries, transcript order, round topology,
  workgroup semantics, retry policy, or chunk merging.
- Never use Fe-Wasm versus Fe-WebGPU parity as the only oracle.
- Never rotate or reassociate ordered Merkle, transcript, or recursive merge
  topology. Schedule it differently only while preserving every dependency.
- Never replace a large typed value with a raw pointer unless ownership,
  lifetime, layout, and malformed-input behavior are represented in Fe types
  and independently gated.
- Never solve compiler blow-up by moving proof code into generated Rust,
  JavaScript, handwritten WGSL, or a data-dependent source generator.
- Never use an unauthenticated fixed control root. Either constrain the control
  plan directly or bind a genuine verification key with an audited trust model.
- Never import the Quilting WebGPU WIP wholesale. Integrate stable capability
  slices with exact proof regression gates.
- Never optimize from a green pixel. Read back semantic buffers and compare
  them to independent models.

## Final assessment

The undertaking is large because it has built a general proof compiler and
runtime substrate while proving one application, not because it has secretly
outsourced the application to Rust or JavaScript. The strongest abstractions
are already visible: nominal protocol domains, reflection-derived canonical
streams, const-generic high precision, one quadratic DAG with multiple
interpreters, factor-derived NTT/FRI schedules, ordered Merkle topology, typed
arena regions, and Fe-owned GPU pacing.

The main risks are now concentration and completion, not foundational
correctness. The active work should close one browser-generated leaf receipt,
then authenticate the complete child verifier trace and emit the first parent
receipt before broadening the gallery or extracting a generic recursion
framework. After that correctness horizon is crossed, compact value-level
interpreters, a small capstone facade, decomposed independent oracles, and the
typed WebGPU resource capability work can make the machinery substantially
more readable and faster to compile without weakening a single exactness gate.
