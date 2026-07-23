# Browser demos

The tracked demo applications are served from one common origin with Trunk:

```sh
trunk serve --config demos/Trunk.toml
```

Open `http://127.0.0.1:8788/`. The landing page links to every demo and selects
the tracked recursive Schedule32 CGA bundle. Trunk supplies the Wasm MIME type,
live reload, and these response headers from `demos/Trunk.toml`:

- `Cross-Origin-Opener-Policy: same-origin`
- `Cross-Origin-Embedder-Policy: require-corp`
- `Cross-Origin-Resource-Policy: same-origin`
- `Cache-Control: no-store`

Serving has no generation hook. A fresh checkout can serve complete tracked
assets—especially the default Schedule32 CGA bundle—without `SONATINA_DIR`.

## Generation and preflight

`demos/serve.sh` is retained as the explicit generation/preflight command:

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
`b2601adc8b80b085aae98f9132a035fdfecec5c3` in an isolated temporary checkout.
Warm generation reuses the cache without contacting the remote.

Set `FE_BROWSER_CACHE_DIR` to relocate that cache. Set
`FE_BROWSER_OFFLINE=1` to forbid a cache miss from fetching; a missing pinned
base then fails with an actionable error. `SONATINA_DIR` remains an optional
source/offline override: it may name either a clean reviewed checkout or a
checkout containing the pinned base, and is never modified. Concurrent
generations serialize cache mutation with `flock`; every reconstructed checkout
is removed after the command. The wrapper is invoked internally by
`demos/serve.sh`, so users do not need another build command. This is temporary
infrastructure until `b2601adc` (or its upstream replacement) is directly
fetchable. Legacy `cga-d1` remains the one exception and requires its older
reviewed `ed43625b` checkout explicitly.

The specialized generators remain authoritative because they produce
independent oracle/reference data, legacy layout contracts, control modules, or
canonical actor artifacts beyond the standard `WebBundle`.

For a new single-source application that consumes the standard `WebBundle`
contract, use the compiler directly:

```sh
fe web serve path/to/kernel.fe --entry kernel --mode render \
  --root path/to/static-app
```
