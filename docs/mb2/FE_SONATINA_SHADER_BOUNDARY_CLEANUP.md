# Fe and Sonatina shader boundary cleanup

Status: consolidated design and historical execution record, subordinate to
[`FE_WEB_ROLLCALL_CAMPAIGN_SSOT.md`](FE_WEB_ROLLCALL_CAMPAIGN_SSOT.md)

Consolidated into shared mb2: 2026-09-05

This document was recovered from the former cleanup worktree. Its checkpoint
narratives and pending markers are historical, not current branch status or
push authorization. The linked SSOT owns current completion claims. Shared Fe
is now `mb2`; Sonatina is `mb2-task-borrows`. Do not resume a separate cleanup
line from the historical worktree instructions below. The design invariants,
phase exit criteria and bounded direct-route study remain in scope.

This plan pauses new Mandelbrot proof protocol work long enough to make the
compiler boundary that carries it explicit, testable, and maintainable. The
proof prover remains the capstone regression and performance gate. This is not
a second campaign checklist.

## Outcome

Every Fe artifact is lowered under exactly one explicit target contract into a
Sonatina module bound to that contract's ISA. Every physical fact has one
deciding owner and one independent verifier:

- memory and pointer representation;
- function and entry ABI;
- target limits;
- intrinsic and resource capabilities;
- shader environment;
- output encoding.

The portable Fe lowerer must not infer a target from unrelated booleans, borrow
Wasm limits for shaders, or duplicate a legality decision made in Sonatina.

## Baseline boundary

The pre-cleanup browser shader path followed this shape:

```text
Fe RMIR
  -> crates/codegen/src/sonatina/spirv_lower.rs
  -> compile_runtime_package_shader_ir
  -> compile_runtime_package_wasm_inner in wasm_lower.rs
  -> Sonatina module constructed with create_wasm32_isa()
  -> Sonatina's current SPIR-V-named lowering
  -> Naga module
  -> WGSL for WebGPU and SPIR-V as a validation/output artifact
```

Local integration checkpoint (2026-09-04, not yet pinned or landed): the
dependency-independent host/body cleanup is committed as `89cac8fa0`. The
Shader candidate described below is restored in the cleanup worktree. Its
driver diff also remains saved at
`/workspace/scratch/mb2-shader-driver-pending.patch`.
The host/body change and explicit runtime-entry prerequisites are now landed
on shared mb2 as `80ccc9fd2`, against the unchanged published pin. Its older
raster-helper path is preserved, not overwritten by the cleanup branch's
unrelated raster changes. Shared release gates passed: three shader-storage
tests (3.08s), four raster tests (7.99s), planned entry selection (2.43s), and
Wasm resident lifecycle (1.44s). Logs are
`/workspace/scratch/mb2-integrated-host-{body,raster,entry,resident}-release.log`.
Only the two compiler files were committed; pre-existing shared changes were
left untouched. This landing does not include the new Shader ISA dependency.
No work is discarded or replaced with a permanent compatibility solution.

Reconciliation follow-up: the mb2 arena regression and three storage-planning
commits are now incorporated into this worktree as `49b5b851d`, `affb3f285`,
`35e20a5e6`, and `4415e597c`. Their committed lowerer/test files match mb2;
the pending Shader changes are restored on top. A combined local-candidate
build exposed cross-worktree artifact reuse in the previously shared Cargo
target: codegen could not resolve two WebIDL constants present in this source,
while WebIDL dep-info named the different mb2 worktree. The failed receipt is
`/workspace/scratch/mb2-reconciled-shader-storage-release.log`.
Do not rely on a shared Cargo target directory across these divergent worktrees.
Cleanup validation now uses
`CARGO_TARGET_DIR=/workspace/fe-worktrees/mb2-boundary-cleanup/target`, with the
same disk-backed sccache and one heavy build at a time. The isolated rerun is
recorded in `/workspace/scratch/mb2-isolated-reconciled-shader-release.log`;
it passed all 26 focused release tests: Shader IR (3, 1.64s), authored raster
(6, 6.64s), and typed allocation (17, 7.19s). No WebIDL source or API change
was needed. The command-scoped local dependency override was removed from
Cargo.lock afterward. This verifies the combined local candidate, not a
published pin or a production Mandelbrot size/performance result.

In the candidate, Fe's
shader entry now selects `Architecture::Shader` and calls the shared
`lower_portable_bodies`. Compute and both raster paths use
`NagaBackend::compile_request` with an explicit WebGPU profile and both output
encodings. Legacy scalar/grid adapters preserve their capability behavior but
now emit by runtime entry handle. Authored raster resolves names only among
runtime section and public-export entries, once, then shares those handles across optimization,
call checks and emission. The duplicate name-based inlining and call-check
helpers were removed.

This is being validated against local Sonatina `9fcfcef1` using the scratch
Cargo configuration `/workspace/scratch/mb2-local-sonatina.toml`. The committed
Fe pin remains `ef13a656`; the local override and its lockfile changes must not
be landed as a portable dependency solution. No publication is authorized.
The three `shader_ir_` release regressions passed in 2.85 seconds after a
4m31s build, recorded in
`/workspace/scratch/mb2-fe-shader-isa-integration-release.log`. The compute plus
fullscreen storage gate passed in 1.46 seconds, recorded in
`/workspace/scratch/mb2-webgpu-request-integration-release.log`. The authored
raster shared-resource gate first exposed that one primary runtime section
does not enumerate every explicit public export. Including the runtime
package's exported functions (not arbitrary Sonatina declarations) fixed that
selection error. Its rerun passed in 4.61 seconds, recorded in
`/workspace/scratch/mb2-raster-export-identity-release.log`. Legacy entry
selection also passed in 3.78 seconds, recorded in
`/workspace/scratch/mb2-explicit-entry-legacy-release.log`. The override's
lockfile edits were restored after these gates. This does not establish browser
execution or complete phase 2. The subsequent host-synthesis split now leaves
canonical lanes, resident actor wrappers/policies, fixed exports and indirect
host-result validation in the Wasm entry point. Shared body lowering returns
its builder without host synthesis, and shader entry collection is independent
of storage policy. Its three shader regressions passed in 1.62 seconds. Wasm
host-result rejection passed in 1.33 seconds, canonical owned bytes/UTF-8
roundtrip execution in 1.32 seconds, and resident actor lifecycle/state
execution in 1.58 seconds. Receipts are under
`/workspace/scratch/mb2-host-synthesis-split-release.log` and
`/workspace/scratch/mb2-host-split-{wasm_e2e,wasm_canonical_arena,resident_actor}-release.log`.
Post-split gates also passed: six authored/fullscreen raster tests (6.74s),
compute/fragment storage (1.98s), and scalar native ISA propagation (1.83s).
These are recorded in `mb2-host-split-raster-release.log`,
`mb2-host-split-compute-release.log`, and
`mb2-host-split-native-identity-release.log` in the same scratch directory.
The native identity test does not establish native aggregate allocation.
Reproducible dependency integration remains open.

The target-policy follow-up removes the concrete `emit_wasm_bulk_memory`
coupling: it was initialized from `enable_scoped_arena`, selected `Memcopy`,
and also controlled prepared-body allocator reclamation. An explicit
`AggregateCopyLowering` now preserves Wasm/shader `Memcopy` and native inline
loops independently of arena-scope analysis. Reclamation of consumed compiler
bodies is no longer gated by a target-memory feature. Native arena analysis
remains disabled. Its three shader regressions passed in 2.32 seconds, recorded
in `/workspace/scratch/mb2-copy-policy-separation-release.log`. Native and
guest-callback setup now also use the shared body-lowering helper rather than
duplicating declaration/body lowering. Actual native scalar execution passed
with the optional backend enabled (1.64 seconds after a 4m39s build), recorded
in `/workspace/scratch/mb2-native-shared-lowering-release.log`. The optional
callback registration/invocation/release execution gate passed in 1.81 seconds,
including stale-token rejection and slot reuse, recorded in
`/workspace/scratch/mb2-callback-shared-lowering-release.log`.
This is not a new memory-effect or copy-legality proof.

This path has accumulated useful machinery, including typed private locals,
resource identity, helper retention, aggregate transport, arena verification,
structured control recovery, Naga validation, and independent execution
oracles. The problem is not that the machinery is fake. The problem is that
its ownership and target model remain obscured by a Wasm-oriented entry point
and by repeated target-specific decisions.

One observed consequence is concrete: aggregate flattening admitted a WGSL
helper with 306 scalar parameters. Naga validation accepted the module, but
Chrome's Dawn implementation rejects functions above its 255-parameter limit.
The Wasm limit and the browser limit had different owners, and neither owned
the complete physical shader ABI.

## Vocabulary

These are separate axes:

| Axis | Examples | Owner |
| --- | --- | --- |
| Source target family | Wasm, native, EVM, shader | Sonatina ISA and Fe target contract construction |
| Shader environment | WebGPU, Vulkan, WebGL2 | shader target contract and capability profile |
| Output encoding | WGSL, SPIR-V, GLSL ES | Naga backend output selection |
| Execution stage | compute, vertex, fragment | shader entry contract |
| Placement policy | invocation, workgroup, subgroup, dispatch grid | Fe-authored schedule interpreted under target capabilities |

The implementation that constructs and validates a Naga module is named the
`NagaBackend`. SPIR-V is one output encoding, not the backend's identity.
WebGPU and Vulkan are environments, not encodings. WebGL2 is a distinct,
narrower environment that may select GLSL ES and reject capabilities legal in
WebGPU or Vulkan.

## Invariants

1. A shader module is never constructed with a Wasm ISA.
2. Wasm and native may share target-neutral CPU preparation, but neither may
   choose shader memory, ABI, or capability semantics.
3. One derived storage plan owns each local's representation plus related root,
   call adaptation, and lifetime decisions. These are related axes, not one
   overloaded enum.
4. Physical helper ABI legality has one owner in Sonatina. Fe may use a cost
   model to decide whether a legal helper is profitable to retain.
5. Intrinsic availability is rejected early in Fe from one capability
   vocabulary. Backend validation independently rejects impossible physical
   realization.
6. Resource identity is carried structurally through values, calls, and joins.
   It is never recovered from source names.
7. The structurizer accepts one documented CFG normal form. Kernel-specific
   repair patterns are not an extension mechanism.
8. Dynamic byte arenas remain available where semantics require them. Fixed,
   typed, non-escaping shader locals do not pay the byte-arena tax.
9. Existing semantic oracles remain authoritative. Smaller output is not
   evidence of correctness by itself.
10. No compatibility route is permanent without a named deletion gate.
11. Address space is explicit in the IR operation or pointer that carries it.
    It is not reconstructed from a scalar type, an instruction name, or the
    ISA's default space.
12. Fe logical layout, byte-arena layout, resource transport layout, and Naga
    physical layout are distinct authorities with checked bridges. A target
    contract coordinates them; it does not pretend they are one layout.

## Current state anchors

- Fe cleanup worktree: `/workspace/fe-worktrees/mb2-boundary-cleanup`
- Fe branch: `codex/mb2-boundary-cleanup`
- Remote `mb2` base when the worktree was created: `4d7c81a3b`
- Reproducible Sonatina pin in Fe: `ef13a6568c0dbcd2e85a390048f81a20a61302ac`
- Sonatina continuation worktree:
  `/workspace/sonatina-worktrees/mb2-task-borrows`
- Tested Sonatina candidate tip: `9fcfcef161bd2db7f6a4786e696e48684332dfdc`

The candidate tip contains local cleanup commits beyond the published pin. Fe
must not depend on it until it is available from the configured remote. The
shared `mb2` worktree receives verified, dependency-independent compiler
increments while its unrelated in-progress proof and demo changes remain
untouched.

## Landing policy

Existing-pin comparison (2026-09-04): the dependency-independent host/body
split passes all three shader storage/borrow regressions with `--locked` and
no Cargo override. Resident state execution also passed (2.14s), as did all
six authored/fullscreen raster regressions (9.44s), using that published pin.
Their logs are `/workspace/scratch/mb2-published-pin-resident_actor-release.log`
and `/workspace/scratch/mb2-published-pin-actor_construct-release.log`.
The initial full canonical arena suite passed four tests but failed
`fe_malloc_uses_shared_byte_aligned_canonical_arena_and_grows_memory`: cursor
1024 versus expected 1066 at `wasm_canonical_arena.rs:120`. Reverting only the
pending compiler diff to parent `2587c9975` reproduces the identical failure.
Evidence: `/workspace/scratch/mb2-published-pin-wasm_canonical_arena-release.log`
and `/workspace/scratch/mb2-canonical-arena-parent-comparison-release.log`.
Resolution on shared mb2: `2aa303099` verifies the actual scoped allocation
contract. The diagnostic Wasm shows checkpoint, two calls to the shared
allocator, then rewind in `allocate_pair`; neither unused allocation was
deleted. Its old expectation incorrectly treated discarded allocations as
persistent. The revised test asserts that emitted call sequence, cursor
restoration, and actual memory growth. The separate returned-byte test still
verifies escaping allocation contents survive the canonical call. All five
canonical arena tests pass in 2.09s against the published pin. Evidence:
`/workspace/scratch/mb2-authored-allocation-scope-diagnostic.log` and
`/workspace/scratch/mb2-canonical-arena-scope-contract-release.log`.
No allocator semantics changed, and this does not establish a general escape
analysis proof for arbitrary raw address conversions.

This worktree is an isolation boundary, not a new product branch. Each phase is
committed as a small Sonatina change plus its exact Fe integration pin and
focused gates. Verified increments return to `mb2` promptly. Do not stack
unpublished Fe branches, do not rebase the paused proof commits during this
cleanup, and do not carry all phases as one eventual merge. The current
worktree should close after the first cleanup completion boundary in phase 2;
later representation phases begin from the reconciled `mb2` tip.

## Phase 0: reconcile and freeze evidence

Entry: the cleanup worktree exists and the Mandelbrot protocol work is paused.

Work:

1. Publish or otherwise make the tested Sonatina candidate reproducibly
   addressable, then pin exactly one revision in Fe.
2. Record one Fe revision, one Sonatina revision, and one reference result for
   every capstone gate.
3. Add a module-wide function parameter census before browser submission.
4. Add a browser compilation-info gate for every production WGSL module.
5. Attribute the long post-stage lowering tail and peak retained memory before
   introducing caching.

Exit:

- one reproducible Fe and Sonatina pair;
- current proof and gallery successes and failures are classified under that
  pair, with no unexplained regression;
- every emitted shader reports its maximum function parameter count;
- Chrome compilation, pipeline-creation, uncaptured-error, and device-loss
  results are captured before execution is called successful;
- compile time, artifact size, Naga expression count, and peak memory have
  stable reference measurements.

Deletion gate: none. The ratchet is that the Fe workspace has one Sonatina pin
and a test prevents pin drift.

Progress:

- [x] Sonatina candidate `4781cce6` checks every final Naga helper and entry
  function after ABI lowering against the current 255-parameter WebGPU/WGSL
  limit.
- [x] Its release unit gate rejects 256 physical parameters and accepts 255.
- [x] The existing packed 260-scalar helper and typed aggregate helper release
  gates remain green.
- [ ] Publish the candidate and pin it reproducibly in Fe.
- [ ] Record the exact current Fe capstone baseline under that pin.

## Phase 1: physical conformance and explicit compilation request

Add a typed shader compilation request before changing representation. It must
separate:

- explicit entry identity;
- compute, vertex, and fragment stages;
- multi-stage pipeline descriptions such as authored raster;
- legacy scalar and grid envelope adapters;
- environment;
- requested encodings;
- logical resource and parameter mapping.

Count the final physical Naga/WGSL parameters after packing and after implicit
heap, trap, builtin, and resource transport are added. A logical Sonatina
argument census is not sufficient. Capture browser compilation information,
pipeline-creation errors, uncaptured errors, device loss, and a known control
readback as distinct receipt fields.

The initial supported environment is WebGPU with WGSL. Existing SPIR-V output
remains a separately validated encoding artifact. Vulkan execution and WebGL2
plus GLSL ES are explicit unsupported profiles until they have their own
execution evidence. Adding an enum variant is not implementing an environment.

Exit:

- every shader entry is selected by a typed handle, not declaration order;
- every final function is checked against the selected environment's physical
  limits before browser submission;
- browser module, pipeline, device, and control outcomes are distinguishable;
- current compatibility entry modes are named adapters rather than stages.

Deletion gate:

- the ad hoc parameter probe is gone;
- declaration-order entry selection is gone;
- mutually exclusive stage-selection booleans begin disappearing behind the
  typed request, with no new boolean combination API.

Progress:

- [x] Sonatina `59ed0178` adds `compile_entry` with an explicit module-local
  function handle. Signature and body selection share that handle in scalar,
  grid, and fullscreen-render lowering.
- [x] The release regression selects a second-declared u32 entry after an i64
  decoy, checks scalar and fullscreen-render output, and rejects a removed
  entry. The existing scalar-helper compatibility regression passes.
- [x] Sonatina `f6a7c99b` adds `compile_raster_entries` for explicit vertex and
  fragment handles. The named compatibility API resolves once into the same
  handle-based lowerer. Shared-helper WGSL and SPIR-V are byte-identical across
  both APIs. Duplicate entries, conflicting compute mode, and removed fragment
  handles fail closed. All five authored-raster release regressions pass, as
  does the explicit single-entry regression.
- [x] Fe shader lowering returns runtime-section function handles from its
  declaration map. Single-entry optimization and residual-call checks consume
  those handles. The legacy API still selects the first runtime section for
  multi-root packages, not an arbitrary Sonatina declaration.
- [x] The release typed-borrow regression also checks that the returned handle
  identifies the selected Fe kernel (one test passed in 3.79 seconds).
- [x] The release `multiple_public_roots_select_planned_entry_deterministically`
  regression passes in 3.21 seconds, preserving the legacy runtime-section
  selection policy and checking both the selected body and input arity.
- [ ] Migrate Fe's final backend invocation to `compile_entry` after reconciling
  the Sonatina pin. Authored raster still uses its existing named-entry API.
- [ ] Replace the declaration-order compatibility entry with the typed request.

## Phase 2: introduce the shader target and Naga backend

The entries below are commit-by-commit history. The local integration
checkpoint near the top of this document supersedes their pending-work
statements, but is not yet a reproducibly pinned Fe commit.

Fe `004bbc708` makes the shared package lowerer generic over an explicit ISA
argument instead of constructing Wasm32 internally. The Wasm dispatch assertion
now belongs to its actual Wasm caller. At that commit the shader wrapper still
selected the compatibility Wasm32 ISA pending the reproducible Sonatina pin;
the routine's host/arena configuration was not yet fully separated. A scalar
test proves a supplied native ISA and pointer width reach the emitted module,
without claiming native aggregate allocation support. That release test and
all three shader-IR typed-borrow/storage regressions pass. Evidence:
`/workspace/scratch/mb2-caller-isa-propagation-release.log` and
`/workspace/scratch/mb2-explicit-isa-shader-regressions-release.log`.

Sonatina `9fcfcef1` adds the self-contained `ShaderCompileRequest` and a
`ShaderPipeline` sum type. Compute, paired raster, fullscreen, and named legacy
scalar/grid adapters are exclusive choices. Naga translation no longer accepts
independent grid/render/compute booleans, optional entry identity, or a named
raster interface. Compatibility APIs resolve those into one typed pipeline
before translation. The duplicate authored-raster name resolver is deleted.
The old public backend flags remain only at that compatibility boundary until
Fe migrates; derived internal predicates are not target-selection inputs.
The explicit scalar/fullscreen request matches compatibility artifacts, as
does paired raster. Release gates pass: one explicit-request regression, five
raster regressions, and eight compute compilation/validation regressions.
Hardware execution tests were not run in this slice. Evidence:
`/workspace/scratch/mb2-shader-pipeline-request-release.log` and
`/workspace/scratch/mb2-shader-pipeline-stage-gates-release.log`.

Sonatina `c63b096d`: `ShaderTargetContract` separates environment from requested
encodings. `compile_for_target` requires Shader ISA identity and explicit
entry handles. Its initial WebGPU profile validates without optional Naga
capabilities, requires each selected writer to succeed, and emits only selected
outputs. Vulkan/WebGL2 profiles and GLSL ES remain explicit unsupported errors,
not implied support from Naga writer availability. Legacy APIs keep their
previous all-capabilities/optional-WGSL policy until Fe migration removes them.
Stage-selection booleans and interface mapping were still outstanding in this
contract increment; the subsequent request commit replaces selection inputs.
Release gates pass: explicit-target output matches the legacy u32 artifact;
WGSL-only and SPIR-V-only requests emit only their selected output; selected
i64, CPU ISA, empty output requests, and unsupported environment/encoding
profiles reject. The five legacy authored-raster regressions also pass.
Evidence: `/workspace/scratch/mb2-shader-target-contract-final-release.log`
and `/workspace/scratch/mb2-shader-target-legacy-raster-release.log`.

Sonatina `7168d89a` moves the single implementation to `isa::naga`, names its
backend `NagaBackend` and its result `ShaderArtifact`. `isa::spirv` only
re-exports the implementation and legacy type names. Removal condition: Fe and
downstream callers have migrated to the Naga API. The feature flag and existing
layout/error names are not yet migrated. The new-path explicit-entry release
test and all five legacy-path authored-raster regressions pass. This mechanical
move did not itself enforce environment profiles; the subsequent target
contract commit supplies capability and required-output validation.

Sonatina `d7a2b551` (2026-09-04): the Shader ISA introduces
`shader-unknown-unknown` and extracts the unchanged byte-arena layout into a
shared `Arena32TypeLayout`. It does not make arena layout authoritative for
typed Naga locals or resource interfaces, and does not narrow memory effects.
Fe had not switched ISA at that commit. Both integration tests pass, including parser,
scalar, mixed-struct, and array-layout checks. The explicit-entry regression
now constructs a Shader module and passes scalar/fullscreen Naga emission,
WGSL reparsing and browser-profile validation, and removed-entry rejection.

The attempted `cargo test --release -p sonatina-ir --lib isa::` gate fails to
compile with 58 errors, including missing `InstSetBase` implementations in
interpreter test doubles and the layout test's `make_dummy_inst`. Those source
sites are unchanged from the committed parent. The independent integration
target is not a substitute for repairing and rerunning this full unit harness.
Sonatina `f294d4cd` repairs those test-only constructor calls, using real
instruction sets instead of blanket `HasInst` test doubles. The unchanged
behavioral assertions now run: all 103 IR library tests pass in release
(0.04 seconds). This closes the recorded unit-harness compilation failure,
not the broader compiler or browser gates.
Evidence: `/workspace/scratch/mb2-shader-isa-release.log`.
Repair evidence: `/workspace/scratch/mb2-ir-harness-repair-release.log`.

Add to Sonatina:

- a shader architecture and ISA;
- a `ShaderTargetContract`;
- a `NagaBackend`;
- a `ShaderArtifact`;
- explicit environment and encoding selections;
- typed stage and pipeline contracts.

The shader ISA reuses existing Sonatina instructions where their semantics are
target-neutral. This phase preserves current layout and effect behavior
conservatively. It does not claim precise multi-space effects yet. Fe constructs
the contract in one place and passes a contract-derived lowering policy into
one portable lowering entry.

Migrate compute first, then every existing shader entry mode. A compute-only
slice cannot claim the shader path is independent of Wasm while raster or
legacy adapters still call the Wasm constructor. Filenames may change after
ownership is clear. Moving code out of `wasm_lower.rs` without changing target
selection is not the milestone.

Exit:

- all shader modes construct Sonatina with the shader ISA;
- Wasm and native modules still construct their own ISAs;
- the contract cannot express incompatible stage, pipeline, environment, and
  encoding combinations;
- behavior and artifacts are unchanged where this phase is mechanical.

Deletion gate:

- `create_wasm32_isa()` is unreachable from every shader mode;
- shader selection no longer depends on unrelated portable-lowerer booleans;
- the old mutual-exclusion runtime errors are gone;
- Wasm-named diagnostics are unreachable from shader compilation;
- `SpirvBackend` and `SpirvArtifact` no longer name the Naga implementation.

This is the first cleanup completion boundary. Land it into `mb2` before the
deeper representation phases continue.

## Phase 3: make memory domains sound and derive one storage plan

First independent step landed directly on mb2 as `f1381f63e`:
`BodyLocalStoragePlan` derives carrier types, typed-private eligibility, and
parameter/scalar materialization before function-builder creation. SSA variable
declaration consumes that plan instead of interleaving emission and policy.
Existing layouts, call ABI, and arena escape rules are unchanged. This is not
the complete storage plan: call adaptations, temporary ownership, and lifetime
decisions still need consolidation. The cleanup worktree must incorporate this
mb2 increment when its pending Shader driver is reconciled.
Release evidence: three shader tests (2.08s), five canonical arena tests (2.62s),
four raster tests (6.60s), compute/fragment resources (1.38s). Logs:
`/workspace/scratch/mb2-local-storage-plan-release.log`,
`/workspace/scratch/mb2-storage-plan-canonical-release.log`, and
`/workspace/scratch/mb2-storage-plan-resource-release.log`.
No capstone shader-size or browser-speed reduction is claimed.

The next mb2 increment separates residual-call storage planning from emission:
`CallStoragePlan` records flat arguments, typed borrows, owned deep copies,
owned materializations, and borrowed materializations. Temporary lifetimes
distinguish no allocation, caller frame, enclosing indirect-result lifetime,
and call-local scope. One emission path replaces duplicate checkpoint logic,
at the original first-materialization position. Physical signature validation
still runs after argument lowering. The plan is currently derived at residual
call lowering at that checkpoint. Release gates pass: three shader tests (2.32s),
all seventeen typed allocation tests (7.36s), and five canonical arena tests
(2.58s). Evidence: `/workspace/scratch/mb2-call-storage-plan-release.log`,
`/workspace/scratch/mb2-call-storage-lifetimes-release.log`, and
`/workspace/scratch/mb2-call-storage-ownership-release.log`.

The follow-up now derives declared reachable-call plans alongside local storage,
before function-builder creation. Declaration identity excludes consumed GPU
intrinsics and control effects; no new intrinsic-name list is introduced. A
body-local key of callee plus argument locals shares representation decisions,
not runtime values. Emission consumes the plan and fails closed if it is missing,
and reuses the plan's reachability mask. Whole-program ABI, result ownership,
and arena escape analyses remain separate inputs, so phase 3 is not complete.
All 31 focused release tests pass: shader storage (3), typed allocation (17),
canonical arena (5), resident lifecycle (1), raster (4), compute resources (1).
Logs are `/workspace/scratch/mb2-body-call-storage-plan-release.log`,
`/workspace/scratch/mb2-body-call-storage-ownership-release.log`, and
`/workspace/scratch/mb2-body-call-storage-resource-release.log`.

Binding facts are now also derived once in this body plan, replacing recursive
whole-body scans during emission. This permits consuming the source blocks
instead of cloning them. The dataflow retains the old conservative all-definition
rule, including unreachable definitions; cycles and missing definitions remain
unproven borrows. The implementation is `5f0d9f68a` on mb2 and `a6cf87ed5` here.
All 33 focused mb2 release tests passed, including exhaustive comparison over
79,507 small binding graphs and a 50,000-local nonrecursive chain. Logs:
`/workspace/scratch/mb2-binding-facts-release.log` and
`/workspace/scratch/mb2-binding-facts-resource-release.log`.
The combined local Shader candidate then passed three Shader IR tests (2.91s)
and seventeen typed allocation tests (7.63s), recorded in
`/workspace/scratch/mb2-shader-binding-facts-release.log`. Its lockfile override
was removed afterward. These gates do not measure capstone performance.

The carrier follow-up is `d22b707c2` on mb2 and `9ee175e4f` here. Each single
SSA binding retains the physical type chosen by the body plan; emission reads
that type instead of reclassifying semantic locals. The duplicate local-type
classifier and its GPU-resource cache lookup are deleted. The 31 selected mb2
release gates include native arithmetic and helper/control-flow execution.
Logs are `/workspace/scratch/mb2-planned-ssa-carriers-validated-release.log`
and `/workspace/scratch/mb2-planned-ssa-carriers-native-release.log`.
Combined local Shader validation passed three Shader IR tests (1.47s) and six
authored raster tests (7.33s), recorded in
`/workspace/scratch/mb2-shader-planned-carriers-release.log`. The local dependency
override was removed from Cargo.lock afterward.

Naming address spaces in an ISA is insufficient. Sonatina's current `Mload` and
`Mstore` effects use the default space, `ObjLoad` and `ObjStore` do not report a
resource-root memory access, generic bulk-memory effects name the EVM memory
identifier directly, and `Ptr<T>` contains no address space. Fe also represents
an arena checkpoint as `Ptr<I8>`, so pointer element type cannot distinguish a
typed local from an arena mark.

First choose and implement a narrow IR/API representation for address space on
memory operations or pointers. Keep effects conservative until the complete
admitted instruction closure is verified through copies, projections, calls,
aliases, joins, checkpoint/rewind, and optimizer invalidation. Do not delete the
existing memory scans merely because a summary field exists.

Then derive one read-only storage plan per RMIR body. The plan owns separate,
related decisions:

- local and value representation;
- storage-root identity and alias projections;
- logical signature transport;
- per-call adaptation and synthesized temporary storage;
- lifetime and reclamation ownership.

It considers typed-use closure, address observation, escape, bytewise
operations, call crossing, dynamic allocation, layout, and address spaces. It
is recomputed from RMIR and the target contract, never serialized into the
Sonatina module, and never treated as a proof annotation. Keep only compact
interprocedural summaries globally; derive and release full body plans while
streaming bodies so memory use does not regress.

The plan chooses representations, not one universal layout algorithm. Fe
logical shape, canonical host/serialized layout, address-observable arena
layout, typed shader-local layout, and resource-interface layout remain
distinct. Typed values remain typed until Naga assigns physical shader layout.
Every bridge either proves compatibility or emits an explicit conversion.

Exit:

- scalar and bulk memory effects identify their actual spaces soundly;
- resource operations report conservative root-aware effects;
- every local, call adaptation, and synthesized frame has one deciding plan;
- the plan has stable golden dumps and planned-versus-emitted assertions;
- full body plans are released after lowering.

Deletion gate:

- hardcoded generic-memory space identifiers are gone;
- typed-then-raw fallback probes are gone;
- duplicate materialized, address-carried, and indirect-local sets are gone;
- post-hoc empty scoped-arena repair is gone only after synthesized call
  allocations are planned and emitter assertions pass;
- duplicate semantic and physical flat-shape calculations are consolidated;
- fixed shader locals no longer use `WASM_LAYOUT`.

## Phase 4: centralize contextual helper ABI planning

First backend extraction committed as Sonatina `ec7778de`, on the local
`mb2-task-borrows` line. For the shared compute/fullscreen/legacy translator,
`naga/helper_plan.rs` derives all helper-variant
argument, packed-argument, result, and private-memory ABIs before helper body
emission. The existing contextual resource/liveness/type planners are reused;
body emission consumes the plans in the same call/variant order. This is an
internal preparation boundary, not yet a public Fe preflight API or proof of
body legality. Fe's classifier is not removed by this commit.
All sixteen focused helper release tests passed, including wide packing,
typed aggregate transport, resource variants, arena transport, and guarded
traps. Evidence: `/workspace/scratch/mb2-sonatina-helper-plan-release.log`.
Command: `cargo test --locked --release -p sonatina-codegen --features
cranelift,wasm,spirv-backend --test spirv_backend helper -- --nocapture`.
The local candidate now extends `9fcfcef1` through `ec7778de`; no publication
has been authorized, and the portable Fe pin remains unchanged.
The Fe integration gate passed three Shader IR tests (1.85s) and six authored
raster tests (7.90s), recorded in
`/workspace/scratch/mb2-fe-helper-plan-integration-release.log`.
Authored raster still prepares its restricted scalar-only helper ABI separately;
it shares helper emission, but does not yet consume the contextual ABI planner.
That path is part of this phase's consolidation requirement, not an exception.

The next Sonatina increment, `ca1f9f45`, exposes `analyze_helper_body` as an
instruction/control-flow query, explicitly not a complete callable contract.
Shared helper planning constructs one body plan per function and shares its
structured CFG across resource variants. Authored raster consumes the same body
query, while retaining its separately restricted ABI preparation. The old
emission-time instruction scan and per-variant structurizer call are deleted.
All sixteen helper release tests pass, with new assertions for scalar costs,
arena-lifetime rejection, and a resource helper whose body is eligible but whose
contextual identity is invalid. Evidence:
`/workspace/scratch/mb2-sonatina-helper-body-plan-release.log`.

Pending Fe integration now consumes that body query in both selection and trace
reporting. Its duplicate memory-instruction allowlist, resource-access detector,
and direct structurizer preflight are removed. Body rejection reports use the
`backend_body` category with the backend's full diagnostic instead of inferring
a second reason from Fe's allowlist. Fe's type predicates and recursive callee
acceptance remain until the full contextual ABI query exists. The gate
`/workspace/scratch/mb2-fe-helper-body-query-release.log` passed three Shader IR
tests (1.59s) and six authored raster tests (6.66s). The local lockfile override
was removed afterward; this integration still requires the unpublished pin.

Sonatina `ba2998c4` routes authored-raster helper ABI construction through
the common planner too. Planning now accepts explicit roots and only the
resource-binding view it needs, not the resource call graph. Raster supplies
its verified scalar-only context, no memory transport, one resource-free
variant per helper, and retained scalar arguments. It no longer constructs
physical arguments, packed arguments, or result structs independently.
Stage-local call-site maps and existing restrictions are unchanged. All sixteen
helper tests and all five authored-raster tests passed, recorded in
`/workspace/scratch/mb2-sonatina-raster-abi-plan-release.log`. Fe integration
passed three Shader IR and six raster tests in
`/workspace/scratch/mb2-fe-raster-abi-plan-release.log`.

Sonatina `81be0b05` also makes the common plan own complete physical function
parameters, including the implicit heap/bump/trap suffix. Emission consumes
that list rather than constructing it again. Sixteen helper and five raster
release tests passed in
`/workspace/scratch/mb2-sonatina-physical-parameters-release.log`; Fe integration
passed three Shader IR tests (1.87s) and six raster tests (8.09s) in
`/workspace/scratch/mb2-fe-physical-helper-parameters-release.log`. The temporary
lockfile override was removed after completion.

Current boundary evidence: Fe's `spirv_helper_candidates` still applies its
own scalar/pointer/resource type predicates before mixing body-query results
with profitability. Its duplicate memory-effect allowlist and structurizer
preflight have been removed in the pending cleanup integration.
Sonatina's `helper_naga_type` already admits prevalidated fixed arrays and
structs, while its argument/result planners additionally resolve resource
identity, liveness, wide-argument packing, and private-memory transport.
Moving just the Fe whitelist into a public backend predicate would retain two
legality models. The shared analysis must be consumed by actual Naga ABI
construction as well as Fe's outlining decision; no Fe-side whitelist widening
is justified solely by these differing type predicates.

The next preparation boundary must carry the actual context, not a moved
whitelist. Inspection of `translate_to_naga` identifies the dependency order:
typed-local use closure and type interning; entry external-resource roots;
resource capabilities and logical result identities; live-argument-aware
resource variants; transitive private-memory ABIs and proven entry heap;
then the existing helper ABI plans. Authored raster must provide its restricted
context through that same boundary. A public report can project dense function
and variant IDs plus adaptation/rejection facts from this preparation without
exposing Naga arena handles. Compilation must consume the preparation itself.
The query must not emit bodies or invoke writers merely to discover legality.
The entry-rooted portion is now extracted as `EntryHelperContext`: resource
capabilities, logical results, resource variants, and transitive memory ABIs
are derived together. The existing missing-entry-arena rejection moved into
that preparation without changing its rule. Seventeen helper release tests
passed, including a new negative case whose borrowed-load body is eligible
but whose entry owns no allocation. Evidence:
`/workspace/scratch/mb2-sonatina-entry-helper-context-ownership-release.log`.
This remains internal and single-entry; typed-local preparation, paired-raster
context consolidation, and a public contextual report are still unfinished.
`51546b51` extracts `analyze_naga_entry_body` from the shared translator. Its
named result preserves the existing parameter count, object-allocation mode,
arena-use, trap, and proven high-water facts. No instruction rules, capacity
rules, or diagnostics were changed by the extraction. This analysis no longer
requires emission to obtain entry arena facts for contextual helper legality.
Combined with the separately committed loop-exit correction `bb89ba01`, all
116 shader-backend tests passed under lavapipe in 2.95s:
`/workspace/scratch/mb2-sonatina-entry-plan-combined-release.log`.
Fe integration passed three Shader IR tests (1.91s) and six raster tests (9.14s)
in `/workspace/scratch/mb2-fe-entry-plan-context-release.log`; the temporary
lockfile override was removed afterward.
Before removing Fe predicates, regression coverage must distinguish a typed
aggregate callable in one context from an unresolved resource identity or
unowned private heap in another. Existing body-only eligibility deliberately
does not prove those cases.

`9347f7c2` groups entry helper context, physical plans, the typed-local function
map, and heap/trap transport into `PreparedEntryHelpers`. The preparer interns
memory parameter types and derives physical helper plans in the same order as
before; instruction emission consumes the result. All 116 shader-backend
tests passed under lavapipe in 2.94s:
`/workspace/scratch/mb2-sonatina-prepared-entry-helpers-release.log`.
Fresh Fe integration for this candidate remains pending. The public report must
distinguish body eligibility from contextual ABI legality rather than silently
using the former as an outlining certificate. In particular, a report over a
not-yet-inlined graph must not discard legal child helpers just because an
unsupported parent prevents whole-graph preparation.

Sonatina owns whether an already-lowered helper is physically representable in
the selected context. The result is richer than a predicate:

- unsupported body;
- lowerable with named ABI adaptations;
- illegal call or resource context;
- legal but expensive;
- invalid source or IR.

The analysis includes live arguments, resource specialization, selected word
and type profile, typed-local representation, transitive heap/trap transport,
stage restrictions, and final physical parameter/result limits. It is
recomputed after transformations that change calls or signatures. Sonatina
re-verifies the emitted module.

Fe owns only the profitability decision for a legal helper. Profitability may
use clone survival, Naga expressions, and final bytes. A frontend expansion
fuse remains as a resource-exhaustion diagnostic, not as permission to select a
Wasm arena ABI.

Exit:

- every production function is legal before browser submission;
- the current 306-parameter composition helper is legalized under 255 without
  application-specific source changes;
- legality, required ABI adaptation, and profitability are separately
  observable.

Deletion gate:

- duplicate Fe and Sonatina helper-legality decisions are gone;
- the duplicated trace rejection classifier is gone;
- Wasm function limits are unreachable from shader helper planning.

## Phase 5: use typed intrinsic identity and early target gating

Intrinsic identity is established at source and MIR classification, then mapped
exhaustively to target requirements. Fe rejects an unavailable intrinsic before
Sonatina construction. Sonatina independently validates physical support.
Source intrinsic identity, resource evidence, residency policy, and backend
capability remain different types even when adapters relate them.

This work starts in the MIR runtime classifier and its generic numeric tables,
not merely in a final call-lowering guard. A user function with the same
spelling as an intrinsic must never be captured.

Exit:

- target-negative intrinsic fixtures fail before Sonatina construction;
- same-spelling user functions remain ordinary calls;
- capability adapters are exhaustive without absorbing application policy.

Deletion gate:

- name-based f32, numeric, and resource intrinsic tables in MIR classification
  and downstream call lowering are gone;
- resource and stage capability spelling has one authority per semantic layer.

## Phase 6: define and verify the supported CFG closure

Concrete invariant recovered by the broad execution gate: every exit into a
returning block must perform both exact predecessor-phi transfer and return
transport before breaking. The merged conditional-exit path did only the
first. Unchanged `a233e45d` reproduced 0 instead of 52 in
`grid_direct_return_multi_exit_f32_phi_executes_on_lavapipe`.
Sonatina `bb89ba01` consolidates return transport for explicit loop-exit edges,
header exits, and conditional-merge exits. All 116 shader-backend tests passed
with lavapipe in 3.27s. Evidence:
`/workspace/scratch/mb2-sonatina-loop-exit-return-fix-release.log`;
parent reproduction: `/workspace/scratch/mb2-sonatina-parent-direct-return-release.log`.
This is an emission correctness fix, not completion of CFG normalization.

First document the reducible CFG shapes the current structurizer supports.
Then normalize specific missing forms, including returns, traps, nested loop
exits, and dense switches. Single exit is useful for returning paths, but does
not erase the distinction among return, trap, and nontermination. Some shared
regions legitimately require tree duplication, so measure both duplicated
blocks and any added control state rather than demanding zero duplication.

Fe should emit `BrTable` or an equivalent target-neutral switch for enum
matches. Sonatina owns normalization and structurization. Raster and compute
share the same rules unless their stage ABI is genuinely different.

Exit:

- adversarial early-return, trap, nested-loop, and branch fixtures normalize to
  the documented closure;
- normalization is idempotent and differential execution preserves semantics;
- compute and raster use common return/control infrastructure;
- structural expansion and added control state are measured.

Deletion gate:

- raster-specific return repair is gone when common infrastructure supersedes
  it;
- equality-ladder recovery is deleted only after Sonatina canonicalization
  covers all useful inputs or measurement justifies losing it;
- no kernel-name exception is added.

## Follow-up outside the shader-boundary DONE gate: CPU allocation

The current Cranelift `Alloca` path always creates a fixed 32-byte stack slot
and `ObjProj` forwards its base. The current Wasm `Alloca` path is also not a
general typed allocation realization. Enabling typed CPU allocation without
fixing these paths risks memory corruption.

The shader cleanup isolates target-neutral preparation from the existing CPU
realizations and prevents shaders from reaching them. A later CPU correctness
project must cover allocations larger than 32 bytes, nested projections, two
simultaneously live objects, caller/callee isolation, loops, repeated calls,
returned aggregate ownership, and the additional Wasm reentrancy, trap,
suspension, and canonical-arena cases. Full CPU allocation modernization is not
required to close this plan.

## Direct RMIR to Naga falsification study

The direct route deserves evidence, not either dismissal or accidental
adoption. Run one bounded study under `/workspace/scratch`; merge none of its
emitter code.

Begin with attribution on real production inputs: fixed RMIR, current Sonatina
passes, a legality-only Sonatina pipeline, structured expansion, final bytes,
compile time, peak memory, and execution. Optimization benefit and round-trip
representation cost are measured independently because both can be large.

Then build a time-bounded scratch emitter for an explicitly enumerated subset.
It must include at least one resource-identity case and one fixed or balanced
storage/helper case, not only helper-free scalar arithmetic. Existing field and
Poseidon fixtures with indexed local arrays are aggregate-local fixtures, not a
scalar-only shortcut. Keep both emitters in scratch and use the same physical
layout and numeric oracles. Compare:

- Sonatina pass contribution to final bytes and execution time;
- structured constructs before and after the Sonatina/Naga route;
- Naga expressions per Sonatina instruction;
- direct output size and execution under identical numeric oracles;
- retained-helper, inlined-clone, transport, and entry-code shares in one hard
  production kernel.

The study may reject the direct route, report itself inconclusive, or authorize
a separate design review. It never selects or integrates a backend. A design
review is warranted only if:

1. the scratch vertical slice passes identical aggregate, resource, control,
   layout, and execution oracles;
2. it demonstrates a material compile-time, memory, artifact-size, or execution
   advantage after accounting for Sonatina's optimization benefit;
3. the attribution shows that the loss occurs at the Sonatina boundary rather
   than in shared Fe preparation or Naga emission;
4. the difficult subset fits the time and feature budget without copying the
   production optimizer or structurizer under a new name.

The 15 percent byte, 10 percent execution, and 1.25 structure-expansion figures
are investigation thresholds, not a conjunctive architecture vote. If the
difficult subset cannot fit the timebox, report the study as inconclusive for
production. The study has no Cargo dependency, CLI selector, fallback path, or
production module. It ends with one report; executable emitter sources and
build outputs are deleted.

## Sonatina v2 optionality

The only project facts currently available about v2 are that it is a VSDG
design, separation logic is intrinsic to node outputs rather than an external
certificate layer, and its implementation is not yet available for coordinated
work.

The v1 cleanup preserves optionality by retaining semantic structure:

- typed allocations and projections;
- explicit address spaces;
- resource identities as values;
- explicit scopes and balanced arena regions;
- per-space effects;
- structured control shapes;
- target contracts as plain data;
- IR-independent execution and mutation oracles.

Current analyses remain derived views and test oracles. Do not serialize them
as proof hints, add a Fe-owned separation-logic checker, or invent a
"v2-shaped" Fe graph. Whether a future frontend supplies or merely enables
inference of separation facts is an open design question for the v2 authors.

## Capstone gates

The paused Mandelbrot prover is the largest integration customer, not a reason
to special-case the compiler. Preserve these classes of evidence across every
phase:

| Gate | Requirement |
| --- | --- |
| Numeric | Wasm equality, independent bigint and Plonky3 oracles, and mutation gates remain green |
| Shader validity | Every module reparses and Naga-validates under the selected environment capabilities |
| Physical ABI | Every WebGPU function has at most 255 parameters before pipeline creation |
| Browser compile | Compilation info, pipeline creation, uncaptured errors, and device loss are captured separately |
| Browser execution | A known control executes before focused AMD Chrome readbacks are interpreted |
| Numerical WebGPU | Nontrivial numerical readback is compared word-for-word with its independent reference |
| Full proof graph | Complete receipt construction and mutation rejection remain distinct from module/pipeline health |
| Resources | Per-pass bindings remain within portable limits with exact stage visibility |
| Size | A change above one percent triggers investigation; semantic oracles decide correctness |
| Compile performance | A change above ten percent is investigated under repeated, controlled measurements |
| Memory | Peak retained memory does not increase; the unattributed post-stage tail is measured before caching work |
| Gallery | Canonical demo, compute, and authored-raster release gates remain green |

The full production browser receipt remains a goal after the boundary can
compile every module legally. Recursive proof aggregation and new protocol
features remain paused during this cleanup.

Historical receipts from other Fe/Sonatina lines remain evidence, not the
baseline for this worktree. Phase 0 records the exact current pair and labels
existing failures, including the over-wide helper, instead of treating every
pre-existing red gate as a cleanup regression. The earlier zero-initialized
physical-GPU control establishes transport and execution only, not complete
graph numerical exactness.

## First bounded slice

The first implementation slice is a one-to-three-day boundary milestone:

1. Reconcile the tested Sonatina candidate and exact Fe pin.
2. Record focused compact-compute, wide-helper, typed-borrow, mixed-memory, and
   authored-raster references under the exact pair.
3. Count final physical parameters, including packing and implicit transport,
   and add an un-packable negative fixture that fails before browser submission.
4. Add browser receipt fields for compilation info, pipeline creation,
   uncaptured errors, device loss, and a known control.
5. Introduce the typed compilation request with explicit entry and resource
   parameter mapping.
6. Add the shader ISA and route one small compute fixture through it while
   preserving layout and effects conservatively.
7. Exercise one mixed-memory fixture containing a typed local, arena use,
   resource access, retained helper, and trap transport.
8. Rename the implementation to `NagaBackend` when it consumes the request and
   shader contract.
9. Migrate the remaining shader entry adapters before claiming no shader path
   reaches the Wasm constructor.
10. Prove unchanged artifacts where the work is mechanical, then run focused
    CPU, Naga, lavapipe, and Chrome gates.

Success:

- every migrated fixture retains identical numeric results;
- Naga validation and both encodings remain green;
- explicit entry identity, physical ABI overflow, and invalid stage/resource
  combinations fail early;
- the contract does not narrow memory effects before they are sound;
- at least one target-selection boolean and its invalid-state diagnostic are
  deleted;
- no global retention of rich per-body plans is introduced.

Failure:

- the contract merely wraps the existing booleans;
- Fe and Sonatina both decide the same physical legality;
- the slice needs kernel names;
- output changes without an attributable representation decision;
- address space is guessed from pointer element type;
- effects become more precise without complete invalidation and optimizer
  verification;
- native typed allocation is enabled;
- Wasm or native gates regress.

Any failure stops expansion and updates this plan with the observed boundary.

## Non-goals

- no new compiler IR;
- no permanent direct RMIR to Naga backend during the falsification study;
- no Fe-side Sonatina v2 clone;
- no homemade separation-logic layer;
- no new handwritten WebGPU host API;
- no JavaScript or Rust implementation of Fe application policy;
- no per-kernel helper or control-flow exceptions;
- no device-tuned semantic limits;
- no new Mandelbrot proof protocol work;
- no wholesale rewrite of the current Sonatina backend before the contract and
  measurements identify deletable boundaries.

## Definition of done

1. One Fe line and one reproducible Sonatina pin carry the cleanup.
2. Shader compilation uses a shader ISA and `ShaderTargetContract`.
3. The Naga implementation is named `NagaBackend`; environment and encoding
   choices are orthogonal.
4. Every shader entry has explicit identity, interface mapping, and a checked
   physical environment profile.
5. No target-selection booleans or Wasm limits remain reachable from portable
   shader lowering.
6. One streaming storage plan owns local representation, root identity, call
   adaptation, and lifetime decisions without conflating them.
7. Physical helper ABI planning has one owner; Fe retains only profitability.
8. Intrinsic and resource capabilities use typed identity and early Fe gating.
9. The structurizer consumes one documented supported closure, and only
   demonstrably superseded repair passes are deleted.
10. Wasm and Cranelift realization code is isolated from shaders, and full CPU
    allocation modernization is scoped separately.
11. The direct-route falsification study is filed once and its scratch emitter
    is removed.
12. All capstone gates above pass, including the 255-parameter and Chrome
    compilation-info gates.
13. Every phase records what it deleted, and no compatibility path lacks an
    owner and removal condition.

When these conditions hold, protocol work resumes from the existing
Mandelbrot checkpoint on the consolidated compiler path.
