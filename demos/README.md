# Browser demos

Use one repository-level command:

```sh
demos/serve.sh keystone
demos/serve.sh mandelbrot-interactive
demos/serve.sh cga
SONATINA_DIR=/path/to/sonatina demos/serve.sh cga-d1
SONATINA_DIR=/path/to/sonatina demos/serve.sh qcga
```

The command generates missing assets, validates the demos that have explicit
asset preflights, and serves the common `demos/` root at
`http://127.0.0.1:8788/`. Pass `--generate-only` to stop after generation.
`FORCE_DEMO_REGEN=1` requests regeneration. Existing per-demo `serve.sh` files
are compatibility wrappers around this command.

`cga` is the tracked recursive Schedule32 bundle used by the UI. It serves from
a fresh checkout without `SONATINA_DIR`. The compatibility alias
`cga-schedule32` means the same thing. Legacy D1 is available only as the
explicit `cga-d1` selector.

Serving any complete tracked bundle does not require a Sonatina checkout.
`SONATINA_DIR` is required only when a selected bundle is missing or
`FORCE_DEMO_REGEN=1` requests regeneration of a bundle whose generator pins the
unpublished Sonatina tree.

## Relationship to `fe web`

For a new single-source application whose browser consumes the standard
`WebBundle` contract, use the compiler directly:

```sh
fe web serve path/to/kernel.fe --entry kernel --mode render \
  --root path/to/static-app
```

That command compiles, watches, and serves one in-memory bundle as
`kernel.wasm`, `kernel.wgsl`, and `manifest.json`.

The older showcase demos are not silently routed through that command yet.
Their reviewed generators additionally produce independent oracle/reference
data, legacy `layout.json` contracts, control modules, or canonical actor
artifacts. CGA and QCGA also pin unpublished Sonatina revisions. Replacing those
generators with a plain `WebBundle` build would remove reproducibility and
acceptance gates. The repository command therefore consolidates orchestration
and serving while leaving each specialized generator authoritative.
