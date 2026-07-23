# mb2 browser CGA goal: completion evidence audit

Status: audited against integrated head `d1558645f` and
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
implemented. The goal remains incomplete for three concrete reasons:

1. the required Sonatina backend revision is not published and a clean locked
   `fe web` build therefore still needs the explicit overlay;
2. FCO can now phase-safely reflect a local subject-only normalized
   `Add<Term<I>, ... Zero>` plan, but the public ingot's imported,
   forwarded-parameter `SparsePlan` remains outside that first base-graph
   slice, so application providers still rescan bounded candidates;
3. clean full-workspace and hardware-WebGPU CI evidence is absent.

## Evidence matrix

| Goal requirement | Status | Authoritative evidence | Exact boundary / remaining work |
| --- | --- | --- | --- |
| Fe-authored interactive Cl(4,1) browser render | **Achieved for the bounded showcase** | `demos/webgpu-cga-inversion/gen-schedule32/kernel.fe` contains the complete DE render, runtime inversion center, `CanonicalCga as Sandwich`, and the five conformal-vector outputs. `demos/webgpu-cga-inversion/main.js` owns only interaction and WebGPU submission. `bun demos/webgpu-cga-inversion/actor-interface.test.mjs` executes the generated one-call Wasm frame. | This proves this Cl(4,1) showcase, not a general geometric-algebra library or arbitrary operator compiler. Interaction math and concrete WebGPU device handlers remain explicit JavaScript host policy. |
| Typed CTFE-derived Garamon/Fuchs-Théry-style `S * P * S` specialization | **Achieved for Schedule32; partial as a reusable facility** | In `gen-schedule32/kernel.fe`, ordinary Fe const functions scan all 80 ordered triples into eight 24-bit browser-profile `u32` keep words; shared recursive `SparsePlan<K0,K1,K2,K3,K4,K5,K6,K7,80,32>` materializes the exact typed witness, and the provider-emitted `Sandwich` implementation uses the same canonical helpers. QCGA uses that same bounded mask planner for its 144→12 plan. `crates/codegen/tests/hybrid_cga_plan_e2e.rs` compares the specialization with an independent Rust raw-80 evaluator and a separately authored recursive generic Fe `MvTF<5>` evaluator. | The shared planner handles at most 192 candidate identities in ascending order. Domain metrics, keep predicates, coefficient semantics, output grouping, and FCO evaluators remain application-specific; FCO does not discover sharing or interpret the ground plan tree directly. |
| Browser-profile WebGPU execution of the specialization | **Achieved** | Generated `frag.wgsl`, `layout.json`, and `actor/shader.wgsl` live under `demos/webgpu-cga-inversion/gen-schedule32/`. `python3 demos/webgpu-cga-inversion/verify-assets.py` checks browser WGSL shape, absence of leaked CTFE/provider tokens, the single runtime raymarch loop, and required canonical artifacts. `smoke-chrome.sh` is the real-browser gate. Last observed real Chromium/SwiftShader acceptance in this worktree produced bit-equal Wasm/GPU hash `3470936828`. | The checked-in files and preflight prove artifact shape, while the smoke command proves execution only when run. No durable hardware-GPU CI result is stored; the recorded run used SwiftShader. |
| Fast interactive presentation with no per-frame readback by default | **Achieved for submission behavior; performance claim remains partial** | `README.md` documents the persistent pipeline, rAF coalescing, and typed input-buffer updates. After initial acceptance, normal interaction does not invoke Wasm or read back GPU data. `?verify=off` is stricter: no Wasm/reference fetch, Worker, oracle render, or readback. `CGA_SMOKE_VERIFY=off CGA_SMOKE_BENCHMARK=continuous ... smoke-chrome.sh` asserts those counters. Last observed 256x256 SwiftShader run submitted at about 60 Hz with average CPU submit near 0.202 ms and `gpuReadbackCount == 0`. | Default mode intentionally performs one startup verification readback; only presentation mode is zero-readback from startup. The no-readback benchmark measures submitted-frame cadence, not GPU completion. Timestamp mode is explicit and necessarily reads timestamp results. No portable performance threshold or hardware-adapter result is established. |
| Explicit Wasm/WebGPU verification against an independent oracle | **Achieved for the pinned full frame and explicit current-view checks** | Browser acceptance compares every GPU byte with the one-call Fe/Worker/Wasm frame; Schedule32 currently agrees at FNV `3470936828`. Before writing `reference.json`, `crates/codegen/examples/gen_cga_schedule32_vec5_demo.rs::oracle` independently reimplements the complete camera, 72-step raymarch, conformal inversion, torus DE, hit policy, shading, and packing in Rust `f32`; generation requires bit-exact agreement for all 16,384 Wasm pixels and records `oracle_agreement.exact_mismatches == 0`. `hybrid_cga_plan_e2e.rs` separately compares the algebraic specialization with a raw-80 Rust evaluator and a generic recursive Fe evaluator. | Browser verification is transitive but exact: independently gated host oracle = generated reference = Fe/Wasm = WebGPU. Interactive current-view verification compares Wasm and GPU for arbitrary controls but does not rerun the Rust host oracle in the browser; the independent host gate is the pinned generation view. |
| Compiler-derived canonical request/response interface | **Achieved for canonical interface v2** | `crates/codegen/assets/canonical-interface.js`, `crates/codegen/src/web_bundle.rs`, and generated `actor/interface.{js,d.ts}` derive nominal records, offsets, codecs, lane schemas, effect intents, transfer policy, and abort-aware dispatch context from Fe signatures plus emitted Wasm layout. `demos/shared/canonical-interface.test.mjs` covers codecs, intent routing, arena reset, ownership, and adversarial cancellation. `CANONICAL_BROWSER_INTERFACE.md` specifies the ABI. | Version 2 deliberately supports fixed records, scalars, owned bytes, strings, and request cancellation. General lists, variants, resources, futures/streams, borrowed/shared memory, and persistent resources are not implemented. |
| Robust generated Worker/MessagePort adapter | **Achieved for protocol-v3 lifecycle and bounded single-Worker supervision** | The six runtime modules implement typed correlation, exact schemas, bounded endpoint and host in-flight counts, per-lane latest-pending host queues, correlated cancellation/`AbortSignal` propagation, late-result suppression, owned-buffer transfer, startup timeout, sanitized errors, and epoch ordering. The opt-in policy documented in `BOUNDED_ACTOR_SUPERVISION.md` adds a rolling restart budget, one bounded backoff timer, startup/runtime classification, terminal propagation, frozen observation events, and an inspectable state snapshot. Its adversarial Bun suite proves crash-loop exhaustion, cancellation and close during backoff, retiring-worker exclusion, stale-result rejection, and recovery epoch order; all browser-runtime `*.test.mjs` suites pass. `CGA_SMOKE_LIFECYCLE=1` separately drives generated composition in Chromium under mailbox pressure, explicit cancellation, one manual restart, and post-restart verification. | This is explicit supervision of one generated Module Worker, not a general supervision tree. There is intentionally no persistence, automatic multi-worker scheduler, or shared-memory ownership claim. Concrete Worker creation and WebGPU handlers remain explicit host implementations. |
| Wasm actor connected to WebGPU actor through a reusable, non-magical runtime | **Achieved** | `module-worker-actor.js`, `gpu-actor.js`, and `actor-router.js` are compiler-packaged, application-neutral modules. Fe effect intent selects lane partitions and placement; applications explicitly provide concrete dispatchers/device handlers. `python3 demos/shared/verify_cga_runtime_reuse.py` proves Schedule32 and QCGA package byte-identical copies of all six runtime modules. | Fe describes ABI and execution intent; JavaScript owns browser capabilities, Worker/MessagePort construction, GPU device lifetime, and visible handler implementations. Calling this a fully Fe-implemented browser runtime would overstate the boundary. |
| Second-application validation | **Achieved for a bounded planner-backed QCGA operator** | `demos/webgpu-qcga3d-quadric/` uses the same canonical runtime and compiler-derived `render`, `verify`, and one-call Fe/Wasm `oracle` lanes. Ordinary Fe CTFE enumerates the 12×12 sparse paper-null support, prunes 144 candidates through the shared mask planner to an exact recursive 12-term plan, and one FCO aggregate executes the contraction. The live browser request carries 15 typed camera/quadric parameters derived from compiler layout. Five representative camera/quadric controls submit typed frames directly to WebGPU with rAF coalescing; Real Chromium presentation fetches no Wasm/reference, creates no Worker, and performs zero readback. Explicit verification compares all 16,384 Wasm/GPU pixels at FNV `2368784280`. | This is a parameterized sparse null-incidence operator, not dense Cl(9,6) or a general QCGA product engine. The provider repeats three bounded metric cases because its interpreter cannot yet branch through the ordinary const helper. The UI deliberately exposes representative coefficients rather than all 15 request fields. |
| One-command build / serve / watch ergonomics | **Partial, externally gated** | `crates/fe/src/main.rs`, `web.rs`, and `web_serve.rs` implement real `fe web build` and `fe web serve`, including atomic bundle snapshots, source polling, explicit opt-in browser reload, static serving, and COOP/COEP/CORP headers. Failed compiles preserve the last good page and bundle. The temporary `demos/fe-web` launcher proves direct compiler WebBundle generation of all 11 flagship artifacts, while `with-browser-cargo.sh` is now the sole six-crate overlay owner for Schedule32, QCGA, and generic demo serving; its contract tests cover exact backend provenance and lock restoration. See `FE_WEB_BACKEND_REPRODUCIBILITY.md` for the measured dependency audit. | A clean checkout still cannot build the plain `fe` command: workspace `Cargo.toml` pins Sonatina `150d327`, while current Fe code uses later float/SPIR-V/canonical-arena APIs. The reviewed `ac266c210cad7872fc98380a73b4ca363877bc1f` exists locally but is not advertised by the audited GitHub remotes. Publish/merge it, repin workspace dependencies and `Cargo.lock`, remove the compatibility launcher, then prove plain `fe web build/serve` without `SONATINA_DIR` or Cargo patches. Vendoring is supported but would duplicate a multi-megabyte six-crate backend to replace the existing 528 KiB reviewed patch series. The specialized demos still require their provenance/oracle generators rather than only the generic WebBundle command. |
| Sparse multivector usability and support-sized storage without syntax changes | **Achieved for the bounded substrate; partial as a general algebra package** | The public ordinary-Fe `ingots/sparse_clifford` package owns bounded `BladeSet` support algebra, `Zero/Term/Add`, the bounded mask planner, CTFE compact rank, recursive `SparseStorage<N>`/`SparseIndex<rank>`, default-zero, and present-only APIs. A real dependent ingot imports it, materializes a three-term plan spanning candidate 140, and executes import-free Wasm. Schedule32 and QCGA now compile as real application ingots depending on that package and publish reproducible `app/fe.toml` plus `app/src/lib.fe` beside dependency-backed kernels. | Domain constructors, metrics, coefficient semantics, output grouping, and FCO evaluators remain application-specific. |
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

`verify_cga_runtime_reuse.py` correctly failed closed in the fresh worktree
because QCGA's ignored generated manifest was absent. The immediately preceding
forced QCGA generation and Chromium acceptance at the same promoted sources
proved runtime identity and pixel equality, but this distinction matters:
QCGA browser evidence is reproducible generation output, not a tracked bundle.

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

### 2. Add phase-safe normalized FCO plan reflection

The first base-graph semantic island is implemented for local subject-only
recursive plans:

```text
Plan<3>
  -> Add<Term<2>, Add<Term<1>, Add<Term<0>, Zero>>>
  -> builder.ty<GroundPlan>().normalized_preorder_types()
```

`provider_ground_type_inspection.rs` proves exact candidate order `[2, 1, 0]`,
resolved constructor-identity comparison, preservation of the old syntactic
API, and deterministic rejection of forwarded parameters. The SGK dependency
guard prevents the executor from reaching merged semantic normalization.
It also proves that
`builder.ty<GroundPlan>().preorder_types()` observes the source alias rather
than normalized `Term` nodes.

Complete the base-graph semantic island specified by
`FCO_PLAN_BRIDGE_OPTIONS.md` without weakening ordinary merged analysis:

1. extend base-only resolution across explicit ingot imports without reading
   generated items, impl selection, associated-type normalization, or the
   merged scope graph;
2. add a ground substitution arena for forwarded invariant type and const
   parameters so the public `SparsePlan<mask..., count, survivors>` can unfold;
3. retain the current immutable normalized handles, node/fuel limits, SGK
   guard, and ordinary semantic resolution policy;
4. check generated output later through ordinary merged analysis.

Acceptance requires a small `SparsePlan` provider to visit every normalized
term in canonical order, publish an executable method, reject a phase-unsafe
alias/trait case deterministically, and preserve normal type-function
diagnostics and cache/fuel behavior.

### 3. Consume the reflected plan without duplicating candidate discovery

After slice 2, migrate Schedule32 and QCGA FCO providers from rescanning their
bounded candidate universes to traversing the exact normalized plan produced
by `ingots/sparse_clifford`. Domain-specific metric signs, operand projection,
coefficient magnitude, output routing, and reduction topology remain explicit
application policy; the shared package should not pretend those semantics are
universal.

Acceptance requires:

- Schedule32 still proves raw-80 = typed-plan = generic-recursive Fe and retains
  the 80-to-32 witness;
- QCGA still proves raw 144-candidate semantics = reflected 12-term execution;
- generated Wasm/WGSL remain browser-profile valid and the pinned full-frame
  hashes/oracles remain exact;
- both apps continue packaging byte-identical actor runtime modules;
- provider sources no longer independently rediscover the keep set.

This is the remaining step from a shared planner representation to a genuinely
shared reflected execution route. It still does not imply arbitrary dense
Clifford/QCGA products or automatic common-subexpression discovery.

### 4. Close durable verification and performance evidence

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

The goal is still active. Clean direct tooling awaits backend publication;
FCO still rescans candidates because normalized recursive plan reflection is
not phase-safe yet; and durable full-workspace/hardware-WebGPU evidence remains
to be produced.
