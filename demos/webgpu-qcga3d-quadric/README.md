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

The ownership map is derived from the generated Fe intents by
`gen/runtime/worker-host.js`; the demo does not claim that a nominal Fe
function submitted WebGPU work. `AllocatedBrowserBytes` preserves the
wasm32 `MemPtr<u8>` carrier while Fe fills the arena, and the canonical wrapper
copies the complete frame before reset and Worker transfer. Acceptance reports
the measured one-call `oracleMs`.
The generic adapter derives the unique placement shared by the supplied
`render` and `verify` lanes from their compiler-generated Fe intent. The
Worker-side proxy does not restate that placement, and mixed-placement handler
sets fail closed.

The visible kernel is a real Fe application ingot depending on the public
`ingots/sparse_clifford` package, rather than a generator-flattened source
concatenation. The generated `app/fe.toml` and `app/src/lib.fe` publish that
reproducible package boundary; `kernel.fe` is an inspectable dependency-backed
kernel excerpt and is intentionally not standalone.

The sparse planner-backed renderer is documented in
`docs/mb2/QCGA_SPARSE_PLANNER.md`: Fe CTFE derives a recursive 12-term plan
from two sparse 12-entry support lists, and one FCO provider publishes the
aggregate contraction used at each hit. Camera and canonical quadric
coefficients are typed runtime fields shared by the generated actor request,
entry-rooted Wasm oracle, and WebGPU input layout.

The canvas scales responsively; the pixel-edge toggle and loupe inspect the
actual output. This is a deliberately bounded rotated-quadric subalgebra
showcase, not a claim of general QCGA or arbitrary multivector storage.
Camera depth, pixel scale, two diagonal weights, and the `x*y` cross term are
live controls over the same compiler-described 15-field input buffer. Input
events coalesce to one direct WebGPU submission per animation frame. They do
not invoke the Wasm actor or read back the GPU; moving a control explicitly
invalidates the displayed verification until the canonical view is reloaded
and verified again.

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
