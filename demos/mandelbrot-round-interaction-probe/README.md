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
proof logic.

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
