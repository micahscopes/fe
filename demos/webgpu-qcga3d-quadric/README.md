# Sparse typed QCGA3D browser showcase

This demo presents the compiler-generated 128x128 rotated-quadric kernel through
the compiler-packaged Worker, MessagePort, and main-thread WebGPU actor runtime. It is the
second-application proof for the compiler-owned canonical browser interface.
The generated `actor-interface.js` derives all three lane schemas from nominal Fe
records:

- `render` and `verify` are explicitly selected host effects backed by the
  main-thread WebGPU actor;
- `oracle` is a genuine Fe/Wasm lane that allocates, computes, and returns the
  complete frame through one canonical call.

The ownership map is visible in `wasm-worker.js`; the demo does not claim that a
nominal Fe function submitted WebGPU work. `AllocatedBrowserBytes` preserves the
wasm32 `MemPtr<u8>` carrier while Fe fills the arena, and the canonical wrapper
copies the complete frame before reset and Worker transfer. Acceptance reports
the measured one-call `oracleMs`.
The Worker-side proxy explicitly selects `placement: "main_thread"` for the
render and verification handlers; it does not use its current JavaScript realm
to reinterpret the placement declared by Fe.

The canvas scales responsively; the pixel-edge toggle and loupe inspect the
actual fixed kernel output. There are no pretend algebra controls because this
first sparse QCGA kernel has no runtime quadric or camera parameters.

Generate the five ignored assets from the reviewed local Sonatina commit and
serve the common demos root:

```sh
demos/serve.sh qcga
trunk serve --config demos/Trunk.toml
```

Then open `http://127.0.0.1:8788/webgpu-qcga3d-quadric/`. Set
`FORCE_QCGA_REGEN=1` to regenerate an existing bundle.

The real-browser equality gate compares the one-call Fe/Worker/Wasm frame with
all 16,384 WebGPU pixels. GPU presentation and verification both cross the Worker-to-GPU actor
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
