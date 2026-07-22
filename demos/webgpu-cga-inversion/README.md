# Fe two-sphere CGA inversion browser gate

This page consumes the compiler-produced D1 Render bundle in `gen/`:

- `kernel.fe`, the exact Fe source;
- `frag.wgsl`, naga-emitted vertex and fragment WGSL;
- `layout.json`, including typed `params`, `span`, and `builtin_inputs`;
- `reference.json`, containing `width`, `height`, and the pinned RGBA FNV-1a hash.
- `frag.wasm`, the same Fe fragment compiled for the browser-wasm oracle.

The fragment computes the hard union of two spheres in inverted space and
raymarches their conformal inverse with distinct material palettes. The browser
writes `(cam_x, cam_y, zoom) = (0.0, 0.0, 0.0125)` as f32 values at
the compiler-stated member offsets. A browser-wasm execution of the full frame
must first match the compiled reference. Green then requires a live WebGPU render
and readback that byte-equals that browser-wasm
frame. A missing WebGPU adapter after a green wasm oracle is amber, never green.
Automation can await `window.__cgaAcceptance.promise` and inspect its deterministic
`state`, `wasmHash`, and optional `gpuHash`/`adapter` fields.

Generation currently depends on a clean checkout of the unpublished Sonatina
commit `ed43625bb5680aeab993371e28a8c8e5c7c16f96`. Set `SONATINA_DIR` explicitly
and generate only this bundle with:

```sh
SONATINA_DIR=/path/to/sonatina demos/webgpu-cga-inversion/generate.sh
```

The script rejects another or dirty Sonatina checkout, applies all four local
Cargo patches, and restores the Fe checkout's `Cargo.lock` byte-for-byte when it
exits. Generated files remain ignored and must not be hand-edited or committed.
Artifact provenance fails closed if either Git revision cannot be read and
reports tracked modifications separately from the presence of untracked files;
it is source-state evidence, not a claim that the wider build environment is
hermetic. Avoid concurrent Cargo commands while generation owns the lockfile.
To generate when needed and immediately serve the common demos root, run:

```sh
SONATINA_DIR=/path/to/sonatina demos/webgpu-cga-inversion/serve.sh
```

Set `FORCE_CGA_REGEN=1` on that command to replace an existing bundle.

After generating the bundle, run the fast schema preflight (this does not earn
execution acceptance):

```text
python3 demos/webgpu-cga-inversion/verify-assets.py
```

Serve the repository's `demos/` directory with `demos/serve.py`, then open
`webgpu-cga-inversion/`. Do not open the multi-file page directly with `file://`.

For the deterministic real-browser smoke gate:

```sh
CHROME_BIN=/path/to/google-chrome demos/webgpu-cga-inversion/smoke-chrome.sh
```

The harness chooses a free localhost port, cleans up its server/profile, enables
headless WebGPU explicitly, and defaults to `?acceptance=offscreen`. Green proves
that a real browser executed both the Fe Wasm oracle and WebGPU offscreen render,
read the GPU texture back, and found every byte equal. It does not prove canvas
presentation. Canvas display remains a separate manual check; set
`CGA_SMOKE_PRESENTATION=canvas` only when the headless platform genuinely
supports presentation. The harness requires the structured acceptance JSON to
report the selected `presentation` as well as `green`. If Chrome is unavailable
it exits 69 and prints `UNAVAILABLE`; that is not a passing browser result.
Platforms using Metal or D3D may override `CHROME_WEBGPU_FLAGS`.
The harness polls that structured state through Chrome's DevTools Protocol and
terminates Chrome itself; it does not rely on `--dump-dom` completing.
