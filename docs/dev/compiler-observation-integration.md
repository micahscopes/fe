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
- Comparison preconditions: attach an explicit numeric contract and physical ABI
  description to comparative conclusions. The direct-route study's original
  checked-add fixture has overflow reporting in the scratch emitter but not in
  the legacy scalar production route. A byte comparison alone hides that semantic
  difference. The explicit `WrappingAdd` fixture now passes all eight inputs on
  both routes in Chrome, including wraparound, but their buffer layouts and trap
  transport still differ. Success: a comparison prominently reports these
  declared mismatches, without claiming to prove semantic equivalence from
  metadata or hashes. MB2 supplies the contract evidence; the consumer presents
  it alongside size deltas.

The wrapping fixture's production capture is also imported and replay-verified:
`/workspace/scratch/mb2-study-wrapping-20260905.capture.json`, capture ID
`ef504dab395aa4927a4c54e2dcbdb73d34b0eec2f9f9ffee82cab32953dcbf17`.
It records 62 initial and 54 final all-module instructions, 2,211 WGSL bytes and
2,572 SPIR-V bytes. This is a different source fixture from the checked-add
capture, not a compiler improvement. Browser evidence is in
`mb2-wrapping-baseline-browser-20260905.log` and
`mb2-wrapping-direct-browser-20260905.log` under `/workspace/scratch/`.

This requests tooling over existing compiler evidence, not Riffcat-based
legality, optimization decisions, or a competing provenance architecture.

Additional comparison precondition from the zero-divisor browser gate:
the Naga trap flag invalidates the result word; there is no in-band zero
sentinel contract. The first harness incorrectly required `[0, 1]`, while
division returned `[7, 1]`. The corrected oracle requires the trap bit on
failure and exact numeric results on success. Preserve validity predicates
when displaying behavioral comparisons, rather than treating invalid result
lanes as mismatches or silently dropping their failure status.
The original log is `mb2-div-chrome-sentinel-assumption-20260905.log` under
`/workspace/scratch/`. Both `mb2-div-20260905.capture.json` and
`mb2-rem-20260905.capture.json` replay as complete, with seven final module
instructions and 677 WGSL bytes each. Equal size is not equal semantics.
These captures do not certify post-trap side-effect suppression or host
consumption of trap buffers; those need separate execution gates.

### Failure/effect contract observation request (2026-09-05)

The executed `shader_trap_effects` fixture is now imported and replay-verified
as `/workspace/scratch/mb2-trap-effects-20260905.capture.json`, capture ID
`2031279757bd06b7ec90c9ffa81bf976c9cd117a6ec7b4855a3224b3547717aa`.
It contains 15 final all-module instructions, 10 selected-root instructions,
two functions, 626 WGSL bytes and 1,508 SPIR-V bytes. The capture predates
committing the fixture: `ec6c1e7b9` is a retrospective source anchor, not a
claim that the historical producer was clean. Its dirty patch was not recorded.

This is a concrete case where artifact verification succeeds but cannot answer
the important question: does a failing helper prevent later observable effects
and consumption of invalid data? Manual inspection finds unconditional ObjStore,
ObjAtomicStore and atomic RMW emission in Sonatina's Naga instruction lowering.
The helper shares the invocation's trap flag, but that flag is not a guard on
those operations. Browser execution confirms ordinary stores after the failing
helper still execute. This agrees with the existing poison-output contract;
it is not evidence that all three operation classes were executed by the probe.

Requested observation, over existing IR and declared contracts:

- Classify failure sites, external stores, atomic stores/RMW, private stores,
  calls and status publication separately. Do not count all Store statements
  as externally visible effects.
- Present may-reach paths from a failure site through helper returns to effects,
  including whether a status guard is present. Absence of sufficient CFG or
  interprocedural evidence must be reported as unknown, not safe.
- Attach the producer's declared failure contract (poisoned outputs versus
  suppressed continuation), status binding and reset/accumulation scope. The
  consumer must not infer these semantics from an instruction hash or name.
- Keep host/pass-graph consumption evidence separate from shader evidence.
  A shader capture alone cannot prove dependent passes inspect its status.

Acceptance fixture: this two-function capture must expose the post-call stores
and link the independent Chrome failure result, without declaring a correctness
pass merely because compilation, Naga validation and artifact replay succeed.
A later guarded candidate must distinguish ordinary stores from atomic RMW,
and must not claim graph-level recovery from a shader-local improvement.
MB2 owns emitting missing typed facts; the Riffcat consumer owns displaying and
comparing them. This is not a request for a second compiler legality analysis.

The same capture now has execution-history evidence in
`/workspace/scratch/mb2-trap-repeat-chrome-20260905.log`: one command submission
with failure followed by success writes status history `[1, 0]`. No host reset
occurs between the dispatches. A final snapshot alone misses the failure.
The useful additional facet is status lifetime: invocation, dispatch and graph
epoch must not be collapsed into a generic boolean named `trap`. This adds no
new artifact-size comparison; the exact 626-byte shader is unchanged.

Resource-budget observation follow-up: Sonatina's target gate now uses Naga's
validated per-entry global-use facts, not module-wide declaration counts.
The concrete regression is `target::resource_tests`: sixteen declarations can
be legal across two eight-buffer entries, whereas a ninth resource reached
through a helper (including an atomic epoch channel) exceeds one entry's budget.
Expose those producer-derived entry/stage binding sets and limit provenance in
the capture alongside bytes. The consumer should distinguish authored resources
from compiler channels and show the exact binding that crosses the budget.
Do not reconstruct liveness from WGSL names or sum counts across entry points.
These new direct Sonatina unit gates do not currently produce Fe recorder
captures; their test results must not be labeled Riffcat-verified comparisons.

Aggregate projection follow-through: Sonatina `8308464a` forwards an already
evaluated component when `ExtractValue` sees a Naga struct/array `Compose`.
It does not reload memory or mutate the aggregate. The focused regression
constructs an old pair and an updated pair, then reads both snapshots; the
outlined helper has no residual Compose/AccessIndex expressions and executes
to 90 on lavapipe. All 137 Naga/SPIR-V integration tests pass (21.88s), including
the new test, against the same release binary. Logs:
`/workspace/scratch/mb2-compose-projection-20260905.log` and
`mb2-compose-projection-suite-20260905.log` in the same directory.
This is a direct backend regression, not a Riffcat production comparison.
Production bytes/timing delta, Chrome execution, and Fe pin integration are
not established for this commit; it is committed but unpublished. E4 remains
open until the matching production comparison executes.

Production projection candidate: Fe `590414488` with an explicit local
Sonatina `8308464a` override passes the isolated release lowering gate. Bundled
WGSL is 84,004 bytes and reparsed Naga has 2,552 expressions/32 helpers, versus
92,084 bytes and 2,869 expressions/32 helpers in the earlier checkpoint.
The raw captured WGSL is 84,008 bytes (the existing four-byte formatting
difference), SHA256
`f9fe53f047e3c36bc9a17db2c9b50a25bc2ad7d22ff7fd1370d08fe8acde643c`.
SPIR-V is 51,444 bytes. Bundle compilation is 10.394s, not a measured speedup.
Riffcat imports and verifies the complete capture, but correctly refuses
single-policy attribution across the differing producer/dirty-source settings.
Tracked dirty patch SHA256 at the candidate run is
`10e857f28e248c513f441ee20e8997aa4cc4120b3e1310d4a3009af6457f8220`;
fixture-only source SHA256 remains `0e22d40d34d6d2ae25424017d376f02bd79d3dce9cb3ad555dc073c88ac43cbb`.
The local override is `/workspace/scratch/sonatina-mb2-proof-path-patch.toml`;
Fe's tracked Cargo manifest and lockfile remain unchanged after the run.

Chrome accepts module diagnostics but pipeline creation reaches the 15s
observation deadline. No dispatch occurs and no device loss is reported. A
subsequent 881-byte control creates its pipeline in 3.2ms on AMD RDNA 3 with
no diagnostics/loss. Do not call the candidate execution green, attribute the
timeout to heap pressure, or infer cancellation of driver work from the host
deadline. No repeated production retry was performed. The earlier 92KB exact
kernel execution remains valid evidence for that earlier artifact only.
Next gate: a matched-producer baseline/candidate and bounded browser pipeline
observation that distinguishes cold compilation from a terminal failure.

Evidence directory: `/workspace/scratch/mb2-compose-production-20260905/`
contains `compute.capture.json`, verified `comparison.json`, raw requests and
the exported bundle. Adjacent logs are `mb2-compose-production-20260905.log`,
`mb2-compose-production-chrome-20260905.log`, and
`mb2-compose-production-control-20260905.log`.

Browser follow-up: a deliberate 45s observation of the exact 84,008-byte
candidate also times out in pipeline creation; the exact earlier 92,088-byte
baseline then does the same. Neither run submits work or reports device loss
through its device promise. This does not isolate an optimization regression,
and outstanding driver work means it does not exonerate the candidate either.
Heavy browser trials stopped after this comparison. Read-only CDP
`SystemInfo.getInfo` subsequently reports Chrome 152.0.7977.64,
`processCrashCount: 3`, no enumerated GPU identity, and `webgpu`, `vulkan` and
OpenGL disabled, despite the preceding requestAdapter reports of AMD RDNA 3.
Those are distinct observations, not proof of which trial caused a crash or
of heap pressure. A fresh healthy host session is needed for a trustworthy
matched browser gate. No browser restart or user-tab closure was performed.
Logs: `/workspace/scratch/mb2-compose-production-chrome-extended-20260905.log`,
`mb2-compose-production-baseline-control-20260905.log`, and
`mb2-compose-browser-systeminfo-20260905.json` in the same directory.

Matched-producer follow-up: a fresh published-pin build on Fe `6e31d9752`
reproduces the original WGSL hash exactly. There are no compiler/application
source changes between Fe `590414488` and `6e31d9752`, and the tracked dirty
patch digest is identical in both runs. Riffcat now reports source/settings
aligned with no setting differences; the capture-directory environment path
differs, and compiler identity intentionally differs. Final Sonatina counts
are unchanged. WGSL falls from 92,088 to 84,008 raw bytes (8,080 bytes, 8.8%);
SPIR-V falls from 51,836 to 51,444 bytes (392 bytes, 0.76%). Reparsed bundled
Naga expressions fall from 2,869 to 2,552 (11.0%). This localizes the observed
reduction after Sonatina IR, consistent with aggregate projection forwarding,
not reduced Fe monomorphization or an inlining-policy change. The Sonatina
revision range also contains the separately tested legacy capability and
early dispatch-limit checks. No execution speedup is inferred from these sizes.
The earlier producer-alignment caveat is superseded for this fresh pair, not
for arbitrary captures or the still-open candidate Chrome execution gate.
Evidence: `/workspace/scratch/mb2-compose-matched-baseline-20260905/`
(`compute.capture.json`, verified `comparison.json`, raw requests and bundle),
plus adjacent `mb2-compose-matched-baseline-20260905.log`. The locked release
test passes in 14.24s; bundle compilation takes 11.385s in this one run.

Production linear-plan inline triage (2026-09-05): replay of the complete
`request-521696-1788655041184817160-compute` capture separates two causes.
`f220` (`sparse_linear_copy_plan`) is backend-callable with 12 physical
parameters. Its one full-inline event copies 2,130 instructions; the following
SCCP frontier reduces surviving original instruction IDs to 325, unchanged
through final recorded cleanup. `f83` (`encode__g1bad`) is instead rejected for
an unsupported compound ABI type. Its one 249-instruction full-inline event
has only 16 original IDs surviving final cleanup. Display names come from the
producer's normalized-stage function records, not inference from WGSL names.

Decision: neither raw clone count is evidence of recoverable output bloat.
Do not relax ABI legality or globally retain large helpers on these counts.
Any E3 retention comparison must account for lost constant propagation and
execute the exact independently checked inputs. Original-ID survival does not
count newly rewritten descendants and cannot be converted proportionally into
WGSL bytes. The next promising measurement is aggregate transport/projection
cost in surviving code, rather than automatically outlining these two bodies.
This narrows the experiment without claiming E2 or E3 complete.

Execution-evidence link gap, reproduced by the production linear-plan gate:
the complete capture at
`/workspace/scratch/mb2-boundary-capstone-20260905/capture/request-521696-1788655041184817160-compute.capture.json`
replays 3,842 module instructions, 634 root instructions, and 92,088 WGSL bytes.
The exact WGSL SHA256 is
`84b59ce0870a92259d258f985b125701b2e71ed55718d62fde182ddd50b1ae01`.
Its successful AMD RDNA 3 Chrome execution is currently recorded separately in
`/workspace/scratch/mb2-boundary-capstone-20260905/chrome-execution.log`.
One workgroup reconstructs 3,328 poisoned words, with zero mismatches across
1,064,960 checked words, validity intact and no traps or device loss.

Request: support an optional hash-linked execution-evidence sidecar, not a
second browser runner. Preserve input and expected-output hashes, dispatch,
adapter/backend, diagnostics, oracle scope, and explicit unknown cache state.
Show module/pipeline creation, input transport, submission/readback and host
comparison as distinct timings. This run measures 52.1ms pipeline creation,
66.1ms dispatch plus readback, and roughly 0.85-0.89s per input transport.
It does not measure GPU timestamp duration or full-proof generation. Do not
promote an external runner's success claim into compiler equivalence proof.
Acceptance: join this result only to its exact artifact, reject a mismatched
artifact hash, and retain failed/partial execution independently of a complete
compiler capture. Keep execution and compilation completion statuses separate.
The existing input exporter is committed on Fe as `8b027599d`.

Related diagnostic presentation: rejected target requests should retain phase,
classification and target requirement. Missing output is unknown/not emitted,
never a zero-byte optimization win. The existing failed binding-collision
capture below is a concrete acceptance input. Typed `fe.shader_request`
presentation and RMIR-only ingestion remain higher-value extensions than a
competing recorder or consumer-owned compiler legality analysis.

RMIR-only ingestion gap, reproduced during intrinsic identity cleanup:
`riffcat-bloat import-fe` rejects
`/workspace/scratch/mb2-intrinsic-identity-wasm-20260905.log` with
`trace contains no exact Fe function-merge marker`. The log contains real
content-addressed RMIR snapshot paths from `FE_RUNTIME_IR_SNAPSHOT_DIR`, but
no shader inliner events because these are Wasm execution gates.
The current CLI offers no dedicated RMIR snapshot import. Keep the scope
partial rather than inventing a function-merge event or a shader stage.

Concrete acceptance inputs are under
`/workspace/scratch/mb2-intrinsic-identity-rmir-20260905/`: snapshot
`18397ae7f3434bc735f74316271869c79c57c940492f7ea195389c79089e792f.rmir`
retains the ordinary `alloc` call, and
`144d77ff81bb1d47d6c45c257311c07cdb46baac173730a441f0c48c2e839e93.rmir`
retains the ordinary `__add_u32` call. Expose source function identity versus
dedicated intrinsic operations from producer facts, without classifying calls
by spelling. Preserve the missing later-stage evidence and dirty-producer
qualification. These snapshots were inspected manually, not imported successfully.

Target-contract observation gap, reproduced by the Fe epoch request gate:
`/workspace/scratch/mb2-fe-epoch-request-20260905/capture/` contains three
complete requests and a backend-rejected binding collision. All four import
and replay successfully. The plain and epoch-enabled requests each retain
15 Sonatina instructions, 10 in the root, and two functions, but produce
626 versus 881 WGSL bytes (1508 versus 1820 SPIR-V bytes). This is required
graph-failure machinery, not optimization regression or extra Fe computation.
The wrapper is introduced during Naga construction after the recorded IR.

Request: record the explicit shader environment, encodings, and optional graph
failure binding as typed request facts. Comparisons should flag contract
differences before offering size conclusions. Acceptance: distinguish these
two captures without inferring atomics from WGSL spelling or user labels;
preserve the collision request as failed with no emitted artifact. Current
replay correctly preserves completion/failure and exact bytes, but does not
explain this target-contract difference itself. Producer was Fe `4c6192b2c`
plus the uncommitted compute-interface/test change and existing shared edits,
with pinned Sonatina `ecb58efe`. Imported settings record the whole tracked
dirty patch SHA256; the fixture source digest is recorded separately.

Producer follow-through: `ShaderRequestFacts` now derives environment, encoding
selection, requested private-heap words, compute dimensions, and optional epoch
binding directly from `ShaderCompileRequest`. The existing strict importer
rejects new event fields, so the compatibility projection places a versioned
JSON object (`fe-shader-request/1`) in `environment["fe.shader_request"]`.
This reserved entry is producer metadata, not a process environment variable.
Legacy adapters without an explicit request project JSON `null`, not a guessed
WebGPU profile. Resource/builtin argument descriptions remain outside this
snapshot; equality is not a complete interface or behavior certificate.

Fresh captures: `/workspace/scratch/mb2-request-facts-20260905/capture/`.
All four import and replay through the existing consumer. Verified comparison
between the second (plain) and third (epoch) requests now reports the exact
epoch difference in `environment_differences`, without CLI setting hints.
Five recorder tests pass, the Fe artifact gate passes, and capture-on/off
WGSL/SPIR-V files are byte-identical for both variants. These are observation
and compatibility gates; the preceding Chrome evidence covers execution.
Consumer follow-up: present these as typed target-contract differences rather
than raw metadata strings, retaining old-capture unknown coverage and the
explicit resource/builtin coverage limitation. No consumer rewrite is required.
