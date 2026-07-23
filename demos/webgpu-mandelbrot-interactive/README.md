# Hosted Mandelbrot browser acceptance

The generated canonical interface contains three nominal Fe lanes:
`update_view_message`, `render`, and `verify`. The hosted page uses a real module
Worker for the Fe control lane. Render and verify are explicitly placed as host
effects on the main-thread WebGPU owner; their request/result validators come
from the same generated `ctl-interface.js`, rather than a demo-owned JavaScript
schema. `ctl.json` contains only application event mapping and artifact names—it
does not restate the actor schema.

The same canonical module-worker binding used by the CGA flagship owns wire
request IDs and epochs here. Its in-flight bound, readiness timeout, serialized
restart, generated owned-byte transfer policy, and stable error codes are
runtime behavior rather than Mandelbrot-specific orchestration.

Interactive rendering crosses the bounded schema-parametric GPU actor, but the
GPU owner still submits directly to the presentation target. Normal frames do
not read pixels back. Only the explicit initial verification lane performs
readback and compares WebGPU, Fe-Wasm, and generated-reference hashes.

The standalone page deliberately retains the raw eight-argument
`update_view`/three-result Wasm export. This keeps its import-free path small
without weakening the hosted Worker claim.

Regenerate all ignored assets before serving or running Worker tests:

```sh
SONATINA_DIR=/path/to/sonatina \
  demos/serve.sh mandelbrot-interactive
bun demos/webgpu-mandelbrot-interactive/worker-control.test.mjs
```

The Worker test executes the generated control lane through a real module Worker
and MessageChannel, restarts it, observes the incremented actor epoch, and
executes again. `actor-runtime.test.mjs` separately covers generated render and
verify schemas, bounded latest-frame backpressure, stable host-effect errors,
and restart lifecycle:

```sh
bun demos/webgpu-mandelbrot-interactive/actor-runtime.test.mjs
```

The page's structured result is exposed as
`window.__mandelAcceptance` for automation. Green browser acceptance requires
all of the following in the same hosted Chrome page:

- the module Worker completed the deterministic 4,007-step control oracle;
- WebGPU rendered and read back the initial frame;
- GPU, Fe-Wasm, and generated-reference FNV hashes are identical;
- Chrome reported a non-empty adapter name.

Run the repository-native CDP gate without Node or Playwright dependencies:

```sh
CHROME_BIN=/path/to/chrome demos/webgpu-mandelbrot-interactive/smoke-chrome.sh
```

The harness serves `demos/` on a free loopback port and launches headless Chrome
with an offscreen SwiftShader WebGPU target, avoiding headless canvas surface
requirements. It owns and removes its temporary browser
profile and server. If Chrome is unavailable it exits 69 and prints
`UNAVAILABLE`; that result is not acceptance evidence.
