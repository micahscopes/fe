# Closed enum results and raster admission

September 7, 2026. Reproduced on mb2 `8b9a43050` with Sonatina `8cdf4e9`.
This is a diagnosis and regression baseline, not a fix or a supported workaround.

## Correction candidate

Sonatina `9ff018641e5a9385010bd9a5684a9762663d07c9` adds conservative integer
return summaries to the existing range analysis. Fe invokes this before raster
helper admission. Unknown/external results remain unrestricted; calls and their
effects are retained. Four bounded summary rounds limit optional precision work.

The updated test is
`raster_enum_match_preserves_closed_return_domain_in_root_and_helper` and requires
both placements to compile. It passed in 7.48 seconds. Both post-lowering
captures under `/laboratory/quilting/scratch/composite-enum-fixed-ir-20260907/`
have zero `unreachable` instructions while retaining calls to `checked_slot`.
Sonatina's selected range suite passed 28 tests, including unknown/external and
out-of-domain results retaining their traps. These are compiler gates; the actual
composition renderer and browser seam behavior still require verification.

The full six-test authored-raster suite yielded five passes. Its native GPU
execution gate failed to acquire an adapter (`Backends(0x0)`), not during shader
validation or execution. No GPU-skip flag was used. Evidence:
`/laboratory/quilting/scratch/composite-raster-suite-20260907.log`. Chrome WebGPU
execution remains a required, separate gate.

Follow-up: the same native GPU test binary passed on AMD Radeon780M/RADV in
4.05s with the existing Vulkan loader exposed through process-local
`LD_LIBRARY_PATH` and the existing Radeon manifest through `VK_DRIVER_FILES`.
No global environment or GPU-skip policy was changed. Evidence:
`/laboratory/quilting/scratch/composite-native-gpu-loader-20260907.log`.
The real composition renderer also compiled and rendered in Chrome; its partial
browser receipt is `/laboratory/quilting-fe/docs/composite-spacing-browser-receipt-20260907.md`.

The historical baseline and reproduction below describe the pre-fix state.

The Quilting composition renderer's density-map API uses a checked shared-edge
slot (`Option<u32>`). Its vertex lowering fails with "vertex body uses
allocation/private-memory/trap operations that have no raster-stage channel".
The captured final vertex IR contains 18 `unreachable` instructions. A retained
`square_slot` helper constructs only Some/None, but its flattened i32 tag is
treated as unconstrained by callers. The exhaustive match retains an invalid-tag
trap. The accompanying variant-payload assertion also introduces a guard.

## Small reproducible distinction

`crates/codegen/tests/authored_raster_e2e.rs` contains
`raster_enum_match_admission_documents_root_helper_gap`. It compiles the same
checked-slot function and exhaustive match in two placements:

| Placement | Current result |
| --- | --- |
| Match in a retained helper | Compiles; captured helper still has an invalid-tag trap |
| Match directly in the vertex body | Rejected for the raster trap-channel limitation |

The test deliberately records the current asymmetry. Passing it does NOT mean
enum-domain propagation is fixed, nor prove safe execution of arbitrary trapping
helpers. Replace the root rejection expectation with successful compilation and
execution checks when the actual correction lands. Keep real invalid inputs and
genuine arithmetic traps covered independently.

Run in release mode:

```sh
cargo test --release -p fe-codegen --features spirv-backend \
  --test authored_raster_e2e raster_enum_match_admission_documents_root_helper_gap
```

Both baseline cases passed in 7.55 seconds (compilation-test time, not rendering
performance). No application or compiler behavior was changed by this test.

## Evidence and intervention limits

Local captures live under `/laboratory/quilting/scratch/`:

- `composite-spacing-ir-20260907/0002-vertices+shade-post.sona`: actual renderer.
- `composite-enum-repro-ir-20260907/`: helper-contained reduced variant.
- `composite-enum-root-regression-20260907.log`: desired-success test fails with
  the same raster admission message, after eliminating a fixture syntax error.
- `composite-enum-admission-baseline-20260907.log`: two-placement baseline.
- `composite-enum-inline-proof-20260907/`: observation events for attempted
  forced inlining; helper selection was never reached because root admission
  failed first. This is NOT evidence that forced inlining fixes or fails to fix
  the generated code.

The full-site forced-inline attempt was also inconclusive: the intervention
names `square_slot`, which does not exist in the site's earlier compute stage.
The diagnostic API currently has no per-stage selector for that intervention.
This is a concrete observation-tool improvement opportunity, not justification
for filtering resources or changing algorithms in JavaScript.

## Correction requirements

Investigate closed return-domain propagation across helper calls before raster
eligibility analysis. Preserve representation and genuine validation boundaries;
do not drop invalid-tag checks merely because an operation is spelled `match`,
and do not inline every helper or route the application through a helper just to
evade admission. The relevant paths are `shader_driver::normalize_spirv_helper_graph`,
Sonatina's contextual raster admission/range analysis, and portable lowering of
`MatchEnumTag` / `EnumAssertVariant`.

Acceptance must include both match placements, multiple variants and payloads,
unknown/external tag cases, real traps, and the actual Quilting spacing renderer.
Its Fe API, canonical shared edge maps and coherent scene publication should
remain intact. Browser seam and interaction verification follows successful
lowering; the last-good browser bundle currently lacks the new spacing feature.
