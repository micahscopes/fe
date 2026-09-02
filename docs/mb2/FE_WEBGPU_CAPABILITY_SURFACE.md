# Fe WebGPU capability surface

Status: implementation contract and gap inventory
First conformance consumer: the immutable, permutation-aware Quilting
tessellation atlas

## Outcome

Fe should describe GPU programs, resources, and lifecycle in ordinary typed Fe.
The compiler should derive the physical WebGPU plan from those descriptions,
and one fixed browser executor should realize that plan without learning any
application protocol.

This is not a second raw Web IDL binding and it is not a collection of
Quilting-specific escape hatches. It is a typed capability layer above the raw
standards bindings with these properties:

- FCO providers derive GPU layouts, field access, and resource evidence from
  nominal Fe types;
- higher-kinded resource families preserve element-changing and policy-changing
  structure without an enum-shaped API ceiling;
- effects expose allocation, upload, mapping, submission, completion, and
  recovery honestly;
- actors own logical resource custody and lifecycle policy;
- stages receive only the capabilities their declared access permits;
- immutable assets are content-addressed compiler artifacts, not generated Fe
  lookup code or JavaScript data tables;
- the compiler and runtime reconcile the same derived evidence and fail closed
  when they disagree; and
- device replacement re-realizes actor resources from their logical recipes.

The fixed browser runtime may perform WebGPU mechanics. It must not select LOD,
interpret atlas records, manufacture draw ranges, choose retry policy, or
reconstruct an application state machine.

## Non-negotiable boundaries

### No source-generation shim

A Rust, JavaScript, or build-script tool must not emit Fe control flow or one
branch per data element. Large static data remains data. If compile-time Fe code
must be synthesized, it is synthesized by an FCO provider from reflected Fe
types and checked as ordinary generated Fe evidence.

### Not a mirror of raw WebGPU

Application code should not traffic in `GPUBuffer`, numeric usage flags,
binding indices, JavaScript promises, or device objects. Raw Web IDL bindings
remain the standards substrate. This surface describes semantic resource roles,
legal operations, and schedules.

### No manifest-authored application protocol

The compiler may serialize a physical execution plan. Every semantic fact in
that plan must trace to Fe types, values, effects, or FCO evidence. Authors do
not repeat binding numbers, strides, field tables, entry names, resource IDs,
or draw protocols in JSON.

### Logical custody is distinct from physical identity

An actor owns the lifetime of a logical resource. A device-generation-specific
`GPUBuffer` or texture is a replaceable realization of that resource, retained
only by the host resource scope. Stage-local binding values may be freely copied
when their access model permits aliasing; that does not duplicate custody or
grant destruction authority.

## Current capability inventory

| Concern | Present on `mb2` at `2d028eac9` | Missing boundary |
|---|---|---|
| GPU placement | `GpuProgram<WebGpuBackend>` | No backend-independent resource program algebra |
| Compute stage | typed workgroup and dispatch policies | Resource hazards/dependencies are inferred only from a narrow pass graph |
| Raster stage | typed vertex/varying/fragment pairing | Only non-indexed fixed triangle lists |
| Buffer handle | `StorageBuffer<T, N>` and `ReadbackBuffer<T, N, M>` | One undifferentiated storage class; no initialization or residency |
| Shader access | typed `load`/`store` intrinsics | Access is not represented independently from resource kind |
| Element layout | compiler accepts `u32` or records containing only `u32` | No FCO-derived scalar/alignment/stride/layout evidence |
| Resource allocation | runtime creates zeroed storage buffers | No immutable asset, dynamic data, usage contract, or explicit realization outcome |
| Actor custody | attributed resource fields are excluded from Wasm state | No scoped provision/release API and no resource-bearing task custody |
| Readback | one typed readback resource can deliver one actor message | No general map/read/write/upload outcome family |
| Recovery | Fe selects retry/degrade/fail policy | Replacement resources are not rehydrated from logical recipes |
| Draw | `TriangleList<N>` | No vertex/index/instance/indirect resources or range views |
| Texture graph | presentation color target only | No typed sampled/storage textures, samplers, depth, or attachments |
| Host executor | compiler-derived pass/resource manifest | Resource semantics are currently collapsed to zeroed storage buffers |
| Validation | WGSL/Naga and browser execution gates | No independent derived-layout oracle or recovery/re-upload gate |

The existing path is a useful proof of stage ownership and typed actor control.
It must be generalized rather than bypassed.

## The resource model

The public vocabulary has four distinct layers.

```text
Resource family + policies            ordinary Fe types / HKTs
             |
             v
GpuLayout<Space, T> evidence          FCO-derived compile-time fact
             |
             v
Actor-owned logical resource recipe  typed custody + recovery identity
             |
             v
Stage-local resource capability      compiler-bound shader value
             |
             v
Physical WebGPU realization          fixed scoped browser executor
```

Conflating any two layers recreates one of the present limitations: numeric
handles leak into the application, resource initialization becomes host policy,
or a device loss destroys data with no semantic recipe for rebuilding it.

### Layout evidence

Layout is parameterized by an address-space/layout policy because WebGPU's
uniform and storage rules are not interchangeable. The intended shape is:

```fe
pub trait GpuLayout<Space> {
    const SIZE: u32
    const ALIGN: u32
    const STRIDE: u32
    const fn fields() -> GpuFields<Self, Space>
}

pub struct GpuLayoutProvider {}

impl<Space> Derive<GpuLayout<Space>> for GpuLayoutProvider { ... }
```

The exact signature may adjust to Fe's provider-goal constraints, but the
invariants may not:

- scalar kind, width, alignment, offset, aggregate size, and array stride are
  derived from resolved type identity;
- declaration order is preserved;
- nested records and fixed arrays recurse through evidence;
- unsupported leaves fail at the derive site;
- runtime values carry no redundant layout table;
- SPIR-V/WGSL lowering consumes the derived offsets; and
- bundle projection independently recomputes or resolves the same evidence and
  rejects disagreement.

Initial supported leaves should match portable WGSL data: `u32`, `i32`, `f32`,
`bool` only where represented by an allowed physical lane, and explicitly
modeled packed vectors. Accidental Rust/Fe host layout is never the oracle.

### HKT resource families

Fe already supports constructor-polymorphic `Functor` and applied-form
element-changing `core::conal::Functor`. GPU resources should use that power
without pretending that every resource supports an executable map.

A resource family should be partially applicable over its element type, for
example conceptually:

```fe
GpuBuffer<Kind, Access, Residency, Init, N, T> : *
GpuBuffer<Kind, Access, Residency, Init, N>    : * -> *
```

This enables generic policies to preserve the resource constructor while
changing `T`, and lets traits quantify over concrete resource families. It does
not imply that mapping a function over a GPU allocation is pure or free. The
constructor HKT describes shape; an executable transform remains a compute
program plus effects.

Orthogonal policy axes should be nominal types or evidence, not one closed
mega-enum:

- kind: storage, uniform, vertex, index, indirect, staging, readback;
- access: read, write, read-write, atomic where legal;
- residency: immutable, actor-resident, frame-transient, externally imported;
- initialization: zeroed, embedded const value, content-addressed asset,
  derived/GPU-produced;
- recovery: replay recipe, restore checkpoint, regenerate, or explicitly
  nonrecoverable; and
- visibility: the exact stage set allowed to bind the resource.

Combinations are admitted by trait evidence. Illegal combinations should fail
as missing evidence rather than reaching a browser validation error.

### Static data is an artifact

The Quilting atlas requires hundreds to millions of barycentric and index
values. Its Fe program should name a typed asset, not spell those values as a
decision tree.

A content-addressed asset contract includes:

- nominal schema/element type and layout-space evidence;
- element count and exact byte length;
- digest and artifact identity;
- provenance linking the artifact to its generating algorithm and inputs;
- a decoder/validation contract; and
- a recovery recipe that re-uploads the same immutable bytes.

The compiler copies or emits the immutable binary as a bundle artifact and
projects only its derived identity into the resource plan. The fixed runtime
verifies the digest and byte length before upload. It never understands the
payload.

Small constant values may be CTFE-evaluated directly. Crossing the configured
inline threshold should produce an ordinary diagnostic directing the author to
an asset, not silently synthesize source or an enormous WGSL constant.

## Effects and lifecycle

GPU shader `load` and `store` are stage operations, not browser effects.
Operations whose result depends on time, device state, queue ordering, or host
ownership are effects.

The effect families should cover at least:

- provision/release a logical resource in an actor scope;
- upload or copy into a mutable resource;
- begin mapping for read or write and receive a typed completion;
- submit a typed pass graph and observe completion/failure;
- acquire/present a surface texture;
- observe loss and re-realization by logical resource identity; and
- cancel pending host work when the owning scope terminates.

They should reuse the existing `Pending<B, T>`, `Suspend`, structured task, and
generation-checked outcome machinery. A new promise table or callback protocol
would be architectural duplication.

The host handler owns WebGPU objects and converts standards callbacks into
typed terminal outcomes. Fe owns retry, batching, backpressure, regeneration,
and degradation policy. Exactly-once completion, cancellation, and stale
generation rejection follow the same rules as timer, Worker, and actor-sink
effects.

## Actor integration

An attributed actor resource field currently means “the host will allocate a
zeroed storage buffer and the Wasm lane receives an inert zero.” The completed
model gives that field three meanings, all derived from its type:

1. this actor scope owns the logical resource recipe;
2. these behaviors/stages may receive capabilities with the declared access;
3. resource loss, restoration, and terminal failure are routed back through
   typed actor lifecycle observations.

The actor remains the durable semantic owner across device generations. The
browser resource scope is its physical child.

```text
Fe actor scope
  state + resource recipes + recovery policy
                  |
                  | provision effect
                  v
physical GPU resource scope (generation N)
  buffers/textures/pipelines/bind groups
                  |
             device lost
                  v
typed loss observation -> Fe decision -> generation N+1 realization
```

Resource-bearing scoped tasks require explicit borrowed or transferred custody.
They must not receive forgeable numeric handles. A task may:

- borrow a stage/use capability for no longer than the actor resource scope;
- consume and return an affine mutation token when exclusive host mutation is
  required; or
- request an operation through the resource effect handler.

It may not destroy or orphan the actor's physical resource directly.

## Raster and draw model

`TriangleList<N>` proves authored raster stages, but it forces vertices into
shader control flow and cannot represent the Rust Quilting atlas efficiently.
The additive policy family needs:

- non-indexed direct draws;
- indexed direct draws with `u16`/`u32` index evidence;
- instanced direct draws;
- indexed instanced draws;
- indirect draws; and
- indexed indirect draws.

Draw policies name typed resource roles and ranges; they do not contain binding
numbers. Direct ranges may be CTFE constants. Dynamic LOD selection should
write a standard indirect command in Fe GPU code, so the fixed host issues one
generic indirect draw and never sees an LOD key.

For Quilting, the target physical shape is:

```text
immutable barycentric vertices  : VertexBuffer<Barycentric, V>
immutable triangle indices      : IndexBuffer<u32, I>
immutable wire indices          : IndexBuffer<u32, W>
immutable patch records         : StorageBuffer<PatchRecord, P> (read-only)
resident selected draw command  : IndirectBuffer<DrawIndexedIndirect>
```

The canonical atlas owner contains global arrays. A patch record is a range
view, not a separate mesh or allocation. Fe/GPU code selects the reconciled LOD
key and writes or chooses the indirect range. Shared atlas ownership avoids one
resource and one draw description per permutation.

The compiler validates:

- vertex and index element layouts;
- legal index scalar type;
- range/count overflow;
- all referenced buffers are in the same actor resource scope;
- resource access and pass ordering are sufficient;
- indirect command layout is exact; and
- a fragment/vertex payload pair remains nominally identical.

## Textures, samplers, and attachments

Buffers are the first implementation vertical, not the final API boundary.
The same resource architecture must admit:

- sampled 1D/2D/3D/cube textures;
- storage textures with format-specific access evidence;
- multisampled textures;
- comparison and non-comparison samplers;
- color and depth/stencil attachments;
- texture views with typed dimension/aspect/mip/layer ranges; and
- copy relations between legal buffer/texture layouts.

Format and sample-type compatibility should be trait evidence. Pipeline
attachment compatibility is derived from stage results and pass targets.
Application JavaScript must not author format strings or bind-group layouts.

## Pass graph and memoization

The pass graph is a pure Fe-authored description after FCO/type normalization.
Effects submit or realize it; they do not define it.

Compiler projection should assign a stable structural identity to:

- normalized stage entry and specialization;
- derived bind-group/pipeline layout;
- shader module digest;
- attachment formats/sample count;
- vertex/index layouts; and
- fixed dispatch/draw policy.

The runtime may memoize shader modules, bind-group layouts, pipeline layouts,
and pipelines by this compiler-derived identity. Cache hits change no semantics.
Device generation is part of physical cache identity, so loss invalidates
objects but not logical resource or pipeline descriptions.

This provides the substrate for a functional rendering pipeline: pure
descriptions are values, reconciliation is deterministic, and effect handlers
realize only changed identities.

## Functional and incremental interpretation

The useful synthesis is not “make WebGPU pure.” It is to keep a pure desired
world and interpret its delta into a stateful device world through scoped
effects.

```text
Fe actor inputs / scene observations
                 |
        pure tracked derivations
 layout -> shader -> pipeline -> pass/resource graph
                 |
       structural desired-world identity
                 |
          pure reconciliation
     Keep | Create | Update | Retire operations
                 |
       scoped effect transaction
                 |
          realized device world
```

This is deliberately “ZIO meets Salsa” in architecture rather than by copying
either API:

- ZIO's useful precedent is that acquisition produces a value requiring a
  scope, successful acquisition registers finalization, and interruption closes
  the scope;
- Salsa's useful precedent is a graph of pure `K -> V` derivations whose
  dependencies and stable identities decide what can be reused;
- Fe contributes typed effects, actors, HKTs, FCO reflection, backend evidence,
  and compiler knowledge of shader/pass structure.

Two rules keep the synthesis sound:

1. Pure tracked queries never allocate a `GPUBuffer`, compile a browser
   pipeline, submit work, or observe a clock/device. They produce immutable
   descriptions and a deterministic `PlanDelta`.
2. The effect interpreter prepares a new physical generation, validates it,
   atomically publishes the new realized-world map, and only then retires
   unreachable objects. A failed preparation leaves the last good world live.

The tracked keys should be semantic structural identities, not source spans or
mutable object addresses. Candidate nodes include:

- reflected type layout;
- normalized shader specialization;
- bind-group and pipeline layout;
- shader module;
- render/compute pipeline;
- immutable asset realization;
- pass dependency/hazard graph; and
- a complete presentation graph for one actor generation.

Runtime invalidation begins from typed actor/FRP inputs. Changing a camera
uniform should not invalidate shader or pipeline queries. Changing an atlas
asset digest should replace that resource and its dependent bind groups without
recompiling unrelated shader modules. Changing a stage type or attachment
format should invalidate the corresponding layout and pipeline descendants.

The query graph is therefore useful both at compile time and at runtime, but
with distinct stores:

- compiler queries memoize semantic lowering, FCO evidence, WGSL, validation,
  and artifact projection; and
- the browser resource scope memoizes physical objects for one device
  generation using compiler-derived identities.

No cache entry is semantic authority. Cache eviction changes performance only.

### Lessons from functional GPU systems

Several established systems provide specific constraints rather than an API to
imitate:

- Accelerate reifies array computations and uses sharing recovery plus fusion
  to avoid duplicated work and intermediate arrays. Fe should likewise retain
  graph sharing explicitly and must prevent FCO specialization from causing
  code explosion.
- Futhark combines pure array semantics with uniqueness-checked in-place
  updates. Fe's affine mutation/custody tokens can provide the corresponding
  safe fast path for uploads, maps, and exclusive resource transitions.
- Halide separates what an image pipeline computes from its execution schedule.
  Fe should keep semantic kernels/pass relations distinct from backend placement
  and work-allocation evidence, while allowing typed schedules to specialize
  them.
- Obsidian uses representations and types to model GPU hierarchy and eliminate
  intermediates. Fe resource families should expose enough static structure for
  fusion, workgroup placement, and storage choice without making those choices
  JavaScript policy.
- render graphs derive ordering and transient resource lifetimes from declared
  pass/resource relations. Fe can make those declarations nominal and typed,
  then use effects only to realize the resulting graph.

Fusion must be evidence-driven and costed. Over-fusing can inflate shader code,
register pressure, compilation time, and specialization count. Materialization
is sometimes the correct schedule boundary. The compiler should preserve a
small explainable cost record when it fuses or materializes a graph edge.

## Fe-idiomatic scoped resource management

Fe currently has affine owned values, borrowed values, generation-safe host
resource tables, scoped task cancellation, and actor supervision. It does not
yet have general destructor/drop glue. The resource contract must not pretend
otherwise.

The initial safe user-facing form is a bracketed/scoped effect, not a hidden
finalizer on an arbitrary record:

```fe
with_gpu_resource(recipe, fn(lease) {
    // lease and all borrows are confined to this resource scope
})
```

As Fe's region/scoped-effect syntax matures, the same semantics can receive a
more direct surface. The invariants are the API:

- provisioning is not interruptible between physical acquisition and
  finalizer registration;
- a successfully published lease has exactly one owning scope;
- only the scope creator holds close authority;
- ordinary consumers receive borrowed/use capabilities, never close authority;
- finalization runs on success, failure, cancellation, actor stop, and explicit
  scope close;
- dependent resources retire in reverse dependency order;
- double close and stale-generation use fail closed;
- finalizer failure is observable but cannot resurrect a retired lease; and
- a task cannot return a borrow whose scope has ended.

Conceptually, the types have a scope brand:

```fe
GpuLease<Scope, Resource>
GpuBorrow<Scope, Access, Resource>
GpuMutation<Scope, Resource>
GpuClose<Scope>
```

The concrete spelling depends on the least invasive way to express fresh scope
identity in Fe. If generative scope brands are not ready, the actor/task scope's
compiler-owned nominal identity can provide the first implementation. Numeric
slots or author-selected scope IDs are not acceptable substitutes.

This differs from C++ RAII in useful ways:

- cleanup is attached to an effect scope and remains correct across suspension;
- cancellation is a first-class exit path;
- host authorities never inhabit ordinary Fe memory;
- a device generation may disappear while its logical lease stays alive; and
- transient pass resources can be compiler-lifetime-managed without becoming
  user-visible objects.

Device loss is not normal resource release. It retires all physical objects in
one generation, reports a typed loss fact, and—if Fe policy requests it—builds a
new physical child scope from the still-live logical recipes. Content-addressed
immutable resources may share one physical realization within a device
generation, but each actor retains an independent logical lease. The host cache
may reference-count or trace reachability; that is a realization optimization,
not application ownership policy.

Frame-transient attachments are a separate class. Their lifetimes are inferred
from the pass graph, so the interpreter may alias compatible physical storage
when live ranges do not overlap. The derived graph and format/access evidence
make aliasing legal; an application never manually reuses a stale texture.

## Compiler/runtime ownership matrix

| Fact or operation | Fe/FCO | Compiler | Fixed browser runtime |
|---|---|---|---|
| semantic resource role | authors | resolves and validates | opaque |
| field layout evidence | derives | reconciles with lowering | consumes byte plan |
| binding and location numbers | absent | assigns | consumes |
| asset schema and digest | names typed asset | validates/emits artifact | verifies and uploads bytes |
| LOD selection | pure/stage computation | compiles | opaque |
| draw/dispatch policy | typed Fe value/type | projects physical command | submits mechanically |
| resource allocation request | effect | emits typed adapter | creates standards object |
| retry/backpressure/recovery policy | Fe actor/task | compiles | executes requested mechanics |
| device loss fact | typed input | emits delivery adapter | observes standards callback |
| physical object cache | absent | emits stable identity | owns per-device cache |
| stale token rejection | typed lifetime contract | emits generations | enforces |

## Ordered implementation slices

Each slice lands as a small commit with positive and negative gates.

### Slice 1 — derived portable buffer layout

- Define layout-space markers and `GpuLayout<Space>` evidence.
- Derive scalars, nested POD records, and fixed arrays through FCO reflection.
- Add `f32` and scalar-kind metadata to compiler resource elements.
- Reconcile provider evidence with SPIR-V/WGSL offsets and bundle projection.
- Reject unsupported leaves, misalignment, zero-sized elements, overflow, and
  manually conflicting evidence.

Gate: one mixed `f32`/`u32` vertex and one storage record compile to validated
WGSL with independently asserted offsets/stride.

### Slice 2 — logical resource families and initialization

- Introduce orthogonal kind/access/residency/init/recovery evidence.
- Preserve `StorageBuffer<T, N>` as a source-compatible alias or thin façade
  only when it retains the full new invariants.
- Add immutable content-addressed asset artifacts and a bounded CTFE inline
  initializer.
- Extend the resource manifest with compiler-derived physical facts and asset
  identities.
- Upload exact bytes and reject digest/length/layout mismatch.

Gate: browser execution reads a nonzero immutable mixed-scalar resource without
application JavaScript upload code.

### Slice 3 — typed indexed and indirect raster

- Add vertex/index/instance/indirect resource roles.
- Add direct indexed and indexed-indirect draw policies.
- Compile vertex fetch and index format without handwritten shader lookup.
- Ensure the host submits only compiler-projected generic draw commands.

Gate: render the compact Quilting `[2, 4, 8]` atlas from immutable barycentric
and index assets with no per-vertex branch forest.

### Slice 4 — actor lifecycle and effects

- Attach provisioning and mutation to scoped typed effects.
- Route completions through the existing resumable executor.
- Make actor resource custody explicit for scoped tasks.
- Rehydrate immutable and recipe-backed resources on a replacement device.

Gate: forced device loss rebuilds the atlas and resumes rendering; stale
generation completion and double release fail closed.

### Slice 5 — texture and attachment vertical

- Add typed texture/sampler/view/attachment families.
- Derive format, dimension, access, and sample compatibility.
- Add legal copy/upload plans and depth/color pipeline reconciliation.

Gate: an offscreen pass sampled by a presentation pass, including resize and
device recovery, contains no authored binding or format table.

### Slice 6 — full Quilting atlas and dynamic LOD

- Package the 20-key canonical `.cqa` fixture as typed immutable artifacts.
- Select LOD from Fe-owned hover/barycentric state.
- Reconcile shared-edge permutations in Fe/GPU code.
- Drive indexed indirect triangle and wire ranges.
- Validate exact topology, seams, interaction, and recovery in Chromium.

Gate: the browser moves continuously between LODs without generated Fe lookup
code, JavaScript LOD logic, per-permutation mesh allocations, or stale frames.

## Compiler observability with Riffcat

The WebGPU compiler path keeps Riffcat observational and out of the semantic
dependency graph. Fe commit `61ef6bc14` exposes the opt-in
`FE_SPIRV_INLINE_SNAPSHOT_DIR` boundary and writes matched `pre`/`post`
Sonatina modules around rooted shader inlining. Riffcat branch
`sonatina-structure-ingest` at `93477d4` consumes those `.sona` files through
its versioned `sonatina-ir/1` adapter. Sonatina and Fe therefore remain the
authorities for legality and transformation; Riffcat supplies content-addressed
phase evidence without becoming a second optimizer or a build requirement.

A representative release workflow is:

```console
$ FE_SPIRV_INLINE_SNAPSHOT_DIR="$snapshot_dir" cargo test --release <focused WebGPU gate>
$ riffcat --corpus "$corpus_dir" ingest "$snapshot_dir"/*.sona --label <source-revision-and-gate>
$ riffcat --corpus "$corpus_dir" bucket --unit sona-module --mode shape \
    --facet structure+types+constants --min-size 1
$ riffcat --corpus "$corpus_dir" root --unit sona-module --mode shape
```

The 2026-09-02 mb2 inliner corpus was re-run through that release-built adapter:
343 functions / 1,222 blocks / 5,862 instructions before inlining became 343 /
3,409 / 39,866 after it, while distinct structural shapes increased only from
285 to 369. This is strong duplication evidence, not semantic-equivalence
evidence. For the typed resource work, record at least the
`structure+types+constants` address before and after lowering; add a
resource-effect view before using any digest as a reusable compiler cache key.

## Required evidence

Every capability must carry evidence at all relevant layers:

- Fe compile-pass fixture demonstrating the intended idiom;
- Fe compile-fail fixtures for illegal compositions;
- provider-output test proving deterministic FCO synthesis;
- independent Rust layout oracle, not generated output as its own oracle;
- SPIR-V/WGSL structural assertions plus Naga validation;
- deterministic manifest/artifact digest test;
- Riffcat pre/post Sonatina phase evidence for compiler-growth-sensitive
  changes, kept as an optional analysis corpus rather than runtime input;
- fixed-runtime unit test for allocation/upload/release and stale generations;
- Chromium execution gate on a production bundle;
- device-loss/recreation gate where the browser permits deterministic
  injection; and
- a downstream Quilting-Fe gate proving the public API is sufficient without
  escape hatches.

Performance evidence should separate compile time, artifact size, upload time,
pipeline creation, first presentation, steady-state CPU frame time, GPU frame
time, and recovery time. No optimization is accepted solely because a frame
looks correct.

## Explicit non-goals for the first vertical

- exposing the full WebGPU JavaScript object model to ordinary Fe applications;
- runtime reflection over arbitrary resource records;
- unbounded CTFE expansion of binary data;
- GPU-side adaptive triangulation before the static indexed atlas path is
  correct;
- a WebGL compatibility translator; and
- hiding browser availability, validation, device loss, or mapping failure.

These are sequencing constraints, not excuses for a permanently narrow API.

## Research anchors

- [Accelerate: Optimising Purely Functional GPU Programs](https://www.acceleratehs.org/publications.html)
  — sharing recovery and array fusion under an embedded functional GPU model.
- [Futhark: Purely Functional GPU Programming with Nested Parallelism and
  In-Place Array Updates](https://futhark-lang.org/publications/pldi17.pdf) —
  uniqueness-checked mutation under race-free pure semantics.
- [Halide: Decoupling Algorithms from Schedules](https://people.csail.mit.edu/jrk/halide12/)
  — semantic computation separated from spatial/temporal placement choices.
- [Obsidian: Hierarchical Data-Parallel Design-Space Exploration on
  GPUs](https://www.cambridge.org/core/journals/journal-of-functional-programming/article/language-for-hierarchical-data-parallel-designspace-exploration-on-gpus/C406E732CBFFD3AF80E3BECBBE7F8B7B)
  — typed GPU hierarchy and representations that eliminate intermediates.
- [Frostbite FrameGraph](https://www.gdcvault.com/play/1024612/FrameGraph-)
  — pass/resource graphs, derived lifetimes, and transient aliasing.
- [Salsa's red-green algorithm](https://salsa-rs.github.io/salsa/reference/algorithm.html)
  — dependency-tracked pure queries and incremental reuse.
- [ZIO Scope](https://zio.dev/reference/resource/scope/) — effectful
  acquire/release, dynamic scope extension, and cancellation-safe finalization.
- [Algebraic Effect Handlers with Resources and Deep
  Finalization](https://www.microsoft.com/en-us/research/wp-content/uploads/2018/04/resource-v1.pdf)
  — linear external resources and finalization across effect handlers.
