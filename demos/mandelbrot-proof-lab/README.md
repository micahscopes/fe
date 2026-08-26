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

The top status row is ordered as trace, LDE, Poseidon commitment, FRI fold,
and overall. Blue means a clean checkpoint accepted. Pink means a requested
mutation was detected and the rejection check accepted. Red means failure.

The gallery is a later integration gate. Do not use it for routine proof-pass
iteration because unrelated surfaces increase compile latency and WebGPU device
pressure.
