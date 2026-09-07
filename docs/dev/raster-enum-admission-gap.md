# Closed enum results and raster admission

September 7, 2026. Reproduced on mb2 `8b9a43050` with Sonatina `8cdf4e9`.
This is a diagnosis and regression baseline, not a fix or a supported workaround.

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
