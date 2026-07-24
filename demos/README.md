# Browser demos

Generate/preflight the selected tracked bundle, then serve every demo from one
common origin with Trunk live reload:

```sh
demos/serve.sh cga --serve
```

Open `http://127.0.0.1:8788/`. The landing page links to every demo and selects
the tracked recursive Schedule32 CGA bundle. The selector remains explicit:
use `qcga`, `mandelbrot-interactive`, or another selector in place of `cga`.
Pass `--no-watch` after `--serve` for a fixed server. This command performs the
same fail-closed generation/preflight described below and then directly execs
the visible `trunk serve --config demos/Trunk.toml` step; there is no project
manifest or implicit framework hook.

Trunk supplies the Wasm MIME type,
live reload, and these response headers from `demos/Trunk.toml`:

- `Cross-Origin-Opener-Policy: same-origin`
- `Cross-Origin-Embedder-Policy: require-corp`
- `Cross-Origin-Resource-Policy: same-origin`
- `Cache-Control: no-store`

The lower-level `trunk serve --config demos/Trunk.toml` command remains
available when generation/preflight is intentionally unnecessary. A fresh
checkout can serve complete tracked assets—especially the default Schedule32
CGA bundle—without `SONATINA_DIR`.

## Generation and preflight

Without `--serve`, `demos/serve.sh` remains an explicit generation/preflight
command for scripts and CI:

```sh
demos/serve.sh                 # validate tracked Schedule32 CGA
demos/serve.sh cga
demos/serve.sh mandelbrot-interactive
demos/serve.sh qcga
```

`cga` means Schedule32; `cga-schedule32` is a compatibility alias. Legacy D1
requires the explicit `cga-d1` selector. `FORCE_DEMO_REGEN=1` requests
regeneration. On the first generation without `SONATINA_DIR`, the command
fetches only the pinned `mb2-render-mode` base
`150d327edfa88374802a6cc8089fd77da5fa818b` from
`https://github.com/micahscopes/sonatina.git` into
`target/fe-browser-cache/sonatina.git`. The commit is checked exactly; the
tracked, checksum-verified 25-patch series then reconstructs reviewed commit
`ac266c210cad7872fc98380a73b4ca363877bc1f` in an isolated temporary checkout.
Warm generation reuses the cache without contacting the remote.

Set `FE_BROWSER_CACHE_DIR` to relocate that cache. Set
`FE_BROWSER_OFFLINE=1` to forbid a cache miss from fetching; a missing pinned
base then fails with an actionable error. `SONATINA_DIR` remains an optional
source/offline override: it may name either a clean reviewed checkout or a
checkout containing the pinned base, and is never modified. Concurrent
generations serialize both cache mutation and the temporary `Cargo.lock`
rewrite with `flock`; the generation lock is acquired before source cleanliness
is checked, so one build cannot make another clean checkout appear dirty. Every
reconstructed checkout is removed after the command. Temporary overlay checkouts, browser profiles, and
lockfile backups stay under the ignored workspace-local `output/demo-tmp`
directory by default; set `FE_DEMO_TMPDIR` to another explicit workspace path
when needed. Demo generation disables an inherited `RUSTC_WRAPPER` by default,
preventing a long-lived compiler-cache daemon from silently putting build
temporaries outside that workspace; opt in explicitly with
`FE_DEMO_RUSTC_WRAPPER`. The wrapper is invoked internally by
`demos/serve.sh`, so users do not need another build command. This is temporary
infrastructure until `ac266c210cad7872fc98380a73b4ca363877bc1f` (or its
upstream replacement) is directly
fetchable. Legacy `cga-d1` remains the one exception and requires its older
reviewed `ed43625b` checkout explicitly.

The specialized generators remain authoritative because they produce
independent oracle/reference data, legacy layout contracts, control modules, or
canonical actor artifacts beyond the standard `WebBundle`.

`demos/serve.sh all` also performs a cross-application runtime gate after
generation. It validates every compiler-declared runtime artifact against its
byte count and SHA-256 digest, then requires Schedule32 CGA and QCGA to package
the same eight-module `fe-browser-actor-runtime` identity. This proves both
demos consume one compiler-owned coordinator, endpoint, router, MessagePort,
module-Worker, GPU-actor, generated Worker host, and generated actor-client
surface. Their concrete WebGPU device handlers and application lifecycle remain
explicit and demo-owned; this gate does not claim that Fe code directly owns or
invokes the browser's WebGPU device.

For a single-source application that consumes the standard `WebBundle`
contract, the temporary compatibility launcher is:

```sh
demos/fe-web serve path/to/kernel.fe --entry kernel --mode render \
  --root path/to/static-app
```

Use `build ... --out path/to/bundle` for an atomic on-disk bundle. The launcher
invokes the real `fe web build|serve` CLI; `demos/with-browser-cargo.sh` only supplies
the checksum-verified, clean Sonatina `ac266c21` overlay that the unpublished
workspace dependency cannot yet fetch. It owns the six Cargo path patches,
workspace-local temporary/target directories, generation lock, inherited
`RUSTC_WRAPPER` removal, and byte-for-byte `Cargo.lock` restoration. A wrong or
dirty `SONATINA_DIR` fails closed.

`fe web serve` also exposes a deliberately explicit live-reload client. Add
this one line to the static application's HTML when browser reload after a
successful Fe rebuild is wanted:

```html
<script type="module" src="/.fe/live-reload.js"></script>
```

There is no HTML injection or project convention behind it. The client polls
the compiler-owned `/.fe/generation` endpoint and reloads only after an atomic
successful bundle publication. A diagnostic-producing edit leaves both the
last good bundle and browser page running. `--no-watch` keeps serving the
one-shot generation, so the same HTML remains valid in fixed-server mode.

Canonical lanes remain explicit and inspectable rather than name-inferred:

```sh
demos/fe-web build path/to/kernel.fe --entry render_kernel --mode render \
  --canonical required --canonical-entry render --canonical-entry verify \
  --canonical-entry oracle --out path/to/bundle
```

This standard bundle contains compiler-derived Wasm, WGSL, manifest,
interfaces, and actor runtime modules. The Schedule32 generator remains
necessary for source composition, its independently executed Rust oracle,
typed-plan witness/provenance, and separate entry-rooted Wasm proof. Publishing
and repinning the backend is required for the intended direct `fe web` UX and
removes this compatibility launcher; it does not erase those
application-specific evidence jobs.

The compiler command itself already accepts either a standalone `.fe` file or
an ordinary Fe ingot directory; an ingot does not need a web-specific project
manifest. The flagship generators are retained for the additional evidence
jobs above, not because `fe web` is limited to single-file programs. Their
current pages also consume the legacy `frag.wgsl`/`layout.json` contract and
independent `reference.json`, whereas the generic bundle deliberately exposes
`shader.wgsl`/`manifest.json`. Repointing those pages without migrating and
re-proving that contract would hide work rather than simplify it.
