# Browser demos

Compiler-produced Wasm/WGSL artifacts, served from one isolated origin via Trunk.

```sh
demos/serve.sh --serve
```

Open `http://127.0.0.1:8788/`. The landing page (`demos/index.html`) links every
demo, including the curated `gallery/`. Pass `--no-watch` after `--serve` for a
fixed (non-live-reload) server:

```sh
demos/serve.sh cga3d-interactive --serve --no-watch
```

Trunk supplies the Wasm MIME type, live reload, and these response headers from
`demos/Trunk.toml`:

- `Cross-Origin-Opener-Policy: same-origin`
- `Cross-Origin-Embedder-Policy: require-corp`
- `Cross-Origin-Resource-Policy: same-origin`
- `Cache-Control: no-store`

## Layout

```
demos/
  sketches/<name>/            source of truth: one actor-declaring Fe ingot per feature
  webgpu-<name>-interactive/  generate.sh (fe web build + gated ctl codegen) + live-pump.js + main.js
  capstones/<name>/           non-actor, multi-backend-target kernels + evidence.json
  shared/                     cross-demo JS: host-runtime, gpu-timestamp, live-pump.js, ...
  gallery/                    curated geometric-algebra showcase (static page, no build step)
  fe-sandbox/                 highlighted-editor / compiler-in-the-browser page
  webgpu-keystone/            gen/ (scalar cross-backend bundle) + webgpu-runner.js/wasm-runner.js,
                               the shared kernel-blind runners imported by 6+ other demo dirs
  index.html, serve.sh, Trunk.toml, README.md
```

`webgpu-keystone` no longer ships its own demo page (retired 2026-08-04 as a
duplicate of `fe-sandbox`'s "poseidon" kernel option, same `gen/` bundle, same
runners). The directory stays: `webgpu-runner.js` and `wasm-runner.js` are load-
bearing shared infrastructure, and its `gen/` bundle is still what `fe-sandbox`'s
"poseidon" dropdown option loads.

## Demo selectors (`demos/serve.sh [SELECTOR] [--serve] [--no-watch]`)

New generation (2026-08-03), gallery-linked, sourced from `demos/sketches/*` via
`fe web build` + a gated Fe-codegen `ctl` example, 2-file `live-pump.js` + `main.js`
harness:

- `cga3d-interactive` - CGA3D pencil of spheres, Cl(4,1)
- `qcga-interactive` - QCGA quadric pencil, Cl(9,6)
- `desargues-interactive` - Desargues' theorem, PGA-2D, via `gaplay::{meet,join}`
- `mandelbrot` - the four-backend capstone (EVM, Wasm, Native, WebGPU), source in
  `demos/capstones/mandelbrot/`

Older generation (2026-07), `KEEP-MODERNIZE` per the demo audit (real, non-duplicate
content; legacy JS-harness weight, no `actor` declaration yet):

- `clifford-interactive` - Cl(3) rotor sandwich `v' = R v ~R`
- `mandelbrot-interactive` - draggable pan/zoom fractal (the only draggable
  Mandelbrot; distinct from the static `mandelbrot` capstone above)
- `qcga` - a single fixed general quadric, raymarched (distinct scene from
  `qcga-interactive`'s pencil; both live under `webgpu-qcga3d-quadric/`)
- `cga` / `cga-schedule32` - CGA inversion (Dupin cyclide), recursive typed
  Schedule32 sandwich; this is the default selector
- `cga-d1` - the same demo's legacy D1 bundle (pinned to an older reviewed
  Sonatina commit; kept as an explicit comparison path, not the default)
- `keystone` - regenerates `webgpu-keystone/gen/` only; there is no page for this
  selector to serve, but `fe-sandbox` depends on the bundle it produces

```sh
demos/serve.sh                       # validate the default (cga = Schedule32)
demos/serve.sh cga3d-interactive
demos/serve.sh qcga-interactive
demos/serve.sh desargues-interactive
demos/serve.sh mandelbrot
demos/serve.sh mandelbrot-interactive
demos/serve.sh clifford-interactive
demos/serve.sh qcga
demos/serve.sh cga-d1
demos/serve.sh all                   # every selector above, then a cross-app runtime gate
```

`FORCE_DEMO_REGEN=1` forces regeneration even when the tracked `gen/` bundle looks
complete. Generation is plain `cargo run --locked -p fe-codegen --example ...` (or,
for the 3 new demos, the demo's own `generate.sh`, which itself calls the release
`fe` CLI plus a gated codegen example) - no external checkout, no `SONATINA_DIR`,
no overlay. `demos/serve.sh all` additionally runs
`demos/shared/verify_cga_runtime_reuse.py`, which validates every compiler-declared
runtime artifact against its byte count and SHA-256 digest and requires Schedule32
CGA and QCGA3D to package the same eight-module `fe-browser-actor-runtime`
identity.

`gallery/` and `fe-sandbox/` have no `serve.sh` selector: the gallery is a static
page with no generation step, and `fe-sandbox` builds its own prebuilt kernel
bundle via `fe-sandbox/build-browser-compiler.sh` (self-contained).

## Single-source compat launcher

For a single `.fe` file or ingot that only needs the standard `WebBundle`
contract (not one of the flagship generators' extra evidence jobs), `demos/fe-web`
wraps the real `fe web build|serve` CLI directly (plain locked cargo, same pin as
`serve.sh`):

```sh
demos/fe-web serve path/to/kernel.fe --entry kernel --mode render \
  --root path/to/static-app
demos/fe-web build path/to/kernel.fe --entry kernel --mode render \
  --out path/to/bundle
```

The flagship generators (`webgpu-*-interactive/generate.sh`, the `gen_*_demo`
codegen examples) remain authoritative for demos that need an independent Rust
oracle, typed-plan witness/provenance, or a legacy `layout.json`/`actor-source.fe`
contract beyond the generic `shader.wgsl`/`manifest.json` bundle.

## Tests

- `demos/test_serve_sh.py` - `serve.sh` selector routing and `--serve`/`--no-watch`
  argument plumbing (subprocess, no cargo/trunk needed; hooks generation via
  `FE_DEMO_GENERATE_CMD`).
- `demos/test_trunk.py` - asserts the built `target/trunk-demos/` dist tree.
- `demos/test_sonatina_overlay.py` - covers `with-sonatina-overlay.sh`, a separate
  isolated-build tool for vendoring Sonatina backend patches
  (`vendor/sonatina/mb2-browser-runtime/`); unrelated to demo generation, which no
  longer touches it.
