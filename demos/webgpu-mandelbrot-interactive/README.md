# Hosted Mandelbrot browser acceptance

The multi-file page uses a real module Worker for the Fe `update_view` control
function. Its structured result is exposed as `window.__mandelAcceptance` for
automation. Green acceptance requires all of the following in the same hosted
Chrome page:

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
