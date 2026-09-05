# Fe web and Rollcall campaign: gate-anchored status

Status: authoritative campaign burn-down

Updated: 2026-09-05

Goal spine: write the math, get the kernel, keep the proof.

This is the single current status ledger for the campaign. The larger
`FE_NATIVE_GALLERY_PLAN.md` remains the design record, evidence narrative, and
Definition of done. Do not reconstruct another master checklist from the
session history. Add scope here only when it changes the goal or a named exit
gate.

The current proof architecture and hindsight review is
[`RECURSIVE_MANDELBROT_ZK_HINDSIGHT_AUDIT.md`](RECURSIVE_MANDELBROT_ZK_HINDSIGHT_AUDIT.md).
It audits the implemented Fe proof stack, the exact boundary between recursive
relations and recursive cryptography, the current compiler and WebGPU costs,
and the recommended consolidation order. It is supporting analysis for this
burn-down, not a second checklist.

## Current priority: compiler boundary consolidation

Generic numeric identity consolidation landed as `2ac48d96a` (cleanup
`dd3c45e15`). HIR now owns the twelve existing checked-arithmetic,
saturating-arithmetic, bitcast, and integer-truncation declaration spellings,
with a body-aware tracked function query. CTFE consumes that vocabulary;
its duplicate checked/saturating name table and separate bitcast/truncation
dispatch entries are deleted. One focused release test evaluated all twelve
through static assertions, including saturating boundaries and bit transport:
`/workspace/scratch/mb2-generic-intrinsic-ctfe-release.log` (1.27s after a
2m22s build). Runtime consumption and expanded authored-name/trap tests are
still uncommitted in the cleanup worktree. Their first release gate passed
the fifteen authored-name cases and two existing f32 groups, but the all-numeric
execution test failed at the preexisting rejection of signed division.
The portable lowerer also rejects signed remainder, power, and saturation.
Log: `/workspace/scratch/mb2-generic-intrinsic-wasm-release.log`.

The follow-up focused execution gate found a correctness defect, not merely
missing support: checked `i32::MAX + 1` returns `-2147483648` instead of trapping.
`checked_i32_intrinsic_addition_traps_on_overflow` reproduces this in 1.51s:
`/workspace/scratch/mb2-checked-i32-overflow-baseline-release.log`.
The unchanged portable `lower_intrinsic_arith` implementation explicitly guards
only narrowed `usize`, dropping the checked flag for other integer add/sub/mul
operations. This behavior is present in cleanup parent `6f7c09279`; the new
identity mapping did not introduce it. The pinned Sonatina Wasm translator also
implements `Uaddo` and `Umulo` with a constant false overflow result, so changing
the Fe opcode alone is not a repair. Keep the failing regression and resolve
checked arithmetic semantics and backend capability support before calling the
runtime intrinsic consolidation verified. No full-prover correctness conclusion
follows from this small arithmetic repro alone.

The first backend repair is committed locally in Sonatina as `a2c48db9`:
Wasm `Uaddo` and `Umulo` now return the wrapped value and actual overflow,
instead of a hardcoded i64 operation and false flag. A direct execution oracle
checks 2,660 cases across 1/8/16/32/64-bit widths against u128 arithmetic,
including zero multiplication and randomized operands. The parent reproduced
`u64::MAX + 1 -> (0, false)`; the repaired oracle passed in 0.03s. Logs:
`/workspace/scratch/mb2-sonatina-overflow-baseline-execution-release.log` and
`/workspace/scratch/mb2-sonatina-overflow-fixed-release.log`.
The affected Wasm-backend suite passed 31/31 in 0.06s after separately reconciling
the stale empty-module memory golden. Its old ceiling had already been removed
by `1fb9968e`; a saved earlier executable reproduced the identical mismatch.
Evidence: `/workspace/scratch/mb2-sonatina-empty-module-saved-baseline.log` and
`/workspace/scratch/mb2-sonatina-overflow-wasm-suite-reconciled-release.log`.
These Sonatina changes are not yet in Fe's published dependency pin and have
not been pushed here. Signed operations, Naga support, and Fe's checked trap
wiring remain open; the checked-i32 Fe regression is still red.

The next local Sonatina slice implements `Saddo`, `Ssubo`, `Smulo`, and `Usubo`
with common result transport. Signed narrow values are extended from their
semantic width before checking wider exact results; signed i64 multiplication
checks unsigned magnitudes without trapping on MIN / -1 or zero. The combined
oracle now checks 8,310 cases for all six overflow operations at five widths,
using independent i128/u128 arithmetic. All 31 Wasm-backend tests passed in
0.06s: `/workspace/scratch/mb2-sonatina-all-overflow-wasm-release.log`.
This closes the Wasm instruction slice, not Cranelift/Naga or Fe integration.
The frontend still drops checked arithmetic flags and its regression remains
red. The local backend changes have not been pushed or pinned by Fe.

Native overflow support is now committed locally in Sonatina as `c11e3d5d`.
Wasm and Cranelift share the decoder for the six existing overflow instruction
identities, while each target retains its physical lowering. Native checks
cover 8,190 cases at 1/8/16/32/64 bits and 30,246 cases at 128 bits against
independent Rust wide-integer arithmetic. The i128 multiply uses 64-bit limb
products and carries because Cranelift rejects `umul_overflow.i128`; constant
lowering now preserves both i128 words. A separate constant gate checks zero,
all ones, the sign bit, and distinct high/low words.
All 26 Cranelift and 31 Wasm backend tests pass in release (0.06s and 0.05s):
`/workspace/scratch/mb2-sonatina-native-overflow-complete-release.log`.
Translation failures now return a function-named error before JIT finalization
instead of silently skipping definitions. Both rejected native allocation
tests assert that stronger error contract. Once the new dependency is
integrated, Fe's panic-catching missing-definition fallback can be removed
after checking remaining declaration/runtime-call handling. These native
changes are not pushed or pinned by Fe. Naga overflow support and Fe checked
trap wiring remain open; no full browser or proof gate is implied.

The scalar-suffixed arithmetic/comparison and resource vocabularies remain
unfinished; this is not the complete intrinsic capability system.

Browser capstone preflight on September 5 is not green. The Node-based existing
health harness timed out navigating to `http://10.0.0.2:8024/health.html` before
its one-word GPU control ran. A separate connectivity-only page loaded
`about:blank` successfully, but the same HTTP navigation timed out after 15s
without a request reaching its local HTTP server. An attempted DevTools-served
control then timed out in `Network.setCacheDisabled`, before shader submission.
These observations do not diagnose a GPU crash, prove driver health, or test
the production shader. The next browser gate must establish page delivery and
the tiny GPU control before running the prover. Existing user pages and servers
were left untouched. Evidence:
`/workspace/scratch/mb2-naga-boundary-chrome-health-node.log`,
`/workspace/scratch/mb2-chrome-connectivity.log`, and
`/workspace/scratch/mb2-chrome-connectivity-isolated.log`.

New Mandelbrot protocol work is paused while the Fe-to-Sonatina boundary is
consolidated. The existing prover remains the browser correctness/performance
capstone; this does not remove any proof or gallery completion gates below.

Landed on mb2: portable body lowering separated from Wasm host synthesis
(`80ccc9fd2`), the scoped authored-allocation regression corrected with emitted
Wasm and execution evidence (`2aa303099`), local storage planning (`f1381f63e`),
explicit call storage/lifetimes (`cd48e2591`), and reachable-call planning before
SSA emission (`af3524c95`). Binding facts are now derived once before consuming
runtime blocks (`5f0d9f68a`), removing recursive full-body scans and the emission
block clone. Its 33 focused release tests passed: exhaustive binding-rule
comparison and a 50,000-local chain (2), Shader IR (3), typed allocation (17),
canonical arena (5), resident lifecycle (1), raster (4), and compute resources
(1). Logs: `/workspace/scratch/mb2-binding-facts-release.log` and
`/workspace/scratch/mb2-binding-facts-resource-release.log`.

The explicit Sonatina Shader ISA, Naga target/profile implementation, and
public contextual helper query are now published through `54c6c633`.
On September 5, `git ls-remote` reported `98696420` on the Sonatina remote's
`mb2-task-borrows` branch; `54c6c633` is its verified ancestor. No push was
performed here. The cleanup candidate now pins that exact tested ancestor in
Cargo.toml and Cargo.lock, without a local dependency override. The verified
integration landed on live mb2 as `462593c76` (cleanup checkpoint `0204d7ce5`).
It installs the Shader ISA, explicit Naga requests, the shared backend-owned
helper analysis, and early residual-f32 rejection, and deletes the Fe helper
legality classifier and raster scalar-only filter. The two lowering files are
byte-identical to the tested cleanup candidate. The live-mb2 integration has
329 insertions and 644 deletions across five files; unrelated proof and WebGPU
worktree edits were not staged. No push was performed here.

Locked published-pin gates passed in the cleanup checkout: the production
linear plan (56,001 WGSL bytes, 2,150 Naga expressions, 30 helpers), six authored
raster tests (33.36s), and the independent f32 render oracle on real llvmpipe
execution (7.12s, no GPU skip). Logs:
`/workspace/scratch/mb2-published-shader-boundary-production-release.log`,
`/workspace/scratch/mb2-published-shader-boundary-raster-release.log`, and
`/workspace/scratch/mb2-published-shader-boundary-f32-gpu-release.log`.
The first production replay reported 38,748ms lowering and 46,725ms total,
slower than the earlier local-override run despite identical artifact counts.
A warm repetition passed with identical artifact counts, reporting 28,779ms
lowering and 36,524ms total. Both saved before/after test executables still
exist, so a same-session paired replay ran without rebuilding either compiler.
The earlier executable reported 25,852ms lowering / 35,716ms total; the new
executable reported 18,338ms / 25,353ms. Both passed with identical artifact
counts. This pair does not reproduce a regression, and the earlier executable
also ran substantially slower than its historical 9,962ms observation. These
single runs are not a controlled speedup benchmark. Pair evidence:
`/workspace/scratch/mb2-shader-boundary-paired-before.log` and
`/workspace/scratch/mb2-shader-boundary-paired-after.log`. Warm log:
`/workspace/scratch/mb2-published-shader-boundary-production-warm-release.log`.
The early intrinsic gate separately passed its RMIR-only test (14.17s), all
13 f32 execution tests (16.36s), and authored-name execution (7.85s). Evidence:
`/workspace/scratch/mb2-early-intrinsic-rmir-release.log`,
`/workspace/scratch/mb2-early-intrinsic-f32-release.log`, and
`/workspace/scratch/mb2-early-intrinsic-authored-release.log`.
Live mb2 then passed all four authored-raster tests present on that branch in
19.37s, after an 8m01s release rebuild. These cover nominal varying derivation,
payload rejection, shared content-addressed resources, and ordered mixed pass
graphs. Evidence: `/workspace/scratch/mb2-main-naga-integration-raster-release.log`.
The six-test cleanup run also includes two tests not yet present in live mb2;
the two test counts must not be conflated. Broader live-mb2 and Chrome capstone
validation remain open, as do the broader
storage, intrinsic-family, CFG-normal-form, and direct-route study obligations.

`d7ec3b402` restricts Wasm's signature and oversized-local policies to the
Wasm32 ISA. The regression first reproduced Shader IR rejecting a 1,001-lane
signature with a Wasm 1,000-lane error. After the fix, Wasm still rejects the
public signature while Shader and native IR construction accept it (5.36s).
This is an IR distinction, not a claim that every physical shader/native ABI
accepts 1,001 parameters; Sonatina's shader packing and limit checks remain.
Four large-aggregate Wasm execution tests passed (8.21s), covering mutable and
read-only borrows, discarded returns, and snapshots. The production linear
gate also passed with unchanged counts: 56,001 WGSL bytes, 2,150 expressions,
30 helpers (27,822ms lowering / 34,894ms total). The tested lowering file landed
byte-identically on mb2; cleanup checkpoint is `6f7c09279`. Evidence:
`/workspace/scratch/mb2-target-arity-baseline-release.log`,
`/workspace/scratch/mb2-target-arity-fixed-release.log`,
`/workspace/scratch/mb2-target-arity-production-release.log`, and
`/workspace/scratch/mb2-target-arity-wasm-large-release.log`.

The following paragraphs record intermediate extraction gates. The earlier
backend extraction planned contextual helper ABIs before emitting helper bodies
and passed sixteen focused release tests, before the public Fe legality query
and classifier deletion described later in this record.
Evidence: `/workspace/scratch/mb2-sonatina-helper-plan-release.log`.
The subsequent shared body query also passed sixteen helper tests and shares
one structured CFG across resource variants. Fe's cleanup integration removes
its duplicate instruction/control preflight and trace classifiers, with all
nine selected gates passing in
`/workspace/scratch/mb2-fe-helper-body-query-release.log`. The full
contextual type/resource ABI query remains. Authored raster now uses the common
ABI planner too, with its existing restrictions preserved. Sixteen helper and
five raster tests passed; Fe integration passed three Shader IR and six raster
tests in `/workspace/scratch/mb2-fe-raster-abi-plan-release.log`.
`81be0b05` moves the complete physical parameter list, including hidden
heap/bump/trap transport, into that same pre-emission plan. Sixteen helper and
five raster release tests passed in
`/workspace/scratch/mb2-sonatina-physical-parameters-release.log`.
Fe integration against this exact local revision passed three Shader IR tests
(1.87s) and six raster tests (8.09s) in
`/workspace/scratch/mb2-fe-physical-helper-parameters-release.log`.
Fe's corresponding integration is pending in `codex/mb2-boundary-cleanup`, whose
`docs/mb2/FE_SONATINA_SHADER_BOUNDARY_CLEANUP.md` records the detailed execution
plan and gate logs. Isolated combined validation passed 26 focused release
tests after shared Cargo artifacts exposed cross-worktree reuse. The newer
binding-facts change is also incorporated there as `a6cf87ed5`; its combined
Shader-candidate validation passed all 20 selected tests (three Shader IR and
seventeen typed allocation). The log is
`/workspace/scratch/mb2-shader-binding-facts-release.log`.

`d22b707c2` makes emission consume the local carrier chosen by the body plan,
deleting the duplicate local-type classifier and its separate GPU-resource
lookup. All 31 selected release tests passed: Shader IR (3), typed allocation
(17), address-taken scalars (1), enums (2), record views (1), raster (4), compute
resources (1), and native scalar/control-flow execution (2). Logs:
`/workspace/scratch/mb2-planned-ssa-carriers-validated-release.log` and
`/workspace/scratch/mb2-planned-ssa-carriers-native-release.log`. The change is
reconciled into the cleanup worktree as `9ee175e4f`; combined Shader-candidate
validation passed three Shader IR and six authored raster tests. Evidence:
`/workspace/scratch/mb2-shader-planned-carriers-release.log`.
Its broader gates exposed an obsolete payload-enum
rejection test, reproduced on the unchanged parent. `dbe192493` corrects that
test to execute supported payload values and trap invalid host tags; both enum
tests passed against the unchanged compiler. Parent/revised evidence:
`/workspace/scratch/mb2-parent-payload-enum-release.log` and
`/workspace/scratch/mb2-payload-enum-contract-release.log`. This is a test
contract correction, not new payload-enum support or a compiler behavior fix.

Remaining boundary work includes representation/lifetime consolidation,
Sonatina-owned GPU ABI and capabilities, early Fe intrinsic gating, CFG normal
form, deletion of superseded mechanisms, the bounded direct-RMIR-to-Naga study,
and full backend/browser capstone validation. No shader-size or browser-speed
improvement is claimed for the planning refactors above.

The broader shader gate uncovered an existing direct-return loop-exit bug,
reproduced on unchanged `a233e45d`: the merged exit stored its phi but skipped
return transport, yielding 0 instead of 52. `bb89ba01` shares exit-return
handling across header, explicit-edge, and merged exits. All 116 focused
shader-backend tests passed, including lavapipe execution, in 3.27s:
`/workspace/scratch/mb2-sonatina-loop-exit-return-fix-release.log`.
The earlier adapter failures required the Vulkan loader library path; they
were not counted as passes. Chrome and production-prover gates remain open.

`51546b51` extracts entry instruction legality, trap requirements, and arena
high-water analysis into a reusable body plan consumed by emission. The
combined candidate passed all 116 shader-backend tests with lavapipe in 2.95s:
`/workspace/scratch/mb2-sonatina-entry-plan-combined-release.log`.
Fe integration passed three Shader IR tests (1.91s) and six raster tests
(9.14s) in `/workspace/scratch/mb2-fe-entry-plan-context-release.log`.
This makes another prerequisite independently reusable; it is not yet the
complete public contextual ABI query.

`9347f7c2` makes entry helper preparation return its context, physical plans,
typed-local function map, and heap/trap transport together. Emission consumes
that result instead of building its memory parameter types independently.
All 116 shader-backend tests passed with lavapipe in 2.94s:
`/workspace/scratch/mb2-sonatina-prepared-entry-helpers-release.log`.
Fresh Fe integration passed three Shader IR and six raster tests:
`/workspace/scratch/mb2-fe-prepared-entry-helpers-release.log`.
The pending Fe request driver now establishes pipeline/resource/builtin context
before outlining and performs selected-root validation centrally. Its ten
release tests passed (three Shader IR, six raster, one compute-resource):
`/workspace/scratch/mb2-fe-request-owned-outlining-release.log`.
This moves context to the selection boundary; the public contextual ABI query
and replacement of Fe's type whitelist remain unfinished. The temporary
lockfile override is removed. Publication permission for Sonatina through
`9347f7c2` has been requested, not assumed; the portable Fe pin is unchanged.

Focused production measurement on the pending Fe integration plus Sonatina
`9347f7c2`: `production_sparse_linear_plan_lowers_in_isolation` passed with
56,001 WGSL bytes, 30 helper functions, and 2,150 expressions in the reparsed
Naga artifact. Lowering took 9,814ms; measured setup/lowering/validation took
12,371ms. Evidence: `/workspace/scratch/mb2-boundary-production-linear-plan-release.log`.
This is a real production-stage structural gate, not numerical execution or a
complete proof. The expression count is not the pre-writer backend census.
A controlled baseline on Fe `d1fb17473`, with only the two pending Fe boundary
integration files removed and the same Sonatina `9347f7c2`, passed with exactly
the same 56,001 bytes, 30 helpers, and 2,150 reparsed expressions. Baseline
lowering took 11,740ms and total measured time was 14,911ms. These single-run
times are not a speedup claim. This Fe integration preserves the measured
shader size; it does not explain earlier reductions. Evidence:
`/workspace/scratch/mb2-boundary-production-linear-plan-fe-parent-release.log`.
The integration was restored afterward and both source hashes verified.

Sonatina `c2bbc9e9` adds the outlined-call counterpart of the loop-exit return
regression. The same loop exercises header, conditional, and jump exits both
as a shader entry and as a private helper. The test requires one retained
helper body and call, and checks that caller computation after the call still
executes. Both numerical tests ran on llvmpipe; the full shader-backend target
passed 117 tests in 2.82s. Evidence:
`/workspace/scratch/mb2-sonatina-outlined-loop-return-suite-release.log`.
This strengthens the CFG/call boundary gate, not the contextual ABI query or
the hardware-browser capstone. The Sonatina commit remains local and unpushed.

Sonatina `6012c45e` separates entry-rooted preparation from function emission.
`PreparedNagaEntry` owns its Naga type/global arenas, typed-local preparation,
entry body facts, proven heap size, external roots/layout, and prepared helper
ABIs. The existing compute/fullscreen/legacy emitter consumes that result.
This reuses the existing checks and does not add a new legality classifier.
Authored raster still has its separate entry preparation; public contextual
selection and partial rejection reporting remain unfinished. All 117 backend
tests passed in 3.42s, including llvmpipe execution:
`/workspace/scratch/mb2-sonatina-entry-preparation-release.log`.
The production linear-stage gate passed against this local candidate with
56,001 WGSL bytes, 2,150 reparsed Naga expressions, and 30 helpers, unchanged
from the controlled comparison. Lowering took 12,370ms and total measured time
was 15,448ms; one run is not a performance trend. Evidence:
`/workspace/scratch/mb2-boundary-entry-preparation-production-release.log`.
The local dependency override was removed from Cargo.lock afterward.

Helper ABI planning now reports each function's local outcome in call-postorder
instead of discarding valid child plans or stopping before later siblings.
Both compute/fullscreen/legacy and authored-raster emission require a complete
report; a partial success is not an emission authorization. A focused release
test rejects an i256-returning parent while retaining its i32 child and later
sibling, then confirms the partial report is rejected by the emission gate.
Evidence: `/workspace/scratch/mb2-sonatina-partial-helper-report-release.log`.
All 117 backend tests also passed in 2.47s:
`/workspace/scratch/mb2-sonatina-partial-helper-report-suite-release.log`.
Follow-up: the report now closes successful ABI plans over direct callees in
call-postorder. A locally representable ancestor of the rejected parent is
also rejected, while the valid child and sibling remain planned. The expanded
focused test passed, and all 117 backend tests passed in 14.31s:
`/workspace/scratch/mb2-sonatina-transitive-helper-report-final-release.log`
and `/workspace/scratch/mb2-sonatina-transitive-helper-report-suite-release.log`.
Typed-local preparation now also retains per-function rejections alongside
the interned type map. A rejected i256 local no longer prevents preparing a
later function's f32 local. Final emission still requires a complete type
report; an interned type does not authorize an invalid use closure or budget.
The focused regression and all 117 backend tests passed (suite 4.80s):
`/workspace/scratch/mb2-sonatina-typed-local-report-release.log` and
`/workspace/scratch/mb2-sonatina-typed-local-report-suite-release.log`.
Resource-result planning now also records per-helper outcomes. Its regression
rejects conflicting resource returns and the caller depending on that missing
identity, while retaining an independent helper's exact argument identity.
The focused regression and all 117 backend tests passed (suite 2.44s):
`/workspace/scratch/mb2-sonatina-resource-result-report-release.log` and
`/workspace/scratch/mb2-sonatina-resource-result-report-suite-release.log`.
Sonatina `4a018ab9` now retains per-helper resource-variant outcomes as well.
Conflicting resource aliases in one helper do not discard an independent
sibling's proven entry-rooted identity or prevent propagating it to a child.
The focused regression passed, including rejection of partial reports for
emission and of conflicting entry declarations as global request errors.
All 117 backend tests passed with llvmpipe in 2.62s. Evidence:
`/workspace/scratch/mb2-sonatina-resource-variant-report-release.log` and
`/workspace/scratch/mb2-sonatina-resource-variant-report-suite-release.log`.
This is still not the public Fe query: the separate reports must be combined
before deleting Fe's classifier. An identity binding alone never authorizes
emission of its rejected owner. The newer Sonatina commits remain local and
unpushed; this reporting change makes no shader-size or browser-speed claim.

Sonatina `e3d00226` combines logical-result, resource-variant, and memory
analysis in `EntryHelperContextReport`. The existing emitter obtains its
context only through the report's complete-result gate, including the proven
entry-arena requirement. A resource rejection no longer prevents independent
memory analysis from running. The contextual regression passed, and all 117
backend tests passed with llvmpipe in 6.78s. Evidence:
`/workspace/scratch/mb2-sonatina-context-report-release.log` and
`/workspace/scratch/mb2-sonatina-context-report-suite-release.log`.
This report is still internal. Typed-local and physical-ABI outcomes must join
the public selection query; authored-raster entry preparation remains separate.
Missing logical resource identities can still prevent deriving downstream
entry-rooted bindings. No missing identity is invented to recover a candidate.

Sonatina `678fb936` feeds per-function typed-local rejections into the shared
physical ABI planner. Root-local validity remains mandatory before entry
preparation; helper storage failures now close over dependent callers in the
same plan that handles signature failures, preserving independent children
and siblings. The new regression uses a representable 20,000-byte typed array
that exceeds the private-storage policy: successful type interning cannot
authorize the helper. Both focused helper-report tests passed, and all 117
backend tests passed with llvmpipe in 3.24s. Evidence:
`/workspace/scratch/mb2-sonatina-typed-physical-report-release.log` and
`/workspace/scratch/mb2-sonatina-typed-physical-report-suite-release.log`.
The complete-context requirement still precedes physical planning. Combining
partial context outcomes with physical selection, exposing the public query,
and replacing Fe's classifier remain open. Authored raster retains its current
entry preparation and supported local subset; no new raster capability is
implied by sharing the planner's rejection input.

Sonatina `47e8e52d` joins partial context and typed-local outcomes with physical
planning in `EntryHelperSelectionReport`. Production entry preparation now
consumes this report's complete-result gate. Independent resource helpers can
receive physical plans despite another helper's resource rejection; rejected
prerequisites also disqualify dependent callers. Missing entry-arena ownership
is a prerequisite rejection, not an assumed heap handle. The focused combined
selection regression passed, and all 117 backend tests passed with llvmpipe in
2.96s, including the missing-arena-owner gate. Evidence:
`/workspace/scratch/mb2-sonatina-contextual-selection-release.log` and
`/workspace/scratch/mb2-sonatina-contextual-selection-suite-release.log`.
The report remains internal: request-level validation and authored-raster
preparation must support the public query before Fe's classifier is removed.
It does not claim legality for unresolved resource identities or incomplete
entry contexts, nor does it emit a partially valid shader.

Sonatina `e2128b46` extracts the existing single-entry signature, resource,
builtin, dispatch, and resource-liveness checks into `NagaEntryInterface`.
Emission consumes the same derived facts; the extraction adds no alternative
validator. The focused regression checks dispatch extent/count/span, zero
dimensions, overflow, and missing builtin arguments. It also verifies that a
valid interface containing an unsupported helper still fails full compilation.
The regression passed, and all 117 backend tests passed with llvmpipe in 2.81s.
Evidence: `/workspace/scratch/mb2-sonatina-entry-interface-release.log` and
`/workspace/scratch/mb2-sonatina-entry-interface-suite-release.log`.
This is interface analysis, not complete request legality: body-dependent mode
checks and paired authored-raster preparation remain separate. The public
capability query and Fe classifier replacement are still pending.

Sonatina `2d8e7b75` moves the existing grid/batch/compute/fullscreen body-mode
checks before helper body emission. Interface and body preparation still
precede this gate; this is not a new complete capability query. The impossible
simultaneous render/grid guard is deleted because `ShaderPipeline` already
excludes that state. A focused derived-body contract test covers incompatible
allocations, arena use, traps, and their compatible counterparts. It passed;
all 117 backend tests passed with llvmpipe in 2.70s. Evidence:
`/workspace/scratch/mb2-sonatina-entry-mode-release.log` and
`/workspace/scratch/mb2-sonatina-entry-mode-suite-release.log`.
The accepted mode surface is preserved; no shader-size improvement is claimed.
Paired authored-raster preparation and public contextual-query integration
remain open, as do the broader CFG and intrinsic-capability tasks.

Sonatina `d009979f` separates paired-root scalar helper preparation from raster
emission. The existing scalar-only contract now supplies per-function
prerequisite rejections to the common physical ABI planner. Unsupported
parents no longer hide valid children or independent siblings. The preparation
regression passed and checks that a partial report cannot emit even a valid
prefix. All 117 backend tests passed with llvmpipe in 4.66s. Evidence:
`/workspace/scratch/mb2-sonatina-raster-helper-preparation-release.log` and
`/workspace/scratch/mb2-sonatina-raster-helper-preparation-suite-release.log`.
Raster helper ABI support is unchanged: aggregate/resource/private-memory
transport is not enabled by this extraction. Paired entry interface/body
preparation and the public request query still need consolidation before Fe's
classifier can be replaced.

Sonatina `b58ce181` extracts paired raster entry preparation before Naga module
construction. The result preserves builtin resolution, the complete scalar
state record, compact external resources, and exact stage visibility, using
the existing interface and root-operation checks. Its regression passed for
state preservation and invalid entry pairing, state suffixes, and builtin
prefixes. All 117 backend tests passed with llvmpipe in 2.43s. Evidence:
`/workspace/scratch/mb2-sonatina-raster-entry-preparation-release.log` and
`/workspace/scratch/mb2-sonatina-raster-entry-preparation-suite-release.log`.
Root CFG structurization still occurs in the individual vertex/fragment
lowerers; this result is not a complete shader-validity certificate. Sharing
those prepared control facts and exposing the public request query remain
before Fe classifier replacement. No new raster transport is enabled.

Sonatina `23f8507b` moves raster root normalization, structurization, and return
arity checking into paired entry preparation, before Naga module construction.
Each stage plan retains its normalized body with the CFG and return IDs derived
from that body; vertex and fragment emission consume those plans without
repeating control analysis. The focused preparation regression passed, and all
117 backend tests passed with llvmpipe in 3.45s, including raster multi-return
and early-return coverage. Evidence:
`/workspace/scratch/mb2-sonatina-raster-cfg-preparation-release.log` and
`/workspace/scratch/mb2-sonatina-raster-cfg-preparation-suite-release.log`.
This closes the raster root-CFG preparation gap described above. The public
contextual capability query and Fe classifier deletion remain unfinished.
No fresh Chrome or production shader-size measurement is claimed for this
refactor. The Sonatina commit is local; live Fe still uses its published pin.

Sonatina `f9e96fcd` preserves the partial helper-selection report through entry
preparation. Previously that boundary immediately required completeness and
discarded independent helper plans after any helper rejection. Emission now
consumes the completeness check explicitly, before emitting any helper body.
Three preparation tests passed, including an unsupported helper and dependent
parent alongside an independently valid leaf. All 117 backend tests passed
with llvmpipe in 2.50s. Evidence:
`/workspace/scratch/mb2-sonatina-entry-partial-selection-release.log` and
`/workspace/scratch/mb2-sonatina-entry-partial-selection-suite-release.log`.
Entry-body validity remains a prerequisite: the initial regression with an
i256 result directly in the entry correctly failed its carrier check. A public
pre-outlining query must distinguish invalid entry operations from call-boundary
shapes requiring legalization; this report is not yet that public query.
No new browser, shader-size, or live Fe integration claim accompanies this gate.

Sonatina `6ae3c3fa` exposes `NagaBackend::analyze_request_helpers` for explicit
Shader requests, including paired raster. It uses the existing interface,
entry-body, typed-local, resource, and physical helper preparation routines,
without emitting function bodies or shader text. The observational result
reports callable functions, variant counts, source instruction counts,
resource access, maximum physical parameter counts, and per-function rejection
diagnostics. Compilation still rederives and requires complete legality.
The public-query regression retains an independent leaf while rejecting an
unsupported helper and its caller; compilation of that same request fails.
Paired raster query coverage agrees with the emitted shared scalar helper.
Three focused tests and all 117 backend tests passed (llvmpipe suite 2.42s):
`/workspace/scratch/mb2-sonatina-public-helper-analysis-release.log` and
`/workspace/scratch/mb2-sonatina-public-helper-analysis-suite-release.log`.
This API still requires valid entry bodies. Fe's pre-outlining driver can have
call-boundary shapes requiring legalization before that prerequisite holds;
resolving this distinction and deleting Fe's classifier remain next, not done.
The new Sonatina series remains local and is not live Fe's dependency pin.

Production query integration evidence on Sonatina `6ae3c3fa` and the pending
Fe cleanup driver: the existing inline trace now observes the contextual query
before and after inlining without changing helper selection. Contrary to the
general entry-validity concern above, this production linear-plan fixture is
already valid for the query before inlining: 216 callable helpers and two ABI
rejections (`encode__g1bad` and `write_sparse_base_air_partition_lane`). After
the existing inliner it reports 30 callable helpers and no rejections.
The release artifact gate passed with 56,001 WGSL bytes, 2,150 reparsed Naga
expressions, and 30 helpers, unchanged from the prior measured artifact.
Instrumented lowering took 13,522ms, total 17,050ms (test 17.20s); these timings
include the extra observational analyses and are not a speedup claim.
Evidence: `/workspace/scratch/mb2-boundary-public-query-production-release.log`.
Next is replacing this path's Fe legality classifier using the backend result
while retaining Fe's profitability policy. Do not infer that all pre-outlining
fixtures meet the query prerequisites from this one successful production gate.
This run used a command-scoped local Sonatina override, with Cargo.lock restored
afterward. No Chrome execution or new proof completion is claimed.

Pending Fe cleanup now selects explicit WebGPU helpers from Sonatina's
`analyze_request_helpers`, after exact merging and graph normalization.
Query errors propagate without a fallback. Fe retains only profitability and
the selected-callee closure policy; its separate raster scalar ABI filter and
the preserve/scalar-only booleans are deleted. Legacy scalar/grid entry points
still use the old classifier, explicitly separate from the WebGPU request path.
The production gate with backend-driven selection passed unchanged at 56,001
WGSL bytes, 2,150 reparsed Naga expressions, and 30 helpers. Instrumented
lowering was 13,244ms, total 16,949ms (test 17.09s), not a speedup claim.
Six raster-focused gates passed in 7.88s, the compute/resource gate passed in
1.36s, and the independent resident scalar-actor Wasm oracle passed in 1.31s.
Logs: `/workspace/scratch/mb2-boundary-backend-selection-production-release.log`,
`/workspace/scratch/mb2-boundary-backend-selection-raster-release.log`,
`/workspace/scratch/mb2-boundary-backend-selection-compute-release.log`, and
`/workspace/scratch/mb2-boundary-backend-selection-wasm-release.log`.
The two-file Fe integration remains in its isolated worktree, backed up at
`/workspace/scratch/mb2-backend-selection-fe-integration.patch`. It requires
local Sonatina `6ae3c3fa`; the command-scoped override's lock changes were
restored. Do not claim this code is in live mb2 or portably dependency-pinned.
Legacy classifier deletion, earlier intrinsic gating, broader gates, and
publication/integration remain unfinished. No browser run accompanied this gate.

Sonatina `54c6c633` routes legacy `analyze_entry_helpers` and explicit request
analysis through the same physical preparation. Legacy selection resolves the
same envelope as `compile_entry`, without imposing the WebGPU environment on
its existing i64 modes. The new legacy i64 and invalid-grid query coverage and
all 117 backend tests passed (llvmpipe, 2.80s):
`/workspace/scratch/mb2-sonatina-legacy-helper-analysis-suite-release.log`.
The pending Fe driver now uses this query for legacy scalar/grid too. Its
recursive helper-legality classifier, scalar/argument/result/body ABI tables,
and duplicate diagnostic classifier are deleted. Both paths preserve Fe's
profitability policy over backend-authorized helpers, with no error fallback.
The two-file Fe integration is now net 493 lines smaller than its committed
base (182 additions, 675 deletions, including the earlier boundary work).
Fe's legacy i64 keystone artifact gate passed in 1.32s; the grid execution gate
passed in 2.89s on llvmpipe with all 4,096 pixels matching the independent oracle
and Wasmtime. Production remained 56,001 WGSL bytes, 2,150 reparsed expressions,
and 30 helpers (instrumented lowering 9,962ms, total 12,554ms, test 12.67s).
Evidence: `/workspace/scratch/mb2-boundary-shared-selection-legacy-release.log`,
`/workspace/scratch/mb2-boundary-shared-selection-grid-release.log`, and
`/workspace/scratch/mb2-boundary-shared-selection-production-release.log`.
Backup: `/workspace/scratch/mb2-shared-selection-fe-integration.patch`.
Cargo.lock is restored after command-scoped local dependency testing. Fe code
is still isolated and uncommitted pending a portable Sonatina dependency pin;
publication approval has been requested, not assumed. No Chrome gate or full
CI completion is claimed. Earlier intrinsic gating and broader cleanup remain.

Phase-5 inspection found a real downstream identity mismatch: MIR excludes
functions with authored bodies from its intrinsic declaration recognizers,
but the portable call guard rejected f32 intrinsic spellings regardless of
body presence. The new execution regression reproduced rejection of an
ordinary u32 function named `__sqrt_f32` before the fix. Live mb2 commit
`8341897a4` aligns that guard with MIR's bodyless-declaration boundary.
Four ordinary names (`__sqrt_f32`, `__rsqrt_f32`, `__min_f32`, `__checked_add`)
execute their authored arithmetic across four inputs each. Three live-mb2
intrinsic tests passed in 7.24s against the published dependency pin, and the
unsupported `__rsqrt_f32` declaration gate still passed in 1.61s. The cleanup
worktree additionally passed all 13 f32-filtered tests in 19.69s.
Evidence: `/workspace/scratch/mb2-intrinsic-spelling-verified-baseline-release.log`,
`/workspace/scratch/mb2-intrinsic-spelling-fixed-release.log`,
`/workspace/scratch/mb2-intrinsic-spelling-f32-release.log`,
`/workspace/scratch/mb2-main-intrinsic-identity-release.log`, and
`/workspace/scratch/mb2-main-intrinsic-negative-release.log`.
This is a concrete correctness fix, not completion of typed intrinsic identity
or early target gating. Name-based bodyless numeric declarations in MIR/CTFE
and the downstream compatibility guard still require coordinated migration.
During testing, concurrent uncommitted ObjAtomic instruction edits appeared in
the shared Sonatina worktree and lacked parser builders. They were left intact.
Cleanup validation now has a detached immutable dependency checkout at
`/workspace/sonatina-worktrees/mb2-boundary-verified` (`54c6c633`) and a
command-scoped override `/workspace/scratch/mb2-verified-sonatina.toml`.
The interrupted dirty-dependency build is not counted as an intrinsic test.
No publication occurred; the live-mb2 fix needs no unpublished dependency.

The first shared intrinsic-identity slice is now on live mb2 at `a6f892737`.
HIR owns the 14 existing bodyless f32 helper identities and a tracked
function-identity query that excludes authored bodies. CTFE consumes that
vocabulary instead of its duplicate helper-name table. Both focused release
CTFE fixtures passed (2 tests, 4.98s) in the cleanup worktree, including all
14 identities and the CTFE-only reciprocal-square-root case. The four source
and fixture files landed byte-identically on mb2. Evidence is
`/workspace/scratch/mb2-shared-f32-identity-ctfe-release.log`.
MIR and portable codegen consumer migration subsequently landed as `8aa2f0da2`.
The cleanup checkout passed all 13 f32-filtered Wasm execution tests (23.46s)
and the authored intrinsic-spelling regression (21.80s), against immutable
Sonatina `54c6c633`. The migration removes 58 net lines on live mb2, replacing
the duplicate runtime enum/name table and downstream call-name guard with the
shared identity. Runtime rsqrt remains explicitly unsupported, unlike CTFE.
Evidence: `/workspace/scratch/mb2-shared-f32-identity-all-f32-release.log`
and `/workspace/scratch/mb2-shared-f32-identity-authored-release.log`.
A live-mb2 published-pin replay passed all three intrinsic-filtered execution
tests in 15.24s, including the authored-name regression. Its release rebuild
took 9m57s; that is compiler build time, not proof or shader execution time.
Evidence:
`/workspace/scratch/mb2-main-shared-f32-identity-release.log`.
This centralizes existing declaration spelling, not explicit intrinsic tags or
complete early target-capability gating; other numeric families remain open.

Published-prerequisite reconciliation: live mb2 was still pinned to
`ece351bda158009412bff7a20e8f8c2b0d25debe`, not the cleanup worktree's
`ef13a6568c0dbcd2e85a390048f81a20a61302ac`. The latter is already published
on the Sonatina remote and is now selected by the mb2 manifest and lockfile.
Its stable physical raster entry ABI requires matching test expectations
(`fe_vertex_main` / `fe_fragment_main`); authored logical entry names remain
unchanged. This prerequisite is not the newer Shader/Naga cleanup series.
The focused release gates passed: authored raster (4), compute-resource helper
retention (1), resident Wasm actor execution (1), the QCGA pencil demo (1), and
typed-borrow/private-storage Shader IR (3).
QCGA emitted 23,796 bytes of DE WGSL plus 10,025 bytes of marker WGSL, passing
its existing size and Naga validation checks. Evidence:
`/workspace/scratch/mb2-published-sonatina-prerequisite-raster-reconciled-release.log`,
`/workspace/scratch/mb2-published-sonatina-compute-release.log`,
`/workspace/scratch/mb2-published-sonatina-wasm-release.log`,
`/workspace/scratch/mb2-published-sonatina-qcga-release.log`, and
`/workspace/scratch/mb2-published-sonatina-shader-ir-release.log`.
These integration runs include the shared worktree's existing resource-test
changes; those changes are not included in this pin increment. No browser
execution, full CI completion, or publication of the newer local Sonatina
commits is claimed by these gates.

Legend:

- `[x]` implemented and backed by the cited gate
- `[~]` the current uncommitted or partially verified slice
- `[ ]` required by the Definition of done
- `[!]` an explicit risk or environmental qualification
- `[M]` needs Micah's hardware or product decision

## Intent reconciliation, 2026-08-26

This ordering was rechecked against the complete archived session
`e806786e-dff4-43c1-b25f-849ba82a8a02`, its 1,333 prompt-side history
entries, the original `GOAL.md`, `VISION.md`, actor and Conal design records,
the later browser-proof plans, and the current proof and browser evidence. The
review changes sequencing, not the goal or its gates.

The recovered invariant is one ordinary typed Fe denotation, followed by an
exact semantic structure, several analysis and placement interpretations, and
one fixed standards-derived realization. Semantic topology and placement
topology remain distinct. A placement may change dispatches, workgroups,
buffers, retention, and recomputation, but it may not change transcript order,
hash domains, recursive interval order, receipt layout, or the authored
mathematics. JavaScript, Rust, Chrome, Vulkan, and revm may realize standards,
compile artifacts, execute artifacts, or serve as independent oracles. They
may not own application policy or a second implementation of the proof.

The operating method remains `write -> derive -> prove -> place -> run ->
measure`. Each increment crosses that whole line at deliberately small depth.
This replaces both subsystem-at-a-time completion and browser-only showcase
forks. Byte identity and successful compilation are evidence about artifacts,
not semantic correctness; independent value and mutation gates remain the G2
authority.

Two older sequencing assumptions are now retired. Browser revm was originally
placed first because it was the only unknown infrastructure risk; S0 is now a
real Chrome-tested generic `fe-revm-browser` engine. BabyBear retargeting and a
first real WebGPU proof placement are also complete. The 114-query scalar
receipt boundary is now closed. The current unknown is exact physical-GPU
execution of the current authenticated FRI checkpoint graph, followed by scalable
WebGPU placement of the production policy and recursive parent proof. These
remain interpretations of the same BabyBear dependency graph, not separate
provers.

The 2026-08-31 release opening-relation gate compiled the production verifier
to a 2,801,240-byte Wasm module in 208.69 seconds, then authenticated 452 base
and 452 interaction leaves from the retained 948,808-byte receipt in 16.21
seconds. The independent path accounting matched 1,585 of 1,585 hashes and the
mutation matrix remained fail-closed. A const-evaluation cycle discovered by
this gate was removed by moving field-neutral FRI multiproof capacities into
`fri_structure`; receipt schemas and field interpreters now derive one shared
shape without depending on the consuming BabyBear impl graph.

The attempted policy-sized scalar path supplied useful negative evidence. The
114-query typed policy, receipt carrier, codec, prover/verifier boundaries, and
mutation surface source-check cleanly, but lowering the selected exact prover
entry produced no artifact or diagnostic after more than two hours and was
stopped. This is a compile-practicality failure of fully static expansion, not
a cryptographic rejection. Preserve the typed policy and receipt model, but
make query iteration and opening verification value-level and loop-compact.
Do not respond by baking generated queries, copying a second prover, or
weakening the 114-query policy.

Real Chrome WebGPU is the early acceptance rail. The existing
`chrome-devtools-mcp` configuration targets the external browser at
`http://10.0.0.1:9222`; use that path whenever the endpoint is live. Native
`wgpu`/Vulkan and llvmpipe remain fast exactness and portability gates, but do
not replace Chrome hardware evidence. No second GPU broker is needed unless
the existing fixed browser harness proves insufficient.

## Campaign gates

- G1, anti-bake: a new consumer works without compiler/runtime demo names.
- G2, semantic exactness: an independent model checks values and behavior, not
  just bytes, hashes, or successful compilation.
- G3, quality: performance and generated-kernel claims use measured receipts.
- G4, no scaffolding: application policy is Fe-authored; JavaScript and Rust
  remain fixed standards adapters, toolchain code, or independent oracles.
- G5, boundary: the exact repository CI command is green:
  `cargo nextest run --release --workspace --all-features --no-fail-fast --locked --exclude fe-language-server --exclude fe-bench`.

The completed gallery and architecture audits refine the remaining proof and
inspection work into six convergence gates. These are subdivisions of the
existing Definition of done, not a second campaign checklist:

- [~] **G-NTT:** one exact factor tree drives scalar and portable stage-grid
  WebGPU NTT/LDE interpretations. DIT, DIF, Bush where legal, and at least one
  irregular factorization retain independent value gates. Real browser WebGPU
  parity is required. A disposable API probe using the existing release
  compiler identified as `fe 26.2.0 (4447fdd27)` established that
  `RBin<Pair, 3>` normalization itself is cheap relative to loading `std`, while
  a recursive scalar interpreter remains practical. Timings were 33.22 seconds
  for a plain `std::conal` import, 26.84 seconds for normalization, 42.05
  seconds when the interpreter derived depth in the same traversal, and 55.32
  seconds when it also solved a separate recursive
  `BalancedBinarySchedule` obligation. The debug baseline had not completed
  after 378.15 seconds and was stopped. Therefore the public design should use
  one product interpretation for execution plus analysis, and release compile
  budgets must gate it. These timings are compiler-cost evidence, not semantic
  correctness evidence. Future type-function, FCO, provider, and graph-
  interpreter probes report both the user-facing release path and a bounded
  debug/developer path when appropriate. Ordinary demo edits do not pay the
  current multi-minute debug baseline merely to duplicate a release gate.
  Heavy semantic proof oracles use a different policy: the
  `mandelbrot_recursive_fixed_oracle` target is selected only by the
  `expensive-release-oracles` feature and rejects debug compilation. G5's
  release/all-features command includes it, while ordinary debug iteration
  does not compile the target at all. The reusable scalar and portable WebGPU
  baseline for this gate is now implemented in the focused
  `parallel_structure`, `parallel_ntt`, and `parallel_ntt_webgpu` ingots at
  commits `3a6a7f47f`, `051248e18`, and `9e356546a`. The factor policies reuse
  the canonical `std::conal::{Par, Pair, Comp}` constructors, with `Unit` as a
  factor-language name for `Par`; `Dit<4>` is definitionally the existing
  `RBin<Pair, 4>`, rather than a numerically similar parallel type. Recursive
  Fe type functions derive `Dit`, `Dif`, and composition-balanced `Bush` trees;
  explicit `Comp` nesting admits irregular factorizations. One `FactorTree`
  interpretation derives points, factors, butterflies, pair exchanges,
  dependency depth, association depth, composition nodes, fanout, and live
  values. A scalar interpreter and a portable `StageGrid` interpreter consume
  the same constructors. The WebGPU interpretation gives each butterfly lane
  its own progress cursor, so repeated actor dispatches advance exact widths
  `2, 4, 8, 16` without a racy host or shared cursor. The zero-import
  `parallel_structure_oracle` pins DIT, DIF, Bush, irregular, and Conal `RBin`
  normal forms. The zero-import scalar oracle checks all four 16-point
  policies, base-field NTT/INTT and coset LDE, and quartic-extension NTT/LDE
  against direct polynomial evaluation. A separate Rust `wgpu` oracle compiles
  compiler-derived browser-profile WGSL, executes forward and inverse N=16
  transforms on llvmpipe, and checks direct-DFT values, canonical round trips,
  stage receipts, progress, validity, and trap buffers across three vector
  families. The emitted prepare, forward, and inverse shaders are 4,302,
  38,868, and 41,987 bytes and require no workgroup shared memory or barriers.
  Structural and stage receipts are analysis evidence only; the direct DFT is
  the independent correctness oracle. Commit `c8288f5f6` migrated the
  production `mandelbrot_proof_gpu` toy checkpoint from a whole-transform
  scalar LDE call to the same typed stage-grid vocabulary. Four contiguous
  trace columns now execute prepare, two inverse stages, coset lift, four
  forward stages, and finish as Fe-authored WebGPU passes over private progress
  cursors. A focused llvmpipe gate checks the exact `1, 2, 1, 4, 1` repetition
  schedule, all 40 progress words, four coset predicates, trap freedom, and all
  64 LDE values against the independent direct DFT. The complete graph retains
  its independent Plonky3 Poseidon gate and clean/tampered mutation behavior.
  Commit `53aa061ea` carries the first FRI layer through the same exactness
  discipline. The clean LDE commitment is absorbed under the typed `MGFC`
  domain by a two-permutation, 16-lane repeated-dispatch placement. The
  resulting quartic challenge drives eight independent factor-2 folds through
  the reusable `fri_baby_bear` denotation. One 14-pass actor compiles to 836,285
  total WGSL bytes in 61.76 seconds. On llvmpipe, pipeline creation took 830.72
  ms, the clean graph took 976.98 ms, and a warm tampered graph took 17.41 ms.
  An independent Plonky3 model checks every challenge and fold field. A focused
  Chromium harness then executed the same immutable bundle on SwiftShader,
  observed all-clean blue bands, exact pink FRI and overall bands after the
  Fe-authored mutation, and clean recovery without reload, console error, or
  device loss. The external Radeon Chrome endpoint was unavailable during this
  run, so hardware parity for this new slice remains open.
  Commit `c468daaa1` extends that checkpoint through the complete exact toy FRI
  chain `16 -> 8 -> 4 -> 2 -> 1`. One 16-pass graph derives and executes all
  four quartic challenges, folds, ordered Poseidon2 Merkle roots, and typed
  transcript bindings with Fe-derived repeats `403, 358, 313, 268`. A derived
  `FriScratchRegions` schema consolidates six physical scratch arrays into one
  1,842-word typed tape. Together with the NTT preparation result no longer
  retaining a redundant validity buffer, the actor now has six graph resources
  and every pass uses at most eight storage bindings, exactly the portable
  WebGPU minimum. The first unconsolidated bundle failed honestly in Chromium
  at 13 or 14 storage buffers per compute stage. No browser limit was raised
  and no application predicate moved to JavaScript.

  A fresh release web build emitted 16 passes, 9,045,922 WGSL bytes, and 2,182
  Wasm bytes in 858.19 seconds. Source diagnostics took 700.65 seconds and
  backend lowering took 118.00 seconds. The equivalent pre-refactor build took
  1,765.65 seconds, including 1,111.51 seconds for source diagnostics and
  585.40 seconds for lowering. Chromium 150 then executed one live graph on
  SwiftShader with five clean blue bands, the exact final two pink rejection
  bands after the Fe-authored mutation, clean recovery, no console or device
  errors, and normal final resource release. A test-only standards readback
  copied the three 741-word Fe-owned proof tapes without decoding them in
  JavaScript. The independent Rust/Plonky3 oracle matched every trace and LDE
  value, generated Poseidon2 parameter, commitment root, FRI challenge, folded
  evaluation, FRI Merkle root, transcript digest, validity flag, and output
  color for clean, mutated, and recovered receipts. The fresh from-source
  release structural gate passed in 833.02 seconds; native execution explicitly
  skipped because this sandbox exposes no `wgpu` adapter. Local Sonatina commits
  `5b96d731` and `95f558bf` are the required structured-control-flow companions
  until they are published and pinned. This closes the executed toy FRI
  placement and browser exactness checkpoint.

  A subsequent local checkpoint extends the Fe-owned proof tape from 741 to
  902 words and adds transcript-selected query sampling, typed authenticated
  opening extraction, 56 query evaluations, 96 Merkle siblings, and explicit
  query validity receipts. All 17 Fe compute actors plus the display actor
  lower into Naga-validated browser WGSL in one release structural gate. The
  complete bundle compiled in 342.67 seconds with bounded per-stage compiler
  memory. Native execution then explicitly skipped because this sandbox has no
  GPU adapter, so this is compilation and validation evidence, not a value or
  performance receipt from physical WebGPU.

  Reaching that boundary also closed three general compiler blockers. Fe now
  avoids re-entering implicit layout-hole planning while resolving a
  self-branded ADT, and browser bundle compilation uses an isolated replicated
  compiler database per stage so completed Salsa state can be released.
  Local Sonatina commit `5202fc9a` lowers byte-exact `i1` private memory and
  `i32 -> i1` truncation, and structurizes narrowly recognized conditional and
  nested-loop corridors to the canonical loop exit. The focused Sonatina gates
  pass 25/25 structurizer tests plus both Naga-valid private-heap probes. More
  general noncanonical multi-exit transport remains deliberately fail-closed.
  Proof receipt validation is now constant-work Fe code that accumulates
  explicit validity bits rather than relying on data-dependent short-circuit
  exits.

  Physical-adapter exactness for the current authenticated FRI checkpoint graph, canonical
  extraction of that GPU result into the production receipt, the 114-query
  WebGPU policy, and recursive parent proving remain open.

  The focused immutable browser lab now publishes that exact graph as 18
  passes, 9,588,911 WGSL bytes, six typed resources, and 22 assets. Cold proof
  lowering and atomic site publication took 253.55 seconds after the release
  CLI build. A fresh headless Chromium profile executed all 18 passes through
  SwiftShader. Clean mode produced five exact blue validity bands; the
  Fe-authored mutation changed only FRI and overall to exact pink; clean
  recovery reproduced all 902 proof words without reload. No console,
  bootstrap, device-loss, or surface errors occurred. The captured clean,
  tampered, and recovered tapes then passed the existing independent direct-DFT
  and Plonky3 oracle, including query indices, evaluations, Merkle siblings,
  reconstructed roots, and inter-layer folds.

  The next local checkpoint widens the same graph from four selected trace
  columns to the complete production toy AIR placement: 17 main columns plus
  411 compiler-derived auxiliary columns. One typed, 428-batch stage grid
  transforms all 1,712 canonical trace words into 6,848 disjoint-coset LDE
  values. The Fe-owned proof tape grows from 902 to 3,042 words by appending
  the canonical trace and one completion flag per column. It still uses six
  physical resources and the portable eight-storage-binding ceiling. The
  existing FRI checkpoint now projects its step, magnitude, active, and
  terminal inputs from that full production LDE. It does not yet commit the
  main and auxiliary AIR trees or derive the AIR composition codeword, so it
  is not a complete STARK receipt.

  A fresh release precompile emitted 18 passes, 9,735,409 WGSL bytes, and
  2,182 Wasm bytes. Source plus dependency diagnostics took 99.21 seconds,
  proof lowering finished at 532.37 seconds, and atomic site publication
  finished at 537.54 seconds. Chromium 150 on SwiftShader compiled and mounted
  the graph in 232.07 seconds. The first clean execution took 133.29 seconds;
  warm tampered and recovered executions took 10.35 and 10.59 seconds. The
  mutation changed exactly the FRI and overall bands to pink, and recovery
  restored all five blue bands. An independent Rust model then matched all
  1,712 browser trace words, all 428 completion flags, every one of the 6,848
  browser LDE values against direct polynomial evaluation, and the complete
  authenticated FRI checkpoint receipt. This closes software-browser
  production-AIR LDE exactness. Physical Radeon parity remains open.

  The next exact slice commits the production LDE matrices themselves. A new
  compact `mandelbrot_proof_baby_bear_domains` ingot owns the nominal protocol
  domains shared by scalar and WebGPU interpretations. This removes the need
  for the GPU actor to import the entire scalar prover merely to recover domain
  tags, and prevents placement code from rebuilding numeric tags. Six new
  Fe-authored passes commit 16 main rows of 17 fields and 16 auxiliary rows of
  411 fields, then reduce both ordered 16-leaf trees through four staged
  Poseidon2 levels. The auxiliary sponge schedule is 2,288 repeated dispatches
  over one compiled body, derived from the fixed-length sponge permutation
  count. Both trees reuse the existing typed FRI scratch before that scratch is
  reset for the FRI schedule; physical resources remain at six and every pass
  stays within the portable binding ceiling.

  The resulting immutable lab has 24 passes, 11,128,114 WGSL bytes, 2,182 Wasm
  bytes, 28 assets, and a 3,060-word proof tape. Fresh publication took 439.56
  seconds. Chromium 150 on SwiftShader mounted the graph in 230.79 seconds and
  executed clean, tampered, and recovered frames in 63.79, 28.21, and 18.43
  seconds. The exact five-band mutation behavior remained green/pink/green.
  More importantly, the independent Rust and Plonky3 model reconstructed all
  32 production leaf commitments and both four-level roots directly from the
  6,848 independently derived LDE values, and matched every browser word. This
  closes software-browser production AIR commitment exactness.

  The following exact slice binds those commitments into the production AIR
  transcript. A compact shared `mandelbrot_proof_baby_bear_encoding` ingot now
  derives the injective row, public-input, and auxiliary-bit field encodings
  used by both scalar and GPU placements. The aggregate scalar interpreter and
  direct GPU field projection share one Fe-owned field-order and width source;
  there is no copied shader offset table. Four packed trace leaves, four packed
  auxiliary leaves, and the public digest are committed under their nominal
  domains. The public digest, both trace roots, and both LDE roots are then
  bound in an ordered four-step AIR transcript.

  The resulting immutable lab has 30 passes, 14,155,138 WGSL bytes, 2,182 Wasm
  bytes, 33 assets, and a 3,184-word proof tape. Fresh release publication took
  789.82 seconds. Chromium 150 on SwiftShader mounted the graph in 344.01
  seconds, executed the first clean frame in 386.93 seconds, and executed warm
  tampered and recovered frames in 38.54 and 23.82 seconds. Clean, mutation,
  and recovery behavior remained exact. The independent Rust and Plonky3
  oracle matched every LDE value, packed field, root, and final AIR transcript
  word from the captured browser tape. The receipt SHA-256 is
  `c0e886d136dc5e944faca70505455cfc3ec46d31e61d91afc4aae374e5a14ccc`.

  This closes software-browser exactness for the production toy trace, LDE,
  commitments, and transcript. It does not close the STARK: the current FRI
  checkpoint still starts from four selected main columns rather than the full
  AIR composition codeword. The next semantic slice derives the composition
  challenge from the final AIR transcript, evaluates and commits all production
  constraints over the LDE, and feeds that composition codeword into FRI.

  The 2026-08-30 composition checkpoint implements that slice.
  The field-generic AIR core preserves one 708-constraint denotation while
  exposing nominal fold checkpoints after the all-row, pair-row, first-row,
  and last-row families. The WebGPU placement serializes those exact
  checkpoints into three derived scratch regions, rather than selecting a
  constraint family through an application opcode or copying its formula.
  Eleven Fe-authored passes derive the composition challenge, fold constraints
  `0 -> 653`, `653 -> 690`, `690 -> 702`, and `702 -> 708`, project all sixteen
  quartic evaluations, commit their ordered tree, and bind the root into the
  production transcript. The existing complete FRI schedule now consumes that
  composition codeword instead of four diagnostic trace columns.

  An entry-rooted zero-import Fe-Wasm gate checks all sixteen coset evaluations
  against the existing independent BN254 bigint composition model, plus a
  changed challenge, changed claim, and three fail-closed invalid inputs. The
  original focused run passed 1/1 in 202.67 seconds. A fresh release rerun after
  removing the exploratory value-observation path passed the same matrix in
  590.64 seconds.

  An earlier intermediate artifact had reported materialized-row parity code
  `18` at coset point zero. That result does not reproduce in the current
  compact entry. Two fresh compilations emitted byte-identical 564,183-byte
  Wasm modules with SHA-256
  `838a5d3994456f24c14044e158d5283aefeb698c5ba7cd7f07c65fbfa898838c`.
  The exact module returned parity code zero immediately in Bun and passed all
  sixteen rows under default optimized Wasmtime in 602.15 seconds. A separate
  Fe regression materializes 411 branded 20-word values, dynamically selects
  the first, middle, and last elements, subtracts every direct/materialized
  pair, and also returns parity zero. The observation-only export and IR dump
  hooks were removed. Because no current artifact reproduces code `18`, this
  closes the present semantic gate without claiming an invented backend fix;
  the earlier intermediate result remains stale checkpoint evidence rather
  than a diagnosed current defect.

  The complete browser-profile graph now contains 48 compute passes plus one
  display pass, six physical resources, a 2,874-word derived scratch tape, and
  no pass with more than eight storage bindings. Every stage lowered to
  Naga-valid WGSL in 2,076.40 seconds; the complete release gate passed in
  2,081.25 seconds. Full-graph lowering times for the new suffix were 431.96
  seconds for pair rows, 162.19 for first row, 78.35 for last row, and 61.47
  for projection. Isolated lowering of those same four stages took 366.87
  seconds total, so the full-graph timing delta remains compiler-cost evidence
  to investigate. Native execution skipped explicitly because this sandbox
  exposes no adapter. Browser value parity, mutation/recovery, and a physical
  hardware receipt for this composition graph remain open.

  The 2026-08-31 standalone acceptance run closes the software-browser part
  of that composition checkpoint. Tightened const-predicate discharge first
  exposed four generic proof-tape wrappers that forwarded only the nonzero
  storage premise and omitted the schema-fit premise required by
  `region_layout`. The reusable ingot now exposes one typed
  `storage_word_capacity<N>() -> u32` const function, and both the underlying
  accessors and wrappers state the identical
  `Space::WORDS <= storage_word_capacity<N>()` obligation. A focused HIR gate
  proves the real associated-const plus const-generic forwarding shape
  discharges by exact assumption evidence. No runtime check, raw capacity, or
  demo-specific compiler rule was added.

  A fresh immutable publication then emitted 48 compute passes plus one
  display pass, 19,687,731 WGSL bytes, 2,182 Wasm bytes, six typed resources,
  a 3,295-word proof tape, and 52 assets. Source and dependency diagnostics
  completed in 34.98 seconds, lowering in 782.00 seconds, and atomic site
  publication in 818.12 seconds. Per-unit measurements identify the current
  compiler hotspots precisely: pair rows took 153.56 seconds, local-step
  constraints 65.94 seconds, orbit coordinates 50.48 seconds, first-row
  constraints 42.88 seconds, the two quotient families 38.26 and 37.43
  seconds, and last-row constraints 33.41 seconds. These measurements now
  replace the undifferentiated full-graph timing as the optimization baseline.

  Chromium 150 on SwiftShader mounted that exact immutable graph in 202.37
  seconds. Clean, Fe-authored mutation, and recovered executions took 16.84,
  8.94, and 7.41 seconds. Clean and recovery produced five exact blue bands;
  mutation changed only FRI and overall to exact pink. Recovery reproduced all
  3,295 proof words byte-for-byte, with no console, mount, validation, or
  device-loss error. The captured browser evidence file has SHA-256
  `df4805fa560d6dcdc1bb88cc7b9cd54cd6e691d9866dcb45788271a75404b5e0`.
  The independent Rust, direct-DFT, and Plonky3 oracle then matched all 6,848
  AIR LDE words, production commitments and transcript, every composition
  value and scratch checkpoint, authenticated FRI query evaluation and sibling,
  mutation behavior, and clean recovery. This closes software-browser semantic
  exactness for the current authenticated toy checkpoint. It is not the
  114-query production WebGPU policy or a recursive parent proof.

  Commit `b543e5f0d` makes the first production-policy WebGPU placement slice
  exact. The compact
  `FriQueryRangePlan<1, 114>` and thirteen-round `FriSchedule<1, 13>` jointly
  derive 25 evaluation openings and 132 authentication siblings per query,
  for 2,850 and 15,048 work items. `fri_structure` interprets those two
  denotations into indexable Cartesian work items and derives fixed dispatch
  counts from a caller-selected portable tile. Query identities are read
  through the plan interpreter, not reconstructed from aggregate metadata, so
  a future nonconsecutive plan can retain its transcript meaning. A focused Fe
  actor uses 64 lanes across 45 evaluation workgroups and 236 sibling
  workgroups. Its 86 padded invocations all resolve to invalid work items. The
  release gate validates 5,848 bytes of browser-profile WGSL, executes both
  passes on llvmpipe, and matches every one of the 17,898 semantic receipts plus
  every padded lane against an independently expanded Rust product space. On a
  warmed filesystem, focused Fe bundle compilation took 12.42 seconds and both
  dispatches plus three readbacks took 126.42 milliseconds. This closes the
  production query-grid topology and portable placement seam. It does not yet
  sample the 114 runtime query indices, materialize their actual openings, or
  authenticate them inside the production receipt.

  Commit `011593abf` now closes the first of those three omissions. A reusable
  `poseidon_baby_bear_webgpu` interpreter gives each invocation one field lane
  and derives an 89-dispatch indexed digest squeeze directly from the shared
  Poseidon2 workgroup schedule. The production query actor runs 114 independent
  `FQ02` squeezes across 29 workgroups, masks the resulting extension-field
  coefficients into the 4,096-position half-domain, and keeps every phase,
  round, identity, workspace offset, and dispatch count in Fe. A compact
  state-count-derived workspace reduces the physical actor contract to five
  storage resources plus the compiler's fail-closed trap channel, within the
  baseline WebGPU limit of eight storage bindings.

  The focused release gate validates 503,482 bytes across the sampler,
  evaluation, and sibling WGSL passes. On warmed llvmpipe it compiled the Fe
  bundle in 19.40 seconds and executed all three passes plus readbacks in 13.73
  milliseconds. All 114 sampled indices match independent Plonky3 Poseidon2
  squeezes, all 2,850 evaluation and 15,048 sibling placements match the
  independent product-space oracle, all 86 placement padding lanes and 32
  sampler padding lanes are exact, all query states finish 89 steps valid, and
  all 1,856 compiler trap lanes remain zero. This is query sampling and work
  placement, not authenticated opening extraction: the sampled indices must
  next select production codeword values and Merkle siblings, and those words
  must be bound into the receipt.

  This slice also exposed two compiler-hardening seams. Instantiating a generic
  foreign `RegionSchema` through the const-sized sampler helper produced a
  Salsa dependency cycle, so the reusable ingot currently owns its exact
  state-count-derived packed workspace. A mutable `bool` reduction across the
  wide lane-validation loop lowered incorrectly on the GPU path; retaining the
  storage-native word status through the loop and projecting to `bool` only at
  the boundary is exact. Both deserve focused compiler regressions before the
  workspace API is generalized further.

  The externally hosted Chrome 149 endpoint was live but could not supply a
  hardware adapter for this run. DevTools reported the AMD Radeon 780M and
  WebGPU feature as configured, while `chrome://gpu` reported hardware
  acceleration disabled after two GPU-process crashes. Both the allowlisted
  `http://10.0.0.2:8000` origin and a secure intercepted localhost origin
  returned `requestAdapter() == null` before any proof shader was requested.
  The 2026-08-31 rerun returned the same adapter-null result before loading the
  newly authenticated graph.
  Therefore this checkpoint is browser-runtime and independent-value evidence
  on SwiftShader, not the required physical-GPU receipt. Restart the external
  browser GPU process, confirm hardware acceleration, and rerun the same
  focused lab before claiming physical-adapter parity.
  Larger production domains, fused shared-memory or subgroup placements, and
  real Chrome hardware parity for this staged path remain open, so G-NTT stays
  partial.
- [~] **G-LAYOUT:** compiler/FCO-derived typed regions replace application
  proof-tape offsets, and an independent decoder checks every region and width.
  Commit `143e0b799` establishes the reusable `region_layout` ingot and the
  provider-privacy compiler floor it needs. An ordinary Fe record of
  `Region<T>` fields now derives declaration-order offsets and total canonical
  width from `CanonicalWords`; its physical coordinates remain private and its
  checked relative accessors fail closed. Release gates prove the derived
  layout against an independent Wasm decoder, reject direct field and private-
  constructor forging plus cross-payload confusion, validate emitted
  browser-profile WGSL, and execute typed stores on llvmpipe without allowing
  an out-of-region write to reach the next region. The production WebGPU toy
  tape now uses one derived `ProofTapeRegions` schema, including a nested
  `FriChallengeRegions` schema, and proves its total width at compile time.
  The full security receipt still needs migration plus an independent
  receipt-wide decoder before this gate can close.

  The parallel `quilting-webgpu-api` worktree provides a relevant upstream
  capability track, but is not yet MB2 evidence. Commits `f9abe8d8d` and
  `1b24aeb37` establish its capability architecture and Fe-derived portable
  storage layouts. Its mixed `f32 / u32 / f32` layout derives offsets
  `0 / 4 / 8`, rejects unsupported layouts, and has reached release compiler,
  browser-manifest, Sonatina, WGSL, and Naga validation. Actual adapter-backed
  execution remains open, as does the active mixed-storage compiler slice.
  MB2 should integrate the resulting typed resource and layout machinery in
  narrow, exactness-gated slices after that work lands, while composing it
  with `RegionSchema` rather than replacing either abstraction wholesale.
- [x] **G-RECEIPT:** the complete nonrecursive BabyBear receipt accepts cleanly
  and rejects claim, domain, transcript, query, path, and encoding mutations.
  The first four-query process-isolated exact gate compiled a 23,636,877-byte
  zero-import Fe prover, executed that exact persisted Wasm in a fresh process,
  and emitted a 47,552-byte canonical receipt. It then compiled a fresh
  10,129,225-byte zero-import Fe verifier and executed that artifact in a fourth
  process over only the copied receipt bytes. The clean receipt accepted. A
  test-only Fe structural interpreter then mutates the typed decoded carrier,
  without host-owned offsets: the base root and transcript chain, an
  authenticated AIR value, its Merkle sibling, a composition opening, and the
  recursively located terminal FRI evaluation all reject. A raw canonical
  validity-word mutation also rejects. The complete gate passes 1/1 in
  1,891.64 seconds; prover execution takes 258.37 seconds after its compiler
  arena exits, and verifier execution takes 10.74 seconds after its compiler
  arena exits. Claim, domain, sampled-query, and malformed-encoding coverage
  remains independently exercised by the focused layer gates.

  Fe commit `9c0d8306e` makes policy-sized Merkle opening storage an ordinary
  typed browser-arena placement of the same ordered dependency graph. The
  separate CTFE-derived 100-bit policy now crosses the assembled boundary with
  114 authenticated queries. A fresh 15,397,939-byte zero-import
  Fe prover compiled in 2,109.22 seconds, executed in a separate process in
  539.32 seconds, and emitted a canonical 948,808-byte receipt. Its Wasm and
  receipt SHA-256 digests are respectively
  `90dc23f002dac6b80ea595dc1247c90c737ba853e1b89baa216504239d71da06`
  and
  `c789a067f63b4ab73d8a4c0b36932e4252b6270b0be3e17cc5d5c27980be3ceb`.
  A separately compiled 14,636,637-byte zero-import Fe verifier with SHA-256
  `7c3c0842615854dccda6a69d6a6afc2a0028e1a823283d0f6abb80465e821423`
  accepted only the copied receipt bytes and rejected mutations to the base
  root, authenticated AIR value, Merkle sibling, composition opening, and
  terminal FRI evaluation in 21.05 seconds. The prover compiler peaked at
  13,288,420 KiB RSS and the prover runtime at 1,740,228 KiB. This closes the
  scalar assembled-receipt gate. It does not constitute recursive
  cryptography, WebGPU proof generation, or evidence that this one-iteration
  verifier is cheaper than direct orbit replay.
- [~] **G-RECURSE:** the field-neutral multi-limb carrier already merges only
  ordered adjacent iteration intervals with identical statements and shared
  boundary commitments. `VerifiedRecursiveInterval<L>` now makes the
  proof-backed layer nominally unforgeable: its carrier is private, and only
  the production sparse receipt verifier or an ordered merge of two verified
  authorities can mint a valid value. A negative Fe gate rejects direct
  construction. A process-isolated exact gate against the committed dependency
  snapshot generated a 46,656-byte canonical production receipt, admitted its
  exact `0 -> 1` interval, and rejected a mutated validity word. This is an
  in-memory trusted relation, not recursive cryptography. A second exact gate
  uses one leaf-indexed Fe prover to emit real `0 -> 1` and `1 -> 2` receipts of
  46,656 and 46,808 bytes. A fresh Fe verifier admits both, mints their private
  authorities, and merges exactly two leaves over `0 -> 2`; duplicate-left,
  swapped-order, and mutated-right inputs all fail closed. The fixed-size
  recursive proof/provider and encoding, logarithmic aggregation evidence, and
  bounded disk claim remain.

  Fe commit `23a51230d` binds the full 114-query security receipt to that
  private leaf authority. A fresh 14,640,593-byte verifier compiled in 620.28
  seconds with SHA-256
  `6f2942a62f03da72d10f9580710b2ba40ecdc6360d135e8a6610a541939a5f2e`,
  admitted the retained 948,808-byte production receipt, and rejected the
  typed mutation matrix in 17.80 seconds. A compile-negative gate separately
  proves that the four-query protocol-shape receipt cannot satisfy this
  security-policy leaf boundary. This closes the authority handoff into the
  recursive layer. It is still not a cryptographic parent proof.

  Commits `0780b801c` and `9db9cb519` establish the first fixed-size parent
  constraint relation without copying its semantics into witness code.
  `quadratic_plan` now interprets one authored quadratic relation directly, as
  multiplication witnesses, or as ordered product and assertion residuals.
  The BabyBear recursive merge instantiates that machinery with 423 derived
  multiplication nodes and 583 assertions. Its 30-bit integer width derives
  from `MAX_RECURSIVE_CLAIM_BOUND`; bit and carry constraints prove strict
  child ordering and overflow-safe leaf-count addition without trusting
  native `u32` comparisons inside the constraint interpreter. The zero-import
  Wasm gate agrees with an independent integer model, rejects statement,
  boundary, adjacency, parent projection, validity, witness-node, range-bit,
  and carry mutations, and rejects an explicit BabyBear modular-wrap attack
  through the final carry. This is the exact merge relation that a parent
  proof must authenticate. It does not yet authenticate either child verifier
  execution or emit a recursive proof receipt.

  Commit `630d6474e` makes the production BabyBear Poseidon2 S-box the first
  child-verifier arithmetic primitive to consume that shared relation
  vocabulary. One four-node `poseidon2_power7_plan` now denotes `x^7` for the
  scalar permutation, multiplication-witness generation, and quadratic
  residual evaluation. Its zero-import Wasm gate checks every intermediate
  product against independent `u64` modular arithmetic, rejects mutations to
  each of the four committed nodes and a wrong expected output, and retains the
  full Plonky3 parameter and permutation oracle. The generic quadratic-plan
  gate and the 423-product recursive merge gate remain green after making the
  relation interpreter an extension of the ordinary multiplication-plan
  interpreter. This authenticates one reusable nonlinear primitive, not a
  complete Poseidon round or child-verifier trace.

  Commit `1798fa6fa` extends that denotation through the complete width-16,
  21-round permutation. Production scalar hashing now consumes one sequential
  `poseidon2_permutation_execution`, and the public relation plan interprets
  the same 564 multiplication nodes as witnesses or quadratic residuals before
  asserting all 16 output lanes. The release Wasm gate rejects mutations to
  every one of the 564 committed nodes and every expected output lane while
  retaining full Plonky3 permutation parity. The actual Mandelbrot proof GPU
  bundle also remains green: its largest pass is the staged
  `advance_commitment_rounds` shader at 156,793 bytes, below the existing
  160-KiB browser-risk ceiling. The staged workgroup schedule remains an
  independently exact placement of the same Poseidon algorithm, not yet a
  placement interpreter derived from this sequential relation plan. Child
  verifier task ordering, transcript wiring, and trace authentication remain.

  Commits `e3dfe054a` and `6079e80cd` carry that relation through the actual
  two-to-one Merkle operation and its first scalable runtime placement.
  Production `compress` now consumes `compress_plan`, so concatenating two
  typed eight-field digests, executing all 564 permutation products, and
  selecting the parent digest are one authored Fe dependency plan. The new
  `QuadraticRelationStreamInterpreter` writes uniform
  `(left, right, output)` product rows and zero-assertion rows directly into a
  typed browser arena. Only its opaque handle and two cursors remain live in
  Wasm, and allocation or incomplete shape failures reject without reading
  uninitialized rows. The zero-import release gate compares all 564 streamed
  outputs with the existing fixed witness interpretation, agrees with
  independent Plonky3 compression values, rejects a mutation to every product
  output, and rejects each of the eight parent-output assertions. The complete
  five-test Poseidon gate passes 5/5 in 84.25 seconds; the hardened focused
  rerun passes 1/1 in 25.98 seconds. This closes lossless memory placement of
  the arithmetic relation only. The rows do not yet authenticate operand
  provenance or program topology. Named Poseidon task rows and typed copy
  interactions must bind round, lane, power step, inputs, constants, prior
  products, and outputs before a recursive proof can trust the stream.

  Commit `47bb98142` derives the first complete production child-verifier task
  plan from the 114-query security policy. One payload-bearing Fe enum names
  six shared verifier stages followed by `Query(plan_position)`, giving 120
  tasks without a generated query table or host-selected opcode. One ordinary
  verifier denotation is interpreted as the scalar boolean result, a fixed-
  shape typed-memory trace, or replay against untrusted stored rows. Scalar
  evaluation short-circuits after rejection, while trace and replay retain the
  fixed suffix needed by a recursive execution relation. The portable storage
  projection remains nominally typed as task kind plus query position because
  payload-enum aggregate loads are not yet supported by Wasm lowering; no
  numeric production opcode table was introduced. The focused zero-import
  release oracle checks all 120 task identities, invalid indices, clean replay,
  coherent task rewiring, changed stored results, and changed query payloads.
  The separately compiled production verifier still accepts the retained
  canonical 948,808-byte receipt and rejects its complete typed mutation and
  malformed-byte matrix. This is a verifier execution placement contract, not
  an authenticated child trace or recursive proof receipt. Exact trace
  execution over both real security-policy child receipts remains next.

  Commit `e13fdaf5d` executes that trace contract over the retained exact
  `0 -> 1` security-policy receipt. A Fe trace-writer placement compiled to a
  15,082,010-byte zero-import Wasm artifact in 918.63 seconds, then verified
  the 948,808-byte canonical receipt and emitted a 972-byte canonical trace in
  15.08 seconds. The trace contains a versioned header followed by exactly 120
  derived task positions and results. Its SHA-256 digest is
  `537df3ee19012a933816b92688f0c648fe2519cee1b1dadcbd442d502865d865`;
  the writer artifact digest is
  `d7b46e6167c3442e69f0df562b9fbb9fb1197f8839a5884ce4f369cc97e7fdd2`.
  A separately compiled 15,078,618-byte zero-import replay artifact with digest
  `ec5b4f362d4080205fb671e52d12db4df0498fda4084afbd6f316cb032f3bd8b`
  compiled in 550.62 seconds. It accepted the exact receipt and trace with
  `(shape_valid, task_mismatches, result_mismatches, rejected_tasks) =
  (1, 0, 0, 0)` and rejected changed versions, invalid header and row boolean
  tags, changed task count, invalid task positions, and truncated or trailing
  receipt and trace bytes. Coherent task rewiring reports exactly one task
  mismatch, and a changed stored result reports exactly one result mismatch.
  The expanded replay matrix completed in 80.15 seconds. The independent Rust
  decoder also checked every emitted task position and result. Combined
  writer-plus-replay artifacts had previously approached the sandbox memory
  ceiling; the canonical trace boundary now lets each interpretation compile
  and execute independently without changing the Fe verifier denotation. This
  is the first exact child-verifier execution trace, not yet an authenticated
  parent proof. Producing and tracing the adjacent `1 -> 2` security-policy
  receipt, then binding both traces into the recursive relation, is next.

  The uncommitted 2026-08-27 adjacent-child slice now closes that exact
  execution boundary. A statically direct `1 -> 2` Fe root compiled the same
  production prover body into a 16,141,944-byte zero-import Wasm artifact in
  1,568.49 seconds. Its runtime graph contained 20,306 functions, only one
  fewer than the earlier value-selected root, disproving the hypothesis that
  the leaf selector caused the policy-sized graph. Incremental prepared-body
  consumption released all 20,306 lowered bodies before backend emission; the
  highest sampled compiler RSS was 15,424,376 KiB with 3,965,256 KiB still
  available. The artifact digest is
  `68cc59be1de73300745840f583da53d98ab386fa7d111ba6b38aa244c7ee1eea`.
  Executing that persisted Fe-Wasm in a fresh process took 216.19 seconds and
  emitted the 953,560-byte canonical `1 -> 2` receipt with digest
  `035bdb10e47d2b85ee1b7756b7e633e0fd6b18aee148baef9212bd0843878323`.

  One shared Fe trace writer then compiled in 341.56 seconds to 15,089,907
  bytes with digest
  `763b241a59490d08ff5c0a8fb8467db43b217196dcb02a38d38e188c44635807`.
  It accepted the retained exact `0 -> 1` receipt and the new exact `1 -> 2`
  receipt in one fresh process, emitting one 972-byte, 120-task trace for each
  child in 26.25 seconds. Their identical digest,
  `537df3ee19012a933816b92688f0c648fe2519cee1b1dadcbd442d502865d865`,
  is expected for the zero fixed-point: task identities and all acceptance
  booleans agree even though the receipts and public intervals differ. A
  separately compiled 15,086,539-byte replay artifact with digest
  `c13303e8cad6856a586310df64af39770915e41973dde752e4e214781c048079`
  completed the clean, malformed, mutation, out-of-range, right-child, and
  cross-leaf matrix in 57.67 seconds. In particular, the right receipt and
  trace cannot replay as the left public interval. This proves exact execution
  of both child verifiers through one Fe-authored task denotation. It still
  does not authenticate the verifier stage internals or emit a cryptographic
  parent receipt; those relations are the next recursive slice.

  Commit `c14fcfc66` begins that authentication slice with the exact
  receipt-header relation. One Fe-authored eight-product, nine-assertion plan
  is interpreted as direct evaluation, multiplication-witness generation, or
  quadratic residual replay. Its production adapter selects the canonical
  first verifier-trace row and projects the live receipt, both typed root
  validity flags, acceptance result, trace shape, and nonzero evaluation shift
  into the relation. The focused zero-import Wasm gate passes 1/1 in 3.19
  seconds, rejects each of the six false input cases, and rejects a mutation to
  every committed product node. This proves header consistency only. It does
  not yet recompute Merkle openings, transcript challenges, FRI folds, or AIR
  query constraints, and it does not emit a parent receipt.

  That focused gate currently requires the local Sonatina proof worktree. A
  clean detached probe against the repository-pinned Sonatina revision fails
  before reaching the test because `mb2` imports the newer
  `SpirvBuiltinArgument` API while the pinned revision exposes only
  `SpirvBuiltinInput`. Restoring a published, reproducible Sonatina pin is now
  a prerequisite for counting the recursive gate as clean-clone evidence.

  Commit `db7bfbe09` separates Fe runtime-package lowering from Wasm backend
  emission through one owned `PreparedWasmEmission` checkpoint. The focused
  release gate drops the compiler database before staged emission, validates
  and executes both direct and staged artifacts, obtains the same value `22`,
  and requires byte-identical Wasm. The staged boundary is where the exact
  adjacent-child compiler can release Salsa state and return allocator pages
  before constructing the backend graph. Prepared-body consumption inside
  lowering and the unpublished Sonatina lineage remain separate prerequisites
  for reproducing the policy-sized compile.

  Commit `16d99d872` adds the first cryptographic internal relation used by
  both base and interaction opening verification. One ordered Merkle-node plan
  constrains its direction bit, algebraically selects left and right children
  without a host branch, executes the existing 564-product production
  Poseidon2 denotation, and constrains all eight parent fields. The resulting
  relation has 573 products and nine assertions. Its zero-import Wasm gate
  agrees with independent Plonky3 compression in both orientations, rejects
  every wrong parent lane, rejects a non-boolean direction, and rejects a
  mutation to every product node in both orientations. The complete two-test
  recursive-verifier target passes 2/2 in 22.90 seconds. This authenticates one
  ordered hash dependency. Leaf commitments, canonical multipath topology,
  and complete base or interaction roots remain to be constrained.

  Commit `a5fc8b393` composes that same ordered node plan into a const-generic
  bottom-up binary path relation. Every direction remains a constrained field
  witness, their little-endian linear reconstruction must equal the public path
  index, and only the final eight-field root is asserted. The four-level gate
  therefore derives 2,292 products and 13 assertions without copying Poseidon
  or introducing a second path evaluator. It agrees with independent Plonky3
  roots for all 16 path indices, rejects each wrong root lane, rejects a path
  index that disagrees with the supplied direction bits, and rejects sampled
  product mutations in every chained node. The complete target passes 3/3 in
  31.49 seconds. Production base and interaction openings use deduplicated
  multipaths, so their canonical fixed-capacity topology and leaf commitments
  remain the next relation layer rather than being approximated by independent
  binary paths.

  Commits `7279e5140` and `ba29192a4` close the canonical production leaf
  relation. The scalar base and interaction LDE commitments and the recursive
  relation now consume one generic `Poseidon2CanonicalCommitmentPlan`, rather
  than separately implementing the sponge. Production shape words derive from
  the receipt types. The base leaf derives 34 Poseidon2 permutations, 19,176
  multiplication rows, and eight output assertions; the interaction leaf
  derives 21 permutations, 11,844 multiplication rows, and eight assertions.
  The zero-import Wasm gate agrees with independent Plonky3 commitments,
  rejects every commitment lane and changed leaf indices, and detects first
  and last product mutations in every sponge block. It passes 1/1 in 63.77
  seconds. A fixed aggregate witness placement previously trapped in Wasm
  allocation at this size; interpreting the identical relation into the typed
  row stream removes that physical local aggregate without changing its
  semantics. The pre-existing chained Merkle relation still passes 1/1 in
  57.65 seconds against the enlarged fixture. Canonical production multipath
  topology is now the next authentication layer.

  Commit `381bad931` makes that topology one reusable Fe interpretation rather
  than another verifier-specific loop. `merkle_core::interpret_multi_root`
  emits each canonical deduplicated hash as a typed task carrying its level,
  node index, opened-pair versus authentication-sibling source, sibling
  position, and direction. The existing scalar `multi_root` now consumes the
  value interpreter over that same traversal, while the recursive verifier
  consumes a quadratic Poseidon2 interpreter. A bounded relation stream keeps
  compile-time capacities but records the exact value-dependent row counts;
  replay must consume precisely those counts through the same authored plan.
  Four independent 16-leaf cases cover adjacent leaves, duplicate requests,
  separated subtrees, and both directions. Their roots, normalized leaf
  counts, sibling counts, and hash-task counts agree with an independent
  Plonky3 model. Structural input mutations and first and last product
  mutations in every emitted hash task reject. The focused zero-import Wasm
  gate passes 1/1 in 46.38 seconds. The generic Merkle regression remains
  green 1/1 in 1.93 seconds and the fixed quadratic-plan regression remains
  green 1/1 in 5.34 seconds.

  The 114-query policy derives a maximum of 456 opened leaves across depth 13,
  hence 5,928 hash tasks, 3,396,744 quadratic products, and 5,937 assertions;
  the gate executes those derived capacities from Fe rather than restating
  them in application code. A first attempt to pass that complete production
  carrier by value exposed a compiler dependency-cycle panic and was removed.
  The next slice is a role-branded typed-arena adapter that links the real base
  and interaction leaf relation outputs to this schedule without copying the
  456-leaf aggregate through a function boundary. Exact retained-receipt
  replay and mutation evidence remain required before multipath authentication
  is complete.

  Commit `be4959fb8` separates that canonical traversal from its physical
  scratch placement without introducing another topology program.
  `MerkleMultiPathReductionStorage` is the single typed interpretation
  boundary: the ordinary value path supplies local double buffers, while
  `merkle_browser` supplies a role-branded Fe memory handle whose address is
  never exposed to the traversal. Direct quadratic evaluation uses the local
  placement; bounded witness streaming and constraint replay use the browser
  placement over borrowed leaf and path carriers. All three therefore consume
  the same request validation, sibling order, hash tasks, and final-root
  checks. The generic zero-import Wasm gate compares both placements with an
  independent Rust root, sibling-count, and exact hash-task model across five
  request shapes and eight malformed-input mutations; it passes 1/1 in 2.16
  seconds. The complete recursive-verifier AIR target passes 5/5 in 67.22
  seconds, retaining the independent Plonky3 value and product-mutation gates.
  This proves the storage interpretation on bounded 16-leaf cases. It does
  not yet instantiate the 456-leaf production carrier or authenticate the
  retained security receipt. The production arena slice below gives the base
  and interaction roles their own typed commitment arenas, fills those arenas
  from the already-gated canonical leaf plans, and consumes them with borrowed
  paths through this browser-backed traversal.

  Commit `982e5e6b6` crosses that production representation boundary. Base and
  interaction commitments occupy distinct nominal Fe arena targets even
  though both physically contain 456 Poseidon2 digests. One runtime loop per
  role consumes the existing canonical leaf commitment plan, retains only the
  current leaf and relation interpreter in Wasm locals, writes the resulting
  digest into Fe-owned linear memory, and lends that role-specific storage to
  the shared browser-backed multipath traversal. The opening and path remain
  borrowed throughout. No 456-digest value, 5,928-sibling path, or complete
  opening crosses a function boundary by value.

  A zero-import production-shape gate instantiates the real 456-leaf and
  5,928-sibling receipt carriers for both roles, while executing a one-leaf
  canonical path through the actual depth-13 tree. Independent Plonky3 code
  derives the complete base and interaction leaf commitments and all thirteen
  parent hashes. Both roots agree exactly. Twelve base mutations and fourteen
  interaction mutations cover changed roots, opened values, siblings, unused
  values, opening and path validity, leaf indices and counts, sibling counts
  and unused siblings, plus the interaction base-root validity and value. All
  reject. The derived worst-case capacities are 12,141,000 base products,
  8,797,608 interaction products, and 5,938 assertions. The complete
  recursive-verifier AIR target passes 6/6 serially in 299.05 seconds,
  retaining the header, ordered-node, binary-path, canonical multipath, and
  leaf mutation gates. This proves the production storage shape and its
  role-preserving composition. At that checkpoint it did not yet replay the
  retained 948,808-byte security receipt, execute a maximum-count relation
  stream, or emit a cryptographic parent proof. The exact retained-receipt
  replay is recorded below.

  Commit `f2f247333` closes the exact retained-receipt semantic replay without
  baking the megabyte-scale proof into source. The process-isolated gate
  requires an explicit `MB2_PRODUCTION_SECURITY_RECEIPT` path, checks the
  canonical 948,808-byte vector against SHA-256
  `c789a067f63b4ab73d8a4c0b36932e4252b6270b0be3e17cc5d5c27980be3ceb`,
  and is ignored by ordinary CI when that external evidence is unavailable.
  Fe alone decodes the complete policy-sized carrier, applies typed mutations,
  derives all base and interaction leaf commitments, and runs both canonical
  multipath relations through their distinct production arenas. An independent
  Rust prefix decoder reads the canonical receipt widths rather than Fe memory
  offsets and independently derives the two sorted leaf sets, sibling counts,
  and hash-task schedules. The real receipt contains 452 base leaves and 452
  interaction leaves; each depth-13 opening executes exactly 1,585 ordered
  hashes, and Fe agrees with both independent schedules.

  A fresh zero-import Fe-Wasm artifact compiled to 2,801,164 bytes with
  SHA-256
  `8a4b3255783a317e0f11add7e488af8c350ac3ed774240e499030d7c46faba6c`
  in 52.39 seconds. Clean replay, twelve post-decode typed mutations across
  roots, opened values, siblings, interaction base-root binding, role-confused
  root values, validity, and sibling count, plus truncation and trailing-byte
  rejection, completed in 14.41 seconds. All mutations reject. This closes
  production base and interaction opening semantics over the retained exact
  receipt. It does not materialize the corresponding 12-million-row maximum
  quadratic stream, authenticate AIR or FRI internals, or emit a cryptographic
  parent proof. The security transcript relation is recorded below.

  Commit `7b062287d` authenticates the complete production
  `SecurityTranscript` task through one interpreter-parametric Fe plan. The
  same authored denotation now drives direct values, multiplication witnesses,
  streamed relation rows, and replay constraints for the canonical interval
  commitment under `AS01`, ordered roots under `AT01`, AIR transcript under
  `AT02`, injectively packed 44-word security profile under `SP01`, and final
  profile extension under `SP02`. Scalar Poseidon entry points are wrappers
  over that plan rather than a parallel hashing implementation. Invalid
  statements, roots, opening results, or task traces still execute the exact
  fixed relation shape and reject through residuals instead of truncating the
  witness. The derived shape is 21 Poseidon2 permutations, 11,844
  multiplication rows, and 24 assertions. Its production AIR family counts
  are independently pinned as `[312, 352, 16, 11]`.

  The zero-import Fe-Wasm gate separately derives the exact Q16 proof-security
  arithmetic, canonical 44-word profile, 32-to-30-bit injective packing, and
  all five Plonky3 Poseidon2 domain transitions in Rust. Three seed families
  match every final digest lane. A changed public statement remains a valid
  different claim and changes the transcript. Twelve semantic mutations cover
  interval shape and boundary order, root validity and embedded-base binding,
  both opening results, the transcript task result, and trace shape. First,
  middle, and final product mutations plus a stored assertion mutation all
  survive local storage but fail relation replay. The focused gate passes 1/1
  in 100.89 seconds; the affected Poseidon, proof-security, and complete
  recursive-verifier suites pass 7/7 in 60.80 seconds, 4/4 in 45.82 seconds,
  and 6/6 in 360.98 seconds. The adapter can project the production receipt
  roots and compiler-derived verifier rows into this relation, but the exact
  retained receipt has not yet executed through that combined adapter. AIR,
  composition, FRI internals, the adjacent child trace, and the cryptographic
  parent receipt remain open.

  The 2026-08-31 joined adapter now executes that combined boundary over the
  retained 948,808-byte production receipt and its exact 972-byte verifier
  trace. Fe decodes both canonical byte streams, independently supplied
  Plonky3 statement and boundary digests enter as public inputs, and one
  `QuadraticRelationValueInterpreter` flows through the receipt header, base
  opening, interaction opening, and security transcript plans. The resulting
  zero-import Wasm module is 4,297,924 bytes with SHA-256
  `ca40908acb5ff01ebbda50bca0ce5e7287c0101cbbb68ba7e55e7a24eb37034f`.
  It compiled in 287.96 seconds, executed the joined relation and mutation
  matrix in 46.32 seconds, and completed its process-isolated exact gate in
  334.56 seconds. Twelve receipt mutations, four coherent task rewires across
  the joined rows, one changed stored result, receipt invalidation, and
  truncated receipt or trace input all reject. The canonical field-codec gate
  remains green independently.

  Reaching this gate exposed a compiler semantic defect rather than a proof
  mismatch. Wasm aggregate reification proved read-only use only for
  array-containing structs, so a mutable no-array provider reference could be
  flattened into a rootless value after Runtime MIR had correctly retained
  the borrow. Commit `808c7fb51` now requires the same exhaustive read-only-use
  proof for every reified aggregate reference. Its focused regression first
  establishes that an ordinary no-array provider reference is eligible, adds
  a write through that reference, and proves reification then rejects it.

  This checkpoint constrains only the first four shared verifier stages:
  receipt header, base opening, interaction opening, and security transcript.
  It does not yet authenticate the `FriAuthentication` or `AirRequestSet`
  stages, any of the 114 query rows, the adjacent child trace, or the merge
  relation. The accepted row booleans therefore remain witnesses awaiting
  their internal execution relations. No recursive cryptographic parent
  receipt is emitted or implied.

  The following adjacent-parent checkpoint now threads two of those joined
  child plans through the exact ordered recursive merge relation without
  resetting or copying the interpreter. The current release compiler emitted
  a fresh 16,096,150-byte zero-import Fe prover for the distinct `1 -> 2`
  security-policy leaf in 6,167.17 seconds. Its SHA-256 is
  `3cbd8c751178747663d6a75f8478413125038b7c814c115abde361ff528bec03`.
  Executing that persisted artifact took 419.09 seconds and reproduced the
  canonical 953,560-byte receipt with SHA-256
  `035bdb10e47d2b85ee1b7756b7e633e0fd6b18aee148baef9212bd0843878323`.
  Its 972-byte verifier trace is byte-identical to the left trace because all
  120 compiler-derived task identities and acceptance results agree for the
  zero fixed point; the prior adjacent trace gate independently established
  that the right receipt cannot replay as the left public interval.

  One 4,343,159-byte zero-import parent module with SHA-256
  `46af039318c03437d21a9c1bc58ac1b36438d17bd37aa7daad99258bb9d6aad8`
  compiled in 344.96 seconds. It decodes both distinct receipts and both
  canonical traces in Fe, accepts independently derived Plonky3 statement and
  boundary digests as public inputs, executes the first four verifier-stage
  relations for each child, then executes the sole authored 423-product merge
  relation through the same `QuadraticRelationValueInterpreter`. The exact
  clean, ten-mutation, and four-stream-truncation matrix passed in 50.43
  seconds. Mutations cover either receipt, either child task topology, the
  shared boundary, parent leaf count and end digest, child statement,
  adjacency, and an authenticated opening root.

  This is one partial parent execution relation over two real security-policy
  children, not a recursive cryptographic proof. Each child still lacks
  internal relations for `FriAuthentication`, `AirRequestSet`, and all 114
  query rows. Those 116 relations must join the existing child plan before its
  accepted trace suffix can be trusted, and a later proving layer must commit
  and authenticate the complete child and merge relation before emitting a
  recursive parent receipt.
- [ ] **G-BROWSER:** the Fe resident region picker proves through WebGPU and
  verifies through both Fe-Wasm and revm-Wasm with cancellation, backpressure,
  mutation, timing, and device-loss evidence. A click selects one private
  parameter and therefore a zero-radius public disk; optional dragging expands
  the public disk around that witness. The user selects one of two existential
  predicates: the hidden point's critical orbit survives through public bound
  `N`, or it escapes by `N`. The receipt binds the disk, bound, and predicate,
  while the point and orbit may remain private. Escape is a definitive
  non-membership certificate; survival is only a finite bounded-prefix claim
  and must not be relabeled as full membership or convergence. A later
  attracting-fixed-point mode may instead prove a rigorous invariant-
  neighborhood and contraction certificate. The fixed browser adapter reports
  raw pointer facts and realizes Fe-requested capture; the Fe actor owns the
  down/drag/up state machine, click-versus-drag threshold, center and radius
  calculation, preview, cancellation, predicate choice, and proof scheduling.

  The 2026-09-03 production round-interaction compiler checkpoint now lowers
  the exact Fe actor with no private byte heap. Typed shader locals, callable
  helper control flow, projected borrows, structured merges and latches,
  redundant phi and zero-initialization cleanup, native selector switches,
  Fe-derived padding defaults, and unused or forwarded private return-lane
  elimination reduce its compute WGSL from the measured 1,600,494-byte
  baseline to 91,675 bytes while the independent production oracle remains
  green. The final shader contains 1,751 Naga expressions and zero private
  heap bytes. Its focused release compile and browser-profile validation gate
  passed in 31.06 seconds against the current integrated worktree. One
  preceding 96,999-byte checkpoint created a pipeline on the external Radeon
  Chrome in about 46 seconds and completed the full surface dispatch/readback
  path in about 1.2 seconds. This is feasibility evidence, not a stable browser
  acceptance receipt. Before that failure, an isolated one-word control wrote
  and read back 42 in 565.7 ms with no validation, allocation, or device-loss
  error on the same adapter. A later full-surface launch lost Chrome's WebGPU
  instance before poster readback. The same long-lived browser process then
  failed to navigate a fresh page to an inert `health.html` within 120 seconds,
  before the control could request an adapter. Chrome's host log also reports
  that its Wayland platform is incompatible with the selected Vulkan path.
  CDP `SystemInfo.getInfo` confirms the live command line uses
  `--ozone-platform=wayland`, reports Vulkan and WebGPU Vulkan-through-GL
  interop enabled, and records four GPU-process crashes. The auto-running page
  is stopped. After relaunching Chrome under X11, the immutable focused probe
  must pass health, compile-only, one-workgroup, full-grid, and readback gates
  separately before this actor is counted browser-green. This stage is one
  production proof relation, not the complete nonrecursive or recursive
  receipt.
- [ ] **G-INSPECT:** the Fe SourceInspector presents authored, semantic,
  analysis, placement, ABI/layout, artifact, and evidence views from one
  content-addressed `SourceAtlas`, with no gallery `docs.json` or runtime render
  manifest.

## A. Shared precision axis

- [x] Generic `Fixed<L>` integer fixed-point and GPU-faithful projection.
  Gates: `precision_fixed_orbit_gpu_oracle.rs` and
  `precision_fixed_projection_oracle.rs`.
- [x] Loop-form BN254 field arithmetic and Poseidon parity across executable
  backends. Gates: `precision_field_bn254fr_oracle.rs`,
  `loop_form_bn254_poseidon_hash2_matches_circomlib_and_u256_form_on_wasm_at_o0_and_o2`,
  and `poseidon_bn254_loop_native_cranelift_leg_is_honestly_reported`.
- [x] Reusable field arithmetic now has an array-native `FieldWords<L>` core
  and modulus-branded `FieldElement<L, M>` values with `+`, `-`, unary `-`,
  `*`, `square`, `pow5`, and canonical Montgomery conversion. A separate
  `U32EmbeddingModulus<L>` promise prevents direct `u32`/`i32` embedding into
  moduli too small to represent every input. The structural HList API delegates
  to the same CIOS core. The ProbeP51 gate checks both APIs, all four arithmetic
  operations, `pow5`, signed and unsigned roundtrips, and zero canonicalization
  against an independent bigint model; the BN254 gate remains limb-identical
  to both established kernels.
- [!] The reusable `FieldElement` API is executed on Wasm but is not yet a GPU
  application API. The honest SPIR-V leg currently fails closed because the
  call-free shader lowering retains the array-returning private `mul_words`
  call. The proven generated Poseidon/Merkle kernels remain the GPU path.
  Closing this requires aggregate-return inlining or real SPIR-V function-call
  lowering, not consumer-side source expansion.
- [!] Type-level limb expansion is intentionally not the crypto scaling path.
  The loop-form field kernel is the answer unless a new measured gate disproves
  it.

## B. Runtime control and reactive spine

- [x] Typed suspend/resume CPS, exact live frames, generated Wasm re-entry,
  affine pending identity, cancellation, and the fixed browser task executor.
  Gates: `suspension_plan.rs`, `wasm_e2e.rs`, and
  `host-completion.test.mjs`.
- [x] Fe stream policy includes scan, bounded queues, merge, switch/latest,
  replay/share, race, throttle, debounce, and deterministic time. Gates:
  `wasm_e2e.rs`, `host-completion.test.mjs`, and
  `fe-render-runtime.test.mjs`.
- [x] A non-destructive `Select` loser can now be consumed explicitly through
  the backend-generic affine `PendingCancellation<B>` effect. The fixed broker
  aborts active browser work, consumes already-settled unclaimed tokens, and
  rejects stale or claimed tokens. The heterogeneous select capstone preserves
  its sink loser across two source wins, then Fe cancels it exactly once and
  leaves no broker slot. Gates:
  `fe_select_preserves_a_heterogeneous_loser_across_repeated_source_wins` and
  `typed pending cancellation consumes active and already-settled losers`.
- [x] Generated WebIDL scalar `Promise<T>` operations now emit
  `Pending<WasmBackend, T>` and execute on the same completion and continuation
  rail. The transport derives its success width from the canonical codec plan,
  adds no operation-specific JavaScript schema, preserves byte-identical sync
  binders, and does not require an allocator for scalar-only worlds. The full
  gate compiles the generated Fe declaration, retains the generated Wasm
  import, settles through the fixed broker, resumes the compiler-derived Fe
  continuation, checks the semantic value `42`, and observes no token leak.
  Gates: `scalar_promises_use_generated_pending_and_the_completion_rail`,
  `generated_scalar_promise_transport_executes_its_semantic_completion`, and
  `generated_webidl_scalar_promise_resumes_a_real_fe_task`.
- [~] Direct rich generated Promise results now use deferred canonical
  lowering. The broker retains the standards value opaquely across nested
  races and selects, allocates only after the final Fe continuation wins
  custody, invokes that exact continuation synchronously, and runs its
  generated post-return cleanup on success or trap. The compiler now wires
  Sonatina's checked LIFO `cabi_realloc` and operation-specific post-return
  exports instead of inventing a JavaScript allocator. A real generated
  `Promise<USVString>` reaches Fe UTF-8 logic and returns the allocator to its
  exact baseline. Settled losers remain allocation-free, and materialization
  fails closed when a direct or record-nested borrowed host descriptor would
  survive another suspension without first being copied into Fe-owned
  storage. Indirect canonical results remain blocked. Gates:
  `generated_webidl_string_promise_releases_after_the_real_fe_continuation`,
  `browser_task_rejects_borrowed_host_storage_across_a_second_suspension`, and
  the generated-result cases in `host-completion.test.mjs`.
- [x] Typed browser sources cover render-surface facts, visibility, animation
  frames, viewport, raw pointer events, Fe-selected capture, wheel, shared
  WebGPU lifecycle, queue-idle completion, and Fe-owned recovery. Gates:
  `native_e2e.rs`, `wasm_e2e.rs`, `bootstrap.test.mjs`, and the focused real
  Chromium SourceInspector/gallery tape.
- [~] Typed `MessagePort<u64>` observation is implemented through the ordinary
  `EventSource` and completion broker. The focused broker suite and
  `fe_message_port_event_source_resumes_from_a_real_port` pass, and the slice is
  landed at `c1817e477`. This item closes when the final G5 run passes.
- [x] Fetch is a generated typed Fe source consumed by SourceInspector without
  application-specific host policy. The generated
  boundary now models `[Global=Window]` without minting a fake Window handle,
  lowers URL-only `fetch`, `Response.text`, and `Response.arrayBuffer` through
  the existing `Pending<WasmBackend, T>` continuation rail, maps byte results
  to canonical `BrowserBytes`, and emits explicit generation-safe disposal for
  owned Response resources. The precompiler derives the selected adapter from
  actual imports and publishes one self-contained, content-addressed module
  containing the fixed runtime, fixed codec, generated semantic adapter, and
  generated transport. The bootstrap attaches it before import preflight, with
  no runtime adapter-selection JSON or caller-supplied environment object.
  Direct borrowed guest arguments and scalar or resource results no longer
  allocate codec scratch memory. Generated completion conversion now returns
  an ownership receipt: Response handles become Fe-owned only after the exact
  continuation returns successfully, while losing races, lowering failures,
  cancellation, and continuation traps roll them back. SourceInspector now
  runs one actor-scoped Fe resource loop. A whole-state compiler-derived task
  input gives it only its own Fe state, while a zero-payload notification edge
  reports that a new command is available. Fe owns request revisions,
  text/binary selection, switch-latest cancellation, HTTP classification,
  response limits, stale suppression, presentation, content copying, and
  explicit Response release. JavaScript owns only opaque Promise/resource
  custody, cancellation mechanics, continuation invocation, and generated
  canonical conversion. Component opcodes 12/13, `ComponentWriter.load_text`,
  `ComponentWriter.load_bytes`, `_loadResource`, and `_deliverResource` are
  deleted. Dependency-bearing resident components compile through their real
  initialized ingot, so `browser_fetch` is part of SourceInspector's compiler
  and watch inventory without adding a manifest. Gates:
  `global_fetch_uses_standards_authority_and_owned_response_resources`, the
  59-test `fe-webidl-bindgen` suite plus its four provenance tests, the
  13-test `fe-host-wasm-codec` suite,
  the 12-test host-runtime suite, the 36-test completion suite, the 16-test
  bootstrap suite, the eight Bun codec cases, the resident-actor contract
  suite, `source_inspector_actor.rs`, the complete 34-test HTML precompile
  suite, an immutable 3-module/12-render/80-asset gallery publication, and the
  real remote-Chrome SourceInspector/gallery tape. Switch-latest suppresses
  stale Fe delivery and rolls back unclaimed authority, but does not yet abort
  the browser's underlying request. Actual request abort waits for the general
  WebIDL/host-ABI representation of borrowed resources nested in
  `RequestInit.signal`; it must not be smuggled into a handwritten fetch case.
  This gate admits no permanent handwritten `fe:web-fetch` import whose
  JavaScript mirrors Fe field order or success-lane widths.
- [~] Compiler-derived rich values now cross typed structured-child mailboxes.
  Attach opaque ports through Fe-owned spawn/Worker placement and carry those
  canonical values through the general `MessagePort` source next.
- [~] Add structured child scopes, admission, supervision, and restart/backoff
  policy. The fixed Module Worker runtime no longer owns clocks, retry windows,
  backoff, or automatic recovery. Ordinary `std::runtime` Fe types and the
  pure `RestartWindow` reducer now own epoch advancement, rolling-window
  admission, restart/backoff selection, exhaustion, and parent cancellation.
  `ChildPlacement<B, C>` plus `supervise_child` now run the complete owning scope
  through the ordinary `Pending`/`Suspend`/`Timer` rail. Worker readiness now
  races the typed spawn pending value against an ordinary Fe timer through
  `Select`; Fe consumes the losing affine operation through
  `PendingCancellation`, classifies startup timeout, and spends the same typed
  restart budget as an immediate spawn failure. Independent Bun mechanics,
  compiled-Fe Wasmtime reducer, and compiled-Fe/Bun structured-scope gates
  pass.
  The compiler now recovers the nominal child type `C` from the parent runtime
  package, resolves the actor whose state has that type, selects its `Worker`
  behaviors, and compiles a distinct zero-import canonical child Wasm artifact.
  The precompiler publishes that child, its generated interface, and the fixed
  actor runtime beside the parent continuation package. Production bootstrap
  installs the capability only when the parent imports `fe:worker-scope`.
  There is no authored child ID, child URL, child manifest, behavior-name table,
  or application routing JavaScript. The reproducible Chromium gate
  `structured_worker.browser.mjs` loads the immutable parent, child, generated
  interface, and Module Worker host over HTTP without errors.
  DEC now uses this same path from a render actor. `DecSurface` owns two typed
  `ScopedTask` behaviors: one runs Fe supervision policy for the nominal
  `DecOperator` child, and one sends a typed `Cochain0 -> Cochain1` mailbox
  request and computes the response receipt in Fe. Render-bundle compilation
  derives those tasks only from the selected GPU actor, compiles the six Worker
  behaviors into a separate zero-import child, and emits one manifest-free
  task package. The fixed render runtime inspects only standard Wasm imports,
  supplies the generated Worker scope/mailbox capability, starts task machines
  from their compiler-derived input widths, and binds cancellation to the
  surface lifetime. Direct workspace builds now resolve local `core` and `std`
  as one nominal graph, eliminating the split `Handles` identity that had
  invited handwritten relation boilerplate. Gates: the four-test DEC operator
  suite; `direct_workspace_member_uses_workspace_core_and_std_as_one_graph`;
  the HTML publication, cache, and deployment-verifier tests; a real one-tile
  `fe web precompile`/`fe web verify` run with 18 files; and a Bun execution of
  those exact published parent, child, interface, mailbox, and completion
  artifacts whose Fe receipt was `1` with zero leaked tokens. Landed at
  `7d881bcc6`, `8817e726e`, and `838d1c01b`.
  Multiple nominal sibling scopes are now landed at `db088b0ea`. The Fe raw
  boundary retains `C` on spawn, failure, and close; MIR derives three opaque
  lifecycle import identities from each semantic child type; resident and
  render artifacts carry a deterministic collection of compiled children; and
  generated packages publish each child under its derived key without an actor
  name, selector, or manifest. The fixed host accepts the resulting typed
  capability collection but keeps every child on one affine completion-token
  rail. The semantic two-child gate performs requests through both child Wasm
  programs, proves `7 -> 14 -> 19 -> 26`, cancels both Fe supervisors, observes
  the correct close for each nominal scope, and finishes with zero live tokens.
  The 44-test fixed browser-runtime suite, ten-test resident-actor suite,
  four-test DEC oracle, and focused HTML publication gate pass.
  Recursively scalar canonical variants now cross those type-derived child
  mailboxes without a JavaScript operation schema. Canonical memory remains a
  tagged union, while the Fe Wasm value ABI remains the declaration-order tag
  plus every case payload lane. One compiler-owned wrapper validates each
  active tag, loads only active request fields, supplies exact zero values for
  inactive Fe lanes, clears reusable response storage, and stores only the
  active result. The generated parent mailbox codec derives the same lane order
  from the child interface and rejects noncanonical inactive values. The real
  two-child gate now sends nested `ScaleCommand` and `ScaleMode` variants,
  receives a `ScaleResult` variant, preserves the semantic
  `7 -> 14 -> 19 -> 26` result, proves inactive response-union bytes were
  scrubbed after arena reuse, and still finishes with zero live tokens.
  Variants containing bytes, strings, or lists remained fail-closed at this
  checkpoint. The 16-test canonical-interface suite,
  ten-test resident-actor suite, four-test DEC suite, 53-test fixed browser and
  codec suite, and focused scoped-task publication verifier pass. The canonical
  interface guide now documents nominal role selection, typed mailbox edges,
  private generated transport names, fixed resident exports, the remaining
  spelling-based render compatibility paths, and the possible future omission
  of semantically unnecessary behavior names.
  Descriptor-bearing structured-child values are now landed as the next
  canonical bridge. `BrowserString`, `BrowserBytes`, and bounded
  `BrowserList<T, N>` derive two Wasm lanes from nominal semantic metadata,
  requests are copied out of the parent Wasm memory into owned transferable
  values, and response sessions allocate through the parent's checked
  canonical stack. Generated post-return release runs only after the exact Fe
  continuation consumes the response. The Wasm compiler admits segment-local
  arena rewind only when semantic descriptor recursion, exact suspension
  liveness, pending-token SSA lineage, a closed pure-callee graph, and the
  nominal `AskBegin` edge jointly prove that no borrowed address escapes.
  Private helpers which return a rich `TaskOutcome` remain ineligible, while
  the public consuming task rewinds entry and continuation segments to their
  exact incoming cursors. The executed Fe parent/child gate carries UTF-8,
  bytes, and a bounded u32 list, returns receipt `533`, and finishes with the
  canonical allocator at baseline and zero completion tokens. The release
  evidence is 13/13 resident actors, 2/2 Worker mailbox cases, the generated
  rich WebIDL continuation, 9/9 canonical-interface cases, and 37/37 fixed
  completion-broker cases. No request field schema, response-width table,
  actor selector, or runtime manifest was added.
  Rich values may now remain live through a second suspension without retaining
  canonical Wasm storage. Semantic `#[host_type]` identity makes the compiler
  emit a borrowed descriptor lane with derived stride, alignment, and maximum
  length. The fixed task adapter copies the returned bytes synchronously into
  private owned storage, while Sonatina commit `48b3ba7e` supplies an opt-in
  opaque checkpoint/rewind pair for this generated protocol. Each start or
  resume checkpoints after live input allocations, captures a returned rich
  frame, rewinds the segment-local suffix, and then releases any re-lowered
  input through checked post-return. No pointer, Wasm allocation, field schema,
  or JSON survives the await. Fixed mechanics reject missing cleanup authority,
  invalid lengths, null, misaligned, and out-of-bounds descriptors. The compiled
  release gate runs two different UTF-8 values concurrently through a later
  timer suspension, then cancels a third task during that later suspension;
  values remain exact, broker tokens return to zero, and the allocator returns
  to its byte-exact baseline. The 9/9 task-machine and 37/37 completion-broker
  suites are green. General owned rich task results remain open.
  Recursive nominal scopes now compose through arbitrary acyclic package
  depth. The compiler follows each child actor's own `ScopedTask` roots,
  recursively derives its nominal children, materializes the nested package
  tree, and rejects a repeated nominal actor in one supervision ancestry path.
  Each nested Worker receives a closed canonical actor Wasm plus a separate Fe
  continuation Wasm. This preserves the actor caller's resettable request arena
  without invalidating suspended host values held by the task module's checked
  canonical stack. No task manifest, actor selector, or authored route was
  added. The fixed Worker host derives scope and mailbox imports from the task
  Wasm, starts only compiler-published self-less task machines, binds their
  lifetime to the Worker, and surfaces unexpected task failure to ordinary
  Worker supervision. A real Bun package gate executes Parent -> Middle ->
  Leaf supervision and a typed nested request, observes stable epoch zero, then
  cancels with no leaked completion tokens. The complete 12-test
  `resident_actor` release suite passes, including the sibling regression and
  nominal-cycle rejection. Wasm preparation also now reifies aggregate
  constants in synthesized task entry and continuation bodies, matching the
  ordinary authored-body path instead of leaking target-specific `ConstRef`
  handles into portable lowering.
  Remaining: attach opaque ports, carry rich canonical values through that
  `MessagePort` placement, and run the render-owned DEC task path in real
  Chromium.
  Recursive resumable SCCs must remain explicitly refused until linked affine
  frames are sound.

## C. GPU pass graphs and perturbation

- [x] General typed compute/multipass pass graphs, including a
  compute-to-compute-to-fragment Rollcall graph. Gates:
  `rollcall_pipeline_pass_graph_compiles_with_external_resources_and_private_mem`,
  `known_color_pass_graph_e2e.rs`, and `rollcall_pass_graph_e2e.rs`.
- [x] Perturbational Mandelbrot has a full-GPU reference orbit, Fe-owned deep
  state and navigation, glitch handling, and independent CPU/GPU receipts.
  Gates: `perturbational_mandelbrot_gpu_oracle.rs`,
  `precision_fixed_orbit_gpu_oracle.rs`, and `demo_compile_gate.rs`.
- [x] All four named GPU substrate gates execute locally without a skip on the
  llvmpipe Vulkan CPU driver. Known Color, Rollcall, the independent bigint
  `Fixed<8>` orbit oracle, and the independent perturbation classifier all
  pass. This proves actual shader dispatch and readback on a software Vulkan
  implementation, not merely SPIR-V or WGSL validation.
- [ ] Replace modeled shader operation counts with measurements derived from
  the lowered Naga representation, and establish frame/submission budgets.
- [M] Execute the same GPU gates on real WebGPU hardware. This host still has
  no `/dev/dri`; llvmpipe is execution evidence but not hardware performance
  or driver-diversity evidence. `MB2_ALLOW_GPU_SKIP` remains forbidden at this
  gate.

## D. Geometric algebra compiler and examples

- [x] Reflection-driven sparse GA expression planning, metric selection, CSE,
  and generated incidence evaluation are real Fe/compiler constructions, not
  a demo-name switch. Gates: the QCGA sparse fixtures in `wasm_e2e.rs` and
  `spirv_e2e.rs`.
- [x] QCGA pencil pick, drag, re-solve, and render is end-to-end Fe-owned. Gates:
  the independent tests in `support/qcga_pencil_acceptance.rs`,
  `qcga_pencil_de_oracle.rs`, the QCGA receipt in `demo_compile_gate.rs`, and
  the real pointer tape in `source_inspector.browser.mjs`.
- [x] The duplicate vertex-shaded QCGA application is retired; the shared
  solver/projection code remains as a non-rendering oracle for the canonical DE
  actor. Gate: `qcga_pencil_de_compiles_as_a_fe_owned_iterative_fragment_surface`.
- [ ] Generalize off-diagonal Clifford products beyond the current supported
  vector/scalar contraction and metric paths.
- [ ] Add measured scalar, packed, subgroup, and workgroup schedule selection,
  including shared memory and barrier lowering where WebGPU permits it.

## E. Fe-native web and gallery

- [x] The gallery page, TodoMVC, SourceInspector, and Event Studio are composed
  and controlled by Fe actors. Gates: `page_projection.rs`,
  `todomvc_actor.rs`, `source_inspector_actor.rs`, the precompiler structural
  tests, and the TodoMVC/SourceInspector Chromium tapes.
- [x] Pointer, wheel, pan/zoom/orbit, picking, batching, and recovery policy are
  Fe-owned for the canonical interactive examples. The fixed host observes
  browser standards facts and transports canonical values.
- [x] The canonical gallery rejects authored browser JavaScript and undeclared
  bundle inputs. Gates: `canonical_gallery_rejects_authored_browser_javascript`
  and `canonical_gallery_rejects_forbidden_non_fe_bundle_inputs`.
- [x] The duplicate Trunk gallery lane is retired. `demos/gallery.html` is the
  sole gallery source; `demos/gallery/`, its `copy-dir` declaration, and the
  legacy landing link are gone. Gate:
  `repository_has_one_canonical_gallery_source`.
- [ ] Evolve SourceInspector into a rich Fe-owned source explorer without
  discarding the existing fe-web component experience. Preserve syntax
  highlighting, semantic links, fuzzy search, symbol outlines, documentation
  and signatures, definitions/references/implementors, navigation history,
  deep links, keyboard/focus behavior, and authored-source/WGSL/Wasm/generated
  provenance views. The resident Fe actor must own query interpretation,
  ranking, routing, selection, cross-link, outline, and explanation policy.
  The fixed host may realize DOM ranges, scrolling, history, and raw standards
  events, but must not acquire source-navigation policy. Feed the component a
  typed, content-addressed compiler `SourceAtlas` or equivalent binary value,
  never a caller-authored JSON manifest. First harden deterministic multi-file
  semantic indexing and add real browser gates, then use this application to
  drive ergonomic owned resident text, scoped browser handles, and rich-code
  patch operations in Fe.
- [~] The current compiler worktree again compiles all 12 canonical render
  actors in one sweep, plus all three resident actors and the Fe page
  projection. The perturbation regression was a redundant fresh-record
  materialize/load round trip inside `build_reference`; a representation-
  checked RMIR identity fold removes it, with its own focused semantic gate.
  This slice closes when it is landed and the browser/runtime gates below are
  repeated.
- [ ] Delete the runtime render manifest. Replace it with compiler-derived typed
  or binary exports for resource, pass, recovery, presentation, and artifact
  location semantics.
- [ ] Contract the fixed JavaScript runtime to standards observation and GPU
  realization, with no application policy or demo vocabulary.
- [ ] Finish migrating, reclassifying, or retiring every legacy showcase.
- [ ] Establish cold, warm, and invalidation compile budgets. The 2026-08-15
  Rollcall evidence refresh took 5m 14s to rebuild a broad release codegen
  chain after a generator-only source edit, which is the current invalidation
  baseline to beat. The 2026-08-24 compile-parallelism audit fixes the order of
  work: add check-compatible phase and Salsa counters, remove repeated HIR,
  voucher, dependency-digest, and duplicate gallery compile work, then pilot
  bounded read-only concurrency. Use Salsa forks only outside tracked queries;
  keep DOM planning/publication serial; begin with two owned-database web or
  codegen workers and at most four analysis-only workers; retain a deterministic
  `jobs = 1` reference. Gates require identical sorted diagnostics, artifacts,
  proof words, roots, transcripts, challenges, codewords, and mutation matrices,
  plus measured release median/p95 and peak RSS. The full evidence and five
  landable slices are in
  `/workspace/scratch/mb2-fe-compile-parallelism-audit-2026-08-24.md`.
- [ ] Add an opt-in riff-cat observation bridge at the Sonatina module boundary.
  Until upstream Fe phase instrumentation lands, this must consume only an
  explicitly requested snapshot or in-memory census. It may report structure,
  types, constants, calls, effects, phase deltas, timing, and memory, but it
  must not participate in Fe name resolution, legality, optimization, or cache
  identity. Keep reports under `/workspace/scratch`; preserve the compiler as
  the semantic authority.

  Immediate actor-stage pilot, refined by the production proof-graph trace on
  2026-09-01:

  1. Land root-only SPIR-V legality inlining first. The backend observes only
     selected shader roots, so module-wide helper expansion is redundant work.
     Keep the bounded frontier, residual-call rejection, Naga validation, and
     artifact gates unchanged.
  2. Extract the current ordered actor loop into a serial planner producing
     owned `ActorCompileUnit` values. One unit is one compute or fullscreen
     stage, or one indivisible authored vertex/fragment pair. Each unit retains
     its authored stage index and complete compiler-derived interface.
  3. Use one short-lived replicated input database per bounded batch, then call
     `salsa::par_map` outside tracked queries over that batch. Salsa forks own
     dependency sharing, memoization, blocking, and cancellation. Each worker
     returns only an owned shader artifact, layout, diagnostics, measurements,
     and its authored index. Drop the complete batch database and backend arenas
     together before starting the next batch.
  4. Assemble and publish serially by authored index. Primary-shader selection,
     pass cycles, resource projection, path collision checks, manifest order,
     hashes, and last-good publication remain in the existing deterministic
     path. `jobs = 1` uses the same planner, worker, and assembler as every
     parallel run.
  5. Gate `jobs = 1` against `jobs = 2` before changing defaults. Compare the
     complete `WebBundle`, materialized file inventory, Wasm, WGSL, SPIR-V
     layout, diagnostics, source digests, pass order, proof words, roots,
     transcripts, challenges, codewords, and mutation matrix. Repeat with
     deliberately perturbed worker completion order. A failed worker must
     produce the same stable diagnostic and preserve the same last-good output.
  6. Measure cold and warm release median/p95, per-unit wall time, CPU
     utilization, and process peak RSS on the production base-trace/LDE graph.
     Start with two workers. Increase the cap only from measured headroom, and
     keep one shared process budget so nested gallery, actor, and oracle pools
     cannot oversubscribe the machine.
  7. Once the actor pilot passes, reuse the same Salsa-owned plan/execute/gather
     construction for independent Wasm and GPU artifacts, then for unique
     gallery compile requests. DOM mutation, bundle materialization, shared
     Wasmtime stores, proof-forest internals, and authored semantic dependency
     order remain serial until separately justified.

  This slice introduces no handwritten semantic cache or invalidation table.
  In-process reuse belongs to Salsa. Any later cross-process artifact store is
  content-addressed only and has no correctness-bearing invalidation policy.

  Pilot evidence on 2026-09-01:

  - [x] Actor stages are owned compile units, with authored raster pairs kept
    indivisible and assembly restored to authored index order.
  - [x] Short-lived batches use a registered Salsa database view and
    `salsa::par_map`; the default batch width is two and the bounded diagnostic
    override accepts one through four workers.
  - [x] One worker and two workers materialize byte-identical complete bundles
    for the compute/resource/fragment acceptance actor.
  - [x] All 17 release `actor_construct` gates pass, including authored raster,
    typed readback, repeated and cycled dispatch, and failure diagnostics.
  - [ ] Measure and exactness-gate the production base-trace/LDE graph under
    one and two workers before widening the pilot to other artifact classes.

## F. Rollcall cryptographic capstone

- [~] Current exact full-workspace release CI baseline. The repo-root Fe test
  boundary passes all 20 workspace inputs after `54e0ff2d5`; the one exact G5
  run is reserved for the final DONE gate.
- [x] Fe Poseidon-Merkle executes on Wasm and native Cranelift and agrees with
  independent/circomlib-derived values. Gates:
  `rollcall_prove_on_wasm_commit_on_evm_and_verify_membership_end_to_end`,
  `rollcall_merkle_root_native_cranelift_leg_is_honestly_reported`, and
  `rollcall_merkle8_root_native_cranelift_leg_is_honestly_reported`.
- [x] EVM registry membership, replay rejection, mutation rejection, and an
  honest depth-20 gas measurement exist. Gates:
  `rollcall_registry_accept_reject_and_claim_at_depth4` and
  `rollcall_registry_gas_at_depth20_is_l2_honest`.
- [x] The field-agnostic S0 browser engine exists as `fe-revm-browser`. It is a
  generic persistent revm session that accepts only raw Fe EVM runtime bytes
  and raw calldata, with no ABI, proof, packing, or application logic in Rust
  or JavaScript. The existing native Rollcall test derives and byte-pins the
  depth-4 runtime plus commit/accept/reject calldata. The `wasm32-unknown-unknown`
  engine executes those vectors and returns the exact native-derived true and
  false ABI words. The same test now executes in actual Chrome for Testing
  152.0.7977.42 through its matching ChromeDriver. Gate:
  `wasm-pack test --chrome --headless --chromedriver <driver> --release`
  `crates/revm-browser --features browser-tests` with
  `WASM_BINDGEN_TEST_WEBDRIVER_JSON` selecting the browser binary;
  `fe_rollcall_verifier_matches_native_accept_and_reject_vectors` passes. This
  is same-source cross-target parity. The independently derived
  Rollcall/Poseidon gates remain semantic truth.
- [x] The same Fe loop-form Merkle body compiles to Naga-valid SPIR-V. Gates:
  `poseidon_merkle_root_loop_compiles_naga_valid_spirv`,
  `poseidon_merkle8_root_loop_compiles_naga_valid_spirv`, and
  `rollcall_merkle_root_spirv_validation_is_honestly_reported`.
- [x] `demos/rollcall/evidence.json` is regenerated from source digest
  `8b8303603bd4937fb883c8d970a2fcd10e551ba8c424873f1e5f04eb15de06db`.
  It records Wasm and native execution with equal 20-limb roots, local EVM
  acceptance and rejection receipts, and SPIR-V validation without claiming
  live GPU execution. Gates: the four tests in
  `rollcall_evidence_verify.rs`.
- [M] Execute the Rollcall pass graph and exact result/pixel oracles on real
  WebGPU hardware. Gate: `rollcall_pass_graph_executes_exact_u32_and_pixel_oracles_on_webgpu`.
- [M] Confirm the on-chain product scope before extending the verifier: target
  chain/L2, gas target, and whether v1 is plain Merkle membership or adds a
  nullifier/ZK claim.

## G. Bounded-proof capstone

- [x] `EscapesByQ12` is specified as a least-terminal-row claim with an exact
  signed Q12 domain, arithmetic-shift rounding, i32 safety argument, and a
  separate future `EntersAttractor` claim. Gate:
  `mandelbrot_bounded_claim_oracle.rs` and
  `MANDELBROT_BOUNDED_PROOF_SPEC.md`.
- [x] Canonical Poseidon BN254 Fr t=3 parameters now derive inside Fe from the
  typed Grain seed, LFSR, self-shrinking rule, rejection sampling, Cauchy MDS
  construction, and batch inversion. There is no generated parameter table.
  The opt-in const budget remains compiler-capped, while ordinary consts retain
  the one-million-step default. Gate: `poseidon_bn254_derived_oracle.rs`
  independently reproduces the plain-field constants, exhaustively checks all
  262,144 self-shrinker inputs, checks all 4,080 derived Montgomery words, and
  executes the concise Fe permutation against canonical and independent bigint
  hash vectors in zero-import Wasm. This closes parameter provenance and the
  Wasm permutation lift. The first bounded trace commitment is recorded below.
- [x] Allocation-heavy proof code now has explicit, checked Wasm arena scopes
  rather than a larger memory ceiling or an application reset convention.
  Sonatina commit `c0252605862035f812ce0ef2cd0e4c82d2261d51` adds typed
  `mem.checkpoint` and `mem.rewind` IR operations; rewind traps unless its token
  lies between the arena base and current cursor. Fe derives scopes through a
  fail-closed interprocedural escape proof. Whole memory-lowerable aggregate
  references may cross proved-safe Fe calls as borrows, while host/effect/GPU
  calls, providers, raw addresses, transport returns, transport stores, and
  nonlocal address formation remain ineligible. The real four-row statement
  call still matches the independent orbit, encoding, Poseidon, Merkle, and
  mutation oracle, then leaves the next canonical allocation at exactly byte
  1024. The canonical-arena suite separately proves that returned browser
  allocations remain live. Gates: `mandelbrot_trace_commitment_oracle.rs`,
  `wasm_canonical_arena.rs`, and
  `local_u32_array_runtime_index_runs_on_wasm_and_traps_out_of_bounds`.
- [~] Fe now generates nominal `EscapeWitness`, `EscapeTraceRow`, and expanded
  `EscapeAirRow` values. Compiled Wasm matches an independent i64 replay across
  directed, invalid, and 512 deterministic cases. Every signed Q12
  quotient/remainder and directed transition is checked. Fe evaluates five
  widened integer polynomial residuals, emits a canonical 15-word
  sign-plus-magnitude row encoding, and verifies alleged directed row pairs.
  The gate mutates all 11 columns on both sides, the public point, and the
  bound, and rejects residual-zero noncanonical shift decompositions. The
  first proof-field slice now evaluates nine row-local and nine transition
  residuals directly in BN254 Fr. It reconstructs signed values algebraically,
  constrains every supplied sign as a bit, executes as self-contained Fe Wasm
  with zero function imports, and rejects directed one-unit mutations. Fe also
  derives the semantic and next-power-of-two trace lengths, emits active rows
  followed by deterministic terminal-state padding, and marks exactly one
  terminal row. The independent gate checks every directed padded row and
  rejects invalid, non-escaping, and out-of-domain requests. Fe integer and
  BN254 first/pair/last constraints make activity monotone, terminal unique,
  select Mandelbrot transitions only on active nonterminal rows, and make
  padding an exact terminal-state fixed point. The field boundary uses nested
  nominal Fe rows rather than a numeric lane protocol. The gate rejects
  non-bit flags, premature inactivity, nonterminal closure, public-point
  mutation, and mutated padding. A generic low-degree range witness now adds
  boolean bits, exact BN254 reconstruction, and a quadratic prefix OR. Its
  signed form enforces canonical positive zero and the asymmetric i32 minimum.
  The Q12 remainder gate uses 12 bits. Because the escape threshold is exactly
  `2^26`, a five-step high-bit OR now constrains the semantic terminal flag.
  The independent oracle proves that these companion constraints close the
  formerly accepted alternate remainder, negative-zero, and premature-terminal
  counterexamples, including exact threshold boundaries and malformed OR
  witnesses. `RangedAirRow` now wires every trace-row column to a Fe type-level
  width: step 21, coordinates 15, squares 30, magnitude 31, real quotient 18,
  imaginary quotient 19, and remainders 12. One nominal Fe entry checks the
  whole row, and its gate accepts a terminal row while reporting exactly the
  three deliberately malformed columns in a combined adversarial row.
  A separate cheap Fe verifier boundary validates the canonical public point
  and bound, terminal step, `trace_length = terminal_step + 1`, and exact
  next-power-of-two domain without replaying the orbit. Its gate rejects each
  public/shape mutation independently. A first real commitment slice now packs
  the 17 range-constrained columns into one injective 210-bit field encoding,
  derives typed `"MR01"` row and `"MN01"` node domains in Fe, and computes a
  Poseidon Merkle root in zero-import Wasm. The tree shape is no longer
  hand-authored: a 22-slot Fe frontier folds any CTFE-proved nonzero
  power-of-two domain through `2^21` while retaining O(log N) digests. Four-row
  compatibility and an eight-row domain both execute. The canonical transition
  and encoded-row schema now live in one reusable Fe kernel ingot consumed by
  both the AIR and commitment layers. A stateful Fe cursor retains the current
  expanded AIR row and advances from its exact Q12 quotients. The production
  commitment consumes that cursor once, folds each active row immediately,
  derives terminal-state padding in Fe, and accepts no host-authored witness
  rows. Its gate covers 4-row, 8-row, and 16-row domains, invalid and
  non-escaping rejection, and exact arena reclamation to byte 1024. The
  independent oracle reconstructs the orbit, encoding, canonical permutation,
  and trees, mutates every four-row column, proves row-order sensitivity, and
  binds both inactive padding positions in a six-active-row/eight-row trace. A
  distinct typed `"MT01"` domain binds each root to an injective 114-bit
  encoding of the public point, bound, terminal step, semantic length, and
  padded length. The same independent gate mutates every public field. The
  low-degree range checks consume bit-decomposition and prefix-OR auxiliary
  columns. The kernel now derives all ten decompositions, their inclusive
  prefix ORs, and the five-bit terminal-threshold prefix in Fe. Their canonical
  411-bit row encoding is split into two injective 253-bit BN254 elements,
  committed under typed `"AR01"` leaves and `"AN01"` nodes, and folded beside
  the main trace in the same pass. Typed `"AT01"` binds the public main-trace
  statement to the auxiliary root before `"MC01"` derives the composition
  challenge. The independent oracle reconstructs every auxiliary bit, packing,
  both trees, the ordered transcript, and the challenge for 4-row, 8-row, and
  16-row domains. Invalid and non-escaping claims fail closed, no host-authored
  range witness enters the production API, and the arena returns to byte 1024.
  The shared Fe field substrate now derives BN254 Fr's maximal two-adic root
  from the prime and generator 5 at compile time, converts it to Montgomery
  form without a generated table, and supplies generic field exponentiation,
  fail-closed Fermat inversion, and roots through order `2^28`. An independent
  bigint Wasm gate checks ordinary powers, the full-width `p - 2` inverse,
  exact root orders, and unsupported-order rejection while retaining the
  existing BN254 and second-modulus gates. One generic Fe Cooley-Tukey
  transform now provides forward NTT and inverse interpolation with no root
  table or size-specialized butterfly schedule. Const predicates reject zero,
  non-power-of-two, and field-unsupported domains. Independent direct bigint
  DFTs and round trips gate the same generic implementation at 4, 8, and 16
  points. Generic Fe coset low-degree extension now interpolates the base trace,
  shifts its coefficients, zero-pads, and evaluates on the larger subgroup. A
  typed validity bit rejects zero shifts and shifts inside the output subgroup,
  preventing the evaluation coset from intersecting the trace zerofier's
  roots. Independent direct
  bigint interpolation/evaluation gates 4-to-16 and 8-to-16 extensions and
  fail-closed invalid cosets. The first genuine composition layer is now
  Fe-authored end to end. It derives the exact four-row canonical trace in Fe,
  evaluates all 17 main and 411 auxiliary column polynomials on a disjoint
  16-point coset, and folds 708 constraints with consecutive powers of the
  post-auxiliary `"MC01"` challenge. All-row, pair-row, first-row, and last-row
  families retain their distinct zerofiers. The fold streams scalar column
  evaluations from compact semantic rows, avoiding both a 428-by-16 field
  matrix and Wasm's current flattened-aggregate parameter limit. There is no
  host witness or generated column table. Typed `"CR01"` leaves and `"CN01"`
  nodes commit the composition evaluations, `"CT01"` binds that root to the
  ordered proof transcript, and `"FC01"` derives the first FRI fold challenge.
  `mandelbrot_composition_oracle.rs` independently reconstructs the integer
  trace and every auxiliary bit, uses a direct bigint inverse DFT rather than
  Fe's radix-2 schedule, evaluates every constraint and zerofier at all 16
  points, and independently derives the main, auxiliary, composition, and
  transcript Poseidon trees. Changed challenges, changed claims, invalid
  claims, non-escaping claims, and out-of-domain evaluations are covered. The
  zero-import composition boundary agrees through the `"FC01"` field value.
  The complete FRI fold chain then pairs evaluations at `x` and `-x` through the
  16-to-8-to-4-to-2-to-1 domains. One const-generic Fe fold implements every
  round, while a Fe const function derives the `"FR01"` through `"FR04"`,
  `"FN01"` through `"FN04"`, `"FT01"` through `"FT04"`, and `"FC02"` through
  `"FC04"` Poseidon domains from the round index. There is no copied round
  table or host-derived protocol constant. The independent bigint gate checks
  every folded value both from the pair formula and from separately
  interpolated even/odd coefficients, then reconstructs every root,
  transcript, next challenge, and fail-closed invalid result. Typed `"FQ01"`
  now derives one query index only after `"FT04"`. Compact pair paths
  authenticate the selected composition values and each corresponding FRI
  pair through depth 3, 2, 1, and the explicit two-leaf base case. A pure
  reusable `poseidon_merkle` ingot adapts zk-kit binary-path semantics to
  field values and typed Fe capacity domains. The Fe verifier rebuilds the
  public transcript, checks every path and index, and checks all four folds.
  The independent bigint oracle separately derives the query, every opened
  evaluation, every sibling, and every root. The composition opening is now
  reconnected to authenticated main and auxiliary AIR columns. The prover
  commits all 16 field-valued rows under typed `"MR02"`/`"MN02"` and
  `"AR02"`/`"AN02"` domains, binds both roots through `"AT02"`, and opens the
  four rows needed for the queried current/next pair. The Fe verifier rebuilds
  both quartet paths, the ordered transcript and composition challenge, then
  recomputes the alleged composition evaluations through the same generic
  `AirColumnView` constraint interpreter. The independent bigint gate derives
  every LDE field, leaf, sibling, root, challenge, and recomputed numerator; it
  passes against zero-import Wasm. A separate typed-verifier gate mutates an
  opened row, quartet path, AIR roots, FRI values and paths, query index, and
  public metadata; every mutation is rejected. Reaching those gates exposed
  and fixed a general Wasm lowering bug where forwarding an `AddrOf` borrow
  deep-copied its pointee and detached mutable writes from caller storage.
  Separate execution gates now prove mutable-borrow identity and preserve
  ordinary aggregate deep-copy semantics. A reusable `canonical_words` ingot
  now derives exact word counts and ordered codecs from nominal Fe records via
  reflection and FCO. Its browser writer and reader interpret those same typed
  codecs over bounded linear memory without a JSON schema. Signed `i32` words
  use an exact Fe bitcast, field decoding rejects limbs outside the 13-bit
  radix and values greater than or equal to the modulus, bool decoding rejects
  non-bit tags, and const-generic arrays include the zero-length case without
  transport lanes. The outer `"MBP1"` versioned receipt derives its complete
  35,503-word layout from the public claim and nested authenticated AIR/FRI
  query types. One zero-import Wasm gate generates the receipt in Fe, copies it
  across the generic canonical host ownership/reset boundary, verifies it in
  Fe, and checks every emitted word against the independent bigint model. It
  rejects truncated, trailing, and misaligned byte lengths, changed protocol
  fields and public inputs, invalid bool tags, oversized limbs, the field
  modulus itself, changed roots, and invalid proof requests. The accepted gate
  took 1,779.46 seconds and peaked near 10.2 GB RSS in this sandbox, exposing a
  real generated-function lowering cost that must not become the ordinary
  edit loop. Verifier-cost evidence and succinctness remain pending. The
  reusable field API's SPIR-V helper-call seam remains open as a compiler
  issue, but BN254 will not be ported into the proof WGSL path. This is a
  complete authenticated toy receipt boundary, not yet a succinct proof.
- [ ] Produce a succinct proof whose verifier is demonstrably cheaper than
  replaying the orbit.
- [x] Finish one complete BN254 toy proof and verifier accept/reject boundary.
  The Fe-derived canonical receipt and independent whole-word/mutation oracle
  described above are the acceptance gate.
- [~] Retarget the protocol to BabyBear before any prover GPU work. BN254 Fr
  must not be ported to WGSL. The BabyBear gate requires extension-field
  challenges, new injective packing, Fe-derived field-specific Poseidon, and
  new independent vectors. Same-source Fe Wasm/native agreement is parity;
  the separately implemented bigint model remains the semantic oracle. The
  u32-native base-field slice is now real: generic `WordField<M>` derives
  one-word Montgomery arithmetic from a prime parameter block using portable
  16-bit partial products, and the BabyBear instance derives its inverse,
  `R^2`, and maximal two-adic root in Fe. The independent u64 gate covers
  arithmetic, inverses, powers, and all subgroup orders; the same multiply
  lowers to Naga-validated u32-only SPIR-V/WGSL. Generic
  `BinomialExtension4<M>` now supplies the canonical BabyBear challenge field
  `F[X]/(X^4 - 11)` without a BabyBear-specific arithmetic body. Its
  zero-import Wasm gate checks addition, schoolbook multiplication, powers,
  and the tower inverse against an independently implemented u64/BigUint
  polynomial model. Every output coefficient of the same Fe multiplication
  lowers to branch-free, u32-only, Naga-valid WGSL. This slice also fixes the
  shared semantic place model so literal array indices remain
  `Index(Constant)`: distinct affine element moves are now accepted while a
  repeated move of the same element still fails. Gates:
  `precision_baby_bear_oracle.rs`, `semantic_borrowck.rs`, and the three
  `arr_const_index_*` UI fixtures. The canonical Plonky3 width-16 Poseidon2
  permutation is now derived in Fe from the shared Grain construction, with
  no authored round-constant table. Its zero-import Wasm gate checks all 141
  constants and five complete permutations against Plonky3. A reusable
  `word_field_encoding` interpreter injectively packs exact-length bounded bit
  strings into 30-bit field payloads, rejects undersized moduli, invalid
  widths, range violations, and output overflow. A security review caught and
  removed the initial one-field, 31-bit digest checkpoint before composition
  or FRI could depend on it. Production commitments now use Plonky3's
  width-16 shape: a rate-8, capacity-8 fixed-length sponge absorbs the nominal
  domain, exact bit length, and payload, then returns an eight-field digest.
  Merkle nodes concatenate two eight-field children into one permutation,
  matching `TruncatedPermutation<_, 2, 8, 16>` and giving roughly 124-bit
  collision security rather than the toy checkpoint's 31 bits. The
  independent bigint/Plonky3 gate covers 128
  randomized schemas, every bit of its directed mutation case, trailing-zero
  length ambiguity, and the complete 411-bit capacity. The production
  Mandelbrot row, public claim, and Fe-derived auxiliary schema now interpret
  through that same utility as 7, 4, and 14 BabyBear fields. A distinct
  zero-import gate independently reconstructs all three encodings, mutates all
  210 main-row source bits and all 203 auxiliary source bits, checks maximum
  values, and proves range violations fail closed. Commits `034a3ac78`,
  `d3b1f1fbe`, and `6fc7dbea2`; gates `poseidon_baby_bear_oracle.rs` and
  `mandelbrot_baby_bear_encoding_oracle.rs`. The portable tree layer now
  preserves zk-kit.fe's `TwoToOneHasher` API and little-endian binary path
  semantics without fixing the field or execution backend. One effect-free
  `merkle_core` body supplies ordinary paths, compact half-pair and
  four-quarter openings, and fail-closed streaming frontiers. Its zero-import
  Wasm gate independently checks all shapes, mutations, invalid indices,
  incomplete trees, and frontier overflow. Typed BabyBear Poseidon2
  interpreters now drive production main and auxiliary trace roots, statement
  binding, and auxiliary transcript binding through that same core. Their
  Plonky3 gate checks full eight-field roots, distinct nominal transcript
  domains, every leaf bit mutation, and
  invalid digest propagation. The transcript now squeezes its composition
  challenge from four Poseidon2 output lanes directly into the canonical
  quartic BabyBear extension, with a dedicated nominal domain and no
  base-field soundness shortcut. Plonky3 independently checks all four
  coefficients and invalid transcripts return no challenge. One shared
  fixed-length field-vector absorber now handles both base-field arrays and
  value-major flattened quartic arrays. The production 17-field main LDE row,
  411-field auxiliary LDE row, and quartic composition row each have distinct
  typed domains over that same mechanism, with independent Plonky3 values and
  position/coefficient mutation checks. Their typed root wrappers reuse the
  same eight-field Merkle compression. The transcript order is now enforced
  by nominal Fe types: proof transcript, main LDE root, auxiliary LDE root,
  quartic composition challenge, composition root, then round-indexed FRI
  challenge and layer stages. FRI domain tags are derived from the const
  round (`FC01`, `FC02`, `FR01`, and so on), not passed as numeric runtime
  IDs. The independent chain gate checks the ordered bindings, first and
  second round challenge coefficients, and a quartic FRI row. Gates:
  `merkle_core_oracle.rs` and
  `mandelbrot_baby_bear_encoding_oracle.rs`. The first production BabyBear FRI
  binary fold now operates directly in the quartic challenge field. It pairs
  evaluations at `x` and `-x`, derives the base-field root from the domain
  size, and returns evaluations at `x^2`. Its zero-import gate independently
  reconstructs extension multiplication, subgroup roots, point inverses, and
  all four output coefficients, mutates the evaluation seed, challenge, and
  shift, and rejects zero coset shifts. That fold now drives a complete typed
  16-to-8-to-4-to-2-to-1 commitment chain. Each round commits its folded
  quartic codeword before deriving the next challenge, squares the coset shift,
  and carries nominal round-indexed roots and transcripts. The independent
  Plonky3 gate reconstructs every layer root, final evaluation, and final
  transcript, and mutates the input codeword, starting transcript, and shift.
  Fiat-Shamir query sampling and compact authenticated openings are now
  landed for that exact 16-to-1 chain. The final typed FRI transcript squeezes
  an `FQ01` BabyBear challenge and derives an index in the eight-point half
  domain. The receipt retains only the queried `x` and `-x` composition pair,
  the corresponding pair at each folded layer, compact generic `merkle_core`
  sibling paths, the typed roots, and the final evaluation. The Fe verifier
  reconstructs every transcript and challenge, authenticates every retained
  pair, checks all four folds, and binds the final row directly to its root.
  The independent Plonky3 gate checks exact query indices and rejects changed
  indices, composition and FRI values, sibling paths, roots, final values,
  validity, and the external AIR transcript. Compiler commit `e794781d1`
  preserves addressable shape for zero-length const-generic arrays without
  inventing payload lanes, with both MIR-shape and executed Wasm regressions.
  Typed main and auxiliary BabyBear LDE quartet adapters now reuse the same
  field-generic `merkle_core` four-quarter algorithm. Their executable gate
  independently reconstructs both 16-leaf Plonky3 roots, checks every legal
  quarter position, rejects an out-of-quarter index, and rejects changed
  leaves, siblings, roots, validity, and path indices. Commit `239b3cf39`
  now factors the exact 708-constraint Mandelbrot fold behind one
  `AirColumnView<F>` and `ConstraintNumerators<F>` interpretation over the
  shared `Radix2Field` boundary. BN254 trace interpolation and authenticated
  openings both consume that field-neutral body, with no backend branch or
  numeric column-ID API. Commits `20ac9e47d`, `39fa0177f`, and `25df3cbe5`
  add the general FCO borrow constructor and recursively borrowed canonical
  receipt encoding rather than a proof-local copying shim. The complete
  independent BN254 bigint composition, authenticated FRI, canonical receipt,
  and mutation gate passed 1/1 in 2,205.48 seconds. An opt-in AIR-only edit
  slice retains all coset-point independent parity checks, while the complete
  receipt remains the default authoritative gate. Commit `04533f5cf` now
  interprets the same 708 constraints over `BabyBearExt4` through an
  `AirColumnView<BabyBearExt4>` backed by authenticated base-field main and
  auxiliary rows. `composition_value<F, TRACE>` derives every trace-domain
  zerofier from its field and compile-time trace size, while
  `radix2_query_geometry<TRACE, LDE>` derives the next-row stride, negative
  offset, and local query position from one transcript-selected index. The
  checkpoint verifier authenticates the four required LDE rows and binds its
  independently recomputed positive and negative AIR composition values to
  the opened FRI pair. Its zero-import Wasm gate reconstructs the quartic
  arithmetic and every constraint family independently, executes the geometry
  at both 4-by-16 and 8-by-64 sizes, and rejects eleven mutations plus an
  out-of-domain query. The final gate passed 1/1 in 49.47 seconds. Fixed
  4-by-16 values now occur only at checkpoint instantiations, not in reusable
  production function signatures. Commit `e9691826f` adds the field-neutral
  recursive `FriSchedule<FIRST_ROUND, INPUT_LOG>` protocol description. Each
  normalized node carries its derived round, input logarithm, output width,
  compact pair width, Merkle sibling depth, and tail. One structural metadata
  interpreter executes at both 16 and 64 points and independently matches
  round count, terminal round, retained evaluations, pair openings, and Merkle
  sibling depth. Compiler commit
  `ad0678f23` makes explicit-return checking normalize both sides before
  unification, closing the language seam where a ground recursive type
  function used as an associated-type receiver normalized in only one
  direction. Commit `d0a2b83b5` then interprets the shared schedule into the
  nested BabyBear complete-codeword carrier and its recursively derived
  fail-closed value. Compiler commit `3976b30ac` makes an enclosing impl's
  symbolic const predicates available as exact assumptions inside its method
  bodies. This closes the language seam that otherwise forced the schedule
  interpreter either to repeat node predicates on a trait method that cannot
  name them or to bypass checked crypto helpers. Commit `6c0317c95` uses that
  machinery to interpret the valid fold traversal from the same
  `FriSchedule<1, 4>` that derives its carrier. `BabyBearFriFirst` handles the
  one composition-transcript boundary, `BabyBearFriTail` recursively commits
  each output layer before deriving the next challenge, and
  `FriScheduleDone` stops without an unused terminal squeeze. This deletes the
  manual 16-to-8-to-4-to-2-to-1 round types and staging functions. The renamed
  `fold_checkpoint_fri16` gate remains zero-import Wasm, matches every
  independent Plonky3 layer oracle, rejects mutations, and passed 1/1 in 97.98
  seconds after the traversal replacement. The complete 41-test const
  predicate suite also passes. Commit `ab1e39966` interprets the same schedule
  into the compact query carrier: every nonterminal node contains its typed
  root, authenticated pair, derived sibling depth, and recursive tail; the
  one-evaluation terminal contains only its root and evaluation. Commit
  `a877cf113` interprets the complete codeword carrier into that compact
  receipt, samples Fiat-Shamir only at the structurally discovered terminal,
  and derives every pair position and path from the schedule's pair width and
  depth. Commit `5b93b90cf` removes the last manual verifier walk. One
  interpreter reconstructs the terminal transcript and query through the
  typed root chain; another authenticates each pair, replays its fold, checks
  the selected child, advances the transcript, and terminates at the one-row
  commitment. The zero-import independent Plonky3 gate passed 1/1 in 148.36
  seconds and rejects changes to the query index, composition and folded
  values, path indices and siblings, final evaluation, intermediate root,
  validity, and external AIR transcript. No 16-to-8-to-4-to-2-to-1 layer list
  remains in the carrier, opener, or verifier. Commits `9f608fe52` and
  `9979f0700` add a field-neutral recursive `FriQueryPlan` and its BabyBear
  interpreter. FQ01 through FQn are independently domain-separated and
  sampled by one structural implementation rather than an authored call
  sequence. Commit `9ba8a581e` extends the field-generic, effect-free
  `merkle_core` with a canonical sparse multipath: requested indices are
  sorted and deduplicated, sibling nodes are emitted bottom-up and
  left-to-right only when they cannot be reconstructed, and nonzero unused
  capacity is rejected. Its independent affine-hash Wasm oracle passed 1/1 in
  0.99 seconds, including eight malformed-proof cases. Commit `d69a06aa6`
  supplies separately domain-checked composition and round-typed BabyBear FRI
  wrappers. Commits `43680a832` and `abf1c8259` then interpret the same query
  plan into each layer's positive and negative leaf requests and feed those
  requests directly into the sparse opener. The zero-import Plonky3 gate
  independently recomputes the actual FQ samples, unique-leaf sets, and
  sibling counts, and passed 1/1 in 205.35 seconds. A complete shared
  multi-query FRI carrier and fold verifier are now landed. Commit
  `59e67aba2` interprets `FriSchedule<1, 4>` and the four-sample query plan into
  one composition multipath, one shared multipath for every nonterminal FRI
  layer, and the one-evaluation terminal. Capacities, round brands, layer
  widths, and sibling bounds remain derived from the schedule and query count;
  there is no second layer list or FQ call sequence. Commit `fc5c766f9` adds a
  two-pass verifier. Its authentication interpretation checks each Merkle
  multipath exactly once while reconstructing the typed transcript and fold
  challenges. A second query-plan interpretation lowers the typed samples to
  one fixed buffer, then an ordinary Fe loop reuses a single
  schedule-specialized fold checker for all four queries. This reduced focused
  Fe analysis from more than 9 minutes 55 seconds to 35.6 seconds without
  moving protocol work into the host. The optimized zero-import Wasm gate
  independently derives the FQ samples, canonical leaf sets, sibling counts,
  roots, and terminal value, then rejects mutations at the composition, each
  of the three sparse FRI layers, terminal, external transcript, and coset
  shift boundaries. It passed 1/1 in 568.12 seconds. Commit `e0ae2416e` now
  interprets those same typed FQ samples through
  `radix2_query_geometry<TRACE, LDE>` into current, next, negative, and
  negative-next row requests, then opens one canonical shared multipath in
  each separately branded main and auxiliary base-field tree. The large row
  arrays are borrowed with `ref`, openings are populated with `mut`, and
  verifiers borrow the completed records. This avoids backend-invalid Wasm
  signatures with thousands of flattened parameters or results without
  moving storage into a host shim. Its zero-import Plonky3 gate independently
  reconstructs both roots, the actual FQ row set, canonical leaf ordering,
  and sibling counts, and rejects value, sibling, and index mutations in both
  trees. The focused opener-only gate passed 1/1 in 27.18 seconds. Commit
  `1e16febd1` then adds the generic `BabyBearAirFriReceipt` carrier and the
  concrete `Checkpoint4BabyBearReceipt16` interpretation. Its verifier
  authenticates each AIR tree and each FRI layer once, reconstructs the typed
  query buffer from the terminal transcript, recomputes every selected
  composition pair through the shared 708-constraint AIR, and replays every
  FRI fold. The combined zero-import Wasm seam gate builds the receipt in Fe,
  accepts the clean value, and rejects mutations to main rows, auxiliary rows,
  composition values, the main root, pre-LDE transcript, coset shift, and
  claimed coordinate. It passed 1/1 in 147.31 seconds. Commit `c05b4cb37`
  closes the production LDE input seam. A typed `EscapesByQ12` claim is now
  interpreted into its exact canonical four-row trace, then the shared
  field-generic radix-2 coset LDE derives all 17 main and 411 auxiliary
  columns. The Fe path commits every generated row without accepting a host
  witness. Its zero-import Wasm gate independently reconstructs the trace,
  performs a direct inverse DFT and polynomial evaluation rather than replaying
  Fe's butterfly schedule, and computes both roots with Plonky3 Poseidon2. It
  passed 1/1 in 157.08 seconds and rejects non-escaping claims and a zero coset
  shift. Commits `5ffa02d5c`, `b3f0123b0`, and `c34720fc2` close the canonical
  BabyBear receipt boundary without a host codec. `ImplBuilder.borrow_mut`
  lets FCO providers synthesize explicitly qualified mutable field decoders,
  with positive replay, negative fail-closed, and frozen command-surface
  gates. Generic canonical codecs now cover u32-native prime fields, quartic
  extensions, Poseidon2 digests, and counted sparse Merkle multipaths. The
  proof carrier consolidates composition and round-branded FRI openings into
  one role-branded `LdeMultiOpening`, derives nested stream codecs from the Fe
  record and recursive schedule types, and emits only counted leaf, sibling,
  and value prefixes. The optimized zero-import Wasm gate parses the public
  carrier topology independently, proves that no unused capacity slots enter
  the wire receipt, roundtrips through Fe decode and verification, and rejects
  truncation, trailing data, noncanonical booleans, the BabyBear modulus,
  over-capacity counts, and authenticated value mutation. It passed 1/1 in
  467.98 seconds. The same run peaked near 10 GB RSS, so whole-receipt
  generated-function lowering is now a measured compile-cost risk. It also
  exposed an outstanding backend bug for an uninitialized local first written
  through a `mut` call: semantic checking accepts it, but Sonatina SSA lowering
  treats the local as undefined. The receipt currently begins from an explicit
  Fe canonical-empty value, not a host workaround. Fixing the general backend
  path and reducing derived decoder compile cost remain compiler work.
  The explicitly named checkpoint remains a four-row, 16-point toy protocol
  and is not yet a succinct production proof. The same permutation has also
  lowered to Naga-valid u32-only WGSL with the local Sonatina conditional-loop
  structurizer fixes, but that browser gate is not landed until those commits
  are published and the Fe dependency pin advances. The complete protocol
  retarget remains pending.
- [~] Make the production claim a chunked recursive high-precision recurrence,
  not one monolithic fixed-Q12 trace. BabyBear is the proof field, not the
  numeric precision ceiling: derive each signed fixed-point coordinate from a
  const-generic vector of bounded base-field limbs, initially reusing the
  established 13-bit `Fixed<L>` representation. Leaf proofs certify exact
  iteration chunks and their multi-limb boundary states. Parent proofs require
  adjacent boundary equality and compress two certified intervals into one
  typed accumulator. Derive convolution tiles and staged carry normalization
  from the limb count, commit boundary states through fixed-size typed digests,
  and independently gate rounding, range, carry, chunk-continuity, and mutated
  boundary failures. The browser should progressively schedule leaf chunks and
  binary merges through the same Fe task, cancellation, and backpressure spine
  used by rendering.
  The field-neutral semantic boundary is now landed at `5eb714b91`. The
  canonical `precision::fixed::Fixed<L>` value stores one sign and an
  LSB-first `[u32; L]` of bounded 13-bit limbs, normalizes signed zero, and
  converts in Fe to the existing recursive limb arithmetic. Generic
  `HighPrecisionEscapeClaim<L>`, `OrbitBoundary<L>`, `OrbitInterval<L>`, and
  `RecursiveOrbitAccumulator<L>` records define exact post-step orbit chunks.
  Leaf certification replays `z_(n+1) = z_n^2 + c`; parent merging requires
  valid children, matching claims, identical shared boundaries, ordered
  intervals, and overflow-safe leaf counts. Their canonical codecs are
  reflection-derived from the nominal records. The zero-import Wasm gate
  `mandelbrot_recursive_fixed_oracle.rs` compares all 37 carrier words against
  an independent bigint model over directed and randomized inputs, and rejects
  malformed limbs, signed zero, invalid booleans, truncation, trailing words,
  changed endpoints, statement mismatch, invalid children, and discontinuous
  boundaries. Focused nextest passes 1/1. This is the recursive semantic
  contract, not yet a succinct recursive proof: fixed-size typed boundary
  digests, multi-limb AIR constraints, BabyBear leaf receipts, and parent proof
  verification remain open.
  Commits `ad20370d7` and `e12032a13` close the fixed-size digest boundary.
  `poseidon_baby_bear::commit_canonical` is a reusable `WordSink`
  interpretation that streams any reflection-derived `CanonicalWords` record
  directly into the typed field sponge. It binds the nominal domain and
  schema-derived word count, rejects non-field words and codec count failures,
  and materializes no second protocol array or host buffer. Its independent
  Plonky3 gate passes 2/2 at unoptimized Wasm. The gate also caught an
  initially over-wide `usize` cursor at the Wasm ABI; the actual bounded cursor
  is now `u32`, rather than hiding the mismatch behind optimization.
  The separate `mandelbrot_proof_recursive_baby_bear` ingot derives
  precision-branded statement and boundary digests from the canonical raw
  carriers. Its 31-word `RecursiveCommittedInterval<L>` exposes only typed
  endpoint digests, iteration bounds, and a leaf count to parent proofs.
  Parent merges reject changed statement or shared-boundary digests, invalid
  children, non-adjacent iterations, and count overflow. The recursive bigint
  oracle now also compares every digest lane against independent Plonky3 and
  passes 1/1. The remaining leaf and parent proof receipts must authenticate
  these carriers; the semantic leaf adapter still replays its interval and is
  deliberately not presented as succinct.
  Commit `92d779ecc` begins the exact multi-limb AIR at the unsigned magnitude
  boundary. One limb-count-derived sparse convolution schedule supplies the
  scalar witness, integer relation verifier, arbitrary-`Radix2Field` residuals,
  and eventual placement plan. It derives BabyBear's safe 13-bit convolution
  width as 30 limbs from the modulus and coefficient bound, rather than baking
  a precision tier. The witness retains every normalized product digit and
  carry in two `L`-sized stages, then constrains guard-bit rounding, its carry
  ripple, wrapped output, overflow, and unique sign-zero normalization. The
  zero-import Wasm gate compares directed and randomized `Fixed<4>` products
  against an independent `BigUint` schoolbook model, checks generated L4 and
  L8 schedules term by term, and requires exact nonzero BabyBear residuals for
  mutated digits, carries, rounding, high digits, and outputs. Focused nextest
  passes 1/1.
  Commits `3968077ed`, `78256ed78`, and `8085a4936` extend that relation into
  an exact signed recurrence DAG. `FixedLinearWitness<L>` retains the addition
  ripple plus both directed subtraction ripples, derives magnitude comparison
  from the final borrow, and constrains signed selection and unique zero for
  both addition and subtraction. One reusable `RadixRangeWitness<L>` derives
  all 13 bits of every radix digit and a running OR prefix, giving local
  degree-two bit, reconstruction, nonzero, and sign-zero relations without a
  precision-specific table. `FixedMandelbrotTransitionWitness<L>` then passes
  the typed outputs of `x*x`, `y*y`, and `x*y` directly into four signed linear
  relations for `z*z + c`; it introduces no copied intermediate columns. The
  expanded zero-import Wasm gate independently reconstructs the 33-word
  linear, 105-word range, and 205-word transition carriers. It checks 19
  directed and randomized transitions against bigint arithmetic and rejects
  mutations in every arithmetic stage, range bit and prefix, signed-zero
  state, or final boundary coordinate. Focused nextest passes 1/1 in 22.85
  seconds after compilation. This is still not a succinct leaf proof. The
  trace interpreter must apply the reusable range carrier to every selected
  intermediate, constrain the wider product-carry bound, and authenticate the
  resulting rows in a BabyBear leaf receipt before a recursive parent may rely
  on the transition. Commit `962f8b4c5` closes the wider product-carry bound.
  Each carry has an 18-bit value decomposition plus an 18-bit slack
  decomposition satisfying `carry + slack = L * (B - 1)`. Together with the
  modulus-derived `L <= 30` condition, this keeps both sides of every
  convolution equation below BabyBear's modulus. The independent gate checks
  all eight L4 carries and rejects changed carries, value bits, and slack bits;
  focused nextest passes 1/1 in 54.92 seconds. Instantiating the typed range
  relations across every transition intermediate and interpreting the same
  plan over arbitrary field-valued opening rows remain the next leaf-AIR gate.
  Commits `f4236566b` and `a3d653c9b` close that scalar polynomial
  interpretation gate. Rounding carry-outs are now explicit boolean witness
  columns, so no residual recomputes them through a host integer shift. The
  separate `mandelbrot_proof_fixed_air_core` ingot mirrors the recurrence as
  nested field-valued `AirProduct`, `AirLinear`, and `AirMandelbrotTransition`
  records and folds 6,185 L4 constraints using field operations only. The
  count is derived from `L`, range width, and operation structure. Three fold
  challenges over 19 directed and randomized transitions are zero, while
  changed convolution digits, carries, rounding carries, signed borrows,
  boundary limbs, and noncanonical inputs are nonzero. The zero-import Wasm
  gate passes 1/1 in 31.26 seconds. Its first adapter attempted to return the
  complete field row as one flattened aggregate and correctly failed Wasm
  validation with an out-of-bounds return size. The executable adapter now
  streams smaller typed subrecords into the same fold, retaining the typed
  authoring schema without placing a 4,000-field value on a backend ABI.
  This establishes the exact relation oracle, not the production trace
  placement. A narrow sparse multi-row schedule must now interpret the same
  convolution and range plan so a leaf proof does not commit thousands of
  columns per transition.
  Commit `79c8e9977` derives that alternative placement plan from the same
  nominal DAG. `SparseTransitionTask` has typed radix, carry, product, linear,
  and boundary variants whose payloads name `ProductNode`, `LinearNode`,
  `RangeNode`, and `CoordinateNode`; application code does not maintain a
  phase-ID or 31-column table. The plan contains `3*L*L + 683*L + 41`
  microtasks after the linear-role refinement recorded below. Independent Rust
  enumeration matches every one of the 2,821 L4
  task signatures, including triangular convolution ranks and the first
  invalid padding row. The derived radix-two placements are 4,096 rows at L4,
  8,192 at L8, and 16,384 at L20. Focused nextest passes 1/1 in 34.89 seconds.
  The wide field fold and sparse task plan are now two interpretations of one
  relation, not competing arithmetic implementations. The next gate must give
  sparse rows their narrow accumulator payload and adjacency constraints, then
  compare their folded denotation to the already gated wide interpreter before
  attaching Merkle, FRI, or WebGPU machinery.
  Commits `409b75d33` and `e8a44c12c` establish the first executable sparse
  subtrace. The task generator now places each close row directly after the
  rows it reduces: radix bits precede their limb close, carry bits precede
  their carry close, product terms precede their coefficient close, and
  boundary limbs stay with their coordinate. Task counts and power-of-two
  domains are unchanged. `SparseTaskRow<F>` is a uniform six-field carrier
  whose two accumulator pairs are independent of limb count. Its radix
  interpretation proves bit and prefix booleans, the running OR recurrence,
  incremental digit reconstruction, limb close and reset, global nonzero,
  signed-zero normalization, and every adjacent accumulator link. An
  independent Rust integer model reconstructs all 1,767 L4 radix rows across
  the 31 semantic range nodes exactly, while the zero-import Fe Wasm audit
  checks 10,553 local and adjacency constraints under three challenges and
  rejects mutations in every semantically used row lane. Focused nextest
  passes 1/1 in 32.69 seconds. This is zero-set equivalence for the complete
  range family, not yet equality of the full sparse and wide folds. The next
  bounded gate is to interpret carry-bit reconstruction in the same row
  carrier, then add convolution term reduction and require both sparse and
  wide interpreters to accept and reject the same directed mutation set.
  Commit `64905d933` completes that bounded-carry slice. The same six columns
  now reconstruct each product carry from 18 boolean bits, reset the active
  scan, retain the completed carry while reconstructing its 18-bit slack, and
  close with `carry + slack = L * (B - 1)`. The independent integer oracle
  matches all 888 L4 rows exactly. The zero-import Fe Wasm audit checks 4,488
  local and adjacency constraints under three challenges and rejects every
  used-lane mutation, including both sides of the value-to-slack handoff.
  Focused nextest passes 1/1 in 40.20 seconds. Convolution term reduction is
  now the next sparse equivalence gate.
  Commit `99e1d6256` adds the convolution reduction without severing it from
  those earlier phases. Seventy-two L4 product rows reduce each generated
  triangular term sequence, close every coefficient with its incoming carry,
  radix digit, and outgoing carry, and prove both initial and terminal zero
  state. `SparseCopyAddress` is nominal and hierarchical. A Fe-derived copy
  bus assigns a distinct challenge power to every range digit and product
  carry, derives source multiplicities from the product DAG, and equates those
  producers with all term and coefficient reads. It therefore introduces no
  application-maintained address table. The independent integer model matches
  every product row exactly, while the zero-import Wasm audit checks 315 local,
  adjacency, terminal, and copy-balance relations under three challenges.
  Mutations in all six row lanes fail. A coordinated two-row mutation that
  preserves the convolution equations and adjacency is rejected by the copy
  bus alone, preventing a locally consistent trace from substituting distant
  source values. Focused nextest passes 1/1 in 37.73 seconds. Product rounding,
  product signs, and linear ripples remain before full sparse and wide
  transition equivalence can be claimed.
  Commit `2125f88e6` closes product rounding and sign normalization in the
  sparse projection. Each product's `L` rounding rows now stay adjacent to its
  finish row. The ripple constrains guard carry-in, retained digit, output
  digit, and boolean carry-out, while the finish constrains the final carry and
  `left_sign XOR right_sign`, masked by output nonzero for canonical zero. A
  second typed copy bus binds the retained window, output digits, guard bit,
  input signs, output sign, and output nonzero to their independently range-
  constrained rows. Redundant wide-carrier aliases such as `round_overflow`
  are projected out rather than copied into the sparse trace. The independent
  model matches all 15 L4 rows. Fe Wasm checks 85 local, group-boundary,
  adjacency, and copy-balance relations under three challenges, rejects every
  row-lane mutation, and rejects a locally valid retained/output substitution
  through the copy bus alone. Focused nextest passes 1/1 in 42.17 seconds.
  The next gate is the four signed linear ripples plus their copy bus.
  Commit `0710f2e75` completes those four signed linear nodes. The former
  overloaded `LinearLimb` task is now a nominal role family for sum, both
  directed differences, and output selection. This adds 12 rows per limb but
  keeps every arithmetic row at six fields and leaves the L4, L8, and L20
  domains at 4,096, 8,192, and 16,384 rows. Independent enumeration now checks
  2,821, 5,697, and 14,901 semantic tasks. The four L4 nodes occupy 68 rows and
  constrain all carry and borrow chains, magnitude comparison from the final
  borrow, signed addition/subtraction selection, output nonzero, and canonical
  output sign. Four per-node typed copy buses bind every repeated input,
  intermediate, selector, final borrow, and output back to the range rows or
  its unique arithmetic producer. The independent model matches every row;
  zero-import Fe Wasm checks 276 local, adjacency, and copy-balance relations
  under three challenges, rejects every lane mutation, and uses the copy bus
  alone to reject a locally valid input/output substitution. Focused nextest
  passes 1/1 in 49.26 seconds. Only final transition boundary equality and a
  combined sparse-versus-wide directed mutation gate remain in this AIR slice.
  Commit `d70304511` closes the final transition boundary. Two sign rows and
  `2*L` limb rows equate the computed `next_real` and `next_imaginary` outputs
  with the claimed next boundary, and a finish row constrains transition
  validity plus zeroed unused lanes. A typed copy bus binds both sides to their
  range rows, so repeating an equal but unrelated pair cannot satisfy the
  relation. The independent model matches all 11 L4 rows. Fe Wasm checks 61
  local and copy-balance relations under three challenges, rejects every lane
  mutation, and uses the copy bus alone to reject a coordinated equal-pair
  substitution. The complete focused gate now passes 1/1 in 67.39 seconds.
  Every wide relation family now has an executable narrow sparse projection.
  The remaining equivalence gate must apply one shared directed mutation set
  to both interpretations before this AIR slice is attached to Merkle and FRI.
  Commit `1c165affa` closes that equivalence gate at the intended zero-set
  boundary. Six shared directed mutations cover a convolution digit, product
  carry, rounding carry, signed borrow, claimed next-boundary limb, and an
  out-of-range public-point limb. For each of three challenges, both the wide
  6,185-constraint interpreter and the corresponding sparse phase reject the
  same semantic mutation. This is not a misleading byte or fold equality
  claim: the sparse projection deliberately adds scan adjacency and typed copy
  buses while removing redundant wide aliases. Together with exact row
  reconstruction and copy-only coordinated mutations for every phase, the
  gate establishes equivalent accepted arithmetic statements. Focused nextest
  passes 1/1 in 84.69 seconds. The multi-limb arithmetic and wide-to-sparse AIR
  equivalence item is complete. The next item is to commit the sparse rows as
  authenticated BabyBear leaf openings and bind recursive parent receipts to
  those leaf proofs rather than replaying a semantic interval.
  Commits `451f35a27`, `2a8924d3c`, and `f09d1d14a` close the base-trace
  authentication slice without overstating it as a STARK. One
  `sparse_transition_row_from_witness` interpreter now derives every active
  six-column row and canonical zero padding from `SparseTransitionTask`; all
  existing independent phase oracles execute through that shared entry. The
  reusable `merkle_core` API can now borrow a large leaf matrix and emit a root
  plus normalized sparse path in one traversal. This avoids both duplicate
  hashing and a 32,768-lane Wasm signature, a failure the L4 consumer exposed
  when `[Poseidon2Digest; 4096]` was initially passed by value. The separate
  `mandelbrot_proof_fixed_air_baby_bear` ingot commits a nominal leaf containing
  limb count, semantic task count, power-of-two trace length, row index,
  active flag, and the six readable witness lanes under the `SL01` Poseidon2
  domain. Its limb-branded root, canonical variable-length multipath, and
  authentication verifier reuse the shared Poseidon2, canonical-word, and
  zk-kit-derived Merkle interpreters. An independent Rust model reconstructs
  all 4,096 leaf messages, applies Plonky3 Poseidon2, and matches the complete
  Fe root. The gate accepts active and padded openings and rejects mutations
  to the root, path index, sibling, metadata, active flag, row value, codec
  length, and out-of-range request. It passes 1/1 in 66.20 seconds; the older
  independent BabyBear LDE multipath regression remains green at 1/1 in
  545.02 seconds.
  Commit `cfdea6d36` closes the deterministic sparse-control relation. Fifteen
  named `SparseControlSelectors<F>` columns identify semantic task kinds
  without a numeric phase ID. Six field-valued payload columns carry the
  generated node, limb, bit or term counters, carry/convolution flag,
  convolution width, and radix weight. One field-neutral AIR constrains every
  selector to be boolean and one-hot, zeros unused payloads, fixes the initial
  and terminal rows, and admits only the exact radix, carry, convolution,
  rounding, linear, boundary, finish, and padding transitions. Its evaluator
  takes only current and next field rows. It does not consult a row index or
  dispatch through `SparseTransitionTask`, so it remains meaningful on LDE
  evaluations. The independent Rust oracle reconstructs all 4,096 L4 control
  rows, while zero-import Fe Wasm evaluates 954,172 local, adjacency, and
  boundary constraints under three challenges. Mutations to selectors,
  counters, flags, widths, weights, phase boundaries, and padding all fail.
  The dedicated gate passes 1/1 in 13.20 seconds, and the complete recursive
  fixed-point regression remains green at 1/1 in 148.06 seconds.
  Commit `fc2d1b4d3` closes the task-selection seam in the arithmetic AIR.
  Four named linear-role selectors avoid cubic role interpolation, and eight
  named range-role selectors distinguish coordinates, product low/high/output,
  and linear sum/differences/output without a 31-way numeric lookup. The range
  roles constrain the exact signed-node pattern, so unsigned sign lanes remain
  canonical zero rather than weakening into don't-care witnesses. The complete
  control row is now 33 readable field columns: 15 task selectors, four linear
  roles, eight range roles, and six semantic payloads. Its latest independent
  zero-import audit reconstructs all 4,096 rows and evaluates 1,109,798
  constraints under three challenges, rejecting mutations across every task,
  role, payload, handoff, and padding family. It passes 1/1 in 24.32 seconds.
  `evaluate_sparse_arithmetic_row` and its adjacency interpreter consume only
  current/next control and six-lane witness rows. Radix weights, carry reset,
  convolution and rounding scans, linear roles and subtraction mode, boundary
  equality, transition finish, and canonical padding are all polynomially
  selected. No task enum or row index enters either production-facing
  evaluator. The complete selector-only L4 audit evaluates 413,678 arithmetic
  constraints under three challenges and rejects directed mutations in every
  semantic family. The older enum-based phase oracles remain independent
  compatibility checks. The full recursive fixed-point gate passes 1/1 in
  97.91 seconds.
  Commit `5bf69746b` upgrades each sparse leaf to the nominal `SL02` protocol
  and authenticates its exact 33-column control row beside the six witness
  lanes. Both leaf hashing and structural control equality are now derived in
  Fe from the record shape: `CanonicalWords` streams the typed leaf directly
  into Poseidon2, while the Fe-authored `Eq` derive provider generates the
  nested comparison. There is no manually maintained 44-field Fe table.
  Generic `WordField<M>` now supplies core `Eq`, so the same structural route
  remains available to other word-field records. The independent Rust oracle
  still spells out a separate field layout, reconstructs all 4,096 `SL02`
  leaves with Plonky3 Poseidon2, and matches the complete Fe root. Directed
  mutations now cover task selector, linear role, range role, semantic
  payload, flag, witness lane, metadata, path, and root fields. The full gate
  passes 1/1 in 139.83 seconds.
  Commit `2f2105011` closes the first committed-copy relation at the AIR level.
  The product bus is no longer a scalar `challenge^address` total that would
  require a host integer address at an LDE point. A generic Fe
  log-derivative interaction instead compresses each nominal `(address,
  value)` pair with `beta` and `gamma`, constrains two inverse ports per row,
  and advances one prefix accumulator from zero back to zero. Address formulas
  consume only the constrained selectors, roles, and semantic counters. Scan
  adjacency lets the new relation remove the older redundant carry-in copy:
  each range-checked carry is bound once to its coefficient output, while the
  arithmetic AIR already propagates it to the next coefficient. An independent
  Rust model reconstructs the conceptual copy multiset from task semantics and
  matches a field receipt over every accumulator and inverse in all 4,096
  rows under three challenge pairs. Source, consumer, inverse, and accumulator
  mutations fail. The established coordinated mutation that preserves the
  local product scan fails exactly once at the interaction terminal, proving
  the new relation is not duplicate arithmetic. The full focused gate passes
  1/1 in 93.53 seconds. This exhaustive gate currently uses the BabyBear base
  field for speed. The production transcript must derive quartic-extension
  compression challenges and use the same field-generic relation, with batch
  inversion before the interaction trace is committed.
  Commit `c40dccaa9` adds two reusable low-degree limb-position selectors for
  the fixed-point rounding boundary. `penultimate` and `last` are generated
  from the limb-count-generic task plan, constrained as boolean and mutually
  exclusive, propagated through radix bit and limb rows, and forced at the
  exact `L - 2` and `L - 1` transitions. This avoids an equality
  interpolation whose degree would grow with the limb count. The authenticated
  control row is now 35 field columns and the sparse leaf domain is `SL03`.
  The independent Rust control model covers both new columns. Its exhaustive
  three-challenge mutation gate evaluates 1,167,135 constraints and passes
  1/1 in 26.97 seconds. The complete recursive receipt, root reconstruction,
  and authentication mutation gate passes 1/1 in 94.80 seconds.
  Commits `3b42093d6` and `ca4ad8d96` extend the same committed interaction
  machinery through fixed-point rounding. One const-generic
  `SparseCopyInteractionRow<F, PORTS>` now carries the prefix accumulator and
  a type-level fixed array of inverse ports; product is its two-port
  interpretation and rounding is its five-port interpretation. The retained
  low/high window is expressed as one contiguous address interval, so output
  index `i` selects low limb `L - 1` followed by high limb `i - 1` through a
  linear address formula rather than an `i == 0` interpolation. Selector-only
  Fe ports bind that window, every rounded output digit, the guard bit, output
  sign and nonzero flag, and both input signs. The independent Rust model
  reconstructs the conceptual multiset directly from semantic task kinds and
  matches all five Fe inverse columns across 4,096 rows under three challenge
  pairs. Source, consumer, inverse, accumulator, and coordinated locally valid
  rounding mutations reject; the coordinated mutation fails exactly once at
  the terminal balance. The complete gate evaluates 45,057 rounding
  interaction constraints and passes 1/1 in 101.44 seconds.
  Commit `49b048194` adds the eight-port linear interpretation. It binds
  left/right inputs, sum and directed-difference intermediates, selected
  outputs, signs, nonzero flags, terminal borrows, and the derived same-sign
  and select-right controls. The address formulas follow the four-node linear
  DAG with constant-degree field polynomials, including the even-node reuse of
  `RealDifference` and `DoubleXy` outputs by their downstream additions. The
  log-derivative model also corrects a weakness in the legacy scalar audit:
  each central range-checked intermediate digit is now counted once for its
  arithmetic production and once for its later selection use, rather than
  relying on an aggregate power-sum cancellation. The independent Rust model
  reconstructs this nominal graph directly from semantic task kinds and
  matches all eight Fe inverse columns under three challenge pairs. Its source,
  consumer, inverse, accumulator, and coordinated locally valid mutations all
  reject, with the coordinated mutation failing only at the terminal balance.
  The complete gate evaluates 69,633 linear interaction constraints and passes
  1/1 in 112.73 seconds.
  Commit `0e1a2ebff` closes the final base-copy family with a two-port boundary
  interpretation. Only the claimed `NextReal` and `NextImaginary` ranges and
  their computed `LinearOutput(NextReal)` and
  `LinearOutput(NextImaginary)` counterparts enter this namespace. The Fe AIR
  selects those four radix sources with constant-degree control polynomials,
  then binds each computed/claimed sign or limb pair without enum dispatch or
  row indices. The independent semantic model matches both inverse columns
  under three challenge pairs. Source, consumer, inverse, accumulator, and a
  locally valid equal-sign flip all reject; the equal-sign flip fails exactly
  once at the terminal balance. The complete gate evaluates 20,481 boundary
  interaction constraints and passes 1/1 in 119.57 seconds. Product, rounding,
  linear, and boundary nominal copy families are now all represented by the
  shared const-generic log-derivative relation.
  Commit `28c5d6a89` replaces per-port witness inversion with the reusable
  zero-tolerant `batch_inverse_or_zero<F, N>` field interpreter. Each typed
  interaction row now performs at most one inversion regardless of its port
  width; inactive zero lanes stay zero and active denominators share prefix and
  suffix products. The complete independent receipts and mutations remain
  byte-identical and pass 1/1 in 117.45 seconds. The same const-generic utility
  can later batch larger row tiles in scalar, packed, or WebGPU schedules.
  Commit `f719d5181` moves interaction challenge derivation to the production
  transcript boundary. The authenticated `SL03` root is squeezed under eight
  independent Poseidon2 domains: beta and gamma for each of product, rounding,
  linear, and boundary. Every challenge is a full quartic BabyBear element.
  The independent Plonky3 oracle matches all 32 base-field coefficients, and
  an invalid root fails closed to a false validity bit plus 32 zero words. The
  complete gate passes 1/1 in 121.93 seconds.
  Commit `f7e09f527` generates and commits the complete quartic interaction
  trace. Every `SI01` leaf has one FCO-derived 97-field schema containing exact
  trace metadata, the `SL03` base root, and the product, rounding, linear, and
  boundary accumulators plus inverse ports. One row-local Fe interpretation
  boundary derives control and witness rows, evaluates all four buses, checks
  their local relations, hashes the nominal leaf, and updates caller-owned
  accumulators. The reusable batch-inversion plan now also has a caller-owned
  placement interpretation. Together these let scalar Wasm reclaim quartic
  temporaries after every row without increasing memory or moving arithmetic,
  scheduling, or commitment work into Rust or JavaScript. The independent
  Rust model reconstructs all 4,096 semantic rows and every port, implements
  the quartic extension separately, checks every inverse and all four terminal
  balances, applies Plonky3 Poseidon2, and matches the Fe root. The optimized
  gate accepts that independently derived root, rejects a one-coefficient
  mutation, and fails closed on an invalid base commitment with the canonical
  zero root. The complete two-test recursive fixed-point binary passes 2/2
  with zero skips in 234.23 seconds.
  Commit `deb6d8314` gives the shared radix-2 arithmetic plan caller-owned
  forward NTT, inverse NTT, and disjoint-coset LDE interpretations. The
  value-returning APIs remain convenience wrappers over that same body. The
  existing direct-u64 BabyBear DFT/LDE oracle now enters through the writer,
  retains invalid-coset rejection, and passes 1/1 in 5.20 seconds.
  Commit `db1f8fac8` adds the nominal 41-field `SparseBaseAirRow`, exposes the
  84-field quartic interaction row as a caller-owned tile interpretation with
  explicit starting accumulators, and composes those pieces into a full-table
  writer with a small typed root digest. This is a placement boundary, not a
  numeric column table: FCO derives canonical field order from the same Fe
  records used by the commitments. An attempted function returning both full
  4,096-row tables honestly failed Sonatina's instruction-result index, so the
  large tables remain caller-owned. Executing both initialized tables in the
  scalar checkpoint remained CPU-bound beyond 14 minutes, so it is not used as
  the ordinary semantic gate. Instead, one zero-import O2 checkpoint executes
  four exact caller-owned base and interaction rows plus their ending
  accumulators, then continues through the complete 4,096-row `SL03` and
  `SI01` roots without allocating the full tables. Its independent model
  matches all 564 canonical words, including exact-root acceptance, a directed
  coefficient mutation, and invalid-root zeroing. It passes 1/1 in 81.17
  seconds. The full-table writer Fe-checks, but its practical execution remains
  part of the production LDE and WebGPU placement gate, not a scalar-Wasm
  performance claim.
  Commit `d9120e986` carries those nominal rows through the next two protocol
  seams without introducing a 125-column index map. The generic fixed-AIR
  control, witness, and four interaction records now derive their canonical
  word traversal directly, and the BabyBear commitment layer aliases those
  exact records instead of maintaining field-for-field copies. One generic
  column interpreter places the reflection-derived 41 base and 84 interaction
  columns through the shared caller-owned radix-2 LDE. The inverse interpreter
  reconstructs typed quartic opened rows from authenticated base-field lanes.
  All local, pair, initial, and terminal constraint evaluators feed a shared
  four-family numerator record, whose zerofier quotient now lives beside
  `Radix2Field` rather than in a Mandelbrot-specific backend. The zero-import
  Wasm gate independently reconstructs every LDE field with a direct u64 DFT,
  checks current and next typed-row geometry, applies the quartic quotient
  independently at four evaluation points under two challenges, rejects two
  invalid requests, and proves directed base and interaction mutations alter
  both the relevant numerator family and final composition. It passes 1/1 in
  273.67 seconds. The executed checkpoint remains the honest four-row to
  sixteen-point placement gate; full 4,096-row placement is reserved for the
  production GPU schedule.
  A transcript-order audit then found that the raw `SL03` root could not safely
  remain the production interaction seed: without a proof linking that raw
  tree to the separately committed LDE, a prover could choose the seed before
  fixing the codeword checked by FRI. Commit `c9f7e9e8c` corrects that boundary
  before it ossifies. Base evaluation and LDE placement are now independently
  callable, every `LD01` base-LDE leaf absorbs limb count, trace size, LDE
  size, position, and all 41 reflection-derived fields, and the typed Merkle
  root is fixed before any interaction witness exists. Production beta and
  gamma values then squeeze from that root under eight distinct `*02` domains;
  the earlier `*01` derivation remains explicitly a semantic-checkpoint tool.
  The independent Plonky3 gate reconstructs the `LD01` root, proves a directed
  base-field mutation changes it, checks all 32 quartic challenge coefficients
  for both clean and mutated roots, and rejects invalid roots and mutation
  selectors. The focused zero-import gate passes 1/1 in 275.53 seconds.
  Commit `192a0950f` completes the corrected interaction side. Production
  challenges now cross the API in a shape-branded wrapper whose payload is
  private, so downstream code cannot repackage raw-checkpoint challenges as
  base-LDE-derived values. The same row-local four-bus interpretation then
  generates caller-owned interaction tiles, a split 84-column LDE writer
  places them independently, and each `LD02` leaf absorbs its proof shape,
  position, exact `LD01` root, and every interaction field. The typed
  interaction root retains that base-root dependency. The independent model
  derives the production `*02` challenges, reconstructs the first four
  interaction rows and every inverse from semantic copy ports, applies a
  direct u64 LDE to all 84 columns, and matches both clean and mutated Plonky3
  roots. It also rejects an unknown mutation selector. The focused zero-import
  gate passes 1/1 in 389.36 seconds.
  Commit `1aff766be` closes the production composition seam without creating a
  second FRI protocol. The compact and sparse provers now share the extracted
  `mandelbrot_proof_baby_bear_transcript` ingot for the canonical `BC01`
  quartic challenge, `BC02` composition leaves, and `BC03` post-composition
  binding; the established compact prover re-exports the same API. The sparse
  transcript commits the exact one-step `RecursiveCommittedInterval<L>` under
  `AS01`, binds the exact `LD01` and dependency-carrying `LD02` roots under
  `AT01`, then binds the statement and roots under `AT02`. Its caller-owned
  codeword writer recomputes both roots and all interaction challenges
  internally before evaluating every composition point, so callers cannot
  pair a codeword with roots or challenges from another commitment. A typed
  composition digest retains the complete statement and LDE dependency chain.
  The independent gate reconstructs the recursive statement commitment,
  `LD01`, `LD02`, nested transcript, all four `BC01` coefficients, every one of
  the 16 four-zerofier quotients, and the `BC02` Plonky3 Merkle root. Separate
  base-field, interaction-field, and valid-statement mutations change exactly
  the expected upstream roots and every downstream transcript artifact; an
  unknown mutation fails closed. The focused zero-import Wasm gate passes 1/1
  in 558.10 seconds.
  Commit `9bc0be73a` binds the named recursive leaf statement to the sparse AIR
  without replaying the transition at the public boundary. One semantic
  `SparsePublicCoordinate` interpretation derives the six point, current, and
  next coordinate positions directly from the generated transition task plan.
  For each coordinate, all `L` fixed-point limbs and its sign become
  fixed-position rational constraints. An executable coherence check resolves
  every derived position back through the task plan, so a layout refactor
  cannot silently preserve a stale numeric row table. Distinct `PC01` and
  `PM01` quartic challenges fold the public constraints and mix them into the
  existing `BC02` composition codeword. The production writer requires the
  exact sparse trace length, recommits the raw one-step interval, rebinds the
  `AT02` transcript, and adds the public contribution to every composition
  point. The four-row scalar placement writer is now explicitly named as a
  checkpoint and cannot be mistaken for a receipt-valid production writer.
  A focused zero-import Wasm gate instantiates the real `L = 4`, `TRACE =
  4096`, `LDE = 8192` public quotient at one extension-field point. Its
  independent Rust model enumerates the semantic task list to recover all 30
  positions, reconstructs both transcript challenges, and matches the complete
  quartic rational composition. It rejects a different valid interval
  statement and proves a changed opened value changes the public contribution.
  The focused gate passes 1/1 in 52.21 seconds. The established independent
  4-by-16 composition, root, and mutation gate remains green at 1/1 in 976.01
  seconds. The full 4,096-row production writer Fe-checks but has not been
  executed as a scalar full-codeword claim; that placement belongs to the
  pending WebGPU schedule.
  The authenticated sparse LDE carrier is now implemented and independently
  gated. One role-branded `MerkleMultiOpening` representation in the narrow
  `merkle_receipt` ingot supplies canonical counted values plus the established
  `MerkleMultiPath`; arithmetic-only `merkle_core` consumers do not inherit the
  generated receipt codec. `LD01` and `LD02` writers use the exact shared leaf
  constructors used by their root commitments, normalize unsorted duplicate
  requests through `merkle_core`, require the reconstructed root to equal the
  transcript-bound root, and retain only requested evaluations. Verifiers
  reject nonzero unused capacity and reconstruct the dependency-bound `LD02`
  leaves from the exact retained `LD01` root. A typed row adapter recovers one
  `SparseOpenedAirRow` only when both authenticated paths contain the same
  index. The zero-import Fe/Wasm gate matches independent Plonky3 `LD01` and
  `LD02` roots and selected LDE values, then rejects ten value, sibling, index,
  unused-capacity, dependency-root, and missing-row mutations. It passes 1/1
  in 838.33 seconds. A separate small carrier-codec gate proves canonical
  roundtrip, semantic authentication, truncation and trailing-data rejection,
  and authenticated value-mutation rejection in 2.80 seconds.
  A fresh uncached rerun also exposed that the older all-in-one toy receipt
  fixture now emits a single Wasm function larger than wasmparser's 7,654,321
  byte limit. Removing the new carrier from that fixture's dependency graph
  leaves the same failure and byte offset, so this is a pre-existing
  whole-receipt generated-decoder cost, not an `LD01`/`LD02` semantic
  regression. The validator limit was not weakened. Production receipt
  encoding must use staged canonical decoders rather than one monolithic
  generated function.
  `SL03` and `SI01` remain exact semantic checkpoints rather than production
  proof roots. `LD01`, `LD02`, and the shared `BC02` composition root cover the
  production codewords, including the fixed-position public relation.
  Commits `e3108816e` and `1e39c20d0` derive the production four-query carrier
  and every capacity from `FriSchedule<1, 13>` rather than maintaining a second
  receipt-shape table. Commit `8b7d2a41c` adds an independently executed
  request-set predicate that rejects missing, extra, out-of-domain, and
  noncanonical unused leaves. Commit `fce3a5d7e` applies that predicate to
  every schedule-derived FRI multipath. The authoritative zero-import
  Fe/Wasm plus independent Plonky3/BigInt integration gate passes 1/1 after
  that hardening, rejecting its established receipt, transcript, opening,
  path, root, terminal, and coset-shift mutations. This run took 2,606.02
  seconds and peaked near 7.2 GiB RSS. The semantic result is green, while the
  whole-ingot compilation cost requires a decomposed gate and compiler-cost
  follow-up rather than normalization as an acceptable edit loop. Commit
  `29916442a` assembles the production sparse receipt entirely in Fe: it binds
  the interval statement,
  `LD01`, `LD02`, and `BC02` roots; folds the complete FRI schedule; samples
  queries only after the terminal transcript; opens the exact AIR,
  composition, and FRI request sets; recomputes the positive and negative
  public-bound `BC02` values from authenticated sparse rows; and verifies all
  four schedule-derived fold chains.
  The exact compiler gate for the real `L = 4`, `TRACE = 4096`, `LDE = 8192`
  specialization passes. This is not yet an independently executed complete
  receipt gate, a succinct recursive leaf proof, or browser proof evidence.
  Its current four-query plan freezes and tests protocol structure only. It
  does not claim a target security level. Before recursive proving, an audited
  Fe policy must derive the query count and related capacities from explicit
  soundness parameters, with an independent calculation gate; four must not
  survive merely as a convenient checkpoint constant.
  Commit `9e3214661` adds the staged production canonical boundary. A generic
  `decode_canonical_words` dual complements the existing fixed-width encoder,
  while the receipt splits header, sparse AIR openings, and FRI into explicit
  Fe-owned codec stages without changing the FCO-derived field order. Browser
  and native hosts receive only `BrowserBytes`; they own no receipt field table
  or decode sequence. The zero-import Wasm gate roundtrips the canonical empty
  production carrier and rejects an invalid boolean, truncation, and trailing
  data, resetting every failure to the Fe-derived empty value. It passes 1/1
  in 124.48 seconds. This proves the bounded staged codec and malformed-input
  floor, not acceptance of a semantically valid production proof.
  The next gate must construct one canonical production receipt, accept it,
  reject targeted transcript, opening, and fold mutations against independent
  models, and feed it through staged canonical decoding. Only that executed
  AIR/FRI receipt may replace semantic replay in the recursive parent carrier.
  The production boundary is now split into separate prover and verifier
  artifacts. The prover writes canonical receipt bytes without invoking the
  verifier internally; the exact Rust gate copies only those bytes into a
  fresh zero-import Fe/Wasm verifier instance, then requires acceptance and a
  targeted mutation rejection. The prover ingot Fe-checks, and the split gate
  builds with `--no-run`, but its first exact execution exposed a compile-cost
  blocker before Wasm emission. The measured prover-only preflight visited
  19,589 semantic specializations, assembled 10,857 runtime functions and 103
  constant regions, and held about 9.7 GiB RSS. It was stopped after roughly
  52 minutes while preparing inline value bodies, without a semantic or
  lowering error. This is materially smaller than the earlier combined
  prover/verifier attempt near 14.8 GiB, so the split is retained, but it is
  not acceptance evidence. Opt-in phase timing identified repeated expanded
  normalization in `prove_production_sparse_interval`, the receipt writer and
  encoder layers, `clear_production_receipt`, and the specialized FRI fold and
  query helpers. The next production-proof slice must express the receipt
  codec and FRI/prover dependency work as compact Fe interpreters over
  CTFE-derived plans, preserve every independent bigint and mutation oracle,
  and show a material reduction in specialization count, package size, peak
  memory, and compile latency before recursion is layered on top.
  Commits `ac3d009ea` and `af2439257` bound whole-package analysis to the
  selected entry graph and cache every base inline body. A fresh exact split
  prover run then completed semantic preflight for 19,525 specializations,
  assembled 10,793 functions and 103 constant regions, and completed inline
  preparation within one 30-second polling interval. It reached portable Wasm
  lowering after 2,642.14 seconds instead of stalling in inline preparation.
  The remaining cost is concentrated in the expanded production schedule:
  `fold_fri_first` took 332.35 seconds,
  `open_fri_multi_query_layers__gd7ee` took 322.59 seconds,
  `write_production_sparse_baby_bear_receipt` took 162.75 seconds, and the
  other FRI/query specializations contributed additional tens of seconds.
  This is direct evidence for a flat CTFE-derived FRI/query/receipt plan with
  scalar, Wasm, and WebGPU interpreters, while retaining the recursive branded
  carrier as the specification oracle.
  That run passed the earlier provider-reborrow seam and then failed closed
  when an arena-owned pointer stored in the Fe prover workspace was loaded by
  a later actor stage. Commits `fa5f29053` and `359ba8438` add field-sensitive
  typed arena provenance across a closed runtime package. A field is trusted
  only when every store to that exact nominal-layout field has an arena-owned
  root and source; opaque, dynamically indexed, public-forged, and mixed-origin
  paths remain rejected. Focused positive, reborrow, staged-actor, and forged
  address gates pass 6/6, and the Wasm-lowering unit suite passes 12/12.
  Commit `fe3d93eaf` adds a bounded exact production-stage checkpoint rather
  than a reduced surrogate. It allocates the real 4,096-row base evaluation
  and 8,192-row codeword objects, initializes them in Fe, executes the shared
  production LDE at coset shift 7, lowers 66 Fe functions to a 305,035-byte
  zero-import Wasm module, and executes successfully. The complete split
  receipt gate now also executes. Commit `002fc25c1` represents structurally
  oversized local aggregates by one compiler-owned arena address while
  retaining their Fe `AggregateValue` semantics and by-value deep copies. The
  focused 8,192-leaf composition opening dropped from 65,549 emitted Wasm
  locals to 816 and still passed its semantic authentication check. Across the
  complete prover the highest measured local count is now 8,205. Local
  Sonatina companions `1fb9968e` and `512f76aa` remove the unrelated 16 MiB
  generated-memory ceiling and add opt-in emitted-body pressure diagnostics.
  The exact split gate then passed 1/1 in 1,204.33 seconds: the zero-import
  prover retained all 19,525 semantic specializations and 10,793 runtime
  functions, emitted 13,905,487 Wasm bytes, executed, and returned a 29,672-byte
  canonical receipt. A separately compiled zero-import verifier retained
  14,720 specializations and 7,859 runtime functions, decoded those copied
  bytes, accepted the receipt, and rejected a mutated canonical header. The
  prover arena ended near 23 MiB, establishing that the prior allocator trap
  was the hard module cap rather than runaway proof allocation.
  This closes the executed production sparse receipt and canonical transport
  boundary for the current one-transition `L = 4` checkpoint. It is not yet a
  succinct recursive proof or a security claim. A subsequent process-isolated
  exact gate executes a 20,413,169-byte zero-import Fe prover and a fresh
  8,703,961-byte zero-import Fe verifier. The verifier accepts the clean
  canonical receipt, then rejects Fe-side typed mutations to the base root and
  transcript chain, an authenticated AIR value, its Merkle sibling, a
  composition opening, and the recursively located terminal FRI evaluation;
  it also rejects a raw canonical validity-word mutation. The host owns only
  receipt bytes and mutation selectors, never field offsets or a receipt
  schema. The complete gate passes 1/1 in 5,401.40 seconds, with the verifier
  child finishing in 708.27 seconds. Focused independent bigint gates remain
  the semantic truth, and claim, domain, sampled-query, and malformed-encoding
  mutations must still execute at this assembled boundary. Commits
  `a3b42c545`, `135daa0ff`, and `4a0e3240d` now derive the
  first explicit production query policy in Fe. The conservative Q16
  direct-domain policy selects 103 queries for a conjectured 100-bit leaf and
  114 queries when reserving a union-bound budget for at most 1,024 composed
  proofs. It fails closed for 2,048 proofs under the present commitment phase.
  The query transcript uses one nominal `FQ02` domain plus a canonical indexed
  field element, independently checked against Plonky3 at indices 0, 1, 99,
  100, 114, 65,537, and `p - 1`. This removes the former decimal-tag and
  99-query ceiling. A compact `FriQueryRangePlan<1, 114>` carries the selected
  plan in O(1) type structure; attempting to normalize the old 114-deep nested
  plan consumed one CPU for five minutes without completing, so large query
  plans no longer rely on that representation.
  Commit `9b47c2b40` also interprets the exact authored sparse composition fold
  as four zerofier-family constraint counts. Its zero-import Fe/Wasm gate
  passes 1/1 in 392.53 seconds, requires every family to be nonempty, requires
  their sum to equal the derived total, and confirms the `L = 4` AIR remains
  within the policy's conservative 8,192-constraint cap. This is structural
  count evidence from the production evaluator, not a replacement for the
  existing independent semantic and mutation oracles.
  Commit `f7aff8f74` now interprets that same evaluator through an
  `AirPolynomialDegree` semiring. Reflection populates every base and
  interaction trace column as a degree-one variable, transcript challenges
  remain degree-zero constants, and the ordinary evaluator sequence derives
  family-specific expression degrees without a copied field list or
  constraint body. The interpreter derives maximum expression degree 19 and,
  after the four family zerofiers, a production composition-degree bound of
  73,709. An independent Rust quotient calculation from the reported family
  degrees agrees exactly. That bound does not fit the claimed 4,096-row
  trace-degree domain. `proof_security` now requires
  a nonzero composition bound strictly below the trace domain and therefore
  fails closed instead of treating the present four-query receipt as a
  low-degree proof. The shape gate passes 1/1 in 82.39 seconds. The focused
  security gate passes 2/2 in 23.73 seconds, including primary-parameter
  validation, recomputation of every derived policy lane, mutations, and an
  exact high-word `u64` canonical-codec round trip. That round trip exposed a
  general Wasm lowering defect where address-taken scalar Slots were read as
  their private i32 pointer carriers. Commit `30303a2dc` loads their typed
  pointees in every value lane; its direct return, arithmetic, call, copy,
  branch, and f32 regression passes 1/1, and the focused lowering unit slice
  passes 12/12.
  This is an honest protocol blocker, not a reason to enlarge the domain or
  weaken the policy. Commit `aca9e388d` adds a reusable Fe-authored quadratic
  arithmetic plan with witness and constraint interpreters. Commit
  `6dcd20a70` uses it for the production product copy-address DAG and lowers the
  shared copy-bus inverse identity to an equivalent quadratic relation on the
  already-constrained boolean selector. The same 17-node product plan now
  drives witness generation and constraint evaluation. Its zero-import Wasm
  oracle rejects every committed node mutation under three independent fold
  challenges. A second independent Rust oracle reconstructs every product row
  and clean product, round, linear, and boundary receipt, rejects inverse
  mutations, and confirms that coordinated mutations survive only until the
  terminal equality check.

  Commit `b4c6f550d` derives component degrees from the exact production
  evaluators. Commit `9cf134e54` adds three semantic coordinate roles and
  removes repeated high-degree coordinate interpolation from the product,
  linear, and boundary copy buses. Commit `25d20d75a` then expresses the nested
  signed-linear finish choice as one seven-multiplication Fe plan interpreted
  into witnesses and quadratic residuals. An independent BabyBear
  reconstruction matches every committed node and rejects all seven node
  mutations under three fold challenges. The production base-LDE checkpoint
  executes the widened 51-field row through zero-import Wasm.

  Commit `eb9224f8f` interprets the complete public-boundary copy expression
  through one shared fourteen-node quadratic plan. Direct port inspection,
  witness generation, constraint evaluation, and degree analysis now consume
  that same Fe-authored expression. The independent BabyBear oracle
  reconstructs every node and requires each single-node mutation to fail
  under the independent challenge set. The exact shape gate confirms that the
  boundary component fell from degree 7 to degree 2. At that checkpoint the
  canonical base row became 65 fields wide; its assembled receipt remained to
  be rerun against the widened schema.

  Commit `421e93e2a` interprets the five rounding ports through one shared
  thirty-eight-node quadratic plan. The independent BabyBear oracle matches
  every node on guard, output-digit, current-sign, round-consumer, and
  finish-consumer rows, then rejects all thirty-eight single-node mutations.
  The production 4,096-row base placement and 8,192-row LDE execute through
  zero-import Wasm with the widened 103-field base row. That exact production
  gate passes in 768.09 seconds.

  Commit `adc203410` interprets all eight signed-linear copy ports through one
  shared fifty-two-node quadratic plan. Witness generation, row/link/terminal
  evaluation, direct port inspection, and degree analysis consume that same
  Fe-authored expression. It reuses the already committed effective-right and
  different-sign arithmetic nodes rather than deriving a second sign-choice
  graph. An independent BabyBear reconstruction matches all fifty-two nodes
  across every semantic signed-linear row class and rejects every single-node
  mutation under five independent fold challenges. That focused gate passes
  in 327.92 seconds. The exact shape gate confirms that the linear all-row
  component fell from degree 7 to degree 2 and that its terminal contribution
  fell from degree 6 to degree 2. The widened 155-field base row then executes
  the full production 4,096-row placement and 8,192-row LDE through
  zero-import 395,467-byte Wasm in 1,019.47 seconds.

  Commit `7001de5b7` linearizes the terminal balance shared by all four copy
  buses. Pair-row constraints already accumulate every preceding row, while
  the independent control terminal and ordinary all-row constraints require
  the final row to be canonical padding with zero port coefficients. The
  terminal therefore checks the stored prefix accumulator directly instead of
  recomputing a quadratic zero delta. A focused zero-import Fe/Wasm gate proves
  all four padding-row deltas are zero and separately mutates the product,
  round, linear, and boundary accumulators; every mutation fails exactly one
  linear terminal equation. It passes in 620.55 seconds. The exact shape gate
  passes in 693.76 seconds and confirms that the last-row family fell from
  degree 2 to degree 1. A broad scalar receipt run independently reproduced the
  product and round receipts before exposing stale pre-plan constraint-count
  expectations; those counts now include every committed plan residual, while
  the complete broad rerun remains part of final receipt revalidation.

  Commit `3d9429b7b` extends the arithmetic plan from its seven-node signed
  finish graph into one shared twenty-three-node row-arithmetic DAG. It covers
  bitness, radix reconstruction, carry reconstruction, convolution products,
  product-sign selection, signed-linear magnitude selection, and the final
  sign choice. The plan shares the radix `state_before * value` product with
  signed-linear selection, expresses difference selection as one affine
  adjustment, and reuses the accumulator bit-square in the finish balance.
  An independent BabyBear reconstruction matches all twenty-three nodes and
  rejects every single-node mutation; that focused gate passes in 116.70
  seconds. The exact structural interpreter passes in 323.25 seconds and
  proves the arithmetic all-row component fell from degree 4 to degree 2. The
  widened 171-field base row then executes full production 4,096-row placement
  and 8,192-row LDE through zero-import 395,467-byte Wasm in 690.37 seconds.

  Commit `b974ed27e` expresses outgoing arithmetic adjacency as a second shared
  eighteen-node DAG. It materializes phase-link selectors, carry reset and
  retained state, product and rounding handoffs, and all three signed-linear
  ripple links. An independent BabyBear oracle scans the actual production
  schedule to prove every node has a nonzero semantic case, matches all nodes,
  and rejects each single-node mutation under five challenges. That gate
  passes in 99.11 seconds. A new pair-component structural report executes the
  exact six production adjacency evaluators independently rather than hiding
  them behind the family maximum. It proves arithmetic and every copy-bus
  adjacency are degree 2, while control adjacency is degree 5. The exact shape
  gate passes in 439.40 seconds. The widened 189-field base row then executes
  the production 4,096-row placement and 8,192-row LDE through zero-import
  395,467-byte Wasm in 658.72 seconds.

  The local-control increment now expresses the three coordinate-domain
  polynomials through one shared three-node quadratic DAG. Witness generation,
  constraint evaluation, degree analysis, and the committed base-row schema
  consume that same Fe plan. The independent BabyBear oracle reconstructs all
  three nodes on every one of the 4,096 production control rows and rejects
  every directed node mutation under three challenges. That focused gate
  passes in 184.92 seconds. The exact shape gate passes in 630.07 seconds and
  proves the final control all-row component fell from degree 3 to degree 2.

  The control-adjacency increment now expresses the remaining deterministic
  transition relation through six shared Fe plan families: 23 phase links, six
  boundary and padding links, 12 range-role links, eight carry links, nine
  product links, and ten signed-linear links. These 68 named nodes are the one
  source consumed by witness generation, constraint evaluation, degree
  interpretation, and the future GPU scheduler. An independent BabyBear model
  matches every node on all 4,095 production links, rejects each directed node
  mutation under three fold challenges, and retains the complete canonical and
  directed schedule-mutation audit. That strengthened zero-import Fe/Wasm gate
  evaluates 1,486,555 constraints and passes in 1,493.11 seconds. The exact
  structural interpreter passes in 2,094.65 seconds and proves that control
  adjacency fell from degree 5 to degree 2.

  The widened schema also received a decomposed transport audit instead of an
  impractical monolithic receipt compile. An independent oracle derives the 17
  product-address nodes that were missing from its stale 84-field interaction
  model, then matches all 5,507 carrier words in the exact Fe-produced base and
  interaction LDE codewords. The corrected nominal widths are 192 base fields
  and 152 interaction fields. That exact codeword gate passes in 1,961.53
  seconds. The production multipath gate generates each opening once, copies
  it in Fe, and applies verifier-side mutations to those copies. This preserves
  the clean and mutation semantics without regenerating the complete prover
  eleven times. Clean authentication passes while mutations of opened values,
  siblings, indices, unused capacity, the dependency-bound root, and requested
  row selection all fail closed. That unoptimized semantic baseline passes in
  2,940.80 seconds. Broad all-export compilation is no longer treated as
  useful evidence: one attempt reached 12.65 GiB after 99 minutes, and the
  older repeated-prover multipath form reached 9.8 GiB after 115 minutes.
  Focused entry lowering and independently composed gates are required for
  this slice. A separate performance fixture must opt into Sonatina's
  optimization pipeline explicitly; Cargo release mode alone does not change
  the default byte-equivalent Fe Wasm lowering.

  The exact production shape is now 691 constraints with family degrees
  `[2, 2, 1, 1]`, all-row component degrees `[2, 2, 2, 2, 2, 2]`, and pair-row
  component degrees `[2, 2, 2, 2, 2, 2]`. The maximum expression degree is 2
  and the composition-degree bound is 4,095, so the authored AIR now fits the
  4,096-row target domain. This closes the degree-reduction gate that began at
  degree 19 and bound 73,709. A fixed preprocessed control root remains
  deliberately rejected: it would add a verification-key and fixed-column
  authentication obligation, while a prover-chosen fixed root would be
  unsound. Direct shared plans keep the authored transition relation, witness,
  constraints, degree analysis, and future GPU schedule structurally
  identical.

  Exact codeword, commitment, opening, and assembled receipt revalidation is
  now the active gate. The focused toy `TRACE = 4`, `LDE = 16` production
  composition gate checks clean output plus base, interaction, and statement
  mutations against independent Plonky3-derived roots, transcript challenges,
  all sixteen composition values, and the final root. It passes 1/1 in 117.48
  seconds. During revalidation it exposed and corrected a stale Rust-oracle
  mutation at column 38; the Fe production fixture and the independent oracle
  now both direct their mutation at base-LDE column 35. The separate typed
  LD01/LD02 multipath gate passes 1/1 in 54.96 seconds, including clean row
  reconstruction and twelve clean, directed, or unknown mutation outcomes.
  The former broad test redundantly regenerated a 4,096-leaf legacy trace for
  each opened row and was stopped after exceeding 6.5 GiB RSS. Its distinct
  LDE and opening semantics remain in focused gates; the production mutation
  gate now consumes the one receipt that already carries all sixteen
  numerators.

  The four-query production regression now binds every interpreted security
  parameter into the transcript before composition. `SparseDirectFriTranscriptProfile`
  contains the derived direct-FRI policy, the exact 691-constraint AIR shape,
  its degree families and composition bound, plus the query count actually
  executed by the receipt. Its reflection-derived 44 canonical words are
  injectively packed into 47 BabyBear fields and committed under `SP01`; the
  canonical statement-and-root AIR transcript is extended under `SP02`.
  Prover and verifier consume a private-field
  `SparseCompositionTranscriptDigest`, so an extended challenge digest cannot
  masquerade as the canonical base AIR transcript. The composition writer
  rederives both codeword roots and requires exact retained-transcript equality
  before evaluating internal or fixed-position public constraints. Independent
  Plonky3 packing/Poseidon tests, policy arithmetic, exact AIR-shape checks,
  and six malformed-profile mutations pass 6/6. The first complete run exposed
  a real stale-challenge bug because composition had been generated before the
  profile extension; the nominal two-level transcript repair closes it. The
  fresh exact production gate emits and accepts the 47,552-byte receipt and
  retains the typed mutation matrix.

  To keep this evidence viable on the 19 GiB machine, the exact gate now uses
  four process legs: compile prover, execute the persisted validated Wasm,
  compile verifier, execute the persisted validated verifier. Compiler arenas
  therefore never overlap Wasmtime JIT or Fe proof state. The first combined
  attempt crossed below the mandated 3 GiB available-memory floor after
  emitting the same prover module and was stopped. The isolated rerun kept the
  artifact and proof semantics identical and passed 1/1 in 1,891.64 seconds.

  The staged canonical boundary no longer traverses the complete receipt once
  to count words before traversing it again to encode them. A rejected
  `CanonicalWordStream::MAX_WORDS` experiment preserved the nominal schema but
  moved the same recursive cost into associated-const analysis; it was not
  retained. `BrowserWordWriter::growing` instead starts with a small
  performance-only region, doubles through Fe-authored allocation and copy
  operations, fails closed at the checked browser-memory ceiling, and exposes
  only the exact emitted prefix. Rust and JavaScript know neither the receipt
  schema nor its capacity. The ordinary reflection-derived stream remains the
  sole field-order authority.

  The release production-codec gate completes diagnostics in 23.71 seconds,
  emits a 915,084-byte zero-import Wasm module in 195.92 seconds, and passes
  clean roundtrip plus invalid-boolean, truncation, and trailing-data rejection
  in 221.22 seconds overall. A separate canonical-words oracle forces 257
  writes through three growth boundaries, checks every copied word, rejects an
  out-of-policy request, and rejects an oversized allocation; it passes in
  58.19 seconds. The same investigation hardened CTFE failures to retain a
  compact body-diagnostic class and the full instantiated callee chain rather
  than an opaque salsa identifier. The 114-query encoder now uses the same
  growing Fe writer, but its full receipt execution remains the next security
  gate.

  The assembled regression receipt still carries four queries. The compact
  114-query range must subsequently drive real sampling, openings,
  authentication paths, and FRI folds before a separate security-sized
  receipt is claimed. The direct-domain policy remains explicitly conjectured
  rather than a DEEP-ALI soundness claim. The Sonatina companions also need a
  published revision and normal Fe dependency pin before this gate is
  reproducible without the local path patch.
- [~] Interpret the BabyBear proof dependency plan through the Conal/CTFE
  WebGPU scheduler for NTT/LDE, AIR composition, Poseidon/Merkle, and FRI.
  The first arithmetic-plan consolidation is landed at `ba2ded864`: one
  `Radix2Field` interpretation boundary now drives the same forward NTT,
  inverse interpolation, and disjoint-coset LDE dependency plan for both the
  multi-limb BN254 field and u32-native BabyBear. The independent direct
  bigint BN254 DFT/LDE gate remains 3/3 green, while the new zero-import
  BabyBear gate checks direct u64 DFT/LDE values, round trips, invalid cosets,
  and u32-only browser SPIR-V. The gate exposed a general semantic defect in
  which generic `Copy` values could be reloaded from mutable source lineage;
  `6cc7f996a` fixes compact and array-containing snapshots and adds executable
  regressions rather than reordering the butterfly around the bug. Commit
  `c5cdca47e` now interprets that exact radix-2 plan over the quartic BabyBear
  challenge field as well. The direct oracle checks every coefficient of NTT
  and coset LDE outputs at multiple seeds and shifts, plus invalid cosets. FRI
  can therefore consume one shared Fe transform body instead of introducing
  a field-specific transform fork.
  The first browser placement checkpoint is now landed in
  `mandelbrot_proof_gpu`. Commits `78b73c0b9`, `7915e22c5`, and `6173d0b3d`
  derive typed compute invocation and repeated dispatch from Fe actor passes.
  Commit `1b894cf17` interprets the four-column LDE and Poseidon2 commitment
  schedule as six Fe-authored GPU passes, including 396 fixed repeated
  commitment steps over 32 lanes. The proof actor derives the canonical 141
  Poseidon2 parameters once on the GPU from an 80-bit Grain state represented
  by three `u32` words, then stores their Montgomery forms in the typed proof
  tape. There is no application constant table or host-side parameter shim.
  The runtime parameter stream and permutation match every independent
  Plonky3 value. Exact LDE and repeated-dispatch execution gates pass on
  llvmpipe, the actor compilation gate passes, and the largest commitment
  shader contracted from 213 KiB to 150 KiB. The complete graph reached
  pipeline compilation on llvmpipe but did not finish within eight minutes,
  so that attempt was stopped and is not execution evidence.
  Sonatina commit `c6659dd1` supplies the structured shared-control-flow fix
  required by the generated proof shaders. With that local backend revision,
  the release compiler and canonical gallery cold build are green: 13 render
  bundles, three component projections, and 92 assets. The browser card is a
  placement and commitment checkpoint, not yet a recursive succinct proof.
  Hardware Chromium then exposed a second general backend defect: every
  invocation materialized the full 8,192-word private-heap capacity even when
  static allocation analysis required only a small prefix. The 32-lane
  commitment kernel therefore requested roughly 1 MiB of private heap per
  workgroup and crashed Chromium's GPU process during pipeline compilation.
  Sonatina commit `03bff3d6` retains the same fail-closed capacity ceiling but
  emits only the statically required heap. Fe commit `85339db39` rejects a
  capacity-sized heap in the proof compile gate. The commitment kernel now
  uses 135 words per lane; the other proof passes use 42, 54, 179, and 178
  words. On AMD Radeon 780M through RADV, Chromium compiled the 149,767-byte
  commitment pipeline in 15.94 seconds with zero WGSL diagnostics and no
  device loss. The cold canonical gallery then loaded all 13 render surfaces
  in sequence. Clean mode rendered all four validity bands as exact RGBA
  `[87, 117, 226, 255]`; mutation mode retained trace, LDE, and nonzero-root
  validity while changing only the expected commitment verdict to exact RGBA
  `[255, 176, 222, 255]`. This is real hardware execution evidence for the
  placement checkpoint only. It does not satisfy authenticated FRI,
  recursion, interactive point selection, Wasm receipt verification, or the
  Mandelbrot verifier executed through the existing revm-in-Wasm rail.
  The first reusable scheduling baseline now consolidates that arithmetic plan
  with the existing Conal vocabulary. `parallel_structure::Dit<k>` is
  definitionally `std::conal::RBin<Pair, k>`, and scalar plus portable WebGPU
  interpreters consume the same `Par`, `Pair`, and `Comp` constructors.
  Compiler-derived N=16 forward and inverse transforms execute on llvmpipe and
  match an independent direct DFT. The production toy checkpoint now consumes
  the portable batched stage-grid interpreter for all four 4-to-16 coset LDEs,
  and its full commitment graph remains exact against the direct-DFT and
  Plonky3 oracles in clean and tampered modes. No larger-domain or fused
  workgroup/shared-memory proof transform has executed. The remaining work is
  production-sized placement and device-tuned interpreters over the shared
  plan, not a fresh NTT implementation.
  Commit `53aa061ea` adds the transcript-derived `MGFC` challenge and first
  factor-2 FRI layer without adding a second prover. The scalar and workgroup
  placements consume the same digest-squeeze and FRI-pair denotations. The
  exact 14-pass graph now executes on llvmpipe and in local Chromium
  SwiftShader, including clean, tampered, and recovered visual receipts. The
  next bounded slice is the complete 16-to-1 fold chain with an authenticated
  terminal value, followed by compact production-query placement.
- [ ] Build and run the complete recursive proof experience in the canonical
  gallery after the BabyBear prover exists. The Fe-authored component lets a
  user click a private high-precision Mandelbrot parameter, optionally drag to
  expand a public disk around it, and prove either survival through a public
  iteration bound or escape by that bound. It schedules leaf chunks
  progressively on WebGPU,
  recursively merges adjacent certified intervals, presents the typed receipt
  plus proof-size and timing evidence, and verifies it in-browser through Fe.
  The circle relation and orbit witness remain inside the same proof rather
  than being checked by host code. A separate attracting-fixed-point mode may
  later prove an invariant disk, contraction bound, and entry of the critical
  orbit, which is meaningful in a way that bare existence of a complex fixed
  point is not. The Chrome gate must exercise region selection, successful
  generation and verification, cancellation and backpressure, deliberate
  receipt mutation rejection, and console and device-loss recovery. Shader
  compilation alone cannot satisfy this gate.
- [x] Define a typed proof encoding and prove malformed-proof and mutation
  rejection in zero-import Fe Wasm against the independent bigint model.
  Browser/native parity for the eventual BabyBear protocol remains part of the
  published-page gate above rather than a claim about this BN254 checkpoint.
- [ ] Run proof submission and verification through structured Fe tasks,
  Worker/MessagePort effects, cancellation, and backpressure.

## External real-GPU handoff

Run these from commit `0d7f3eddd` or later on a Linux host whose printed
adapter is a hardware adapter. Do not set `MB2_ALLOW_GPU_SKIP`; a skip is a
failed campaign gate. `WGPU_BACKEND=vulkan` keeps the path browser-profile
compatible. On a non-Linux host, omit that variable and retain every other
condition.

On Nix, `vulkaninfo` may find a wrapped loader while `wgpu` cannot dynamically
load `libvulkan.so.1`. Make the Vulkan loader's `lib` directory discoverable
through `LD_LIBRARY_PATH` before running these commands. This sandbox required
`/nix/store/7krvb015vp4wq7lj6v3wadjy4q9asc8q-vulkan-loader-1.4.341.0/lib`.
Use `--no-capture` when collecting the final receipt so the adapter name is in
the evidence.

```console
mkdir -p /workspace/tmp /workspace/.sccache
env -u MB2_ALLOW_GPU_SKIP TMPDIR=/workspace/tmp CARGO_INCREMENTAL=0 SCCACHE_DIR=/workspace/.sccache WGPU_BACKEND=vulkan cargo nextest run --release --locked --no-capture -p fe-codegen --test known_color_pass_graph_e2e
env -u MB2_ALLOW_GPU_SKIP TMPDIR=/workspace/tmp CARGO_INCREMENTAL=0 SCCACHE_DIR=/workspace/.sccache WGPU_BACKEND=vulkan cargo nextest run --release --locked --no-capture -p fe-codegen --test rollcall_pass_graph_e2e
env -u MB2_ALLOW_GPU_SKIP TMPDIR=/workspace/tmp CARGO_INCREMENTAL=0 SCCACHE_DIR=/workspace/.sccache WGPU_BACKEND=vulkan cargo nextest run --release --locked --no-capture -p fe-codegen --test precision_fixed_orbit_gpu_oracle
env -u MB2_ALLOW_GPU_SKIP TMPDIR=/workspace/tmp CARGO_INCREMENTAL=0 SCCACHE_DIR=/workspace/.sccache WGPU_BACKEND=vulkan cargo nextest run --release --locked --no-capture -p fe-codegen --test perturbational_mandelbrot_gpu_oracle
```

Acceptance requires four passing test binaries, no `SKIPPED` line, and a
printed adapter that is not SwiftShader, llvmpipe, or another software
fallback. The semantic receipts are:

- Known Color preserves storage bits `0x3f800000` and `0xc0000000`, then paints
  pixel `[0, 0, 128, 255]`.
- Rollcall preserves leaves `[3, 5, 7, 9, 11, 13, 15, 17]`, produces nodes
  `[896599, 12151, 30583, 185, 377, 569, 761, 8]`, leaves its private-memory
  trap at zero, and paints pixel `[95, 166, 5, 255]`.
- `Fixed<8>` matches every packed and exact audit word for all directed orbit
  checkpoints and emits the explicit invalid-reference sentinel for the
  escaping `(1, 1)` case.
- Perturbation matches the independent BigInt `Fixed<8>` classifier, reports
  no false classifications, resolves the escaping-reference overlap with zero
  magenta pixels, and shows magenta only for the four deliberately ambiguous
  boundary samples.

## Immediate burn-down order

1. Completed: add the first post-checkpoint WebGPU proof slice at toy scale. Expose one
   ordinary Fe factor-2 FRI pair denotation from the existing BabyBear proof
   code, make the scalar fold and a portable one-invocation-per-pair placement
   consume it, and extend `mandelbrot_proof_gpu` without hand-authored WGSL.
   Gate the values against the scalar path and an independent field oracle,
   gate a mutation, then execute the exact card through real Chrome. This is
   the next `write -> derive -> prove -> place -> run -> measure` slice.
2. Completed: extend that placement into the complete toy `16 -> 8 -> 4 -> 2 -> 1` FRI
   chain with ordered layer commitments and transcript-derived challenges.
   Keep large buffers device-resident and cross the host boundary only for
   required transcript observations and final typed receipt extraction. Record
   dispatch, readback, live-memory, shader-size, pipeline-compile, and
   device-loss evidence.
3. Completed: compact the policy-sized scalar boundary. Retain the 114-query Fe-derived
   `FriQueryRangePlan` and typed receipt, but replace recursively expanded query
   execution with checked value-level loops over canonical request buffers.
   Re-run the four-process exact prover/verifier/mutation gate, and pin the
   required Sonatina revision only after it finishes.
   This slice is now partial. Wasm trait preflight traverses only dynamic-return
   dependencies, while all policy-sized sample, AIR-request, composition,
   Merkle, and FRI-opening paths borrow their Fe-owned carriers. A second
   schedule interpretation writes the complete `16 -> 8 -> 4 -> 2 -> 1` FRI
   chain and its sparse query layers into caller-owned typed storage. Its
   focused release gate passes 1/1 in 51.49 seconds against the established
   value interpreter, whose values already have independent Rust/Plonky3
   coverage, including invalid-shift rejection. The staged canonical codec
   gate remains green 1/1 in 233.38 seconds. A bounded detailed graph trace
   reached 3,079 runtime instances. Its dominant owners were canonical codec
   and provider operations: `write` 425 times, `encode` 249, `word_count` 164,
   `from_u32` 162, and `encode_stream` 160. Poseidon functions appeared about
   35 times each and FRI-layer functions 12 to 13 times each. Therefore the
   primary multiplication is reflected receipt encoding, not the proof
   recurrence itself.

   The first full exact retry reached a concrete Wasm lowering diagnostic after
   about 34 minutes: an ordinary Fe record containing typed `BrowserPtr`
   handles could not be materialized into compiler-owned aggregate storage.
   In aggregate layout, a `BrowserPtr<T>` field is a memory `RawAddr` branded
   with `T`'s concrete target layout. The backend now preserves that typed word,
   plus the already-admitted whole memory-provider word, as an exact `i32` lane
   in compiler-owned aggregate storage. Untyped raw addresses, object and const
   references, and non-memory transports retain their fail-closed rules. A
   semantic fixture allocates two typed Fe objects, returns their handles in an
   ordinary record, stores and reloads that record through typed memory, then
   uses both preserved handles and checks the resulting values. The complete
   focused typed-allocation and provenance suite passes 14/14 in release mode,
   including the forged-provider rejection.

   A subsequent one-hour exact 114-query retry remained CPU-active in Fe-to-
   Wasm lowering, observed about 4.8 GiB peak RSS with at least 12 GiB system
   memory available, and emitted neither a Wasm artifact nor a new diagnostic
   before the explicit timeout. This is not receipt, security, or proof-runtime
   evidence.

   The first exact retry after landing the Fe-owned growing canonical writer
   reached a definitive backend boundary. In release mode the runtime graph
   completed with 16,811 Fe specializations and 92 constant regions. Portable
   lowering completed all 16,811 functions, but Wasm validation rejected the
   module with `too many locals: locals exceed maximum` at byte offset
   14,182,530. The run took about 32 minutes to reach validation, peaked near
   13.9 GiB RSS, and retained at least 3.4 GiB available system memory. The
   trace exposed aggregate results with 166,444 and 117,196 flattened lanes,
   per-layer invalid multipaths with up to 47,883 lanes, and thirteen distinct
   `write_multi_path_and_root_from_leaves` lowerings, one taking 41.41 seconds.
   This is a representation failure, not a cryptographic or receipt mismatch.
   Preserve the typed receipt and schedule as semantic authority, but move FRI
   layers, Merkle multipaths, and canonical transport behind Fe-owned typed
   memory handles and value-level loops. Gate each compact interpreter against
   the existing typed implementation and its mutation oracles before retrying
   the exact 114-query boundary.

   A measured canonical stage-grid experiment was then rejected. The provider
   derived declaration-order routing from the same Fe schema, and both the
   cumulative-bound and linear typed-route forms passed the independent
   canonical oracle. The linear form passed in release mode after 257.65
   seconds. It nevertheless made exact compilation less tractable: a fair warm
   six-minute trace remained inside root-body lowering for graph instance 1,
   instead of making measurable progress through the former 3,079 instances.
   The experiment was reverted rather than landing semantically correct code
   with pathological compiler behavior. Do not retry that heterogeneous
   routing shape. Preserve the existing canonical Fe codec and measure exact
   runtime-body interning or streamed prepared-MIR retention next. Re-run the
   full four-process 114-query boundary only after a focused gate demonstrates
   lower graph size, wall time, or peak memory without changing codec values.

   The subsequent value-level and arena-owned receipt repair crossed the real
   scalar protocol boundary. The exact Fe prover compiled to valid zero-import
   Wasm, executed in 285.99 seconds, and emitted one canonical 948,808-byte
   receipt. A separately compiled 15,269,897-byte Fe verifier accepted that
   receipt and rejected the complete typed mutation matrix, validity-word
   mutation, truncation, and trailing bytes in 17.73 seconds. Merkle browser
   storage matches both the pure Fe interpreter and an independent Rust tree
   oracle. The caller-owned FRI writer matches its independently checked Fe
   value interpreter. These are receipt and protocol results, but they are not
   yet a completed fresh four-process rerun.

   The first fresh rerun exposed a compiler scaling regression rather than a
   proof mismatch. Its prover reached valid Wasm at 16,392,448 bytes after
   3,869.88 seconds, but late lowering grew to 13.7 GiB RSS. The run was
   interrupted at the campaign's 3 GiB system-availability floor immediately
   after validation and before the child persisted the module. Inspection
   showed that every address-carried Fe value copy had become its own Sonatina
   control-flow loop. This kept Wasm bodies valid and compact but multiplied
   the lowering graph.

   Sonatina commit `a6cb7bc6` therefore adds one generic, overlap-safe
   `Memcopy` IR instruction with precise read/write effects, parser and
   verifier coverage, and direct Wasm `memory.copy` lowering. Its overlap test
   passes. Fe's actual Wasm interpreter emits that instruction for aggregate
   value copies, while the shared shader/SPIR-V interpreter retains the
   checked portable loop. The 8,192-word address-carried value gate proves
   execution and the presence of bulk memory, and the complete typed arena and
   provenance suite passes 17/17 in 20.41 seconds.

   The exact 114-query verifier then compiled and validated at 14,636,637
   bytes in 675.65 seconds, versus 15,269,897 bytes in 1,123.70 seconds before
   bulk memory. That is about a 40 percent wall-time reduction. The module
   contains 783 `memory.copy` operations and accepts the retained real receipt
   while rejecting the full mutation matrix in 41.00 seconds. This proves the
   optimization at the production verifier boundary.

   The final compiler-memory slice consumes prepared Fe MIR bodies as they are
   lowered, retains only their call interfaces, and asks the GNU allocator to
   release completed body spans at bounded intervals. Local Sonatina commit
   `172a3489` adds an owned Wasm backend entry that consumes each Sonatina
   function body after deriving its WAFFLE body, while the borrowed and owned
   paths share validation and final emission. Borrowed-versus-owned bytes,
   export names, validation, instantiation, and mutable execution match in a
   release gate. The overlap-safe `memory.copy` release gate also passes.
   Fe's complete typed allocation and provenance suite passes 17/17 in 14.80
   seconds, and the generated resumable actor continuation gate passes 1/1 in
   9.98 seconds.

   With those changes, the fresh 114-query verifier compiled in 394.80 seconds
   at 6,566,328 KiB sampled peak RSS and retained its exact 14,636,637-byte
   artifact and SHA-256
   `7c3c0842615854dccda6a69d6a6afc2a0028e1a823283d0f6abb80465e821423`.
   The fresh prover compiled and persisted valid Wasm in 2,109.22 seconds,
   rather than the earlier interrupted 3,869.88-second attempt. It emitted
   15,397,939 bytes with SHA-256
   `90dc23f002dac6b80ea595dc1247c90c737ba853e1b89baa216504239d71da06`
   at 13,288,420 KiB sampled peak RSS, leaving 3,443,664 KiB available at the
   lowest sample. Executing that exact artifact produced the canonical
   948,808-byte receipt in 539.32 seconds. The separately compiled verifier
   accepted it and rejected the complete typed mutation matrix in 21.05
   seconds. This closes the scalar four-process 114-query boundary. The two
   local Sonatina commits still require publication and an exact Fe dependency
   pin before the coordinated compiler increment is reproducible from a clean
   checkout.

   The Sonatina refresh was also audited against the actual
   `fe-lang/sonatina` `main` at
   `8e6c99f67cf3f20b9672cab61d8655c2ff33a6a7`, not the stale `micah/main`
   tracking branch. Relative to merge base
   `039a9f530856a7c0097ffc9dad904eed3bf33fe3`, the MB2 worktree has 157 local
   commits and upstream has 40 commits, principally EVM memory placement,
   stackification, pointer escape analysis, and shared optimizer changes. A
   final-tree merge simulation found five textual conflict regions across
   `cfg_edit`, GVN, SCCP simplification, and expression simplification. Refresh
   Sonatina in an isolated worktree after the codec compaction checkpoint, run
   focused Wasm, SPIR-V, and EVM gates there, and move the Fe dependency pin
   only when the exact scalar receipt and browser proof gates remain green. Do
   not blindly rebase the live 157-commit proof substrate.
4. In progress: carry the portable schedule to production-sized NTT/LDE, AIR composition,
   Poseidon/Merkle, FRI folding, and opening extraction. Close typed proof
   regions as each buffer enters the graph, preserve direct-DFT, Plonky3, and
   independent bigint gates, and run each widened stage in Chrome before adding
   another. Add Merkle retention/recomputation and a peak-memory policy before
   mobile-sized execution. The complete 428-column, four-row production toy
   AIR LDE, separate main and auxiliary commitments, packed trace commitments,
   and ordered AIR transcript now pass Chromium/SwiftShader, independent
   direct-DFT, and Plonky3 gates. The complete composition and toy FRI replay,
   production 114-query work topology, and all 114 transcript-derived query
   indices are now exact on software WebGPU. The next production checkpoint
   also derives all 13 FRI round placements in Fe and extracts 2,850 quartic
   evaluation openings plus 15,048 eight-word Merkle siblings from one
   device-resident proof arena. A shared stateful cursor now owns semantic
   placement for scalar, array, four-round, and GPU-storage interpretations.
   The first GPU interpretation exposed a `MemAllocDynamic` aggregate inside
   a runtime loop and failed closed during SPIR-V lowering. The retained
   interpretation instead advances the same cursor through 13 Fe-derived
   repeated dispatches, persisting only physical cursor state between rounds.
   On llvmpipe, independent Rust recurrences and Plonky3 sampling matched every
   round word, query index, evaluation field, sibling digest, padding lane, and
   bounds trap. Release bundle compilation took 135.93 seconds; software-GPU
   execution plus readbacks took 2.49 seconds for 17,898 opening lanes and
   emitted 737,609 WGSL bytes. An immediate warm rerun compiled in 52.35
   seconds and executed plus read back in 21.42 milliseconds, so cold shader
   and pipeline caches dominate the first receipt.

   The following checkpoint compacts those raw openings into the canonical
   ordered multipath directly on WebGPU. One Fe invocation owns each semantic
   FRI round, derives requested leaves from the transcript-selected queries,
   collapses duplicates into 1,227 ascending quartic leaves, and emits the
   bottom-up, left-to-right frontier of 1,988 Poseidon2 digests. A separate
   Rust traversal matches all thirteen validity bits, leaf and sibling counts,
   leaf indices, field words, digest words, zeroed capacity lanes, padded work
   receipts, and bounds traps. Raw and compact outputs intentionally reuse the
   same buffers. Together with a combined metadata and padding buffer, this
   keeps the actor at seven storage resources plus its fail-closed trap. A
   separate-output design required ten storage bindings and was rejected under
   the browser profile; no device limit was raised to accommodate it. The
   release gate compiled 766,074 bytes of Fe-derived WGSL in 76.02 seconds and
   executed raw extraction, in-place compaction, and all software-GPU readbacks
   on llvmpipe in 417.93 milliseconds.

   The next producer checkpoint removes synthetic Merkle nodes from that
   arena. `fri_commitment_webgpu` interprets all 8,191 retained quartic FRI
   evaluations as domain-tagged Poseidon2 leaves, then builds the thirteen
   fixed left/right trees level by level. The shared staged Poseidon machine
   now accepts caller-owned storage regions, so leaf and parent placement reuse
   one permutation denotation rather than copying its rounds into the query
   actor. The actor remains at seven resident storage resources plus its
   fail-closed trap. Its four additional pass roles and their dispatch counts
   are derived from the FRI schedule sizes: 2,048 leaf workgroups for 44
   permutation steps and 1,024 parent workgroups for 540 level-ordered steps.
   The release llvmpipe gate seeds only the evaluations and compares every one
   of 16,369 emitted node digests and validity bits against an independently
   constructed Plonky3 tree. All 114 sampled queries, raw openings, compact
   prefixes, padding receipts, and bounds traps remain exact. Compilation of
   the nine compute shaders took 281.24 seconds and emitted 1,479,842 WGSL
   bytes; execution plus all readbacks took 1.935 seconds on llvmpipe.

   The arena still receives synthetic folded evaluations at this boundary.
   The gate now proves real device-resident Poseidon/Merkle production plus
   selection and canonical compaction, but it does not yet prove that those
   evaluations came from the production composition and FRI fold chain. Next
   connect those producers, then encode the compact prefixes into the canonical
   receipt without introducing a host-side query table. The shader size and
   cold compilation time make semantics-preserving pass fusion through the
   typed factor/workgroup interpreter an immediate practicality slice. Physical
   Radeon execution remains a separate required gate.

   The production FRI producer now interprets `FriSchedule<1, 13>` as one
   Fe-owned actor cycle over an 8,192-value quartic BabyBear codeword. It
   derives every transcript challenge, factor-2 fold, domain-tagged Poseidon2
   leaf, ordered Merkle parent, layer root, and successor transcript through
   thirteen rounds, ending at one retained value. `CycledDispatch` preserves
   the semantic phase body, while the new generic `TaperedDispatch` placement
   halves active workgroups with each round and contracts the retained tree
   work by the exact per-level step count. The fixed host reads only this
   compiler-derived placement metadata. It contains no FRI round table,
   challenge code, tree cursor, or proof-specific scheduling branch. The
   physical work count falls from 10,796,045 padded workgroups to 605,295,
   a 17.84-fold contraction, without changing the staged Fe kernels.

   The focused release structural gate compiled all fourteen passes, parsed
   and Naga-validated every browser WGSL module, checked the exact taper
   metadata and work count, and passed 1/1 in 332.78 seconds. `fe web dev`
   emitted 2,195,973 WGSL bytes and zero Wasm bytes for the focused page. A
   separate generic three-cycle taper gate executed on llvmpipe without a skip
   and returned exact storage receipt `[3, 111223]`. A
   clean external Linux Chrome 149 WebGPU process executed the complete graph
   to `ready` in `webgpu` mode and painted exact RGBA `[0, 255, 0, 255]`
   without device loss. The observed warm browser run was approximately seven
   seconds, but monopolized the machine's GPU during that interval. This green
   receipt proves all actor-owned schedule, canonical-field, challenge, folded
   evaluation, Merkle-root, transcript, and final-round validity channels. It
   is not yet the independent Plonky3 buffer comparison: the focused actor
   still seeds a deterministic synthetic composition codeword and exposes only
   its completion color. Next add generic cycle backpressure, read back and
   compare the challenges, roots, transcripts, and folded values against the
   independent recurrence, then attach the real production composition and
   opening/receipt stages.

   The generic backpressure slice is now implemented and executed. Fe's
   nominal `CooperativeDispatch<Dispatch, REPEAT_BATCH>` policy selects a
   physical queue boundary without changing the nested dispatch, actor cycle,
   or dependency graph. The compiler transports only the derived batch size;
   the fixed browser adapter has no FRI name, round table, or proof predicate.
   It submits at most that many repeated dispatches, waits for the exact queue
   prefix to become idle, and resumes the serialized presentation. The
   production actor applies batch size eight only to `hash_leaves` and
   `reduce_tree`, yielding 522 queue-drain boundaries while retaining all
   605,295 workgroups and 2,195,973 WGSL bytes. The fixed runtime passed 33/33
   policy tests, including an exact `2 + 2 + 1` batch trace followed by its
   successor stage and two concurrent frame requests retaining their complete
   ordered state snapshots. The generic three-cycle llvmpipe gate retained exact
   storage receipt `[3, 111223]`, and the production structural gate passed
   1/1 in 306.83 seconds.

   Fresh external Chrome 149 runs reached `ready` in `webgpu` mode at 10.13
   and 12.47 seconds and painted exact RGBA `[0, 255, 0, 255]` without a
   console error or device loss. Browser-only instrumentation on the latter
   run observed exactly 537 command-buffer submissions and 522 queue-idle
   waits. This proves that the Fe-selected pacing is being consumed, but does
   not yet prove that batch size eight eliminates the original whole-machine
   stall: the run still observed a 3.37-second main-thread tick gap and a
   3.71-second maximum queue wait under a cold, instrumented load. Keep batch
   size tuning and user-observed Radeon responsiveness open rather than
   trading away exactness or claiming smoothness from the green pixel.

   The independent production-buffer gate is now closed, and it materially
   corrected the meaning of that green pixel. A generic CDP observer, which
   knows only caller-selected compiler-declared `U32` resource names, copied
   the complete `round_placements`, `arena`, and `control` buffers from an
   external Chrome 149 WebGPU run. The first raw comparison found that the
   deterministic composition codeword was exact but the first nontrivial FRI
   fold differed from an independently implemented Plonky3 recurrence. The
   browser actor had reached its own completion state while almost every
   nonzero exponent used a zero two-adic root. This is precisely why actor
   completion is retained as a liveness signal, never an exactness oracle.

   A minimized ordinary Fe root-power expression localized the defect to
   Sonatina's structured SPIR-V loop emission. When a loop header phi had
   several predecessors outside the loop, the emitter selected one as a
   synthetic preheader and unconditionally replayed its edge after the
   preceding structured branch. That overwrote the path-specific value emitted
   by a sibling branch that exited another loop into the same header. The fix
   emits conventional preheader initialization only when there is exactly one
   outside predecessor. With several, the preceding structured arms retain
   ownership of their exact edges. A focused pure-lowering regression models
   the direct-edge plus sibling-loop-exit topology and Naga-validates the
   resulting WGSL. It passes in both debug and release. The BabyBear oracle
   also exercises dynamic log-13 root powers for exponents `0`, `1`, `2`, `3`,
   `17`, and `4095` through zero-import Wasm and browser SPIR-V; its complete
   release suite passes 6/6.

   A fresh-cache rebuild with the corrected compiler completed in 304.36
   seconds and emitted fourteen passes, 2,193,827 WGSL bytes, and zero Wasm
   bytes. The external Chrome run completed in `webgpu` mode with no console
   error or device loss. Its immutable raw receipt contains 169 placement
   words, 421,759 arena words, and 311 control words, totaling 1,688,956 bytes.
   Their SHA-256 digests are respectively
   `d459a6f4bc85ec2906b17c38efb7b2b87e67fb11ed3fff2f292a5c0da690c5a0`,
   `85d6a73c95f4e9eac3cc5a3a74c1ed2d11f42da27f45b7526919d997d93a84ed`,
   and
   `d9aaa882cccc0dac84ba0ae364f7c0f8ac2b13945fea0b224900983e25a0ca37`.
   The ignored release gate
   `production_fri_browser_buffers_match_independent_plonky3_recurrence`
   then passed 1/1. It compares all 169 derived placement words, 32,768
   composition words, 32,764 folded-evaluation words, 8,191 fold-validity
   words, 130,952 ordered Merkle words, 16,369 node-validity words, and all
   311 challenge, root, transcript, and completion-control words. This is the
   first independent exactness receipt for the complete production-sized
   thirteen-round browser FRI actor. Its input remains a deterministic
   synthetic composition codeword, so connecting the production AIR
   composition and authenticated opening/receipt stages remains the next
   protocol boundary.

   The first complete 49-pass replay exposed a compiler semantic defect before
   this gate could be claimed: Runtime MIR aggregate facts retained the first
   structural value of a reassigned local, so an inlined quartic constraint
   checkpoint could return its initial accumulator even though the authored Fe
   helper had advanced it. Commit `4393a076b` snapshots propagated facts,
   invalidates the reassigned fact and its transitive aggregate dependents, and
   gates a wide mutable helper return on llvmpipe. Commit `16c89e589` separately
   makes the value-based projection and liveness specializer fail closed for
   repeated destinations instead of applying an SSA assumption to mutable
   Runtime MIR. The focused real 78-constraint AIR helper then returned all
   four independently derived `challenge^78` extension lanes on WebGPU.
   With both guards enabled, the exact 49-pass graph compiled about 18 MB of
   Fe-derived WGSL in 1,946.53 seconds, compiled every llvmpipe pipeline in
   209.69 seconds, executed the clean graph in 301.82 seconds, and executed the
   retained tamper case in 6.51 seconds. Every 428-column AIR LDE value,
   composition lane, commitment root, transcript state, FRI layer, and query
   opening matched the independent direct-DFT and Plonky3 models. The complete
   release gate passed 1/1 in 2,465.82 seconds without a skip or device loss.
   This closes the local software-WebGPU composition replay. Physical Radeon
   execution and performance-oriented kernel fusion remain separate gates.

   The focused production FRI actor now carries its own complete 114-query
   opening boundary after the thirteen exact fold-and-commit rounds. Four
   Fe-authored passes squeeze the `FQ02` Fiat-Shamir queries from the terminal
   transcript, walk the typed FRI placement cursor across all rounds, extract
   2,850 quartic evaluation candidates and 15,048 eight-field sibling
   candidates, then compact duplicates into canonical ordered multipaths. The
   temporary compaction activity is no longer stored in the immutable proof
   arena. A distinct activity region and the canonical metadata occupy one
   typed tail of the opening workspace, keeping the actor at seven declared
   resources plus the fail-closed trap and therefore within the portable
   eight-storage-binding minimum. No proof-specific query or round table moved
   into the browser host.

   The independent Rust model separately derives every indexed Poseidon2
   squeeze, stateful evaluation and sibling cursor, activity bit, compact leaf,
   sibling frontier, zeroed capacity lane, padding receipt, validity bit, and
   metadata word. Its focused release geometry gate passes 1/1 in 0.04 seconds.
   The release structural gate compiled and Naga-validated all eighteen WGSL
   passes, checked their exact repeats, tapers, cooperation, resource lengths,
   and binding counts, and passed 1/1 in 339.11 seconds. A fresh focused web
   build emitted 2,846,543 WGSL bytes, zero Wasm bytes, and completed lowering
   in 388.60 seconds and the full build in 438.61 seconds. Authoritative Chrome
   exactness remains open: the external Chrome process lost its WebGPU instance
   during the first readback attempt, and its same-origin `requestAdapter()`
   subsequently returned null even after closing all proof tabs. Relaunch the
   external browser, execute exactly one focused proof page, and compare all
   seven buffers before attaching the production AIR composition input or
   claiming this opening actor on physical hardware.

5. G-RECEIPT is closed at the scalar 114-query boundary. Continue through
   G-RECURSE and G-BROWSER. The security-sized verifier is now bound to the
   private leaf authority, but the current recursive carrier and
   verified-adjacent-interval authority remain semantic scaffolding, not a
   recursive cryptographic proof. The fixed-size merge constraint relation is
   now derived and independently gated. The production Poseidon2 `x^7` S-box
   also shares one four-multiplication plan across scalar evaluation, witness
   generation, and quadratic constraints. The complete 21-round permutation
   now consumes that plan as one 564-node sequential relation. Commit
   `6079e80cd` streams those quadratic rows through typed browser memory while
   retaining the fixed witness and Plonky3 value oracles. Commit `491c47dc2`
   derives a nominal round, lane, and power task for every product, using
   fieldless Fe enums rather than a parallel integer phase table, and binds the
   four rows of every S-box with all seven internal copy edges. The independent
   schedule gate checks all 564 tasks plus invalid indices. A stronger mutation
   gate coherently rewires each of the 141 S-boxes so that every changed row
   remains locally quadratic and all stored output assertions remain zero; the
   copy topology rejects all 141 rewires. The complete zero-import release
   oracle passes 6/6 in 54.41 seconds, including the independent Plonky3
   parameters and permutation. At that checkpoint only the internal power
   topology was authenticated. Commit `39494ceb6` adds a generic streamed
   constraint interpreter that re-executes any authored quadratic relation
   against its untrusted memory-backed rows. It checks stored operand copies,
   local products, stored assertion copies, live assertions, and exact shape
   without maintaining a second wiring program. Its independent tiny-relation
   gate distinguishes clean execution, broken products, coherent rewires,
   changed stored assertions, changed claims, and incomplete streams; the
   focused release gate passes 1/1 in 11.09 seconds. Commit `9e9c406dd` applies
   that interpreter to the original Poseidon compression relation and deletes
   the manual seven-edge checker. The mutation gate now rewires all four rows
   of each S-box coherently, retaining valid local products and valid internal
   power copies. Re-interpreting the original Fe denotation rejects all 141
   rewires through its round-state, derived-constant, external/internal linear,
   initial-input, and final-output dependencies. The complete zero-import
   Poseidon release oracle passes 6/6 in 83.01 seconds with Plonky3 unchanged.
   This closes semantic topology authentication for one streamed compression;
   it does not yet make those rows an authenticated STARK trace or a recursive
   proof receipt. Commit `0a08d0d24` next applies the same stream-and-replay
   vocabulary to the complete recursive merge relation: 423 products and 583
   assertions are written incrementally to typed browser memory, then the sole
   authored merge denotation re-authenticates product operands, range bits,
   carries, public digests, interval order, leaf-count addition, and assertions.
   The fixed aggregate oracle and streamed oracle intentionally execute through
   separate zero-import exports. Keeping both representations live in one call
   first trapped `fe_cabi_alloc`; separating them preserves the old independent
   gate while proving the bounded-memory placement on its own terms. The final
   release gate passes 1/1 in 11.57 seconds, retains the complete integer and
   modular-wrap mutation matrix, and adds a coherent operand rewire whose local
   product remains valid but relation replay rejects. This authenticates the
   merge relation stream, not either child verifier or a recursive receipt.
   Commit `47bb98142` now gives the production security verifier the same
   interpretation boundary. Its 120-task plan derives six shared stages and
   114 transcript-query checks from the typed policy. Scalar evaluation, typed
   memory placement, and replay all consume the same authored denotation. The
   focused zero-import task gate passes 1/1 in 5.29 seconds. A final
   15,265,515-byte production verifier with SHA-256
   `6f23f402a8779f34b20d8cac46f2d0f4f25e8fc268f096d76bd7703eb0a493b4`
   compiled in about 7 minutes 35 seconds, accepted the retained canonical
   receipt whose SHA-256 is
   `c789a067f63b4ab73d8a4c0b36932e4252b6270b0be3e17cc5d5c27980be3ceb`,
   and rejected the complete typed mutation matrix, validity corruption,
   truncation, and trailing bytes. A fresh full four-process rerun was stopped
   during the unchanged prover compilation at 2.4 GiB available system memory,
   below the campaign's 3 GiB safety floor; it produced no proof diagnostic and
   is not counted as a failed proof gate. The exact trace interpreter has not
   yet run over that receipt, so this checkpoint proves scalar semantic
   preservation plus the independent trace structure, not their conjunction.
   Next execute both actual security child verifiers into typed traces, bind
   their stage internals into authenticated relations, derive
   staged workgroup placement from the same dependency vocabulary, and emit the
   leaf and parent proof receipts. Schedule independent leaves and sibling
   merges as a typed balanced reduction, and run the progressive point/disc
   picker, cancellation, generation, mutation rejection, Fe-Wasm verification,
   and revm-Wasm verification in Chrome.
6. Finish the four standalone hardware tests in the external runbook. The
   `mandelbrot_proof_gpu` clean and tampered Chromium modes already pass on AMD
   Radeon 780M through RADV, but the named Rollcall, fixed precision,
   perturbation, and known-color binaries still require no-skip hardware
   receipts.
7. With multiple nominal child namespaces landed, generalize the render-owned
   DEC path to rich canonical port payloads and nested scopes, then execute it
   in real Chromium.
8. Close G-INSPECT, delete the runtime manifest, and finish the legacy
   disposition.
9. Run the exact G5 command once at the final DONE gate.

## Bonus compiler leverage, subordinate to the proof path

These are bounded side-goals, not new campaign gates. Take them when a focused
increment directly reduces a measured proof or gallery compiler cost. Do not
delay the exact scalar receipt, production WebGPU placement, recursion, or the
browser proof flow merely to complete this list.

1. Compact FCO-derived canonical codecs. Derive one typed receipt schema or
   stage grid, then interpret it with small value-level loops instead of
   monomorphizing a distinct `write`, `encode`, `encode_stream`, and
   `word_count` call chain for every reflected field. Preserve the authored Fe
   schema and every independent decode and mutation oracle.
2. Intern exact runtime bodies after substitutions and runtime
   representations are resolved. Sharing requires identical signatures,
   constants, effects, layouts, and callee bindings. Source bytes or generated
   bytes are not a correctness criterion.
3. Stream prepared MIR through dependency-ordered lowering and release bodies
   after their escape, ABI, and call-graph obligations are discharged. Record
   peak RSS as well as wall time.
4. Persist normalized semantic bodies, prepared runtime bodies, and final
   artifacts across compiler processes under compiler-version and semantic
   digests. A cache hit must be observationally identical to a clean build and
   retain the same independent gates.
5. Keep repeated dimensions such as FRI query count as checked data in one
   derived schedule where semantics permit it, rather than multiplying nominal
   programs. Transcript order, domain separation, receipt layout, and typed
   bounds remain fixed.
6. Parallelize independent body lowering only after compaction and under an
   explicit RAM budget. Faster duplication is not the objective; smaller exact
   compilation is.

7. Integrate first-class pointers as a staged semantic transplant from Sean's
   pointer work, not as a mechanical branch rebase and not as a new proof gate.
   The order is language syntax and types, borrow and provenance rules,
   pointer-bearing products and storage-escape rejection, complete pointee
   identity in runtime MIR, Wasm's exact `i32` carrier, then migration of
   proof-critical and browser carriers. Preserve MB2 runtime-control effects,
   FCO normalization, arena rewind and suspension checks, and independent
   receipt gates. The existing typed `BrowserPtr` aggregate fix is a
   compatibility oracle. Completion means the equivalent first-class pointer
   cases pass and the forgeable `BrowserPtr` and `MemPtr` wrappers can be
   deleted, not merely wrapped again.
8. Decide the durable GPU compiler boundary from measured artifacts rather
   than extending the Wasm-shaped shader path by inertia. The Riffcat semantic
   addressing audit is
   `/workspace/scratch/riffcat-semantic-addressing-report-2026-09-02.md`; the
   evidence-complete review bundle is
   `/workspace/scratch/mb2-gpu-backend-boundary-pro-consultation-2026-09-02.tar.gz`.
   Exact merging contracts the production round from 6,934 to 3,179 Sonatina
   instructions. Preserving every lowerable resource helper then expands it to
   8,913 root instructions, 63,288 Naga expressions, and 2,063,940 WGSL bytes.
   Retaining only helpers that perform real resource access recovers the full
   regression at 7,276 instructions, 51,198 expressions, and 1,600,494 bytes,
   with a focused callable-accessor versus inlined-cursor regression. Compare
   three boundaries before deepening the arena ABI: current Sonatina
   hardening, native typed Fe-to-Naga lowering, and a minimal target-neutral
   typed layer using only checked-out public code. The decision experiment
   must use the same resource fixture, one balanced arena helper, Naga
   validation, browser execution, independent value oracles, size,
   compile-time, and peak-memory measurements. An interim Fe-side graph must
   carry an explicit consolidation and deletion gate so it cannot ossify into
   a parallel language backend. The first external review sharpens the next
   slice: instrument rooted inlining by source helper and report call
   multiplicity plus cloned-instruction survival after every cleanup pass.
   Sonatina commit `516d2461` adds that observation-only census, disabled by
   default. On the production round kernel, 13,029 cloned instructions across
   663 call sites become 7,260 survivors after final cleanup. The four
   non-scalar balanced candidates account for only 1,749 survivors in total:
   `sparse_range_roles_from_node` has 840 survivors across six call sites,
   `sparse_control_row_from_task` has 618 across two, and the two single-call
   candidates have 291 together. Even deleting all 1,749 at zero replacement
   cost cannot supply the approximately 2,730-instruction reduction needed to
   cross the one-megabyte gate. Backend-only balanced arena admission is
   therefore falsified as the primary unblocker. Add a bounded GPU
   materialization mode on the existing RMIR-to-Sonatina path so statically
   sized private locals remain typed aggregates and Naga locals, while only
   genuinely dynamic allocation uses the byte arena. Do not begin with broad
   control-provenance plumbing. The complete census is
   `/workspace/scratch/mb2-round-interaction-inline-census-2026-09-02.log`.
   A focused physical-browser site materialized the exact 1,600,494-byte
   compute file, whose root private arena is 717 u32 words, without changing
   either shader digest. External Chrome then lost its WebGPU instance during
   poster readback and the same process subsequently returned no adapter for
   a one-word control. Host logs confirm three GPU-process exits and also say
   that Wayland Ozone is incompatible with the selected Vulkan path. Relaunch
   Chrome with a compatible Ozone/Vulkan combination, run the control first,
   then run the immutable production artifact before attributing the loss to
   shader size or private-memory pressure.

   The September 3 follow-through closed that uncertainty for the focused
   production round-interaction stage. Sonatina resource specialization and
   Fe's typed-private materialization first made the stage callable. A
   conservative compiler lifetime proof now coalesces equal-layout typed
   private storage only when the complete component intervals are
   non-overlapping in one basic block; its independent regression observes one
   allocation, no byte-arena fallback, and validated SPIR-V. More importantly,
   the Fe stage now reinterprets the 52-node linear and 23-node round plans
   already committed in the base trace instead of regenerating witness and
   constraint plans per invocation. The focused linear shader fell from
   2,760,032 to 111,254 WGSL bytes, a 96.0 percent reduction, while the focused
   round compute shader is 91,675 bytes. The complete 41-pass production graph
   compiled and Naga-validated in 370.48 seconds. The independent copy-bus
   oracle and its mutation cases passed separately in 220.15 seconds.

   A fresh single-surface site then advanced physical Chrome 149 through
   explicit health, compile, one-workgroup, full-grid, readback, and surface
   gates on an AMD RDNA 3 adapter. The one-word control returned 42. The exact
   91,675-byte compute shader produced no compilation message, scoped error,
   uncaptured error, or device loss. Module creation took 10.5 ms, pipeline
   creation 25.8 ms, one 64-invocation workgroup 8.3 ms, and the warm 64-group
   grid over 4,096 rows 3.8 ms. A separate full dispatch plus four-byte
   readback took 3.5 ms plus 0.8 ms. The Fe render runtime reached ready in
   1.18 seconds, emitted one frame, and read both declared resources without
   loss. Those buffers were intentionally zero-initialized, so this closes the
   focused physical execution and transport risk, not numerical exactness for
   the complete 41-pass proof graph. Fe commits `db05ecc9e` and `dd2f73554`
   contain the compiler and proof-stage checkpoints respectively.

The Definition of done is not yet met. In particular, complete-proof real-GPU
exactness, manifest deletion, Worker/DEC general messaging, complete legacy
disposition, and the bounded recursive proof remain open.
