# Hosted Mandelbrot browser acceptance

The multi-file page uses a real module Worker for the Fe
`update_view_message` control lane. The request and response are nominal Fe
records; `ctl-interface.js`, its TypeScript declarations, the canonical arena
Wasm wrapper, and the Worker validators are generated from those exact semantic
types. `ctl.json` contains only application event mapping and artifact names—it
does not restate the actor schema.

The standalone page deliberately retains the raw eight-argument
`update_view`/three-result Wasm export. This keeps its import-free path small
without weakening the hosted Worker claim.

Regenerate all ignored assets before serving or running Worker tests:

```sh
cargo run -p fe-codegen --example gen_mandelbrot_interactive_demo
bun demos/webgpu-mandelbrot-interactive/worker-control.test.mjs
```

The Worker test uses a real module Worker and MessageChannel, executes the
generated canonical lane, restarts it, observes the incremented actor epoch, and
executes again. Its structured result is exposed as
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
