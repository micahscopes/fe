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
SONATINA_DIR=/path/to/sonatina \
  demos/serve.sh mandelbrot-interactive
SONATINA_DIR=/path/to/sonatina \
  demos/serve.sh qcga
```

`cga` means Schedule32; `cga-schedule32` is a compatibility alias. Legacy D1
requires the explicit `cga-d1` selector. `FORCE_DEMO_REGEN=1` requests
regeneration. The reviewed browser-backend Sonatina commit is not fetchable
from its remote by SHA. The tracked 25-patch overlay reconstructs it exactly
from Fe's fetchable `150d327e` dependency in an isolated temporary checkout;
the source checkout named by `SONATINA_DIR` is never modified. The same wrapper
is invoked internally by `demos/serve.sh`; users do not need another build
command. It is temporary infrastructure until `b2601adc` (or its replacement)
is reachable from the Sonatina remote. A clean checkout already at
`b2601adc` takes the direct path.

The specialized generators remain authoritative because they produce
independent oracle/reference data, legacy layout contracts, control modules, or
canonical actor artifacts beyond the standard `WebBundle`.

For a new single-source application that consumes the standard `WebBundle`
contract, use the compiler directly:

```sh
fe web serve path/to/kernel.fe --entry kernel --mode render \
  --root path/to/static-app
```
