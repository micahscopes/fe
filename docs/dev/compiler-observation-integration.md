# Compiler observation integration

Status: first recorder slice integrated into the shared worktree; broader corpus
and overhead gates remain open. See the checkpoint below for the measured scope.

## First recorder checkpoint (2026-09-05)

Sonatina commit ca5210d1ff41af48d893c82f2a8380ada3e3f5c6 adds caller-owned,
typed pass-boundary callbacks inside the existing selected-function pass round.
It preserves analysis lifetime and scheduling. All 49 pipeline tests passed.
This is the existing mb2-task-borrows integration line, not a new task branch.

Fe adapts the pilot hooks into typed producer-owned records and preserves the
fe-bloat-event/1 JSONL projection. Request configuration is supplied by the shader
driver. The recorder reads no environment variables and has no global request
counter. Environment variables remain a driver-level compatibility interface:

- FE_BLOAT_CAPTURE_DIR enables capture into new request directories.
- FE_OBSERVE_MAX_EVENTS controls the event budget (maximum 100,000).
- FE_OBSERVE_STRICT makes recording errors fail the gate explicitly.
- FE_BLOAT_FORCE_INLINE_HELPERS remains an independent experimental policy knob.

Ordinary event exhaustion produces a readable incomplete prefix, emits an
out-of-band diagnostic and leaves compilation successful. A failed capture stops
subsequent snapshot extraction and, at the next frontier, releases clone records
unless the separately requested legacy clone trace still needs them. This does
not yet establish a strict bound on a single inliner frontier's peak memory.

The first complete and budget-truncated requests, exact artifacts, producer patch
and consumer instructions are under
/workspace/scratch/mb2-observe-compat-20260905/README.md. The original capture used
an explicit local overlay of the committed Sonatina revision. It must not be
relabeled as a published-pin run. Four Fe recorder unit tests passed. Both requests
imported and artifact-verified through Riffcat, with complete and incomplete status
respectively. Replaying the complete capture twice produced identical JSON.

For the small scalar fixture, capture off/on and budget exhaustion produced
identical 518-byte WGSL and 1,300-byte SPIR-V. Baseline, forced-inline and truncated
outputs each passed a 2,313-pixel independent oracle on software Vulkan (llvmpipe).
The wrong-result shader failed at pixel (1,0). These results are not Chrome,
physical-GPU performance, broad corpus neutrality or Mandelbrot proof generation.

Published-pin follow-up: the locked release build without any Cargo patch overlay
passed against published Sonatina ca5210d1. Its capture harness under
/workspace/scratch/mb2-observe-published-20260905 passed observer-on/off WGSL and
SPIR-V equality again. Baseline artifacts also matched the original overlay run
byte for byte. The manifest and lockfile now use the published git revision.

## Outcome

### Production finding: aggregate replay transport (2026-09-05)

The 19-stage composition fixture passes release compilation and Naga validation
at Fe db96a9235 with published Sonatina ca5210d1. It emits 12,936,420 backend WGSL
bytes in total; the largest stages are linear_ports (1,011,478 bytes) and
linear_boundary (1,011,759 bytes). This is not a browser execution gate.

The linear_ports helper sparse_linear_copy_plan is already 36,397 instructions
at the pre-merge Sonatina boundary and remains 36,394 after rooted cleanup. Its
690,388-byte WGSL body contains 51 groups of 210 scalar result-to-local stores:
10,710 stores, 620,298 literal bytes. The replay witness has 52 four-limb field
elements. Each call receives a typed pointer but returns 214 scalar lanes:
witness (208), cursor/validity (2), and selected value (4). Each successive call
rebuilds the 210-lane state even though the witness is unchanged.

This evidence identifies a representation boundary, not a proven 620 KB saving.
The current ABI requires those transfers, and source-level value snapshots must
remain observable. The final rooted inliner is not where this body first grows.

The Fe probe at 6e8be9d2d isolates a state-return chain with observable old values.
Its release gate checks lowering and Naga/SPIR-V validation only. Sonatina test
commit 9bf0e3de on mb2-task-borrows separately proves whole aggregate helper
returns execute through WGSL on llvmpipe: a returned snapshot remains 42 after
the original storage changes to 99. The helper remains outlined and uses no byte
arena. That test-only commit is local and is not the Fe manifest pin.

The corrective boundary must span all three operations:

1. Select a native aggregate result for eligible private shader helpers, with
   legality owned by the existing Sonatina Naga contract. Keep external and Wasm
   interfaces unchanged.
2. Carry intermediate aggregate values intact through call, copy, projection,
   construction and control-flow joins. A call-only ABI change that immediately
   splits the returned struct into tuple_vars merely moves the expansion.
3. Materialize typed values with whole-value transfers. Reuse storage only under
   the existing lifetime/snapshot proof; never equate value copying with pointer
   aliasing. Keep raw byte-observable values on their declared representation.

Current source anchors in sonatina/wasm_lower.rs are lower_body_signature,
scalar_tuple_element_tys, local_flat_values, aggregate RExpr::Call lowering,
lower_copy_value_into_place and lower_materialize_to_typed_object. A local-store
peephole alone cannot repair the already flattened intermediate representation.
Derive the value representation alongside the per-body storage plan, not through
an observational digest, another independent helper classifier, or a global flag.

Validation must include changed-field and unchanged-witness cases, old snapshots
live across calls, branches/loops and aliasing negatives, followed by the exact
production capture and independent proof oracle. Measure final artifacts and
execution separately. No production size or runtime improvement has landed yet.

### Native-value experiment, not yet promoted (2026-09-05)

The local Fe candidate now carries eligible private shader products through SSA
construction, projection, copies and helper returns. Resource-bearing products
retain their resource-aware transport, rather than reinterpreting resource
identities as ordinary fields. Sonatina commits f8b1bcda, c7359895, 8a5802c5 and
931b141c add aggregate projection, insertion/construction and aggregate phi
transport. These commits are local, not the published Fe dependency pin.

Focused release gates preserve the one-allocation storage-reuse expectation,
typed borrows, the Wasm snapshot oracle, and an outlined WGSL struct-returning
helper. Sonatina's construction and snapshot tests execute on llvmpipe; all 20
phi-filtered regression tests pass, including simultaneous aggregate swaps.

The first complete native-value capture emitted 15,066,556 backend WGSL bytes.
It exposed an additional constructor round trip: existing structured children
were projected into scalar leaves and rebuilt. Preserving those children directly
reduced the next complete capture to 8,499,913 bytes, versus the earlier
12,936,420-byte baseline. All 19 stages compile and validate, but the unchanged
size gate still fails for control_relation (1,339,122 backend bytes).

The linear_ports shader is now 460,011 bytes, and sparse_linear_copy_plan is
101,986 bytes, versus 1,011,478 and 690,388 respectively in the baseline.
Remaining control_relation cost is concentrated in absorb_sparse_control_link
(668,493 bytes). Its emitted calls still split structured records into long
scalar argument lists, including individual call statements over 4,200 bytes.
Private argument transport is therefore the next boundary to inspect, not a
reason to relax the size gate or change the authored proof.

Artifacts: /workspace/scratch/mb2-bloat-native-preserved-fields-20260905/ and
/workspace/scratch/mb2-native-preserved-fields-census-20260905.jsonl. The release
test took 203.53 seconds, including instrumentation; this is not proof-generation
time. The candidate used the explicit local Sonatina overlay against 931b141c.
It is not a fresh-checkout or browser-execution gate, and the Fe candidate must
not be described as a landed production optimization yet.

### Complete argument/result transport checkpoint (2026-09-05)

Extending the same planned representation to eligible private arguments closes
the remaining expansion: all 19 production stages pass the unchanged validation
and size gate. Backend WGSL totals 2,706,004 bytes (baseline 12,936,420), with the
largest stage at 235,398 bytes. linear_ports is 153,718 bytes and its
sparse_linear_copy_plan helper is 24,628 bytes. control_relation is 178,401 bytes;
absorb_sparse_control_link is 27,394 bytes. The full instrumented test takes
172.99 seconds. These are emitted-code and compilation measurements, not GPU
execution or proof-generation timings.

The argument plan is recorded at private signature declaration and consumed by
parameter binding and call preparation. Resource-bearing products, explicit
typed borrows, addressable parameter slots, external interfaces and Wasm keep
their existing distinct transport. Existing aggregate child values are reused
directly rather than recursively projected and reconstructed.

The five focused Fe shader/lowerer tests pass. The broader lowerer module run is
25/26: authored_mvt5_specialization_measures_smaller_nested_residual expects
(102, 6) but observes (8, 8). Saved older binary fe_codegen-fe6d5edd75ca8533 fails
identically before this argument/result transport work. Do not silently update
that structural-count expectation or describe the broader module suite as green.

Capture: /workspace/scratch/mb2-bloat-native-arguments-20260905/.
Census: /workspace/scratch/mb2-native-arguments-census-20260905.jsonl.
Logs: /workspace/scratch/mb2-native-abi-lowerer-regressions-20260905.log and
/workspace/scratch/mb2-mvt5-prior-binary-20260905.log.

The Fe checkpoint pins Sonatina 5d2d82be7449729b066768dac81974cf78280510. Validation
used the explicit local dependency override; publication and a fresh-checkout
gate remain pending. Publish Sonatina before publishing the Fe dependency pin.
Concurrent Quilting changes in Sonatina's working tree are not included in that
pin. Browser execution and independent proof-oracle validation remain required
before claiming a production prover improvement.

Full capture provenance and a reproducible literal-text census are recorded in
/workspace/scratch/mb2-bloat-diagnosis-20260905.md. The exact composition artifacts
are under /workspace/scratch/mb2-bloat-composition-20260905. They are suitable for
comparison with Quilting's independent captures, not proof of a shared cause.

One compiler-owned observation boundary feeds the Riffcat tooling. It must answer
where shader expansion occurs, which representations and transformations account
for it, and whether a controlled change improves an exact, behavior-tested
artifact. Ethdebug integration is out of scope and is not a prerequisite.

The compiler emits observations and transformation records. Riffcat owns reports,
structural comparison and cross-run alignment claims. No compiler legality,
optimization or cache decision may depend on observation output or Riffcat hashes.

## Existing candidate and integration ownership

The initial Fe candidate is `908116b4b0714a36fb77bb53808d2762c2f6c4e8` on the
`bloat-toolkit` branch. It combines capture hooks, a named force-inline experiment,
and example harnesses. Its separate InstanceIndex prerequisite must not overwrite
the shared branch's existing raster support.

Shared mb2 includes repeated-loop helper retention added after the candidate base.
Preserve that policy when integrating the hook changes. Do not replace
`spirv_lower.rs` wholesale with the candidate version.

The shared worktree is `/workspace/fe-worktrees/mb2`. Compiler integration and
acceptance gates belong here. Agree with the Riffcat owner on recorder-file
ownership before changing the candidate; consumer work and compiler integration
can proceed independently, overlapping recorder rewrites cannot.

## First implementation slice

Use a small typed recorder interface with an explicit caller-owned context.
Configure it at a driver/request boundary. Record types carry scope-local entity
references; producer-local arena numbers are not cross-stage or cross-run identity.
Keep JSONL as an external encoding, not the internal API. Preserve a projection
readable by the existing structured Riffcat importer while migrating producers.

Retain the useful existing hooks: pre/post exact merge, normalized helper graph,
helper eligibility and retention decisions, inlining frontiers, cleanup and final
IR, followed by exact backend artifacts. Record new clone events separately from
cumulative original-ID survival observations. Do not call the latter descendant
tracking. Record missing fast-path or rewrite attribution explicitly.

Observation must not choose optimization policy. Keep named force-inline controls
in a separate experimental configuration and record both requested changes and
their dependency-closure consequences. Do not make that experiment a prerequisite
for ordinary capture.

Prefer one small module or crate initially. Do not require a multi-crate registry,
new compiler IR, debugger integration or universal provenance system before the
existing producer can use it. Remove superseded capture paths as replacements
pass their compatibility gates.

## Required gates before promotion

1. Disabled observation performs no snapshot construction or clone-detail
   collection and creates no output files.
2. Compare exact WGSL and SPIR-V bytes with observation disabled and enabled in
   separate fresh processes, using the same compiler, source and settings. Cover
   the scalar helper, a shared/multi-entry resource fixture, a repeated loop helper
   and the production sparse round-interaction kernel. Preserve the ordinary pass
   schedule; do not silently substitute pass-by-pass scheduling for batched runs.
3. Import and replay captures through Riffcat with artifact verification. Preserve
   separate backend and outer WebBundle byte counts. Unknown lineage stays unknown.
4. Exercise missing completion, forced truncation, write failure, artifact
   tampering, request isolation and no-clobber behavior. Ordinary observation
   failure should leave compilation semantics intact and report partial capture;
   an explicit strict mode may fail the observation gate.
5. Run the independent finite-domain oracle on the exact saved scalar shaders and
   its wrong-result negative control. Record software Vulkan as software Vulkan.
   Compiler validation, browser execution and proof correctness are distinct gates.
6. Measure counts-only and detailed capture separately: compiler wall time, peak
   RSS, capture bytes and exact emitted bytes. Output limits alone do not bound
   clone collection or snapshot construction memory. Use optimized builds and one
   heavy build at a time. Do not infer speedup from instrumented timings alone.

## Next tooling capabilities, in order of usefulness

- A stage waterfall with explicit scopes and the first growth boundary highlighted.
- A per-helper expansion ranking separating new clones, cumulative observations,
  surviving original IDs, unknown rewritten descendants and retained shared code.
- Representation breakdowns for typed locals, byte-arena operations, numeric
  legalization, resource specialization and structurization clones. Different
  causes must not be collapsed into one duplicate-instruction count.
- Sonatina-to-Naga expression attribution and WGSL function/range measurements.
  Report exact, estimated and unattributed quantities separately; do not allocate
  final bytes proportionally to instruction counts and call that exact evidence.
- Revision-pair comparisons keyed by source, Fe revision and dirty patch, Sonatina
  pin, target configuration and intervention. Stage alignment is an explicit claim,
  not identity inferred from equal stage labels or function names.
- Saved-artifact experiment replay with behavior results and regression budgets.
  Smaller output is not an improvement if validation or behavior regresses.

Use the Mandelbrot round-interaction kernel and Quilting triangulation as real
consumers after the small fixtures pass. Budgets should flag regressions relative
to reviewed baselines, not invent a universal shader-size correctness limit.
