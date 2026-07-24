# mb2 browser CGA goal: completion evidence audit

Status: audited against integrated head `17fe07f60` and
`/workspace/mb2-HANDOFF.md` on 2026-07-23. The handoff remains useful design
history, but its 2026-07-22 checkpoint predates the Schedule32 and
planner-backed QCGA promotions; this matrix is authoritative for current
completion claims. Re-run the cited gates after later implementation commits
rather than treating a source hash as a permanent completion marker.

This is a completion audit, not a milestone announcement. `Achieved` means the
current tree contains direct evidence for the stated scope. `Partial` means a
real implementation exists but the goal asks for a broader or stronger claim.
`Missing` means the named result is not present. A schema preflight is never
treated as browser execution evidence.

Current verdict: the bounded browser showcase, exact verification path,
canonical actor runtime, second application, shared bounded sparse planner,
support-sized storage, real public Fe ingot, and opt-in live reload are
implemented. The goal remains incomplete for four concrete reasons:

1. the required Sonatina backend revision is not published and a clean locked
   `fe web` build therefore still needs the explicit overlay;
2. the public authored recursive `CliffordGp<Cl41Metric>` now derives the exact
   signed canonical-50-to-32 plan through a symbolic coefficient
   interpretation, but that plan has not yet replaced the older
   demo-specific Schedule32 provider in the browser hot path;
3. the reusable algebra and actor substrates remain bounded: application code
   still owns domain metrics/reductions and concrete browser capability
   handlers, while general sparse products, automatic shared-DAG discovery,
   and transferable resources/value streams are incomplete;
4. clean full-workspace and hardware-WebGPU CI evidence is absent.

## Evidence matrix

| Goal requirement | Status | Authoritative evidence | Exact boundary / remaining work |
| --- | --- | --- | --- |
| Fe-authored interactive Cl(4,1) browser render | **Achieved for the bounded showcase** | `demos/webgpu-cga-inversion/gen-schedule32/kernel.fe` contains the complete DE render, runtime inversion center, `CanonicalCga as Sandwich`, and the five conformal-vector outputs. `demos/webgpu-cga-inversion/main.js` owns only interaction and WebGPU submission. `bun demos/webgpu-cga-inversion/actor-interface.test.mjs` executes the generated one-call Wasm frame. | This proves this Cl(4,1) showcase, not a general geometric-algebra library or arbitrary operator compiler. Interaction math and concrete WebGPU device handlers remain explicit JavaScript host policy. |
| Typed CTFE-derived Garamon/Fuchs-Théry-style `S * P * S` specialization | **Achieved as an exact ordinary-ingot proof; browser-provider migration pending** | The public ordinary ingot exposes one coefficient-generic recursive `CliffordGp<Cl41Metric>` recurrence. Its `f32` interpretation remains exact against raw80; its support-semiring interpretation derives the conservative five-blade grade plan; and its bounded signed-symbolic interpretation invokes that same public recurrence exactly twice, canonicalizes the 50 `Sym²(S)×P` monomials, cancels to 32 survivors, and feeds computed masks into public `SparsePlan`. `recursive_clifford_canonical50.rs` compares the full ordered 32×7 candidate/output/left/point/right/magnitude/sign sequence against an independent Rust raw80 evaluator, validates import-free R1 Wasm, and forbids runtime wide carriers and R2-only operations. Source-contract checks forbid copied keep/sign/output tables, GP-sign helpers, raw triple scans, Python, source concatenation, and provider builders. | This proves exact derivation and executable typed-plan reflection, but the tracked browser bundle still consumes its older demo-owned 80-candidate Schedule32 source. The canonical-50 order differs, so browser migration must use the independent geometric oracle rather than preserve the old term order or hash artificially. The exact proof currently takes about 10 minutes and roughly 1 GiB at CTFE; memoization/shared evaluation is a major ergonomics requirement before routine rebuilds. |
| Browser-profile WebGPU execution of the specialization | **Achieved for the older exact Schedule32 provider; canonical-50 migration pending** | Generated `frag.wgsl`, `layout.json`, and `actor/shader.wgsl` live under `demos/webgpu-cga-inversion/gen-schedule32/`. `python3 demos/webgpu-cga-inversion/verify-assets.py` checks browser WGSL shape, absence of leaked CTFE/provider tokens, the single runtime raymarch loop, and required canonical artifacts. `smoke-chrome.sh` is the real-browser gate. Last observed real Chromium/SwiftShader acceptance in this worktree produced bit-equal Wasm/GPU hash `3470936828`. | These artifacts still derive their plan through the demo-owned 80-candidate helpers rather than the new public recurrence interpretation. The checked-in files and preflight prove artifact shape, while the smoke command proves execution only when run. No durable hardware-GPU CI result is stored; the recorded run used SwiftShader. |
| Fast interactive presentation with no per-frame readback by default | **Achieved for submission behavior; performance claim remains partial** | `README.md` documents the persistent pipeline, rAF coalescing, and typed input-buffer updates. After initial acceptance, normal interaction does not invoke Wasm or read back GPU data. `?verify=off` is stricter: no Wasm/reference fetch, Worker, oracle render, or readback. `CGA_SMOKE_VERIFY=off CGA_SMOKE_BENCHMARK=continuous ... smoke-chrome.sh` asserts those counters. Last observed 256x256 SwiftShader run submitted at about 60 Hz with average CPU submit near 0.202 ms and `gpuReadbackCount == 0`. | Default mode intentionally performs one startup verification readback; only presentation mode is zero-readback from startup. The no-readback benchmark measures submitted-frame cadence, not GPU completion. Timestamp mode is explicit and necessarily reads timestamp results. No portable performance threshold or hardware-adapter result is established. |
| Explicit Wasm/WebGPU verification against an independent oracle | **Achieved for the pinned full frame and explicit current-view checks** | Browser acceptance compares every GPU byte with the one-call Fe/Worker/Wasm frame; Schedule32 currently agrees at FNV `3470936828`. Before writing `reference.json`, `crates/codegen/examples/gen_cga_schedule32_vec5_demo.rs::oracle` independently reimplements the complete camera, 72-step raymarch, conformal inversion, torus DE, hit policy, shading, and packing in Rust `f32`; generation requires bit-exact agreement for all 16,384 Wasm pixels and records `oracle_agreement.exact_mismatches == 0`. `hybrid_cga_plan_e2e.rs` separately compares the algebraic specialization with a raw-80 Rust evaluator and a generic recursive Fe evaluator. | Browser verification is transitive but exact: independently gated host oracle = generated reference = Fe/Wasm = WebGPU. Interactive current-view verification compares Wasm and GPU for arbitrary controls but does not rerun the Rust host oracle in the browser; the independent host gate is the pinned generation view. |
| Compiler-derived canonical request/response interface | **Achieved for canonical interface v4** | `crates/codegen/assets/canonical-interface.js`, `crates/codegen/src/web_bundle.rs`, and generated `actor/interface.{js,d.ts}` derive nominal records, bounded named-record variants, and exact nominal bounded `u32`/`f32` lists, including offsets, codecs, lane schemas, effect intents, transfer policy, and abort-aware dispatch context. List codecs use element-specific typed arrays, enforce `MAX`, copy across the Wasm arena, and recursively transfer owned full-span buffers. Wasmtime executes Fe-side typed `MemPtr<u32/f32>` reads and covers aligned owned response copying plus invalid results; Bun covers nested record/variant transfer and adversarial descriptors. `CANONICAL_BROWSER_INTERFACE.md` specifies the ABI. | Version 4 provides a bounded borrowed view, not a general collection: Fe can read the pointed-to element, but typed pointer arithmetic/indexing and a typed list allocator remain absent. Read-only aggregate reification admits only static single-leaf field projections and fails closed on whole/multi-leaf/dynamic/mutating/address-taking uses. Unbounded/nested lists, tuple variants, resources, futures/streams, borrowed/shared memory, and persistent resources remain unsupported. |
| Robust generated Worker/MessagePort adapter | **Achieved for packaged runtime v4 lifecycle, bounded admission backpressure, and single-Worker supervision** | The eight runtime modules implement typed correlation, exact schemas, bounded endpoint and host in-flight counts, per-lane latest-pending host queues, correlated cancellation/`AbortSignal` propagation, late-result suppression, owned-buffer transfer, startup timeout, sanitized errors, epoch ordering, and compiler-packaged host/client composition. The endpoint's opt-in `saturation: "wait"` policy admits at most `maxPending` sent requests and retains at most `maxQueued` unsent requests in FIFO order; transferable ownership remains with the caller until admission, and queued abort/close/reset never sends, cancels, or detaches the payload. Default saturation still rejects immediately. `BOUNDED_ACTOR_BACKPRESSURE.md` and its adversarial tests pin overflow, ownership, synchronous send failure, drain, and lifecycle behavior. The opt-in supervision policy adds a rolling restart budget, bounded backoff, startup/runtime classification, terminal propagation, and epoch inspection. All seven browser-runtime suites pass. | This is request-admission backpressure, not a value stream, persistent transferable-resource handle, or general supervision tree. There is intentionally no persistence, automatic multi-worker scheduler, or shared-memory ownership claim. Concrete WebGPU capability handlers remain explicit host implementations. The internal MessagePort wire protocol remains its separately versioned protocol v3. |
| Wasm actor connected to WebGPU actor through a reusable, non-magical runtime | **Achieved** | `module-worker-actor.js`, `gpu-actor.js`, `actor-router.js`, generated `worker-host.js`, and generated `actor-client.js` are compiler-packaged, application-neutral modules. Fe effect intent selects lane partitions and placement; the generated composition derives Wasm and main-thread-host ownership from those intents without application lane lists. Applications explicitly supply only concrete WebGPU handlers. `python3 demos/shared/verify_cga_runtime_reuse.py` proves Schedule32 and QCGA package byte-identical copies of all eight runtime modules. | Fe describes ABI and execution intent; JavaScript still owns browser capabilities, Worker/MessagePort construction, GPU device lifetime, and visible handler implementations. Calling this a fully Fe-implemented browser runtime would overstate the boundary. |
| Second-application validation | **Achieved for a bounded planner-backed QCGA operator** | `demos/webgpu-qcga3d-quadric/` uses the same canonical runtime and compiler-derived `render`, `verify`, and one-call Fe/Wasm `oracle` lanes. Ordinary Fe CTFE enumerates the 12×12 sparse paper-null support, prunes 144 candidates through named computed keep/count constants to an exact imported recursive 12-term plan, and `QcgaIncidenceProvider` traverses only those normalized `Term<Candidate>` leaves. Raw-144 Wasm equivalence and browser-valid WGSL remain exact. Five representative controls submit typed frames directly to WebGPU; Chromium presentation fetches no Wasm/reference, creates no Worker, and performs zero readback. Explicit verification compares all 16,384 Wasm/GPU pixels at FNV `2368784280`. | This is a parameterized sparse null-incidence operator, not dense Cl(9,6) or a general QCGA product engine. Metric signs and scalar contraction policy remain explicit application semantics. The UI deliberately exposes representative coefficients rather than all 15 request fields. |
| One-command build / serve / watch ergonomics | **Partial, externally gated** | `crates/fe/src/main.rs`, `web.rs`, and `web_serve.rs` implement real `fe web build` and `fe web serve`, including atomic bundle snapshots, source polling, explicit opt-in browser reload, static serving, and COOP/COEP/CORP headers. Failed compiles preserve the last good page and bundle. The temporary `demos/fe-web` launcher proves direct compiler WebBundle generation of all 11 flagship artifacts, while `with-browser-cargo.sh` is now the sole six-crate overlay owner for Schedule32, QCGA, and generic demo serving; its contract tests cover exact backend provenance and lock restoration. See `FE_WEB_BACKEND_REPRODUCIBILITY.md` for the measured dependency audit. | A clean checkout still cannot build the plain `fe` command: workspace `Cargo.toml` pins Sonatina `150d327`, while current Fe code uses later float/SPIR-V/canonical-arena APIs. The reviewed `ac266c210cad7872fc98380a73b4ca363877bc1f` exists locally but is not advertised by the audited GitHub remotes. Publish/merge it, repin workspace dependencies and `Cargo.lock`, remove the compatibility launcher, then prove plain `fe web build/serve` without `SONATINA_DIR` or Cargo patches. Vendoring is supported but would duplicate a multi-megabyte six-crate backend to replace the existing 528 KiB reviewed patch series. The specialized demos still require their provenance/oracle generators rather than only the generic WebBundle command. |
| Sparse multivector usability and support-sized storage without syntax changes | **Achieved for the bounded substrate; partial as a general algebra package** | The public ordinary-Fe `ingots/sparse_clifford` package owns bounded `BladeSet` support algebra, `Zero/Term/Add`, the bounded mask planner, CTFE compact rank, recursive `SparseStorage<N>`/`SparseIndex<rank>`, default-zero, present-only APIs, and the authored recursive `CliffordMvF32<N>`/`CliffordGp<M>` semantic baseline. A real dependent ingot imports it and executes through Wasm and browser-profile WGSL. Schedule32 and QCGA compile as real application ingots depending on that package. | The authored recurrence is dense and intentionally independent of the optimized plan. Domain constructors, support propagation through composed high-level products, grade pruning, output grouping, and general FCO evaluators remain incomplete. |
| Proportional HIR/CTFE, Wasm, SPIR-V/WGSL, and browser testing | **Partial** | HIR/type-function fixtures and CTFE tests cover recursive normalization and FCO; codegen tests execute Wasm and audit WGSL/SPIR-V; browser scripts perform actual Chromium acceptance and no-readback checks; runtime JS has adversarial unit tests. The commands above were rerun for the latest runtime changes, and both demo preflights pass. | Full workspace release CI has not been rerun at this head. Hardware WebGPU coverage is absent, and browser results are command output rather than durable CI artifacts. The external Sonatina pin prevents a clean locked workspace build, weakening reproducibility. |
| Precise generated/actorized/verified/performance/open documentation | **Partial** | `webgpu-cga-inversion/README.md`, `webgpu-qcga3d-quadric/README.md`, `QCGA_SPARSE_PLANNER.md`, `CANONICAL_BROWSER_INTERFACE.md`, `FCO_SPARSE_CONSTRUCTOR_SPIKE.md`, `FE_WEB_BACKEND_REPRODUCIBILITY.md`, and this audit document the important boundaries explicitly. | Schedule32 artifacts are tracked, while QCGA `gen/` is intentionally ignored and must be regenerated before its reuse/preflight commands work in a fresh worktree. Performance evidence is still SwiftShader-specific and command-local rather than a durable hardware result. |

## Recent audit reruns

The following inexpensive gates were rerun directly in a fresh worktree:

- `demos/shared/canonical-interface.test.mjs`: passed;
- `webgpu-cga-inversion/actor-interface.test.mjs`: passed, including the
  generated one-call Wasm frame;
- `webgpu-cga-inversion/actor-lifecycle.test.mjs`: passed;
- both remote `ls-remote` checks returned no `ac266c21` ref, while the local
  clean reviewed checkout remains exactly at that commit.

After unifying the sparse prelude, the focused support, storage, Schedule32,
DE-WGSL, QCGA-Wasm, QCGA-WGSL, asset-preflight, and Python acceptance gates all
passed. A broader rerun then exposed and repaired two stale shared-source
assumptions: forced inlining had erased the public
`cga_semantic_plan_hybrid` Wasm export, and the recursive-Vec5 WGSL test still
constructed the pre-storage sparse records. The repaired full hybrid
raw-80/typed-plan/generic-Fe equivalence test and recursive-Vec5 browser-WGSL
test both pass. The `fe web serve` server suite passes all seven live-reload,
atomic-publication, mount, and response-header tests.

At `bf8b1e1e9`, the imported dependent-plan gate reflected all 32 canonical
survivors in ascending order and the reviewed Sonatina overlay executed the
resulting dependent ingot through Wasm. Canonical interface v3 passed all 11
Rust layout/derivation/fail-closed tests and its direct JavaScript codec test.
All seven browser-runtime suites passed, including the adversarial
ready-boundary supervision recovery case. Fresh Schedule32 and QCGA bundles
were generated from that compiler state. Real Chromium/SwiftShader Schedule32
acceptance matched all 16,384 Wasm/GPU pixels at FNV `3470936828`; its strict
presentation path created zero Workers, invoked zero Wasm oracle renders, and
performed zero color or timestamp readbacks. QCGA acceptance likewise matched
all pixels at FNV `2368784280`; mutating the cross-term control submitted a new
typed GPU frame; strict presentation mode created zero Workers and performed
zero readbacks. The subsequent shared-scalar hardening reran the complete
ground-plan/provider suite: 21/21 passed.

At the reflected-provider promotion, Schedule32 retained exact
raw-80/typed/generic/FCO Wasm equivalence and browser-valid WGSL while removing
the provider's `0..80` scan. QCGA retained raw-144/reflected Wasm equivalence,
WGSL shape `(14 add, 4 sub, 24 mul, 1 call, 0 loop)`, FNV `2368784280`, and
1,624 colors while removing its `0..144` scan. Final Chromium/SwiftShader
acceptance from regenerated assets again produced Schedule32 FNV `3470936828`
and QCGA FNV `2368784280`; both strict presentation paths created zero Workers
and performed zero readbacks. The Schedule32 256×256 continuous path submitted
120/120 sampled frames at 60.002 Hz with 0.162 ms average CPU submit time.
An actor-composition rerun exposed and fixed a same-turn canonical restart
race: calls started before `restart()` now reject at a synchronous lifecycle
fence instead of slipping onto the replacement epoch. All seven runtime suites
and the QCGA live actor test pass after regeneration.

At `563b44a43`, the compiler-owned WebBundle grew generated
`actor-client.js` and `worker-host.js` modules. Schedule32 and QCGA deleted
their handwritten Worker bootstraps and now use composition derived from the
generated Fe lane intents. On the exact integrated source, Chromium/SwiftShader
again produced Schedule32 FNV `3470936828` and QCGA FNV `2368784280`.
Both strict presentation paths remained interactive while creating no Worker,
invoking no Wasm oracle, and performing no GPU or timestamp readback.

`verify_cga_runtime_reuse.py` correctly failed closed in the fresh worktree
because QCGA's ignored generated manifest was absent. The immediately preceding
forced QCGA generation and Chromium acceptance at the same promoted sources
proved runtime identity and pixel equality, but this distinction matters:
QCGA browser evidence is reproducible generation output, not a tracked bundle.

At `17fe07f60`, the authored/reflected bridge compiled two ordinary ingots
independently and compared all 32 output blades for two deterministic inputs.
The public recursive `CliffordGp<Cl41Metric>`, authoritative reflected
Schedule32, and independent raw-80 Rust evaluator agreed exactly. Measured
output shape was 331,527 bytes of RMIR, 175 calls, and 16,489-byte Wasm for the
authored semantic recurrence versus 51,155 bytes of RMIR, one call, and
926-byte Wasm for reflected Schedule32. Regenerating the evidence-only
coefficient selector changed no render Wasm, WGSL, actor runtime, interface, or
manifest artifact, so the bridge is not part of the browser hot path.

At `b4ef7374a`, packaged actor runtime v4 added bounded opt-in FIFO admission
backpressure and regenerated the tracked Schedule32 bundle. The authoritative
preflight accepts v4 and rejects stale package versions; Schedule32 and the
fresh ignored QCGA bundle package byte-identical copies of all eight runtime
modules. Both asset preflights, all seven runtime suites, and the distinct
internal MessagePort protocol-v3 test pass. QCGA remains exact at FNV
`2368784280` with 1,624 colors.

At `6e71fc924`, the public recursive carrier became coefficient-generic while
retaining the `f32` API. An ordinary dependent ingot interprets the same public
`CliffordGp` recurrence over the support semiring, with exactly two product
calls, and carries its CTFE aggregate result through grade projection into a
computed `SparsePlan`. The focused gate executes the ascending five-blade plan
through Wasm; on the authoritative integrated head it passed 1/1 in 48.59
seconds. The full compatibility suite also retains raw80 Wasm equality and
browser-profile WGSL/Naga validity. This proves recurrence-to-plan const/type
normalization; the later canonical-50 proof adds exact signed cancellation.

At `510804d54`, the signed-symbolic interpretation closes that remaining
algebra gap. The exact ordinary-ingot gate passed in 591.16 seconds and compared
all 224 fields of its 32 normalized terms with independent raw80. The compiler
changes that make the reflected plan executable are reusable and fail closed:
declared nominal const-parameter widths reach ground-plan evaluation,
type-level integer truncation is preserved as a deferred cast until outer
substitution, positive base-FCO `usize` to `i32` narrowing rejects values above
`i32::MAX`, and mismatched generic argument kinds reject. On the authoritative
dirty worktree, the nine base-const evaluator tests plus the exact narrowing
and mismatched-kind regressions pass while the pre-existing unstaged CTFE
formatting change remains outside the integration commit.

## Typed actor semantics boundary

The current system is actorized at a precise browser boundary:

```text
Fe signatures + effects
  -> compiler-owned nominal ABI, schemas, lane intents, transfer policy
  -> generated runtime-v4 endpoint / Worker / MessagePort / GPU modules
  -> explicit application dispatchers and browser capability handlers
```

It is accurate to say that a generated Wasm actor communicates with an
explicit main-thread WebGPU actor through typed compiler-packaged transports.
It is not accurate to say that Fe itself currently implements Worker creation,
WebGPU device ownership, supervision, cancellation, or the browser event loop.

## Ranked remaining-work runbook

Completed slices are intentionally absent from this list. In particular, do
not reopen shared `Zero/Term/Add`, bounded mask planning, support-sized storage,
public-ingot packaging, representative QCGA interaction, actor-runtime reuse,
or live reload unless a regression contradicts their cited gates.

### 1. Publish, repin, and prove the browser-capable backend

This is the only external prerequisite and the highest-leverage remaining
slice. Follow `FE_WEB_BACKEND_REPRODUCIBILITY.md`:

1. make reviewed Sonatina `ac266c210cad7872fc98380a73b4ca363877bc1f`
   (or a reviewed upstream replacement) fetchable from the dependency URL;
2. repin the four direct workspace dependencies and all six locked Sonatina
   packages to one durable revision;
3. from a fresh checkout with no `SONATINA_DIR`, Cargo patch, or warm private
   cache, run:

   ```text
   cargo check --workspace --locked
   cargo test -p fe --bin fe web_serve
   cargo test -p fe-codegen --test fco_cga80_direct_lanes \
     --test fco_cga80_direct_de_spirv --test wasm_e2e --test spirv_e2e
   cargo run -p fe -- web build <minimal.fe-or-ingot> \
     --entry <entry> --mode render --out <new-dir>
   cargo run -p fe -- web serve <minimal.fe-or-ingot> \
     --entry <entry> --mode render --root <app>
   ```

Acceptance requires plain locked Cargo and plain `fe web`; only then remove the
compatibility launcher and six-crate Cargo patching layer. Do not remove the
specialized flagship generators: their independent oracle, plan witness,
provenance, and legacy artifact publication are additional jobs, not evidence
that generic `fe web` cannot compile an ingot.

### 2. Generalize reflected execution and generated actor composition

Phase-safe imported-plan reflection is now complete: base-only resolution
crosses explicit ingot imports, forwarded invariant type and const parameters
are evaluated with typed `u32`/`usize` semantics and lexical state restoration,
and the real dependent `SparsePlan<...,80,32>` visits the exact canonical
survivors before generating executable Wasm. Resolver, freeze, phase-safety,
and duplicate-import fail-closed regressions pin the boundary.

Schedule32 and QCGA now both consume their exact normalized imported plans;
neither provider independently rediscovers its keep set. Named computed QCGA
mask/count consts are evaluated with shared fuel, depth, cycle, width, and kind
guards. Their domain-specific metric signs, operand projection, coefficient
magnitude, output routing, and reduction topology remain explicit application
policy rather than being mislabeled universal algebra.

The tree/compact/shared-DAG comparison is complete for the current bounded
operators. Schedule32's unshared tree uses 440 multiplies and produces
4,031-byte Wasm; compact terms and the fully searched DAG both use 320
multiplies and produce 3,991-byte Wasm. There are 32 unique cross-term product
keys, so the only 12 reuse edges are the already-known magnitude-two terms and
the DAG adds no further sharing. QCGA's 12-term incidence plan likewise has 12
unique product keys and no sharing edges. These are useful negative results,
not evidence against sharing in larger composed programs.

The next reusable slices are:

- migrate the browser provider from its demo-owned 80-candidate schedule to
  the exact canonical-50 recurrence-derived witness, preserving the independent
  geometric oracle and keeping wide symbolic coefficients out of runtime WGSL;
- memoize/share the two expensive symbolic CTFE projections so exact
  specialization is practical in an incremental browser build;
- move flagship runtime operands onto the public support-derived
  `SparseStorage`/default-zero API where that improves rather than obscures
  generated code;
- extend support propagation and grade pruning through composed products
  without claiming arbitrary dense Clifford or QCGA support prematurely.

### 3. Close durable verification and performance evidence

Once the clean backend is available, run the full locked workspace/release
suite and publish durable browser artifacts for:

- Schedule32 verification and strict `verify=off` no-Wasm/no-Worker/no-readback;
- interactive QCGA presentation and explicit exact verification;
- lifecycle pressure, cancellation, restart, and runtime-identity gates;
- at least one hardware WebGPU adapter in addition to SwiftShader.

Record adapter identity, resolution, submission and GPU-completion timing
separately. A 60 Hz submission loop or CPU submit time is not a hardware GPU
throughput claim. Keep QCGA described as the bounded paper-null incidence
operator until broader algebra support is independently implemented and
tested.

## Overall status

The central result is no longer speculative: browser-profile WebGPU executes
the Fe-authored CTFE-derived Schedule32 Cl(4,1) specialization interactively,
the default presentation path performs no per-frame readback, explicit
verification is bit-exact through Fe/Wasm and an independent host oracle, and
the reusable generated actor runtime is exercised by both Schedule32 and a
typed interactive QCGA operator. Sparse representation and planning now live
in a real public Fe ingot.

The goal is still active. Clean direct tooling awaits backend publication; the
exact signed specialization is proven but not yet wired into the browser
provider; general composed sparse execution and richer resource/stream
substrate remain incomplete; and durable full-workspace/hardware-WebGPU
evidence remains to be produced.
