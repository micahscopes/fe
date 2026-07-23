# Sparse typed QCGA3D browser showcase

This demo presents the compiler-generated 128x128 rotated-quadric kernel through
the shared Worker, MessagePort, and main-thread WebGPU actor runtime. It is the
second-application proof for the compiler-owned canonical browser interface.
The generated `actor-interface.js` derives all four lane schemas from nominal Fe
records:

- `render` and `verify` are explicitly selected host effects backed by the
  main-thread WebGPU actor;
- `oracle` is explicit Worker-side frame orchestration;
- every pixel in that frame is computed by the genuine Fe/Wasm
  `oracle_pixel` lane through the generated canonical interface caller.

The ownership map is visible in `wasm-worker.js`; the demo does not claim that a
nominal Fe function submitted WebGPU work. The full-frame oracle currently makes
16,384 canonical calls because direct Fe construction of `BrowserBytes` would
introduce a `u256` pointer into the wasm32 scalar envelope. Acceptance reports
the measured `oracleMs` rather than hiding that cost.

The canvas scales responsively; the pixel-edge toggle and loupe inspect the
actual fixed kernel output. There are no pretend algebra controls because this
first sparse QCGA kernel has no runtime quadric or camera parameters.

Generate the five ignored assets from the reviewed local Sonatina commit and
serve the common demos root:

```sh
SONATINA_DIR=/workspace/sonatina demos/webgpu-qcga3d-quadric/serve.sh
```

Then open `http://127.0.0.1:8000/webgpu-qcga3d-quadric/`. Set
`FORCE_QCGA_REGEN=1` to regenerate an existing bundle.

The real-browser equality gate compares all 16,384 Fe/Worker/Wasm and WebGPU
pixels. GPU presentation and verification both cross the Worker-to-GPU actor
port. Generated request/result validators, request correlation, restart
semantics, bounded queues, and owned-byte transfer are exercised by:

```sh
bun demos/webgpu-qcga3d-quadric/actor-runtime.test.mjs
```

The presentation-only contract fetches no Wasm/reference, creates no Worker,
and performs no readback:

```sh
CHROME_BIN=/path/to/chrome demos/webgpu-qcga3d-quadric/smoke-chrome.sh
CHROME_BIN=/path/to/chrome QCGA_MODE=off demos/webgpu-qcga3d-quadric/smoke-chrome.sh
```
