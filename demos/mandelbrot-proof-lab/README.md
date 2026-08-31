# Mandelbrot proof GPU lab

This is the focused browser iteration lane for
`demos/sketches/mandelbrot_proof_gpu`. It intentionally boots one Fe surface,
not the full gallery. The page contains no JavaScript and no authored JSON
manifest. The ordinary Fe actor remains the only proof and scheduling source.

Run the exact scalar and browser-profile WebGPU oracle first:

```sh
SCCACHE_DIR=/workspace/.sccache \
RUSTC_WRAPPER=sccache \
CARGO_INCREMENTAL=0 \
CARGO_BUILD_JOBS=1 \
cargo test --release -p fe-codegen \
  --test mandelbrot_proof_gpu_e2e -- --nocapture
```

Then start only the lab page:

```sh
SCCACHE_DIR=/workspace/.sccache \
RUSTC_WRAPPER=sccache \
CARGO_INCREMENTAL=0 \
CARGO_BUILD_JOBS=1 \
cargo run --release -p fe -- web dev \
  demos/mandelbrot-proof-lab/index.html \
  --port 8000 --host 0.0.0.0
```

Open `http://10.0.0.2:8000/` in the externally hosted Chrome. The existing
`chrome-devtools` MCP configuration connects to
`http://10.0.0.1:9222`, so the normal browser loop is:

1. Navigate the existing browser page to the lab URL.
2. Capture console messages and a screenshot.
3. Assert that one `fe-surface` mounted and that Chrome kept its WebGPU device.
4. Check clean mode with `tamper = 0`.
5. Activate the surface by clicking or focusing it, then change the generated
   `tamper` control to 1 and check mutation mode. Browser automation that edits
   the control without a composed click must first await `surface.live()`.
6. Return to 0 and check recovery without reloading the page.

The actor places all 17 main and 411 Fe-derived auxiliary AIR columns through
the same typed 4-to-16 stage grid. The test harness reads back all 6,848 LDE
words and compares them with an independent direct-DFT oracle. The top status
row is ordered as trace, LDE, Poseidon commitment, FRI fold, and overall. Blue
means a clean checkpoint accepted. Pink means a requested
mutation was detected and the rejection check accepted. Red means failure.

The gallery is a later integration gate. Do not use it for routine proof-pass
iteration because unrelated surfaces increase compile latency and WebGPU device
pressure.

The same acceptance flow is executable against either a local Chromium or the
externally hosted hardware browser:

```sh
node demos/mandelbrot-proof-lab/mandelbrot_proof_lab.browser.mjs \
  /workspace/scratch/mb2-mandelbrot-proof-lab-site
```

For the external browser, serve the immutable precompiled artifacts on the
real origin that Chrome was launched to treat as secure:

```sh
FE_BROWSER_URL=http://10.0.0.1:9222 \
FE_BROWSER_HOST=10.0.0.2 \
FE_BROWSER_PORT=8000 \
node demos/mandelbrot-proof-lab/mandelbrot_proof_lab.browser.mjs \
  /workspace/scratch/mb2-mandelbrot-proof-lab-site
```

`FE_BROWSER_ORIGIN` remains available for request-interception diagnostics,
but its origin must itself satisfy Chrome's secure-context requirements.

The harness contains no proof implementation. It checks the compiler-derived
pass order and repetitions, drives the public `fe-surface` parameter interface,
captures the rendered poster, and requires exact clean, mutation, and recovery
status colors with no browser or device-loss errors.

## Current measured checkpoint

The latest 2026-08-30 release checkpoint emits 30 passes, 14,155,138 WGSL
bytes, 2,182 Wasm bytes, six typed resources, and a 3,184-word proof tape.
Fresh site publication took 789.82 seconds. Chromium 150 on SwiftShader mounted
the graph in 344.01 seconds, ran the first clean case in 386.93 seconds, and ran
warm tampered and recovered cases in 38.54 and 23.82 seconds.

The Fe-authored placement commits 16 production main AIR rows and 16 production
auxiliary AIR rows, reduces both ordered 16-leaf trees, commits packed main and
auxiliary trace rows plus public inputs, and binds those roots and both LDE
roots into the ordered production AIR transcript. The shared Fe encoding ingot
owns both the aggregate scalar encoding and the direct GPU field projection.
The independent Rust and Plonky3 oracle matched all 1,712 trace words, 428
completion flags, 6,848 LDE values, packed fields, production roots, final AIR
transcript, and authenticated FRI checkpoint receipt. The captured receipt
SHA-256 is
`c0e886d136dc5e944faca70505455cfc3ec46d31e61d91afc4aae374e5a14ccc`.

This is software-browser exactness evidence, not yet a complete STARK proof.
The current FRI checkpoint still starts from four selected main columns rather
than the full AIR composition codeword. Physical-GPU parity also remains
required. The large cold-start time is now an explicit pressure to improve
pipeline caching and semantics-preserving pass fusion before gallery
integration or larger domains.
