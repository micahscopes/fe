# Fe-native gallery and web-runtime composting plan

Status: active direction

Origin: honesty audit of Claude Code session
`e806786e-dff4-43c1-b25f-849ba82a8a02` and the subsequent Codex gallery work.

## Execution ledger (2026-08-11)

- Phase 0 is landed and pushed in `855b982a4`: canonical-gallery attribution,
  provenance, fixed-host hashing, a purity gate, corrected claims, and removal
  of the handwritten gallery source-viewer script.
- The first Phase 1 slice is implemented for both Mandelbrot actors. Their Fe
  `navigate` behaviors consume one owned, typed `SurfaceEvent`, return the
  complete ten-field Fe state record, and compile to a fixed versioned
  surface-transition export. The browser supplies raw facts and carries
  returned values; it does not normalize the wheel, perform pan/zoom math, or
  read a control manifest. No replacement JSON specification was introduced.
- Independent Wasmtime gates execute the brute transition over 3,520 stateful
  events and compare every result bit-for-bit with a separately derived Rust
  expansion oracle. The perturbational actor executes a mixed raw-event tape
  through the same fixed ABI, including its real resource slot. Negative gates
  reject partial state records, and release gallery precompilation validates
  both manifest-free control artifacts.
- Native/Cranelift event parity is still open. An attempted gate against the
  real checked-in actor exposed two named limitations in the pinned Sonatina
  backend: floating-point comparison instructions are not translated, and
  records/tuples returning more than two floating-point values require an
  indirect-return ABI. The adapter failed closed and was not retained as a
  sham parity claim. Until native lowering gains those features, the Wasmtime
  event oracle is browser-Wasm proof only; it must not be described as native
  parity.
- The first Phase 2/3 scheduling and resident-state slice is implemented
  without manifest growth.
  All eight parameterized canonical gallery actors declare `LatestPerFrame` as a
  Fe capability on `navigate`. The compiler now lowers that choice to the
  fixed v4 frame export: the generic host writes untouched 52-byte raw event
  records into exported memory and crosses into Wasm once at the presentation
  boundary; the generated wrapper accumulates movement/wheel facts, keeps the
  newest remaining facts, and invokes the authored Fe transition once. A
  deterministic host conformance tape proves the raw records remain separate
  in transport and a burst makes zero Fe calls while collecting, then exactly
  one Fe call and one render at flush. An independent Wasmtime three-event
  burst proves coalescing occurs in generated Wasm. CGA3D, QCGA, Desargues,
  plasma, gradient, and DEC join both Mandelbrots on the nominal
  `SurfaceEvent`, complete-state, manifest-free path. The generated module now
  keeps complete non-resource actor state in private Wasm globals. A fixed
  companion export seeds or
  explicitly replaces that state at initialization, extent, and restoration
  boundaries; frame calls carry only the raw batch and inert external resource
  slots. Slider and scripted edits enter that same transition as raw
  `event_kind`/`param_index`/`param_value` facts, after any older pending gesture,
  and never use state replacement for typed actors. JavaScript retains returned
  values only as a presentation mirror for GPU uniform upload.
- The fluent Phase 1 parameter interface is implemented in Fe without a new
  manifest protocol. `Param::{drag_x, drag_y, wheel, wheel_scale}` compose with
  a reflection/provider-derived `ApplyParamBindings` transition; parameter
  kind policy, bounds, robust angle wrapping, ties-to-even integer rounding,
  and extent values execute in Fe. A new fieldless-enum Wasm value lane is
  generic rather than `ParamKind`-specific. A standalone new-actor gate proves
  the mechanism needs only Fe source and that private binding/provider details
  do not leak into the generated JSON audit envelope. Pinch remains open.
- The fixed host has a deterministic 52-byte transport/order tape. Independent
  Wasmtime gates cover direct edits, all simple gallery bindings, both
  high-precision Mandelbrot transitions, provider name drift, and partial-state
  rejection. The complete 25-test gallery gate is green; the perturbational
  receipt remains 456,259-byte reference WGSL plus 45,239-byte fragment WGSL,
  and the brute 3,520-event tape remains bit-exact at every step.
- Cold multi-tile builds now reuse only content-keyed *clean* dependency
  diagnostic proofs across fresh compiler databases. The key covers the
  dependency's transitive Fe/config closure, resolved edges, workspace profile,
  arithmetic mode, and parser recovery mode; failures and DB-bound diagnostics
  are never cached. A cache-disabled release precompile built all ten gallery
  bundles in 105 seconds, with repeated shared checks reporting 0--1 ms, and
  the verifier accepted all ten modules and 34 publication files.
- The first generic resident component slice is implemented without a
  component-specific JSON protocol. `InitialState`, `ResidentTransition`, and
  `ProjectState` are Fe roles lowered to three fixed Wasm exports. The fixed
  `<fe-component>` adapter transports lifecycle/input facts plus bounded UTF-8
  text and applies a bounded Fe-authored DOM command stream. Fe owns visible,
  focus, prevent-default, text/value/checked/hidden/class/disabled effects and
  stable numeric repeat keys; the adapter owns standards event subscription,
  Wasm memory transport, validation, and keyed DOM reconciliation.
- A resident Fe TodoMVC actor is both a standalone example and a canonical
  gallery tile. Fe owns its bounded UTF-8 todo storage, monotonic keys, add,
  toggle/toggle-all, all/active/completed filters, edit/commit/cancel, destroy,
  clear-completed, lifecycle state, and complete projection. An independent
  Rust reducer compares semantic state and decoded effect operations at every
  step of a mixed UTF-8 event tape; it does not use byte equality as a behavior
  oracle. A real Chromium gate additionally exercises DOM visibility, native
  keyboard input, prevent-default, caret continuity, focus, surviving keyed
  identity, filter reconciliation, repeated disconnect/reconnect cleanup, and
  state continuity. That browser gate exposed and now guards three bugs that
  byte/hash checks could not: dropped `<template>` contents, focus overwritten
  by post-click browser defaults, and needless keyed-row moves detaching a
  focused input.
- The removed handwritten JavaScript code viewer is restored as a real
  resident Fe `SourceInspector`, both standalone and around the canonical
  gallery. Fe owns artifact-kind selection, open/loading/error state, opaque
  request identities, stale-completion rejection, text-vs-binary presentation,
  focus, Escape, and navigation cancellation. The fixed component host adds
  only same-origin URL/fetch realization plus `href`, text, and byte-count
  effects. Accepted response bodies are copied into Fe-owned resident memory;
  an independent Wasmtime tape caught and now guards against accidentally
  retaining the host's reusable scratch pointer. A Chromium gate opens actual
  authored Fe and compiler-generated WGSL/Wasm/manifest artifacts through
  composed shadow-DOM clicks and verifies Fe-owned lifecycle/presentation.
- Inspectable authored `.fe` files are now ordinary content-addressed
  publication assets. HTML `href` is rewritten in place and the deployment
  verifier checks the direct source digest; there is no asset JSON or new
  runtime manifest. This makes source inspection work on static Pages rather
  than only under the development file server.
- This component slice is not the Phase 5 end state. Todo page composition and
  numeric action/node/class/template declarations are still ordinary authored
  HTML; the component directory route still needs to supersede the render-lane
  probe; an attribute-mutation event ABI, persistence, unbounded collections,
  native backend parity, and Fe-generated page composition remain open. The
  example intentionally caps itself at 32 todos and 96 UTF-8 bytes per title
  while those richer storage interfaces are designed.
- Animation-frame/GPU-completion facts and their clock state machine have not
  yet been exposed as typed events to the resident Fe actor. Native/Cranelift
  parity, typed lifecycle/reactive streams, pointer capture, picking/messages,
  Fe page composition, and runtime-manifest removal remain open.

This ledger records achieved evidence, not a relaxation of the phases or the
Definition of done below.

## Goal

Make the gallery the canonical proof that rendering, application state, input
mapping, scheduling policy, picking, and page composition are authored in Fe.
Permit only compiler-generated artifacts and one fixed, standards-derived
browser host. Migrate or retire every per-demo JavaScript/Rust scaffold while
preserving independent gate oracles.

The target is not literally zero JavaScript bytes. Browsers currently expose
DOM, event-loop, and WebGPU objects through host APIs. The target is zero
application policy and zero demo knowledge in JavaScript, zero per-demo Rust
generation, and one readable Fe artifact from which the compiler derives the
rest.

## North-star attribution rule

One authored application artifact is the Fe program. Everything downstream --
Wasm, WGSL, manifests, adapters, control tables, page structure, and the live
loop -- must be either:

1. compiled or projected from Fe by the toolchain; or
2. part of one fixed, versioned, demo-blind host runtime shipped by the
   toolchain.

Two deliberate outsiders remain:

- the compiler/toolchain itself, currently Rust; and
- independent test oracles, whose independence supplies their proof value.

### Manifest and JSON protocol composting rule

The current render manifest is a transitional compiler-generated audit and
publication envelope, not the desired application ABI. Do not grow new JSON
specifications for events, state, scheduling, effects, resources, passes, or
page composition. Those contracts must be typed in Fe and lowered to generated
binary layouts or standards-derived host bindings. During the migration the
existing manifest may expose hashes, ownership, and debugging projections, but
the browser runtime must progressively stop interpreting its semantic payload.

The endpoint has no render manifest. A surface is one Fe-generated module or
artifact with typed, versioned exports for shaders, resources, pass structure,
controls, state, and effects. The fixed host receives only that artifact URL
and realizes the exported contract. HTML/module URLs and their content-addressed
names provide publication identity without a second application description.
Provenance is checked directly from compiler/source data at build time and may
be emitted as an optional attestation for reviewers, but is never a runtime
input.

## Honesty baseline

The current gallery is a collection of Fe render/control programs hosted by a
substantial generic JavaScript browser kernel. It is not yet an Fe-authored web
application or Fe-native event system.

### Genuinely Fe today

- Fractal, CGA, QCGA, DEC, gradient, and palette mathematics.
- GPU actor placement, stage roles, ordered pass graphs, and typed storage
  resources.
- WGSL emitted from Fe for every canonical gallery renderer.
- The brute Mandelbrot's fixed-precision orbit and adaptive precision policy.
- The perturbational Mandelbrot's `Fixed<8>` reference pass, binary32 delta
  pass, reanchoring/cancellation logic, and color policy.
- Control arithmetic in all eight parameterized actors' typed `navigate`
  behaviors: fluent affine bindings and parameter-kind policy, plus the
  Mandelbrots' specialized pan sensitivity, zoom curves, clamps, cursor
  anchoring, and high-precision center updates. These behaviors compile from
  Fe to Wasm, and their `LatestPerFrame` choice is declared in Fe.
- CTFE-projected parameter names, ranges, initial values, kinds, and extents.

### Fixed JavaScript host today

`crates/codegen/assets/render-runtime/fe-render-runtime.js` currently owns:

- DOM/custom-element construction and slider widgets;
- browser pointer/wheel listener registration;
- pointer capture, active-pointer state, and drag delta production;
- CSS-coordinate to backing-pixel conversion;
- legacy-only wheel normalization via `Math.sign(deltaY)` for examples not yet
  migrated to the canonical path;
- fixed raw-event batch memory transport plus legacy positional argument
  construction for examples not yet migrated;
- resident-state initialization/extent/restoration, returned presentation
  snapshots, plus legacy state couriering/result blitting for unmigrated paths;
- animation-frame/GPU-completion clock delivery and presentation gating;
- WebGPU adapter/device acquisition, loss recovery, buffer allocation, pipeline
  construction, pass encoding, and presentation;
- visibility/intersection lifecycle and poster/live transitions; and
- Fe-generated Wasm instantiation.

This runtime is fixed and demo-blind, which satisfies the original minimal-shim
rule, but it still owns semantics that the stronger Fe-expressibility goal now
asks us to absorb.

### Important corrections

- The canonical throttle is Fe-declared and compiler-lowered into generated
  Wasm. JavaScript buffers untouched raw records and supplies the
  `requestAnimationFrame`/GPU-completion clock; the Wasm wrapper coalesces and
  invokes the authored Fe transition once per permitted frame. Complete actor
  state is resident in private Wasm globals; JavaScript keeps only a GPU-upload
  mirror and supplies explicit initialization/extent/restoration. Slider and
  scripted changes are raw typed events handled by the same Fe transition. The
  browser clock state machine is not resident in Fe yet.
- Cursor-anchored pan/zoom mathematics is Fe; cursor acquisition,
  normalization, drag state, and timing are JavaScript.
- The gallery is not using Rust-Wasm to fake its renderers. `fe web dev` and
  precompile are native Rust toolchain operations. Browser Wasm artifacts are
  compiler output from Fe; live GPU rendering uses Fe-generated WGSL. The
  perturbational renderer's Wasm is its generated fixed Fe surface-transition
  export; its two render passes are Fe-generated WGSL.
- `std::reactive::{Event, Signal, Stream}` exists, but it is not on the
  gallery's execution path.
- The safe `std::web` facade intentionally does not yet expose callbacks,
  events, promises, or asynchronous operations.

## The recovered parameter interface

The v5 design intended fluent Fe declarations such as:

```fe
lambda: Param::unit(init: 0.15).wheel(per_notch: 0.01),
theta: Param::angle(init: 0.6).drag_x(per_px: 0.01),
zoom: Param::range(min: 0.5, max: 4.0, init: 1.6).pinch(),
```

The affine portion of that interface now lands as ordinary Fe library code:
`drag_x`, `drag_y`, `wheel`, and `wheel_scale`. A generic reflection provider
matches parameter/state labels and derives one complete-state transition, so
mouse, wheel, slider, scripted edits, and Wasmtime tests share the same Fe
policy path. Binding metadata is private Fe data and is not projected into the
runtime manifest. `pinch()` and typed multi-pointer facts remain to be added.

The narrower legacy `UpdateSurface` bridge still exists for compatibility:

```fe
fn update_view(self, dx: f32, dy: f32, dzoom: f32, mx: f32, my: f32)
```

That compatibility path still recognizes exactly the argument names `dx`,
`dy`, `dzoom`, `mx`, and `my`; accepts only scalar `f32` results; and maps the
result to a leading subset of actor state. Its manifest and runtime mediate the
call with string-valued `drag`, `wheel`, `pointer`, `state`, and `resource`
sources. No curated sketch uses it now: all eight parameterized canonical
actors use the nominal typed event and complete-state transition instead.

The capability-marked Fe behavior is a sound intermediate step. The
reserved-name, positional, scalar-only ABI is bespoke surface area to remove.

## Gallery and examples baseline

### Canonical `demos/gallery.html`

- Ten tiles are sourced from Fe ingots.
- Eight tiles have typed Fe `SurfaceTransition` controls and Fe-declared
  `LatestPerFrame` scheduling. None emits the legacy JSON `control` block.
- Known-color and rollcall are pure Fe-derived GPU graphs with no Wasm module.
- Perturbational Mandelbrot is a two-pass Fe GPU graph; its Wasm is the typed
  Fe control lane only.
- No tile has its own `main.js`.
- The page remains authored HTML/CSS. The former handwritten inline
  source/WGSL/manifest viewer has been removed. Restoring it as an actor-like
  Fe web component is the first planned consumer of the page/component path;
  the fixed runtime still consumes the transitional render manifest today.
- `qcga_pencil` remains excluded because vertex/fragment plus typed
  pick/message lanes have not landed.
- DEC contains Worker/message-shaped Fe functions, but the gallery currently
  exercises only its fragment actor and direct parameter sliders.

### Legacy strata

Published-looking older examples still carry considerable per-demo scaffolding:

- `demos/webgpu-cga3d-interactive`
- `demos/webgpu-qcga-interactive`
- `demos/webgpu-desargues-interactive`
- `demos/webgpu-clifford-interactive`
- `demos/webgpu-mandelbrot-interactive`
- `demos/webgpu-mandelbrot`
- `demos/webgpu-cga-inversion`
- `demos/webgpu-qcga3d-quadric`
- `demos/rollcall`
- `demos/webgpu-keystone`
- `demos/shared`
- `demos/fe-sandbox`

There are also old `crates/codegen/examples/gen_*.rs` demo generators. Some
directories are valuable conformance fixtures or tooling demonstrations, but
they must not be presented as equivalent to the canonical Fe application
path.

## Work plan

### Phase 0: make attribution enforceable

1. Declare one canonical gallery and label every other demo as canonical,
   legacy, host conformance, compiler tooling, or independent oracle.
2. Emit a provenance section per artifact containing:
   - authored Fe roots;
   - generated WGSL/Wasm/manifests;
   - compiler version;
   - fixed runtime hash;
   - host imports and capabilities; and
   - any non-Fe authored inputs.
   This temporarily extends the existing generated manifest so the honesty
   gate can ship before the typed host ABI; it must not become a new semantic
   JSON protocol.
3. Display an exact runtime badge such as `Fe GPU + Fe control Wasm / fixed
   browser host` rather than a broad "compiled from Fe" claim.
4. Add a canonical-demo CI gate rejecting:
   - per-demo JavaScript;
   - handwritten WGSL;
   - committed generated Wasm/manifests used as source;
   - per-demo Rust generators; and
   - undeclared host behavior.
5. Correct stale claims, especially `gallery.html`'s `script-free` description.

Exit condition: a reviewer can mechanically answer what is Fe, generated, fixed
host code, or an oracle for every published tile.

### Phase 1: replace the reserved gesture ABI

Chosen first-slice ABI (no new manifest protocol):

- `std::web::SurfaceEvent` is an attributed Fe record of raw browser facts:
  pointer, movement delta, raw wheel delta and mode, buttons, timestamp, and
  backing extent, plus untouched direct-parameter proposals identified by a
  fixed event-kind discriminant and declaration-order parameter index.
- A `SurfaceTransition` behavior takes exactly that one context record and
  returns a named Fe record matching the actor's complete non-resource state.
- The compiler validates both record shapes from resolved semantic types and
  exports the transition under one fixed, versioned Wasm ABI symbol. The source
  behavior name is ordinary Fe and is not part of the host contract.
- The fixed host discovers that export directly and transports the fixed event
  layout. The generated module commits the complete returned state in
  declaration order; the host keeps only a presentation mirror. Neither side
  consults a `control` manifest block, argument-source strings, or result-field
  names.
- The old `UpdateSurface` projection remains only as a migration compatibility
  path for examples not yet converted, and is deleted after the gallery sweep.

1. Introduce typed Fe browser/surface facts, conceptually:

   ```fe
   #[web_surface_event]
   struct SurfaceEvent {
       pointer_x: f32,
       pointer_y: f32,
       delta_x: f32,
       delta_y: f32,
       wheel_delta: f32,
       wheel_mode: u32,
       buttons: u32,
       timestamp: f32,
       width: f32,
       height: f32,
       event_kind: u32,
       param_index: u32,
       param_value: f32,
   }
   ```

   The first executable slice keeps this boundary record deliberately flat and
   takes it as `event: own SurfaceEvent`. That gives the Wasm value lane thirteen
   direct scalar leaves and keeps Fe handlers readable; nested convenience
   records can remain ordinary library views built inside Fe when the general
   canonical record lane is ready.

2. Replace `UpdateSurface(dx, dy, dzoom, mx, my)` with a capability-typed
   transition taking a record and returning a typed state record.
3. Derive argument/result layout from resolved Fe types and reflection. Remove:
   - reserved argument names;
   - scalar-only `f32` restrictions;
   - leading-subset result mapping; and
   - the runtime switch over string-valued argument sources.
4. Restore fluent param bindings for simple affine interactions, but implement
   them as Fe library/provider-generated transitions so slider, mouse, scripted
   input, and tests share one state path.
5. Carry raw wheel `delta`, `deltaMode`, surface dimensions, and pointer facts
   across the host boundary. Fe owns normalization and policy.
6. Model pointer capture as a typed host effect or an explicit fixed
   surface-policy default, rather than hidden demo behavior.

First migrations: brute Mandelbrot and perturbational Mandelbrot, followed by
CGA3D, QCGA, Desargues, and plasma.

Exit condition: adding a new gesture never requires compiler-recognized names or
runtime source-string cases.

### Phase 2: make scheduling policy Fe-owned

1. Expose animation-frame ticks, GPU completion, resize, visibility, and device
   loss as typed host events.
2. Add Fe policies/combinators for:
   - latest-per-frame;
   - sample;
   - throttle;
   - debounce;
   - accumulate; and
   - drop/backpressure behavior.
3. Batch/coalesce browser input and cross into Wasm once per presentation frame
   when the Fe policy permits, instead of invoking Wasm for every pointer event
   and throttling only the GPU render.
4. Test scheduling against deterministic event/frame tapes in native and Wasm
   execution.

Exit condition: the gallery's throttle choice and state machine are authored
and tested in Fe; JavaScript merely supplies frame/GPU-completion facts.

### Phase 3: complete canonical values, callbacks, and runtime control effects

1. Finish canonical allocator/PostReturn and rich-record transport.
2. Connect generated WebIDL callback adapters to compiled Fe callback bodies.
3. Add the MIR suspension/re-entry transform for resumable Fe tasks.
4. Make actor state resident in a live Fe instance instead of couriered in
   JavaScript uniform arrays.
5. Provide browser implementations of Fe `EventSource` for pointer, wheel,
   resize, animation-frame, visibility, and device-loss streams.
6. Put `std::reactive::{Event, Stream, Signal}` on the real gallery path.
7. Preserve affine subscription/cancellation semantics across the host boundary.

Exit condition: browser events resume a resident Fe actor; Fe owns combination,
cancellation, state transitions, and scheduling policy.

### Phase 4: typed messages, picking, and actor placement

1. Finish canonical message lanes for GPU actors and pass graphs.
2. Implement hit testing as an Fe `pick` lane.
3. Bring `qcga_pencil` into the canonical gallery through
   pick -> drag -> solve -> render messages.
4. Exercise DEC's Worker/message lanes rather than showcasing only its fragment
   behavior.
5. Route MainThread/Worker placement from Fe-declared capabilities.
6. Keep the browser runtime geometry- and demo-blind.

Exit condition: `qcga_pencil` works end to end without per-demo JavaScript and
without runtime knowledge of its control points or geometry.

### Phase 5: compose the page in Fe

1. Land the planned `WebPage` actor and `const compose() -> Page` projection.
2. Generate mounts, captions, source/provenance links, and layout declarations
   from Fe.
3. Replace the inline source viewer with either:
   - a general Fe `SourceInspector` actor; or
   - a fixed host component selected declaratively from Fe.
4. Retire handwritten `demos/gallery.html` after a compatibility window.
5. Keep CSS initially as a fixed, selectable compiler-shipped theme; grow Fe
   page/style vocabulary only where it removes repeated application policy.

Exit condition: one Fe page entrypoint composes the entire gallery.

### Phase 6: shrink the JavaScript WebGPU kernel

1. Project typed WebGPU command/resource plans from Fe actor structure into a
   generated binary/host layout so the host executes data rather than
   re-deriving policy or interpreting a JSON object model.
2. Expose standards-derived WebGPU host imports with opaque resource handles.
3. Move resource lifetime, pass selection, recovery decisions, and presentation
   scheduling into a Fe orchestration actor.
4. Generate JavaScript import adapters from WebIDL/host ABI metadata.
5. Retain only the irreducible browser object/promise adapter as fixed host code.
6. Remove semantic runtime dependence on the render-manifest JSON schema. Keep
   no replacement manifest: publish one Fe-generated surface artifact whose
   typed exports carry the contract, and delete the manifest fetch/parser/path.

Exit condition: the fixed JavaScript contains browser API realization but no
application scheduling, state, geometry, parameter, or pass-selection policy.

### Phase 7: simplify and generalize the Fe demos

#### Mandelbrot family

1. Replace eight flattened center fields and nine-value replies with a reusable
   `DeepView<Expansion<N>>`-shaped state once typed record ABI support lands.
2. Generalize reference-orbit storage, reanchoring, cancellation/glitch
   detection, iteration budget, and palette into Fe library policies.
3. Keep perturbation at one fixed high-precision reference tier; it does not
   need the brute renderer's per-pixel limb ladder.
4. For the brute renderer, derive precision-specialized pipelines from a Fe
   policy and select one once per frame, avoiding a dynamic precision branch in
   every fragment while keeping the authored Fe compact.
5. Preserve generated-code/performance inspection so abstraction does not hide
   accidental branching or compilation blowups.

#### Other examples

1. Extract reusable `PanZoom`, `Orbit`, `RangeControl`, `Camera`, and
   `PickDrag` Fe constructions.
2. Make every rich example a consumer of those general forms rather than a new
   wiring implementation.
3. Require a new-demo generalization test: a new interactive actor must need
   only Fe source and no runtime/compiler change.

Exit condition: the raw Fe demos are short and readable while expanding through
CTFE/providers/reflection into the full typed program.

### Phase 8: migrate or retire legacy scaffolding

For each legacy directory:

1. Identify its unique Fe value, if any.
2. Port that value onto the canonical actor/runtime path.
3. Delete per-demo `main.js`, `live-pump.js`, handwritten interfaces, committed
   generated shaders, `generate.sh`, and `gen_*.rs` generation.
4. Move true host conformance work into tests/fixtures and label it as such.
5. Preserve independent Rust/JS oracles only where their independence is the
   point; never ship them as application machinery.
6. Treat `fe-sandbox` honestly as compiler tooling. Its Rust-Wasm browser
   compiler is an intentional toolchain artifact, not evidence that the hosted
   application itself is Fe-native.

Exit condition: no legacy application is presented as canonical, and every
remaining non-Fe example file has an explicit toolchain, host-test, or oracle
reason to exist.

## Cross-cutting gates

- **Attribution:** every browser behavior has one named Fe/generated/fixed-host/
  oracle owner.
- **No bespoke names:** compiler and runtime contain no demo names or reserved
  gesture parameter names.
- **Generalization:** a new interactive demo changes only Fe source.
- **Event parity:** deterministic event tapes produce identical Fe state across
  native and browser-Wasm execution.
- **Browser E2E:** real pointer/wheel/pick/resize/device-loss paths are tested.
- **Performance:** input batching, frame latency, GPU submissions, shader size,
  compile time, and pipeline-switch cost have regression budgets.
- **Surface-area burn-down:** each phase deletes or centralizes more bespoke
  code than it introduces.
- **Independent proof:** Rust/JS oracles remain independent and never become
  runtime dependencies.

## Definition of done

The campaign is complete only when:

- adding an interactive gallery demo changes only Fe source and ordinary
  assets;
- rendering, state, input mapping, scheduling policy, picking, and page
  composition are authored in Fe;
- no reserved gesture argument vocabulary remains;
- pointer/wheel tapes exercise the same typed Fe transition in native tests and
  browsers;
- throttle/coalescing policy is Fe-authored and Fe-tested;
- `qcga_pencil` and DEC message/Worker lanes run through the general mechanism;
- the gallery is composed in Fe;
- remaining JavaScript is generated or one fixed standards-derived host
  adapter;
- no runtime render manifest exists, and no JSON schema carries application
  events, state, scheduling, effects, resource/pass semantics, artifact
  location, or page composition;
- Rust remains only in the toolchain and independent gates; and
- every legacy showcase is migrated, reclassified, or retired.

## Recommended first execution slice

1. Land the provenance/classification gate and correct attribution text.
2. Design the typed `SurfaceEvent`/state ABI with no reserved names.
3. Implement it through the existing synchronous Fe-Wasm control lane.
4. Migrate and parity-test both Mandelbrot actors.
5. Add Fe-declared `latest_per_frame` policy, initially realized by the generic
   host.
6. Then land runtime control effects so that policy becomes Fe-executed and
   actor state becomes resident without changing the public surface contract.

This slice gives an immediate honesty and API improvement while preserving a
straight upgrade path to the full Fe-native event model.
