# Browser demos

## Standards-based Fe HTML

The canonical gallery, resident SourceInspector, and TodoMVC component use inert
`application/fe` source declarations and the fixed, demo-blind browser hosts
published by `fe web dev`:

```sh
cargo run --release -p fe -- web dev demos/gallery.html --port 8000 --host 0.0.0.0
cargo run --release -p fe -- web dev demos/source-inspector.html --port 8000 --host 0.0.0.0
cargo run --release -p fe -- web dev demos/todomvc.html --port 8000 --host 0.0.0.0
```

TodoMVC's lifecycle, UTF-8 storage, reducer, filters, edit policy, stable keys,
and DOM effect projection are in `demos/sketches/todomvc/src/lib.fe`. The
browser adapter transports standards events and applies the fixed component
effect vocabulary; it contains no Todo-specific policy. The example is also
embedded as a tile in `gallery.html`.

The gallery body is composed by the role-selected `GalleryPage` actor in
`demos/sketches/gallery_page/src/lib.fe`. Its const `GalleryBuilder` expands
through the typed `std::web::page` vocabulary into the header, ordered tiles,
captions, source links, render declarations, and resident-component mounts.
TodoMVC and SourceInspector each project their own initial DOM from a const
`ComponentComposition` behavior in the same Fe module as their reducer. The
HTML body contains only the inert page declaration; its remaining authored CSS
is an explicitly transitional presentation shell, not application behavior or
a second page manifest.

`demos/sketches/source_inspector/src/lib.fe` owns source/generated-artifact
selection, loading/error state, stale-response rejection, binary-vs-text
presentation, focus, Escape, and navigation cancellation. The generic browser
adapter performs same-origin fetches only when Fe emits a resource effect.
Precompiled authored `.fe` links are direct content-addressed text assets, not
entries in another JSON asset manifest, so the same inspector works on static
hosting.

The sections below document the older Trunk/compatibility showcase lanes.

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

### `rollcall/`

A membership-registry app (Rung 4), not a WebGPU render sketch: plain static
HTML/JS, no `application/fe` tags, served as an ordinary site (not through
`serve.sh`):

```sh
cargo run --locked -p fe --features web-server -- web dev \
  demos/rollcall/index.html --no-watch --port 0
```

Loads `gen/kernel.wasm` (the real compiled `poseidon_merkle_root_loop` Fe
kernel) and builds a Poseidon-Merkle root live in the page. `evidence.json`
is a four-leg (`evm`/`native`/`wasm`/`webgpu`) capstone-evidence receipt,
regenerated by `cargo run -p fe-codegen --features native-backend --example
gen_rollcall_evidence` and re-verified for tamper-evidence + cross-leg
agreement by `cargo test -p fe-codegen --test rollcall_evidence_verify`. The
on-chain leg is `revm`, local only (no testnet deploy); as of this rung, the
native/Cranelift and GPU/SPIR-V legs both fail closed with the SAME honestly
recorded reason (function-local array lowering via `MemAllocDynamic` is
wasm-only on the pinned Sonatina rev) -- see `RUNG4_ASSEMBLY_PLAN.md`.

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
