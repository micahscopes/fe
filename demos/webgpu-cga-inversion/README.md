# Fe CGA inversion browser gate

This page consumes the compiler-produced D1 Render bundle in `gen/`:

- `kernel.fe`, the exact Fe source;
- `frag.wgsl`, naga-emitted vertex and fragment WGSL;
- `layout.json`, including typed `params`, `span`, and `builtin_inputs`;
- `reference.json`, containing `width`, `height`, and the pinned RGBA FNV-1a hash.
- `frag.wasm`, the same Fe fragment compiled for the browser-wasm oracle.

The browser writes `(cam_x, cam_y, zoom) = (0.0, 0.0, 0.0125)` as f32 values at
the compiler-stated member offsets. A browser-wasm execution of the full frame
must first match the compiled reference. Green then requires a live WebGPU render
and readback that byte-equals that browser-wasm
frame. A missing WebGPU adapter after a green wasm oracle is amber, never green.
Automation can await `window.__cgaAcceptance.promise` and inspect its deterministic
`state`, `wasmHash`, and optional `gpuHash`/`adapter` fields.

Generation currently depends on the unpublished local Sonatina fork. Set
`SONATINA_DIR` to that checkout and run `FORCE_CGA_REGEN=1 demos/serve.sh` to
force a fresh bundle after compiler changes. The local Cargo patches may update
`Cargo.lock`; restore the pinned lockfile before committing.

After generating the bundle, run the fast schema preflight (this does not earn
execution acceptance):

```text
python3 demos/webgpu-cga-inversion/verify-assets.py
```

Serve the repository's `demos/` directory with `demos/serve.py`, then open
`webgpu-cga-inversion/`. Do not open the multi-file page directly with `file://`.
