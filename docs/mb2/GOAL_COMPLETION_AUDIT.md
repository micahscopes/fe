# mb2 browser CGA goal: completion evidence audit

Status: audited against the integrated `mb2` lineage on 2026-07-23. Re-run
the cited gates after later implementation commits rather than treating a
cherry-picked source hash as a permanent completion marker.

This is a completion audit, not a milestone announcement. `Achieved` means the
current tree contains direct evidence for the stated scope. `Partial` means a
real implementation exists but the goal asks for a broader or stronger claim.
`Missing` means the named result is not present. A schema preflight is never
treated as browser execution evidence.

## Evidence matrix

| Goal requirement | Status | Authoritative evidence | Exact boundary / remaining work |
| --- | --- | --- | --- |
| Fe-authored interactive Cl(4,1) browser render | **Achieved for the bounded showcase** | `demos/webgpu-cga-inversion/gen-schedule32/kernel.fe` contains the complete DE render, runtime inversion center, `CanonicalCga as Sandwich`, and the five conformal-vector outputs. `demos/webgpu-cga-inversion/main.js` owns only interaction and WebGPU submission. `bun demos/webgpu-cga-inversion/actor-interface.test.mjs` executes the generated one-call Wasm frame. | This proves this Cl(4,1) showcase, not a general geometric-algebra library or arbitrary operator compiler. Interaction math and concrete WebGPU device handlers remain explicit JavaScript host policy. |
| Typed CTFE-derived Garamon/Fuchs-Théry-style `S * P * S` specialization | **Achieved for Schedule32; partial as a reusable facility** | In `gen-schedule32/kernel.fe`, ordinary Fe const functions scan all 80 ordered triples, `static_assert(survivor_count() == 32)`, recursive `Schedule<32>` materializes the typed witness, and the provider-emitted `Sandwich` implementation uses the same canonical helpers. `crates/codegen/tests/hybrid_cga_plan_e2e.rs` compares the typed specialization with an independent Rust raw-80 evaluator and a separately authored recursive generic Fe `MvTF<5>` evaluator. `crates/codegen/tests/spirv_ctfe_cga_schedule.rs`, `fco_staged_cga_bridge.rs`, and `fco_cga80_direct_*` cover staging and both backends. | The bounded 80-to-32 decomposition is real. There is no reusable typed algebra/plan package that accepts arbitrary supports/operators and derives a canonical plan. FCO publishes an explicitly planned body; it does not discover sharing or normalize an arbitrary recursive type during provider expansion. See `FCO_SPARSE_CONSTRUCTOR_SPIKE.md` and `FCO_PLAN_BRIDGE_OPTIONS.md`. |
| Browser-profile WebGPU execution of the specialization | **Achieved** | Generated `frag.wgsl`, `layout.json`, and `actor/shader.wgsl` live under `demos/webgpu-cga-inversion/gen-schedule32/`. `python3 demos/webgpu-cga-inversion/verify-assets.py` checks browser WGSL shape, absence of leaked CTFE/provider tokens, the single runtime raymarch loop, and required canonical artifacts. `smoke-chrome.sh` is the real-browser gate. Last observed real Chromium/SwiftShader acceptance in this worktree produced bit-equal Wasm/GPU hash `3470936828`. | The checked-in files and preflight prove artifact shape, while the smoke command proves execution only when run. No durable hardware-GPU CI result is stored; the recorded run used SwiftShader. |
| Fast interactive presentation with no per-frame readback by default | **Achieved for submission behavior; performance claim remains partial** | `README.md` documents the persistent pipeline, rAF coalescing, and typed input-buffer updates. After initial acceptance, normal interaction does not invoke Wasm or read back GPU data. `?verify=off` is stricter: no Wasm/reference fetch, Worker, oracle render, or readback. `CGA_SMOKE_VERIFY=off CGA_SMOKE_BENCHMARK=continuous ... smoke-chrome.sh` asserts those counters. Last observed 256x256 SwiftShader run submitted at about 60 Hz with average CPU submit near 0.202 ms and `gpuReadbackCount == 0`. | Default mode intentionally performs one startup verification readback; only presentation mode is zero-readback from startup. The no-readback benchmark measures submitted-frame cadence, not GPU completion. Timestamp mode is explicit and necessarily reads timestamp results. No portable performance threshold or hardware-adapter result is established. |
| Explicit Wasm/WebGPU verification against an independent oracle | **Achieved for the pinned full frame and explicit current-view checks** | Browser acceptance compares every GPU byte with the one-call Fe/Worker/Wasm frame; Schedule32 currently agrees at FNV `3470936828`. Before writing `reference.json`, `crates/codegen/examples/gen_cga_schedule32_vec5_demo.rs::oracle` independently reimplements the complete camera, 72-step raymarch, conformal inversion, torus DE, hit policy, shading, and packing in Rust `f32`; generation requires bit-exact agreement for all 16,384 Wasm pixels and records `oracle_agreement.exact_mismatches == 0`. `hybrid_cga_plan_e2e.rs` separately compares the algebraic specialization with a raw-80 Rust evaluator and a generic recursive Fe evaluator. | Browser verification is transitive but exact: independently gated host oracle = generated reference = Fe/Wasm = WebGPU. Interactive current-view verification compares Wasm and GPU for arbitrary controls but does not rerun the Rust host oracle in the browser; the independent host gate is the pinned generation view. |
| Compiler-derived canonical request/response interface | **Achieved for canonical interface v1** | `crates/codegen/assets/canonical-interface.js`, `crates/codegen/src/web_bundle.rs`, and generated `actor/interface.{js,d.ts}` derive nominal records, offsets, codecs, lane schemas, effect intents, and transfer policy from Fe signatures plus emitted Wasm layout. `demos/shared/canonical-interface.test.mjs` covers codecs, intent routing, arena reset, and ownership. `CANONICAL_BROWSER_INTERFACE.md` specifies the ABI. | Version 1 deliberately supports fixed records, scalars, owned bytes, and strings. General lists, variants, resources, futures/streams, borrowed/shared memory, and cancellation are not implemented. |
| Robust generated Worker/MessagePort adapter | **Achieved for the stated v1 semantics** | The six modules in `crates/codegen/assets/browser-runtime/` implement typed correlation, exact schemas, bounded endpoint pending counts, per-lane latest-pending host queues, full-span owned-buffer transfer, startup timeout, sanitized errors, restart epochs, and close/fail behavior. All six `*.test.mjs` suites pass under Bun. Commits `cb9a34dca`, `5177f6268`, and `b4686bbc8` close malformed-request hangs and restart ordering. | This is deterministic request/response lifecycle behavior, not distributed actor supervision: there is no cancellation/abort propagation, crash policy, persistence, multi-worker scheduler, or shared-memory ownership model. Concrete Worker creation and WebGPU handlers are still host implementations by design. |
| Wasm actor connected to WebGPU actor through a reusable, non-magical runtime | **Achieved** | `module-worker-actor.js`, `gpu-actor.js`, and `actor-router.js` are compiler-packaged, application-neutral modules. Fe effect intent selects lane partitions and placement; applications explicitly provide concrete dispatchers/device handlers. `python3 demos/shared/verify_cga_runtime_reuse.py` proves Schedule32 and QCGA package byte-identical copies of all six runtime modules. | Fe describes ABI and execution intent; JavaScript owns browser capabilities, Worker/MessagePort construction, GPU device lifetime, and visible handler implementations. Calling this a fully Fe-implemented browser runtime would overstate the boundary. |
| Second-application validation | **Achieved as runtime validation; QCGA algebra goal is partial** | `demos/webgpu-qcga3d-quadric/` uses the same canonical runtime and has `render`, `verify`, and genuine one-call Fe/Wasm `oracle` lanes. Regeneration at Sonatina `ac266c21` yields FNV `2368784280` with 1,624 colors; shared-runtime identity and QCGA asset preflight pass. The browser gate compares all 16,384 pixels. | The QCGA kernel itself explicitly says it is one fixed rotated quadric, not dense Cl(9,6) or a general QCGA engine. It has no runtime camera/quadric parameters, no recursive Garamon-style evaluator, and no CTFE-derived general operator plan. Its value here is actor/runtime reuse and a typed sparse contraction, not completion of the CGA planner for QCGA. |
| One-command build / serve / watch ergonomics | **Partial, externally gated** | `crates/fe/src/main.rs`, `web.rs`, and `web_serve.rs` implement real `fe web build` and `fe web serve`, including atomic bundle snapshots, source polling, static serving, and COOP/COEP/CORP headers. `demos/serve.sh <demo> --serve` composes generation/preflight with Trunk serving. | A clean checkout cannot currently build the `fe` command: workspace `Cargo.toml` pins Sonatina `150d327`, while current Fe code uses later float/SPIR-V/canonical-arena APIs. The reviewed `ac266c210cad7872fc98380a73b4ca363877bc1f` exists locally but GitHub reports `upload-pack: not our ref`; demo scripts mask this with Cargo path overlays. Publish/merge the backend commit, repin workspace dependencies, update `Cargo.lock`, then prove plain `cargo run -p fe -- web ...` without `SONATINA_DIR`, `--config patch`, or demo shell glue. Browser auto-reload is also not part of `fe web serve`; watch currently means atomic rebuild. |
| Sparse multivector usability and support-sized storage without syntax changes | **Partial** | `sparse_cl41_grade1.fe` proves support/grade masks, compact rank, cardinality, default zero, and present-only rejection. `sparse_coefficient_wasm.rs` executes default-zero sparse access. `sparse_type_driven_storage_wasm.rs` proves recursive `Storage<2>`/`Storage<5>` types and executable Wasm, while `sparse_conformal_constructor.rs` proves ordinary and FCO-derived semantic constructor façades. | These are closed fixtures/spikes, not a reusable standard-library API. The browser Schedule32 kernel still has demo-local `MvTF`, support logic, and provider definitions. Missing are a public support witness, ergonomic sparse construction/defaulting, support-derived storage selection integrated with CGA operations, and backend coverage for the resulting aggregate shapes. |
| Proportional HIR/CTFE, Wasm, SPIR-V/WGSL, and browser testing | **Partial** | HIR/type-function fixtures and CTFE tests cover recursive normalization and FCO; codegen tests execute Wasm and audit WGSL/SPIR-V; browser scripts perform actual Chromium acceptance and no-readback checks; runtime JS has adversarial unit tests. The commands above were rerun for the latest runtime changes, and both demo preflights pass. | Full workspace release CI has not been rerun at this head. Hardware WebGPU coverage is absent, and browser results are command output rather than durable CI artifacts. The external Sonatina pin prevents a clean locked workspace build, weakening reproducibility. |
| Precise generated/actorized/verified/performance/open documentation | **Partial** | `webgpu-cga-inversion/README.md`, `webgpu-qcga3d-quadric/README.md`, `CANONICAL_BROWSER_INTERFACE.md`, `FCO_SPARSE_CONSTRUCTOR_SPIKE.md`, and `SCHEDULE_STRATEGY_COMPARISON.md` document most boundaries explicitly. | The documentation is distributed and some generation wording predates tracked Schedule32 artifacts. This audit is the consolidated source until the dependency and sparse-planner slices update the user-facing runbook. |

## Typed actor semantics boundary

The current system is actorized at a precise browser boundary:

```text
Fe signatures + effects
  -> compiler-owned nominal ABI, schemas, lane intents, transfer policy
  -> generated protocol-v3 endpoint / Worker / MessagePort / GPU modules
  -> explicit application dispatchers and browser capability handlers
```

It is accurate to say that a generated Wasm actor communicates with an
explicit main-thread WebGPU actor through typed compiler-packaged transports.
It is not accurate to say that Fe itself currently implements Worker creation,
WebGPU device ownership, supervision, cancellation, or the browser event loop.

## Ranked next implementation slices

### 1. Publish and repin the browser-capable Sonatina backend

This is the highest-leverage prerequisite. Make the reviewed backend commit
reachable from the dependency URL (or merge it to an upstream reachable
revision), repin all workspace Sonatina crates and `Cargo.lock`, and run:

```text
cargo run -p fe -- web --help
cargo run -p fe -- web build <minimal.fe> --entry <entry> --mode render --out <dir>
cargo run -p fe -- web serve <minimal.fe> --entry <entry> --mode render --root <app>
```

Acceptance requires no local path override and no demo-specific generator
script. Then migrate one showcase to plain `fe web serve` and decide explicitly
whether opt-in browser reload belongs in this transparent server.

### 2. Turn the bounded Schedule32 planner into a reusable sparse CGA package

Move support masks, grade selection, compact rank/default-zero access,
support-sized storage, canonical `Term/Add/Zero` plan witnesses, and FCO
publication out of the demo fixture into one ordinary Fe API. Parameterize it
by metric/support/operator within the already proven finite bounds. Make the
browser kernel consume that package rather than carrying local copies.

Acceptance should prove one source of semantics across:

- recursive typed witness and support cardinality;
- support-sized runtime storage with absent coefficients defaulting to zero;
- direct/raw expansion, shared provider body, Wasm, and WGSL;
- the existing independent full-frame host oracle plus raw-80 algebra oracle.

This closes the largest remaining gap between a convincing one-off and the
requested Garamon/Conal-style facility.

### 3. Upgrade the second application from a runtime witness to a planner test

Keep QCGA as the shared-runtime proof, but replace the fixed scalarized quadric
with at least one parameterized sparse operator generated through the reusable
planner from slice 2. Camera and quadric coefficients should cross the same
compiler-derived interface; presentation stays no-readback, and explicit mode
compares one-call Wasm, WebGPU, and an independent host evaluator.

Acceptance must continue to pass the exact six-module runtime identity check.
Do not describe this as general QCGA until it supports more than the current
fixed paper-null contraction and documents the supported subalgebra.

## Current overall result

The exciting core is real: the browser executes a Fe-authored, typed
CTFE-derived 80-to-32 Cl(4,1) specialization interactively, and generated typed
actors connect its Wasm and WebGPU verification paths. The goal is not complete.
The clean toolchain is blocked on an unpublished backend revision, the sparse
planner/storage work remains fixture-level rather than a reusable Fe package,
and QCGA is presently a fixed second-runtime application rather than the same
recursive planner at a larger algebra.
