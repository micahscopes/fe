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

Chrome pipeline smoke gate: the saved largest `reduce_composition` artifact
successfully creates a compute pipeline on the host AMD RDNA-3 adapter in
13,365.2 ms, with no compilation messages, validation errors or observed device
loss. This is compilation only, with no command submission or proof execution.
The exact saved file is 235,397 bytes (distinct from the backend event count of
235,398), SHA-256
`69530a62b31df0b202927001f3c3ef043f7beabca2a1dd6bde584bde3a1447b1`.
The isolated secure localhost document was fulfilled through DevTools after
development-server URLs failed to load. No existing application tab was used.
Evidence: `/workspace/scratch/mb2-native-shader-chrome-smoke-isolated-20260905.log`.
Full browser dispatch and independent production-result checking remain pending.

The focused `private_aggregate_snapshots_execute_without_aliasing` execution
gate passes on llvmpipe against published Sonatina pin
`2567ec76f6a3e113aee468e89cb3f504ef3e578e`. It preserves a typed private borrow
for indexed state access and a structured return, then observes the original
and two updated copies. GPU readback matches the hand-derived result 4 and the
compiler-declared trap channel remains zero. This is software Vulkan execution,
not production proof execution or Chrome dispatch. The existing scalar runner
now accepts the optional trap descriptor instead of assuming two bindings.
Evidence: `/workspace/scratch/mb2-native-aggregate-execution-trap-20260905.log`.

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

### E1/E4 follow-up: reducer exception (2026-09-05)

The paired saved-file census identifies one regression hidden by the total:
`reduce_composition` grows from 148,162 to 235,397 bytes. Its `main` grows from
49,274 to 143,786 bytes, while the surrounding helper text shrinks. Inspection
shows repeated nested zero-record construction. This motivated the backend-only
zero-field insertion experiment below, without changing the authored proof or
shared-state semantics.

Follow-through: Sonatina `1bbd24a0bf21f2b1065565ad64cf0931160cc9f9` implements
typed zero insertion folding. The field type must match; negative floating
zero is not folded to positive zero. Six focused aggregate integration tests
execute on llvmpipe, and the signed-zero/nonzero unit test passes.

The complete production capture passes all 19 validation/size gates in 198.48s
(instrumented test time, not execution). Saved WGSL totals 2,531,533 bytes,
174,471 fewer than the previous native-aggregate capture. The reducer falls
from 235,397 to 133,270 bytes, below the original 148,162-byte baseline. The
largest saved shader is now control_relation at 174,346 bytes. The reducer's
961 literal zero-wrapper assignment lines (59,650 bytes) are gone. This is
literal text evidence, not a claim about driver register allocation or runtime.

Producer: Fe `8db372576501578bf5dadd9a63b9bef7075a9a7b`, with the existing
shared actor-test and HIR provider edits, using the explicit local Sonatina
override at `1bbd24a0`. The parent Sonatina raster-helper change is also present;
this is not a clean same-revision toggle experiment. The shared Fe lockfile was
restored to its published pin after Cargo resolved the override. The new
Sonatina commit is local; publishing it and updating Fe's reproducible pin
remain separate integration steps.

Capture: `/workspace/scratch/mb2-bloat-zero-aggregates-20260905/`.
Census: `/workspace/scratch/mb2-zero-aggregates-census-20260905.jsonl`.
Logs: `/workspace/scratch/mb2-bloat-zero-aggregates-20260905.log`,
`/workspace/scratch/mb2-zero-aggregate-focused-20260905.log` and
`/workspace/scratch/mb2-zero-aggregate-signed-zero-20260905.log`.

Chrome pipeline creation for the new reducer passes on AMD RDNA-3 with no
reported diagnostics, validation errors or device loss. Observed time is
230.5 ms with uncontrolled cache state, not a cold-compilation or runtime
speedup. No commands were submitted. The first attempt lost its execution
context; after adding an explicit document-load wait, the isolated harness
completed. Log: `/workspace/scratch/mb2-zero-reducer-chrome-loaded-20260905.log`.
Artifact SHA-256: `56b315e4640f2228492a00bbff771d52e320a0e4b985615fcbe109c34a032a3a`.

Both reducer artifacts create pipelines on Chrome AMD RDNA-3 without reported
validation errors or device loss. The baseline run takes 12,541 ms. The new
artifact's repeat takes 268 ms, compared with its earlier 13,365 ms observation.
Cache state is uncontrolled, so these numbers are not a speedup estimate. No
dispatch occurred. Logs: `/workspace/scratch/mb2-e1-reduce-before-20260905.log`
and `/workspace/scratch/mb2-e1-reduce-after-20260905.log`.

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

### Eager use and concrete missing views (2026-09-05)

Use Riffcat eagerly for representation and size investigations. When inspection
must supplement it, record the question, the missing producer evidence or
consumer view, a real fixture, and a success criterion here. Do not silently
replace the shared recorder with a second diagnostic pipeline.

The saved zero-aggregate reducer was imported and artifact-verified with the
existing `riffcat-bloat` binary. Outputs:
`/workspace/scratch/mb2-zero-reducer-riffcat-20260905.capture.json` and
`/workspace/scratch/mb2-zero-reducer-riffcat-20260905.report.json`.
It reports complete capture status, 18,200 all-module instructions before merge,
9,300 after merge, and 133,270 final WGSL bytes. These scopes differ and are not
an instruction-to-byte conversion. The import explicitly marks the historical
source digest and dirty-patch digest unrecorded; this is artifact verification,
not a reproducible causal A/B or proof behavior gate.

Concrete follow-ups, with ownership kept distinct:

- Compiler recorder entry coverage, closed for scalar and grid adapters: these
  formerly passed `capture: None` into rooted inlining. They now share
  `compile_observed_shader` with explicit browser requests while retaining their
  existing backend contracts and helper queries. Release regressions verify
  observation-on/off WGSL and SPIR-V equality, exact captured artifact bytes,
  partial-budget behavior and strict-budget rejection (11.89s). The existing
  attributed compute/fragment regression also passes (2.76s); its typed record
  stores were executed in Chrome and read back as `[1065353216, 3221225472]`.
  The direct-route study's scalar capture was imported and replay-verified:
  `/workspace/scratch/mb2-study-scalar-20260905.capture.json`. It records 59
  initial module instructions, 52 final instructions and 1,941 WGSL bytes.
  Logs: `mb2-scalar-observation-test-20260905.log`,
  `mb2-observed-explicit-resource-test-20260905.log`, and
  `mb2-resource-store-browser-20260905.log` under `/workspace/scratch/`.
- Compiler recorder: snapshots before and after RMIR preparation, including
  instance, argument-shape specialization, pass identity and occurrence ordinal.
  Reproducer: `mvt5_f32_nested_helper_render.fe`. Its old residual-count test
  expects `(102, 6)` but now observes `(8, 8)`. The current production capture
  begins at Sonatina pre-merge and cannot identify the earlier change.
  Success: the capture locates whether aggregate expansion disappeared before
  inlining, during shape seeding, or during residual pruning. Behavior remains
  a separate executed oracle, not inferred from matching facets.
- Consumer representation views: rank aggregate construction, extraction,
  scalar flatten/rebuild, zero insertion and arena accesses separately.
  Reproducer: the saved reducer and `linear_ports` captures. Success: expose the
  214-lane transport and redundant zero reconstruction without manual WGSL
  text counting. Report unavailable operand/type evidence as missing; MB2 owns
  adding that evidence to the recorder where needed.
- Consumer presentation: compact stage summaries and explicit scoped deltas,
  including repeated-pass occurrences, without requiring a hand-written jq
  filter over the full report. Success: show the first expansion and subsequent
  cleanup, preserving all-module versus reachable versus emitted-byte scope.
- Producer/consumer provenance: a missing source or dirty-patch digest should
  appear prominently in replay/comparison conclusions, not only as a caller
  setting. Artifact verification must remain usable, but must not imply that
  the experiment can be reconstructed from revisions alone.

This requests tooling over existing compiler evidence, not Riffcat-based
legality, optimization decisions, or a competing provenance architecture.
