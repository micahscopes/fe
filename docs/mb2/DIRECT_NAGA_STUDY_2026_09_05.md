# Bounded direct RMIR to Naga study

Conclusion: inconclusive for production. Neither adopt nor permanently reject
the direct route. Continue consolidation of the existing Sonatina boundary.
This report closes the disposable experiment, not the compiler cleanup goal.

The experiment used real Fe RMIR, typed private records and borrows, retained
acyclic helper functions, and a separate straight-line resource-identity case.
It did not parse debug text or fall back to Sonatina for unsupported operations.
Its maximum engineering window was September 5, 21:15-23:15 UTC. It stopped
early rather than adding an optimizer or broader control-flow implementation.

## Executed evidence

Chrome ran the exact emitted WGSL on AMD RDNA3 in isolated test pages. These
runs reported no validation errors or device loss. They are small execution
oracles, not full Mandelbrot proofs or controlled performance benchmarks.

| Case | WGSL bytes | Result |
| --- | ---: | --- |
| Direct checked typed storage, initial | 6,776 | Eight result/trap pairs pass |
| Direct checked typed storage, trap transport pruned | 6,665 | Same eight pairs pass, including three overflows |
| Production checked-source storage | 1,941 | Five non-overflow inputs pass; no overflow trap channel |
| Direct explicit wrapping storage, initial | 7,405 | Eight wrapping cases pass |
| Direct explicit wrapping storage, trap transport pruned | 5,960 | Same eight wrapping cases pass |
| Production explicit wrapping storage | 2,211 | Same eight wrapping results pass |
| Direct two-resource identity | 502 | Four distinct/swapped/equal/overflow cases pass |

A valid-WGSL wrong-index mutation of the resource shader fails all four numeric
cases. Resource helpers were expanded, not retained under a general resource
calling convention. Indices were fixed and in bounds. Loops, general aliasing,
bytewise operations and arbitrary pointer transport were outside the subset.

The checked-source comparison exposed a real semantic difference: ordinary
`IntrinsicArith.checked` is ignored in the legacy portable lowering, except
protected narrowed-usize operations. The scratch emitter checks u32 addition.
Absent checks cannot be credited as optimization. Explicit `WrappingAdd`
aligned numeric semantics, but physical entry layouts still differ.

Removing unnecessary trap parameters and post-call guards reduced the scratch
wrapping output by 1,445 bytes (19.5%). This is not a production improvement.
The scratch emitter still retains unoptimized local slots and a test-only
array transport. Its smaller engineering footprint is not a maintainability
comparison with a backend supporting the whole language.

## Riffcat and provenance

Artifact-verified captures under `/workspace/scratch/`:

- `mb2-study-scalar-20260905.capture.json`: 59 initial module instructions,
  52 final, 1,941 WGSL bytes, 2,424 SPIR-V bytes.
- `mb2-study-wrapping-20260905.capture.json`: 62 initial module instructions,
  54 final, 2,211 WGSL bytes, 2,572 SPIR-V bytes. Capture ID:
  `ef504dab395aa4927a4c54e2dcbdb73d34b0eec2f9f9ffee82cab32953dcbf17`.

The baseline executable was built from Fe `9a6bcf758` plus the recorded overlay
`mb2-study-baseline-source-overlay-20260905.patch`, SHA256
`2564d6034371b14173e0e0844f3b8d7f704cc89aff25488cf22a764270036942`,
and Sonatina `1d206a520e34090e06744ded33e9dd9e9539df11`.
Do not relabel it as a clean build of the later documentation commits.
Wrapping fixture SHA256:
`bef59dcaf15ff251fe180e6bf2d81d9ca02efdd07517e475d6fc7db9f880eab5`.

Browser logs under `/workspace/scratch/`:

- `mb2-direct-naga-resource-chrome-20260905.log`
- `mb2-direct-naga-resource-negative-20260905.log`
- `mb2-wrapping-baseline-browser-20260905.log`
- `mb2-wrapping-pruned-direct-browser-20260905.log`
- `mb2-checked-pruned-direct-browser-20260905.log`

The existing recorder gap for scalar/grid entry points was fixed in production
as `5cfee3e1e`. Observation-on/off and partial/strict-budget release regressions
passed, as did the explicit compute/fragment regression. Riffcat requests for
RMIR-stage evidence, scoped comparisons and numeric/ABI preconditions are in
`docs/dev/compiler-observation-integration.md`.

## Why no architecture decision follows

The experiment did not obtain a legality-only Sonatina comparison, identical
physical entry layouts, hard-production per-origin attribution, or controlled
peak-memory/runtime comparisons. Instruction counts cannot substitute for any
of those gates. The historical production capture shows substantial cleanup,
but does not measure the counterfactual optimized direct output.

Thus the study establishes feasibility of a bounded direct typed/resource
slice, not an advantage after accounting for Sonatina optimization. Its tiny
results neither explain nor fix the large production shaders. Expanding it to
answer those questions would exceed the agreed disposable subset.

Next: keep target/ABI/storage/control contracts explicit in the existing path,
resolve checked-arithmetic support honestly, and run the production capstone.
No second backend, compiler dependency or production selector is introduced.
Retain fixtures, shader artifacts and evidence; remove the disposable emitter
and its dedicated build directory. The shared Fe target cache is not disposable.
