# Fe interactive CGA inversion browser gate

This page consumes the compiler-produced typed CGA Render bundle in `gen/`:

- `kernel.fe`, the exact Fe source;
- `frag.wgsl`, naga-emitted vertex and fragment WGSL;
- `layout.json`, including typed `params`, `span`, and `builtin_inputs`;
- `reference.json`, containing `width`, `height`, and the pinned RGBA FNV-1a hash.
- `frag.wasm`, the same Fe fragment compiled for the browser-wasm oracle.

The staged Schedule32 bundle is selected explicitly with
[`?bundle=schedule32`](./?bundle=schedule32). With no `bundle` parameter the page
continues to load `gen/`; selection never copies or promotes generated files.

The fragment raymarches the conformal inverse of an offset torus, producing a
cyclide-like surface. The browser writes
`(cam_x, cam_y, zoom, inv_cx, inv_cy) = (0.0, 0.0, 0.0125, 0.5, 0.0)` as five
f32 values at the compiler-stated member offsets. A browser-wasm execution of the full frame
must first match the compiled reference. Green then requires a live WebGPU render
and readback that byte-equals that browser-wasm
frame. A missing WebGPU adapter after a green wasm oracle is amber, never green.
Automation can await `window.__cgaAcceptance.promise` and inspect its deterministic
`state`, `wasmHash`, and optional `gpuHash`/`adapter` fields.

The default canvas page is interactive: move the pointer to move the inversion
circle, click to freeze or unfreeze it, drag to pan, wheel to zoom around the
pointer, and use Focus inversion or Reset to restore a useful view. The initial
default frame still completes the full browser-Wasm/reference/WebGPU acceptance
gate. After that, interaction is presentation-first: animation-frame-coalesced
draws reuse the persistent WebGPU pipeline and update only the typed input
buffer. They do not recompute a full Wasm frame or perform GPU readback.

Use **Verify current view** for an explicit fresh Wasm/GPU byte comparison. Add
`?verify=continuous` to retain trailing-coalesced verification after interaction
(for example `webgpu-cga-inversion/?verify=continuous`). A generation token
prevents an older asynchronous verification from publishing over a newer one.
Interaction math is JavaScript, not Fe: values are rejected if non-finite,
quantized to an immutable five-f32 parameter block for each generation, and zoom is clamped to
`0.0025..=0.05`. Explicit or continuous full-frame verifications are
trailing-debounced and serialized.

For the lowest-latency visual-only path, open
[`?verify=off`](./?verify=off). This mode does not fetch or instantiate any
Wasm module, does not construct an oracle Worker, does not compute the startup
full-frame oracle, and never performs
GPU readback. It loads and initializes only the compiler metadata, WGSL, source,
and persistent canvas pipeline used for presentation. The Verify button is
hidden and structured status is `{"state":"presentation","verified":false}`;
this mode is deliberately not acceptance evidence and never reports green.
The selectors compose: use
[`?bundle=schedule32&verify=off`](./?bundle=schedule32&verify=off) for the
Schedule32 no-Wasm, no-readback presentation path.

Presentation quality is explicit. Use
[`?bundle=schedule32&verify=off&quality=teaser`](./?bundle=schedule32&verify=off&quality=teaser)
for the fixed 256×256 teaser profile, or `quality=full` (the default) for the
existing device-pixel-ratio-aware automatic resolution, capped at 768×768.
An explicit benchmark `resolution` remains available with `quality=full`;
`quality=teaser` accepts only its fixed `resolution=256`. Structured acceptance
status and the visible controls report both the selected profile and actual
canvas resolution. These profiles change presentation resolution only: both
execute the same Fe-generated kernel and the same 72-step raymarch bound.

Low-overhead CPU-side instrumentation is available as
`window.__cgaPerformance`. It records artifact-fetch, GPU-initialization,
first-frame submission, and (when enabled) initial-acceptance durations, plus a
rolling window of interaction-triggered rAF cadence and `renderFrame`
submission CPU time. This interaction cadence is not continuous throughput.
The compact
UI stat is rate-limited to four text updates per second. These are host timings,
not GPU execution timestamps. Instrumentation never inserts a queue wait,
readback, or GPU synchronization; in `verify=off` it preserves the no-readback
contract.

For comparable continuous submission measurements, use
[`?verify=off&benchmark=continuous&resolution=256`](./?verify=off&benchmark=continuous&resolution=256).
The resolution is fail-closed and must be one of `128`, `256`, `512`, or `768`;
the same harness works with the legacy and Schedule32 bundle selectors. It
warms up for 30 frames, samples 120 consecutive rAF submissions, and publishes
`window.__cgaBenchmark` plus the same structured object under the acceptance
result. The reported cadence is submitted-frame cadence and the CPU timing is
submission overhead. It explicitly reports `gpuCompletionMeasured:false`;
without a timestamp query and result readback it makes no GPU-completion claim.
The benchmark uses the direct WebGPU path and preserves the strict no-Wasm,
no-Worker, no-readback `verify=off` contract.

For an explicitly synchronized GPU measurement, add `timing=gpu`:

```text
?bundle=schedule32&verify=off&benchmark=continuous&resolution=256&timing=gpu
```

This opt-in mode requests the WebGPU `timestamp-query` feature, writes beginning
and end timestamps around each render pass, resolves them, maps the 16-byte
result, and reports GPU elapsed milliseconds plus completed-frame cadence.
Consequently it performs one timestamp readback and GPU-completion wait per
frame and is not the low-overhead presentation benchmark. If the adapter does
not expose `timestamp-query`, status reports
`mode: "gpu_timestamp_unsupported"` and `gpuCompletionMeasured: false`; it
never substitutes rAF cadence or CPU time for GPU execution time. Omitting
`timing=gpu` continues to request no optional GPU features, perform no
readback/synchronization, and report only submitted-frame cadence and CPU
submission overhead.

The displayed canvas is responsive and uses device-pixel ratio up to 2, capped
at 768 pixels per side. Deterministic acceptance remains a separate 128x128
offscreen render. The shipped fragment constructs recursive `MvTF<5>` point and
sphere values inside the distance-estimator loop, executes a support-specialized
typed `S*P*S` helper, and normalizes its conformal-vector result in Fe. The older
scalarized D1 fixture remains an independent full-frame regression baseline.

The Wasm oracle Worker uses the compiler-generated canonical actor shape as one
transport contract: request/result validators, lane names, and owned-byte
transfer policy are not restated by the demo. The shared module-worker binding
owns request IDs and epochs, bounds in-flight requests, times out readiness,
serializes restart, and exposes stable error codes without forwarding Worker
exception text. GPU lane routing remains explicit so device ownership is
inspectable rather than hidden in framework configuration: Fe effect
requirements select the main-thread WebGPU lanes and generated validators,
while the application supplies only the visible `render` and `verify` device
handlers. Inside the Worker, the same generated intent partitions lanes between
the Wasm adapter, worker-local oracle effect, and main-thread GPU client; the
application supplies those three concrete dispatchers but no lane lists.

Legacy D1 generation uses Sonatina `ed43625b`; Schedule32 uses the later browser
runtime commit `b2601adc`. The latter is reconstructed reproducibly from the
tracked patch series without modifying the checkout supplied as
`SONATINA_DIR`. Generate D1 directly from its clean reviewed checkout with:

```sh
SONATINA_DIR=/path/to/sonatina demos/webgpu-cga-inversion/generate.sh
```

Generate the staged CTFE-derived Schedule32 bundle instead with:

```sh
SONATINA_DIR=/path/to/sonatina CGA_BUNDLE=schedule32 \
  demos/serve.sh cga
```

Each bundle pins the Sonatina revision it was reviewed against. The script
rejects another or dirty prepared Sonatina checkout, applies Cargo path
overrides for all required Sonatina crates, and restores the Fe checkout's `Cargo.lock` byte-for-byte when it
exits. Generated files remain ignored and must not be hand-edited or committed.
Artifact provenance fails closed if either Git revision cannot be read and
reports tracked modifications separately from the presence of untracked files;
it is source-state evidence, not a claim that the wider build environment is
hermetic. Avoid concurrent Cargo commands while generation owns the lockfile.
To generate when needed, then serve the common demos root, run:

```sh
SONATINA_DIR=/path/to/sonatina demos/serve.sh cga-d1
trunk serve --config demos/Trunk.toml
```

Set `FORCE_CGA_REGEN=1` on that command to replace an existing bundle.
Both `demos/serve.sh cga-d1` and `smoke-chrome.sh` run the same fail-closed bundle
preflight: all five generated files must exist and pass `verify-assets.py`.
When any file is missing they generate once if `SONATINA_DIR` is set, otherwise
they list every missing artifact and print the pinned-local-Sonatina remedy.
Consequently this is also the one-command generate + browser acceptance path:

```sh
SONATINA_DIR=/path/to/sonatina demos/webgpu-cga-inversion/smoke-chrome.sh
```

After generating the bundle, run the fast schema preflight (this does not earn
execution acceptance):

```text
python3 demos/webgpu-cga-inversion/verify-assets.py
```

Run `trunk serve --config demos/Trunk.toml`, then open
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

Select the verification contract with `CGA_SMOKE_VERIFY=default|off|continuous`.
`default` preserves the one-shot green acceptance above. `continuous` adds
`?verify=continuous` and still requires green. `off` adds `?verify=off`, requires
the explicit structured state `presentation` with `verified:false`, and rejects
any `wasmHash` or `gpuHash`; a passing off-mode smoke is reported as an
unverified presentation result, never as green acceptance. Query parameters are
combined with `CGA_SMOKE_PRESENTATION=offscreen|canvas` without replacing one
another.

`CGA_SMOKE_VERIFY=off` is also the deterministic zero-readback performance
smoke. It defaults to canvas presentation, drives 16 pointer moves plus one
wheel interaction through CDP, and requires bounded rAF/submit-CPU samples. Its
structured evidence rejects any Wasm/reference fetch, oracle Worker creation,
oracle render, or GPU readback. It reports observed interaction cadence but deliberately applies
no hardware-independent threshold. This smoke exercises interaction cadence,
not the opt-in continuous benchmark described above.
