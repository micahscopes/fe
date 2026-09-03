# Production round interaction WebGPU probe

This is a focused physical-browser feasibility lane for the exact
production-width Fe round-interaction actor. It stays outside the gallery and
does not relax the separate sub-1 MB compiler quality gate.

Precompile it with an optimized compiler:

```sh
TMPDIR=/workspace/tmp \
SCCACHE_DIR=/workspace/.sccache \
RUSTC_WRAPPER=sccache \
CARGO_INCREMENTAL=0 \
CARGO_BUILD_JOBS=1 \
cargo run --release -p fe -- web precompile \
  demos/mandelbrot-round-interaction-probe/index.html \
  --out /workspace/scratch/mb2-round-interaction-probe-site
```

The page should create one Fe surface containing the
`write_round_locals` compute pass and `paint` display pass. Browser acceptance
requires a live WebGPU device, successful pipeline creation and dispatch, no
uncaptured error or device loss, and typed readback of the `interaction` and
`validity` resources. The rendered color alone is never an exactness oracle.

Run the focused feasibility probe against the externally hosted Chrome:

```sh
FE_BROWSER_URL=http://10.0.0.1:9222 \
FE_BROWSER_HOST=10.0.0.2 \
FE_BROWSER_PORT=8000 \
node demos/mandelbrot-round-interaction-probe/round_interaction.browser.mjs \
  /workspace/scratch/mb2-round-interaction-probe-site
```

Before the full surface probe, isolate browser acceptance into three explicit
stages. `compile` creates only the production shader module and pipeline. `one`
dispatches one workgroup of 64 invocations. `full` dispatches the authored
64-workgroup grid without mounting the render surface. `readback` adds a
four-byte storage-buffer copy and map after that full dispatch.

```sh
FE_BROWSER_COMPUTE_STAGE=compile \
FE_BROWSER_URL=http://10.0.0.1:9222 \
FE_BROWSER_HOST=10.0.0.2 \
FE_BROWSER_PORT=8000 \
node demos/mandelbrot-round-interaction-probe/round_interaction.browser.mjs \
  /workspace/scratch/mb2-round-interaction-probe-site
```

Change `compile` to `one`, `full`, or `readback` to advance one gate at a time.
These modes derive their shader, bindings, buffer sizes, entry point, and
dispatch geometry from the compiler-emitted manifest. They do not duplicate
proof logic. `compile` also accepts a site with several compute passes and
reports each Fe source entry as Chrome finishes its pipeline. Select one pass
from a larger graph with `FE_BROWSER_COMPUTE_ENTRY=source_entry`; the execution
modes deliberately require that single-pass selection.

The adjacent `production.html` page exercises the complete production sparse
AIR actor outside the gallery. Its Fe dispatch aliases place every authored
stage at a queue submission boundary while preserving all repetitions inside
that stage. Precompile it with the same command by replacing `index.html` with
`production.html`.

For diagnostic bisection, `prefix` creates the manifest-declared resources once
and executes an ordered prefix of compute passes, submitting after each one:

```sh
FE_BROWSER_COMPUTE_STAGE=prefix \
FE_BROWSER_PASS_LIMIT=20 \
FE_BROWSER_URL=http://10.0.0.1:9222 \
FE_BROWSER_HOST=10.0.0.2 \
FE_BROWSER_PORT=8000 \
node demos/mandelbrot-round-interaction-probe/round_interaction.browser.mjs \
  /workspace/scratch/mb2-production-base-trace-site
```

This mode is an execution observer, not an alternate scheduler. It fails closed
when the manifest contains a dispatch policy it does not implement. The normal
surface runtime remains the acceptance gate for Fe-authored scheduling.

For a browser-level GPU trace that survives a GPU subprocess restart, add an
output path under disk-backed workspace scratch:

```sh
FE_BROWSER_COMPUTE_STAGE=compile \
FE_BROWSER_TRACE_PATH=/workspace/scratch/round-interaction-compile-trace.json \
FE_BROWSER_URL=http://10.0.0.1:9222 \
FE_BROWSER_HOST=10.0.0.2 \
FE_BROWSER_PORT=8000 \
node demos/mandelbrot-round-interaction-probe/round_interaction.browser.mjs \
  /workspace/scratch/mb2-round-interaction-probe-site
```

The trace starts after navigation and is streamed directly to disk. It includes
GPU, command-buffer, and Dawn/WebGPU categories. The harness prints every phase
before entering it, so navigation, compilation, dispatch, and readback failures
remain distinct. The trace contains execution diagnostics, not proof data or an
alternate implementation.

This browser harness contains no proof implementation. It checks the
compiler-derived pass and resource geometry, waits for queue completion, and
records hashes and summary counts from test-only readback. Exactness remains a
separate independent-oracle gate.

If the production probe loses its device, run the one-word WebGPU control on
the same Chrome process:

```sh
FE_BROWSER_HEALTH_ONLY=1 \
FE_BROWSER_URL=http://10.0.0.1:9222 \
FE_BROWSER_HOST=10.0.0.2 \
FE_BROWSER_PORT=8000 \
node demos/mandelbrot-round-interaction-probe/round_interaction.browser.mjs \
  /workspace/scratch/mb2-round-interaction-probe-site
```
