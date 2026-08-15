# Fe-native gallery and web-runtime composting plan

Status: active direction

Origin: honesty audit of Claude Code session
`e806786e-dff4-43c1-b25f-849ba82a8a02` and the subsequent Codex gallery work.

## Execution ledger (2026-08-12)

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
- The native/Cranelift blockers found by the first real actor attempt are now
  fixed upstream and pinned exactly. Sonatina lowers ordered `f32`
  comparisons, uses a caller-owned result buffer for more than two scalar
  returns, and legalizes the mixed-width enum comparisons Fe emits. Its full
  23-test Cranelift backend target passes. Fe's narrow checked native surface
  ABI now executes the actual `ParamBindings` actor transition and the exact
  ordinary Fe `LatestPerFrame` function selected structurally from
  `SurfaceScheduling<P>`. One stateful tape compares every one of the eight
  typed event variants, parameter edits, wheel directions, wrap/round/clamp,
  extent changes, visibility, frame/completion backpressure, and device
  loss/recovery three ways: native Cranelift, the generated resident browser
  Wasm wrappers, and independent Rust semantic models. Application state is
  bit-exact at every transition; native policy state and browser decisions
  match the independent policy model at every boundary. The full native Fe
  target is 7/7 in 1,505.92 seconds, including the existing
  native/Wasmtime/circomlib Poseidon receipt. This lands generic typed
  event/scheduling native parity; native tapes for each high-precision
  Mandelbrot transition remain an explicit Phase 7 migration gate rather than
  an inferred claim.
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
- The canonical gallery body is now one role-selected Fe `GalleryPage` actor.
  Its const `GalleryBuilder` expands through a typed `std::web::page` operation
  vocabulary into the header, ordered tiles, captions, source links, all ten
  render declarations, and two resident-component mounts. The HTML body
  retains only one inert `data-fe-page`
  source declaration; there is no page Wasm or page JSON manifest. The fixed
  precompiler realizes standards nodes and validates ordering, balanced trees,
  attributes, mount identities, and sources before ordinary program discovery.
  A semantic projector fixture, negative structure gate, deployment verifier,
  and Chromium gate independently prove the typed result and actual behavior.
- TodoMVC and SourceInspector now each declare a self-less const
  `ComponentComposition` behavior beside their resident reducer. The facade
  analyzes each module once, then emits its resident Wasm and projects its
  typed initial light-DOM fragment without JSON or a second compiler pass.
  Gallery and standalone HTML now carry empty mounts rather than duplicated
  component markup. Component-local `id`/`for` pairs are generically prefixed
  from the mount identity, so the Fe module stays reusable without global-ID
  collisions or gallery-specific strings. Semantic projection, mount/actor
  agreement, exact structural checks, and both standalone and gallery Chromium
  tapes guard this interface. On the measured release build, a warm full
  gallery precompile reused all ten render bundles and projected the page plus
  both component views in 4.8 seconds; this is evidence, not yet a formal
  performance budget.
- Component actions and DOM parts no longer rely on parallel hand-authored
  number tables. Semantic fieldless action enums derive their closed transport
  identity through FCO; typed part records derive opaque `Part<Role>` handles
  through field reflection. Text, input, checkbox, link, repeat, visibility,
  and class operations require the corresponding role at Fe checking time.
  Provider-synthesized methods now preserve a trait method's `const` qualifier,
  and exported Wasm entrypoints validate every host-supplied fieldless-enum
  leaf before application code can observe it. Independent reducer oracles
  compare semantic state/effects rather than generated bytes.
- Component event transport now has separate semantic-action and opaque
  resource-request lanes. The fixed host enforces nested actor ownership for
  both directions: composed events stop at the nearest `<fe-component>`, and
  visibility/focus/node/template effects cannot resolve through a nested
  component even when its independently derived part numbers coincide. The
  real gallery Chromium tape caught the latter bug when SourceInspector's
  ready source bytes were initially written into TodoMVC node 9; it now guards
  both nested-event exclusion and projection isolation while still exercising
  detached keyed-row construction.
- Native page projection now initializes the page's real Fe ingot and local
  dependency graph before invoking the same typed projection API. This lets
  `GalleryPage` consume SourceInspector's exported `InspectorAction` directly;
  there is one Fe action vocabulary rather than duplicated enums or matching
  integers. A focused native-ingot gate proves that dependency source is in the
  compiler-derived watch inventory, while virtual/browser compilation retains
  its filesystem-free facade.
- Development diagnostics remain structured through the rebuild coordinator
  and browser event stream. The terminal consumer now renders the actual
  compiler severity/code, file, line/column, source label, and notes on initial
  failure, and emits hot-rebuild failures through `fe_web` tracing while
  retaining the immutable last-good publication. A source-line regression gate
  prevents the previous generic-only message from returning.
- The perturbational Mandelbrot's Fe-authored initial expansion now represents
  center `(-1.1723286, 0.29582354)` and zoom `3.89e-8`. Its dedicated compile
  gate validates every expansion word, the zoom, control Wasm, and complete GPU
  pass graph.
- Both Mandelbrot renderers now consume the same branch-free
  `fmath::PixelSample::ordered_2x2` construction. It places one subpixel sample
  in each quadrant of every aligned 2x2 pixel block while retaining exactly one
  expensive orbit per pixel; there is no duplicated LoD ladder and no
  per-sample precision branch. They also share one Fe `escape_palette` policy.
  The exact Fixed renderer returns an `EscapeSample` whose classification and
  count remain wholly integer, with only the first escaped magnitude projected
  through the highest 13 fractional bits for continuous presentation color.
  A Wasmtime sampling oracle independently proves sample containment, all four
  quadrants, centroid, and plane orientation. The existing BigInt Fixed oracle
  now proves both count and presentation magnitude across all four production
  tiers (14,288 arithmetic cases plus 40 escape cases), rather than comparing
  generated bytes. The deep-center oracle remains green for a strictly ordered
  512-pixel row. Browser-profile WGSL validation and the complete graph receipt
  are green: exact WGSL 391,796 bytes, reference 456,259, perturb fragment
  45,400, combined perturb graph 501,659; the perturb fragment grew 161 bytes
  and the exact shader 6,017 bytes without multiplying modeled orbit work. A
  release gallery precompile completed in 104.5 seconds and verified 12 Fe
  modules / 50 deployment files. Actual offscreen GPU execution is unavailable
  in this container (`wgpu` reports no active adapter), so that leg remains an
  honest external/browser requirement rather than a claimed local pass.
- A new canonical `raymarch` tile is a complete Fe-authored 3D signed-distance
  renderer rather than a handwritten Shadertoy port hidden in JavaScript/WGSL.
  Its 336-line actor owns a 112-step primary march, the smooth-union
  torus/core/satellite/floor distance estimator, finite-difference normals,
  five-tap ambient occlusion, a 28-step soft shadow, material/sky/fog/gamma
  policy, and typed yaw/pitch/distance/morph interaction. The host remains the
  same demo-blind surface adapter. Generic `Vec3` operations and an `OrbitRay`
  camera construction moved into `fmath`, so the application states the scene
  and can seed later 3D examples without copying camera math. The shared
  one-orbit ordered pixel sampling policy is reused outside Mandelbrot for the
  first time. An independent Rust scalar model compares 1,029 field samples and
  four complete rays against the production Fe functions executed in Wasmtime.
  The canonical compile gate additionally executes the resident camera
  transition from raw events and browser-validates the 83,496-byte WGSL under a
  100 kB budget; control Wasm is 4,931 bytes. The Fe-composed gallery now
  projects 11 render actors plus two resident components, and its release
  artifact verifies 13 Fe modules / 54 files with both component browser tapes
  green. Actual pixel execution retains the same local no-adapter qualification
  recorded above.
- The surface boundary no longer exposes its browser/presentation identity as
  an application-visible integer. `SurfaceEventKind` is an append-only Fe
  fieldless enum covering gesture, direct edit, animation-frame, GPU-complete,
  visible/hidden, and device lost/recovered facts. The compiler derives its
  scalar position and variant bound from the resolved Fe record, carries that
  bound only into the generated Wasm wrapper, and traps an out-of-range host
  tag before authored Fe can observe it; no manifest field or number table was
  added to an example. The fixed adapter appends the real rAF timestamp to the
  untouched input batch, so generated Wasm still coalesces a burst and crosses
  the frame boundary once, and routes completion/visibility/device boundaries
  through the same typed resident transition. An independent Fe fixture gives
  all eleven variants distinct semantic receipts under Wasmtime, proves a
  gesture plus frame preserves accumulated movement, and proves tag eleven
  traps. The fixed-host tape separately verifies the 52-byte transport and
  exact call boundary. Both Mandelbrot semantic gates remain green, including
  the 3,520-event bit-exact tape; the raymarch control Wasm grew 137 bytes to
  5,068 bytes. A cold release gallery precompile completed in 112.3 seconds and
  verified 13 Fe modules / 54 files with both Chromium component tapes green.
  At that checkpoint the typed facts were exposed, but the
  `requestAnimationFrame`/GPU-completion dirty/presenting state machine and its
  presentation decision still remained in JavaScript.
- The canonical `LatestPerFrame` state machine is now ordinary resident Fe.
  `SurfaceScheduleEvent`, `SurfaceScheduleState`, and `SurfaceScheduleStep` are
  nominal typed records; `LatestPerFrame::decide` owns presenting, visibility,
  device-loss backpressure, and the `present`/`request_frame` decisions. Each
  scheduled behavior selects the policy as
  `SurfaceScheduling<LatestPerFrame>`, and the compiler rejects a missing or
  mismatched policy rather than silently falling back to JavaScript. Generic
  Wasm lowering validates the fieldless event enum, retains the state prefix in
  private globals, hides the authored behavior name, and exposes only the two
  decisions through fixed `fe_surface_schedule_v1`; no manifest field or JSON
  policy table was added. The fixed host retains raw queue storage and browser
  rAF/promise realization, supplies frame/completion/visibility/device facts,
  and obeys Fe's replies. Its old dirty/presenting branch is now reached only by
  legacy artifacts without the policy export. An independent Wasmtime tape
  covers visibility, frame admission, repeated-frame rejection, GPU completion,
  device loss/recovery, hidden retention, and invalid-tag trapping. A separate
  five-test fixed-host tape proves untouched 52-byte input, one application
  transition per permitted gesture frame, queued input across an unresolved GPU
  submission, and exact obedience to completion-driven frame requests. All 11
  Fe sources selecting `LatestPerFrame` (nine canonical render actors and two
  generalization/oracle fixtures) carry the typed policy. The resource-bearing
  perturbation graph remained green in 427.22 seconds with unchanged semantic
  shader receipts; raymarch WGSL remained 83,496 bytes while its control Wasm
  became 5,853 bytes. A cold-cache release site precompile built 11 render
  bundles in 116.37 seconds and verified 13 Fe modules / 54 files; both real
  Chromium component/gallery tapes passed. The full 62-test codegen unit suite
  and the 3,520-event bit-exact Mandelbrot transition oracle also passed against
  this final contract. This completes the Fe-owned
  latest-per-frame decision loop, not all of Phase 2. At that checkpoint,
  compiler-derived elimination of the uniform per-actor scheduling behavior
  and moving heterogeneous input ordering wholly into generated Wasm remained
  the immediate follow-up burn-down.
- `SurfaceScheduling<P>` now selects the resident policy implementation all the
  way through without a per-actor wrapper behavior. The nine canonical actors
  and two generalization/oracle fixtures declare only
  `uses (SurfaceTransition, SurfaceScheduling<LatestPerFrame>)` on their
  application transition; none imports the schedule event/state/step records or
  spells a `schedule` function. From the nominal `P`, the compiler finds the
  unique public inherent Fe function whose semantic arguments/results are the
  marked `SurfaceScheduleEvent`, `SurfaceScheduleState`, and
  `SurfaceScheduleStep` records. Its source name and argument labels are not
  recognized, and either state/event order is accepted. The exact semantic
  function is rooted from its dependency ingot as a compiler-internal Wasm
  root, reconciled to its emitted package symbol, hidden, and called only by
  fixed `fe_surface_schedule_v1`; a marker-identical policy with no structural
  implementation fails closed. The standard policy is deliberately named
  `LatestPerFrame::decide`, demonstrating that `step` is not an ABI word.
  Heterogeneous application input ordering also moved out of JavaScript. The
  generated batch wrapper retains a homogeneous gesture fast path that invokes
  the authored Fe transition once, but folds mixed gesture/direct-edit records
  through that same transition in source order inside one host-to-Wasm call.
  The host's eager pre-edit flush was deleted. A scheduled typed artifact
  missing its Fe policy export now fails at boot and cannot enter the legacy
  JavaScript dirty/presenting scheduler; that compatibility branch remains only
  for genuinely unscheduled legacy lanes. Independent Wasmtime receipts prove
  both the one-call gesture fast path and that a newer parameter edit cannot
  swallow an older gesture; the real `ParamBindings` fixture additionally
  proves all non-edited gesture state survives the ordered fold. Six fixed-host
  tests prove the raw two-event queue crosses only at Fe's admitted frame and
  that scheduled fallback is rejected. The full 62-test codegen suite passed in
  185.18 seconds and all 28 serialized gallery gates passed in 2,168.06 seconds,
  including the 3,520-event bit-exact Mandelbrot tape and unchanged shader
  receipts. Raymarch remains 83,496 bytes of WGSL and is now 6,307 bytes of
  control Wasm; perturbation control Wasm is 4,872 bytes. A fresh release site
  precompile built 11 render bundles in 111.997 seconds and verified 13 Fe
  modules / 54 files; both real Chromium SourceInspector/gallery and TodoMVC
  tapes passed. Sample/throttle/debounce/accumulate policy families and native
  event parity remain open Phase 2 work.
- Resident scheduling is no longer a compiler-enumerated `LatestPerFrame`
  special case. `GpuSchedule`, `#[gpu_schedule(latest_per_frame)]`, and the
  policy-named transition export were deleted; the binary surface now exposes
  only policy-neutral `fe_surface_transition_scheduled_v1` and
  `fe_surface_schedule_v2`. Any nominal `P` selected through
  `SurfaceScheduling<P>` is accepted when it has exactly one public ordinary
  Fe function with the structural schedule event/state/step shape. Same-leaf
  method names no longer destabilize this selection: the wrapper roots and
  hides the exact semantic runtime-instance identity after package-wide symbol
  disambiguation, rather than guessing an emitted name.
- The fixed host now notifies the resident Fe policy when untouched gesture or
  direct-edit input enters the raw queue and requests a browser frame only when
  Fe says to do so. `SurfaceScheduleState` gained private timing/count memory,
  and `SurfaceScheduleStep` gained the typed `SurfaceQueueAction` effect:
  `Retain`, `KeepLatest`, or `Drop`. JavaScript only realizes that bounded queue
  operation. No policy name, timing value, queue rule, manifest field, or JSON
  table crosses the application boundary. Ordinary Fe implementations now
  provide latest-per-frame, sample-latest, 32 ms throttle, 80 ms debounce,
  four-event/64 ms bounded accumulation, and drop-while-presenting behavior.
  Immutable `SurfaceScheduleStep::{with_queue, with_deadline, defer}`
  combinators keep the policy bodies concise without introducing aggregate
  references the current Wasm value lane cannot yet lower. These are concrete
  standard policies; parameterized policy constructors remain a later library/
  generic-instance refinement rather than a compiler or host vocabulary.
- Five source-only actor variants independently select those additional policy
  types and execute deterministic Wasmtime tapes over their private state and
  three semantic decisions. A fixed-host tape separately proves sample/drop
  queue realization and invalid-action rejection. The fixed-host suite is 7/7,
  the codegen unit suite is 62/62, and all 29 serialized gallery gates passed in
  2,451.47 seconds. The 3,520-event exact Mandelbrot oracle remains bit-exact
  with all six clamp boundaries covered; perturbation shader receipts remain
  exact WGSL 391,796 bytes, reference 456,259, perturb 45,400, combined
  501,659. Raymarch WGSL remains 83,496 bytes while its richer control Wasm is
  6,607 bytes. A cache-disabled release precompile built all 11 render bundles
  in 116.02 seconds (exact control Wasm 25,517 bytes; perturbation control Wasm
  5,173 bytes), and deployment verification accepted 13 Fe modules / 54 files.
  Real Chromium SourceInspector/gallery and TodoMVC tapes both passed against
  that exact cold-built site; unavailable WebGPU pixel execution remains the
  previously named environmental qualification. Native event/scheduling parity
  remains open Phase 2 work.
- The first Phase 4 typed-message slice now runs through the ordinary gallery
  build without CLI or Rust name inventories. Public Fe functions whose
  resolved effect rows carry host execution, placement, or capability markers
  derive the canonical interface in declaration order; ordinary helpers,
  render stages, surface transitions, and policy implementations are not
  guessed into it. `Worker`/`MainThread` are compiler-only placement evidence,
  erased from runtime arguments after trait-bound expansion rather than
  supplied as fake values. The generated Wasm value lane now safely reifies
  read-only, statically projected memory-provider records and whole-record
  forwarding while continuing to reject dynamic indexing, stores,
  address-taking, object references, and enum flattening.
- Generated interface JavaScript, declarations, and the fixed actor modules
  are published as one content-addressed ES-module package. Publication is
  fail-closed: bundle-relative paths must be unique and exactly equal the
  manifest-declared set, bytes and SHA-256 metadata must match, and the
  deployment verifier rechecks every emitted module. `<fe-surface>.post()` is
  one fixed demo-blind host call into that generated actor; lane admission,
  validation, Worker placement, cancellation, and request/result codecs come
  from Fe-derived bindings. DEC's real `d0` lane now crosses
  surface -> generated router -> module Worker -> Fe Wasm in Chromium and its
  returned cochain matches the independent hand oracle (`-1` on all six
  spokes, zero on all six ring edges). This is semantic behavior evidence, not
  byte matching. The complete DEC suite is 4/4, WebBundle tests are 9/9,
  html-precompile tests are 31/31, and the two focused CLI discovery/policy
  gates pass. A fresh release precompile built 11 render bundles in 118.095
  seconds and verified 13 Fe modules / 64 files; the real Chromium gallery and
  SourceInspector tape including DEC passed against that exact publication.
  The Fe-declared `submit_view` MainThread/WebGPU capability is honestly still
  unconnected: invoking it rejects explicitly instead of fabricating the
  application-defined `{ submitted: bool }` response. Completing that provider
  and `qcga_pencil` pick -> drag -> solve -> render remain Phase 4 work.
- The QCGA Phase 4 solver/value prerequisite is now promoted and independently
  green. The portable Wasm value lane recursively flattens fixed arrays and
  records, materializes owned aggregate parameters into independent
  target-layout arena storage, supports bounds-checked dynamic projections and
  typed stores, and retains exact `usize` narrowing/division/remainder
  semantics needed by the production Gauss-Jordan solver. Loads and stores use
  MIR's recursive field offsets and array strides rather than assuming one word
  per leaf; a packed `[u8; 4]` gate proves byte-stride selection and OOB
  trapping. The promoted QCGA suite executes real zero-import Fe Wasm lanes and
  is 8/8: rank-eight pencil recovery, rank-nine perturbed rejection, independent
  f64 incidence/basis checks, vertex projection, topology tearing, stream
  receipts, and honest placement declarations. It also keeps the still-missing
  raster-placement wall explicit rather than inferring end-to-end picking from
  solver success.
- That promotion exposed and fixed a general Sonatina-to-WAFFLE miscompile.
  WAFFLE's stackifier could rematerialize a cross-block expression such as
  `1 / load(pivot)` at each later use; when a loop mutated `pivot`, the emitted
  Wasm no longer preserved Sonatina SSA's definition-time value. Sonatina now
  converts cross-block uses to explicit maximal SSA before structurization, and
  a minimal mutable-memory oracle plus the complete 27-test Sonatina Wasm
  backend target are green. Fe pins the exact fixed revision. Fe's lowerer unit
  suite is 10/10, all codegen tests typecheck, and the 71-test Wasm integration
  target produced 70 semantic passes with its sole stale diagnostic matcher
  subsequently corrected and passing focused. Fieldless enums retain their
  compact compiler-derived scalar lane. The general Wasm value path now also
  flattens payload enums as one checked tag plus one statically typed lane tree
  per variant; inactive lanes are zeroed deterministically. Construction,
  matching, scalar/aggregate extraction, public argument/result flattening,
  invalid-tag trapping, and the previously walled effectful `Result` flagship
  all execute under independent Wasmtime oracles. The current 74-test Wasm
  integration run completed every expensive arithmetic oracle (including the
  serial Poseidon-Merkle gate) and produced 73 passes; its only failure was an
  obsolete assertion that public fixed arrays must be rejected, while the
  compiler now correctly flattened the array and returned executable Wasm.
  That stale rejection was promoted to a Wasmtime value/order oracle and passes
  focused, giving 74/74 semantic outcomes across the full run plus the exact
  corrected gate.
- Authored rasterization now has a standard nominal Fe surface rather than a
  QCGA-local marker convention. `VertexStage<V>` and `FragmentStage<V>` are
  paired through exact semantic identity of `V`; `RasterVertex<V>` separates
  the standard `ClipPosition` result from the recursively derived f32 varying
  record, so applications maintain neither numeric locations nor a parallel
  interface declaration. The compiler rejects malformed outputs, payload
  disagreement (including structurally identical but nominally different
  records), multiple stage pairs, and mixing authored raster stages with the
  synthesized fullscreen envelope. QCGA now declares one real
  `PencilRaster uses (GpuProgram<WebGpuBackend>)` actor and has deleted its
  demo-local `VertexStage<B>` / `FragmentStage<B>` traits and free placement
  adapters. Its CPU mesh stream remains a semantic oracle over the shared
  ordinary Fe vertex/shading constructions, not a replacement renderer. The
  focused positive/negative compiler gates are 2/2 and the promoted QCGA suite
  remains 8/8.
- The paired authored-raster backend and first real gallery consumer are now
  implemented. Sonatina lowers one Fe vertex/fragment pair into one validated
  module with `VertexIndex`, compiler-derived varying locations and perspective
  interpolation, shared vertex/fragment state visibility, authored clip
  position, and packed fragment color; dynamic resources, memory/object lanes,
  and unsupported traps still fail closed. Fe derives the non-indexed
  `TriangleList<N>` draw count, routes even a single authored raster pass
  through the pass-graph executor, and never substitutes the fullscreen
  envelope. The generic Wasm value lane also treats representation-preserving
  `RetagRef` over statically projected nested records as value identity; an
  executed nested-record regression guards that semantic fix.
- `qcga_pencil` is now the canonical gallery's twelfth render actor. Its
  `PencilRaster` owns a nominal `PencilFrame` split into four mutable
  `PencilControls` leaves and an immutable 25-leaf solver certificate. One
  Fe `InitialState` behavior computes the complete solved state at boot behind
  fixed `fe_surface_initialize_v1`; no host defaults table was added. Its
  FCO-derived `ApplyParamBindings` transition and ordinary
  `LatestPerFrame` policy own pencil blend, orbit, dolly, and scheduling. The
  browser carries raw pointer facts and returned uniforms only. Recursive
  compiler reflection derives the 29-leaf GPU layout while `view()` exposes
  only the four interactive parameters.
- Evidence for that promotion is semantic and independently layered. The QCGA
  suite is 8/8 after moving its Rust oracle outside the publishable ingot; the
  oracle executes Fe initialization and navigation in Wasmtime, proves the two
  solved quadrics against independent f64 incidence checks, observes yaw
  change, and proves the basis certificate is bit-preserved. Actor construction
  is 9/9, the authored-raster execution target is 2/2 (its Wasmtime oracle
  passes; the actual GPU leg reports an explicit no-adapter skip), and the
  fixed-host tape is 10/10 including a varying/state-slot separation gate. A
  release precompile publishes 12 render bundles as 14 Fe modules / 68 files;
  the deployment verifier and real Chromium Fe gallery/SourceInspector tape
  pass. The strict Chromium raster interaction leg remains externally required
  because this machine exposes no usable WebGPU adapter. The remaining Phase 4
  frontier is semantic QCGA point picking and drag/resolve messaging, not
  raster placement.
- The next QCGA interaction slice closes that named frontier without adding a
  browser hook. The fixed host appends pointer down/move/up to the nominal
  `SurfaceEventKind`, preserves raw identity/coordinates/buttons, and derives
  the homogeneous motion tag from Fe's resolved enum. `PencilRaster` owns a
  closed `PickedControl` state; ordinary Fe projects all nine points, selects
  the nearest marker, inverse-projects captured motion into the camera plane,
  edits an independently copied `ControlPoints` value, and solves the pencil
  again before returning one complete state. The same authored raster appends
  nine visible control markers and uses a vibrant normal-tracked palette with
  grazing/tangent sheen. An independent Wasmtime receipt derives the initial
  screen location itself, executes typed down/move/up records, observes the
  pick state and moved point, proves all eight untouched points bit-exact, and
  checks both returned quadrics against all nine points with an independent
  f64 polynomial oracle. The complete QCGA suite is 8/8 and the fixed-host
  boundary is 11/11. This is still an authored triangle surface; the companion
  iterative distance-estimator fragment surface is explicitly the next scene
  view, not something claimed by the richer shading.
- That application simplification rests on a general portable-value fix rather
  than a QCGA flattener. Root Wasm exports materialize complete read-only value
  views, projected aggregate reads and address-taking retain target-layout
  offsets, and aggregate `Use` performs a byte-accurate deep copy into fresh
  canonical arena storage. QCGA can consequently spell
  `let mut next: ControlPoints = points` instead of copying 27 fields. An
  executed array regression requires compilation and proves post-copy mutation
  cannot alias; enum-state wrappers separately validate both incoming and
  returned fieldless tags. Native/Wasmtime/browser transition parity remains
  green with all eleven lifecycle/input variants. Shader lowering deliberately
  omits host-forgery traps because its inputs arrive through compiler-derived
  WebGPU bindings, while public Wasm boundaries retain them.
- A thirteenth canonical render actor, `qcga_pencil_de`, now supplies the
  companion fragment-surface view promised above. It imports the raster
  ingot's one `PencilSceneState`, initialization, event transition, camera/
  inverse projection, solver, control-point projection, current pencil member,
  and normal/tangent material; it owns only the DE interpretation. Production
  visibility iterates the coefficient-scale-invariant first-order estimate
  `abs(F) / length(gradient F)` for up to 128 conservative bounded steps, with
  exact analytic quadric normals, DE soft shadow/AO, vivid normal-tracked
  color, tangent sheen, fog, ordered subpixel sampling, and projected control
  markers. There is no analytic ray-root routine, handwritten WGSL, or
  geometry-aware JavaScript in the shipped view. Its compile gate retains the
  authored loop, browser-validates 55,961 bytes of WGSL under a 190 kB budget,
  and proves both renderers expose the same compiler-derived 57-leaf scene
  layout. It initializes the two generated Wasm modules independently, runs
  the same event, and semantically compares every decoded leaf.
- Correctness does not rest on that cross-view equality. A separate zero-import
  Wasmtime gate evaluates the production Fe estimator at 500 directed points
  against an independent Rust polynomial/gradient model, including a scaled
  representative of the same projective sphere. It then compares seven
  production iterative rays (four hits, three misses) to separately derived
  analytic ray/quadratic roots and checks hit residuals. Exact roots therefore
  remain a genuinely different oracle, never a disguised production DE. The
  canonical precompiler discovers the actor through ordinary Fe page
  projection and publishes 13 render bundles / 72 assets; actual WebGPU pixel
  execution retains the existing external-browser qualification on this
  no-adapter machine.
- This component/page slice is not the full resident-component end state. The
  outer gallery shell does not yet own routing, tile lifecycle, scheduling, or
  component-to-component messages as a long-lived Fe actor, and the stylesheet
  remains an authored transitional HTML shell. The
  component directory route still needs to supersede the render-lane probe; an
  attribute-mutation event ABI, persistence, unbounded collections, and native
  backend parity remain open. TodoMVC intentionally caps itself at 32 todos and
  96 UTF-8 bytes per title while those richer storage interfaces are designed.
- A low-hanging Wasm language-parity sweep is now explicit early work. The
  sequential gallery policy exposed that ordinary boolean unary `!` reached
  MIR as `Unary::Not` but the Wasm target rejected it. The exact portable
  `IsZero` lowering and executed truth-table/composed-comparison regression are
  now implemented, and the policy again spells idiomatic `!waiting`. Audit the
  remaining ordinary unary/binary/control operations against native so small
  backend omissions do not force visible contortions into exemplary Fe code.
- Gallery render declarations now opt into compiler-correlated sequential
  activation, with a reusable Fe state machine owning the cursor, waiting
  state, fail-open completion policy, and 30-second deadline. This is clearly
  labeled in Fe source as a compatibility workaround for buggy browser WebGPU
  implementations which crash or lose a shared device during burst gallery
  initialization, not as a Fe/WebGPU semantic requirement. The fixed host
  currently contributes generic descendant discovery, lifecycle transport,
  timer realization, and `live()` realization. Phase 3/6 must compost those
  handwritten cases into generated EventSource/effect adapters from the typed
  Fe/browser capability vocabulary; no gallery title, count, or reducer may
  enter that adapter while it remains.
- The single-invocation geometric-algebra compilation frontier is now stated
  precisely. A WebGPU shader invocation is already one GPU lane and cannot
  dynamically spawn additional lanes. Fe can instead use CTFE/FCO to turn a
  statically shaped GA expression with statically known operand supports into
  a sparse, call-free SSA DAG: load each leaf once, omit structural zeros,
  share repeated products, and balance independent output reductions so the
  GPU compiler can expose instruction-level parallelism within that lane.
  `sparse_clifford` already supplies conservative support interpretation,
  bounded `SparsePlan` materialization, provider-emitted straight-line terms,
  and one explicitly balanced canonical schedule. It does not yet accept an
  arbitrary operator-expression tree, normalize across that tree, derive CSE,
  or choose an emitted schedule automatically. That is real groundwork, not a
  completed expression compiler.
- The first reusable GA expression-compiler slice is now implemented and
  specified in `docs/mb2/GA_SINGLE_INVOCATION_COMPILER.md`. The new `ga_expr`
  ingot provides typed metric, expression, support, program, numerical-policy,
  and exact-term vocabularies. A PGA twice-wedge fixture proves the complete
  Fe path: compositional support inference, a closed six-term CTFE type
  witness, FCO-emitted shared straight-line arithmetic, zero-import Wasmtime
  execution over 2,005 cases against independent schedule and dense semantic
  oracles, and browser-valid WGSL with six survivor products and no runtime
  planning control flow. This is deliberately recorded as a vertical slice:
  the exact planner is still example-specific, automatic arbitrary-tree term
  normalization, reflected leaf-slot derivation, hash-consing, and scheduling
  remain Phase 7 work.
- The twice-wedge slice must not be described as a generic expression
  compiler. Only its node vocabulary and conservative `GaSupport`
  interpretation are generic; `AlgebraicTwiceWedgePlan6`, its exact-term
  constructor, output-lane routing, and evaluator provider are specialized to
  `(a ^ b) + (a ^ b)`. This gap is now answered by the bounded strict G2/G3
  provider recorded below; the specialized fixture remains only a historical
  staging proof. `qcga_pencil` is the first non-toy migration target. It may
  retain ordinary camera and matrix-solver coordinates where
  those are the clearest domain representation, but hand-expanded GA
  coefficient routing/evaluation/gradient constructions are debt to replace,
  not an acceptable per-demo specialization.
- The first operator-substitution G2/G3 prototype is now green. One unchanged Fe FCO
  provider lowers both `Sum<Outer<A,B>,Outer<A,B>>` and the structurally
  different `Neg<Outer<Sum<A,B>,Difference<A,B>>>` by reflecting a zero-sized
  `GaProgram` marker and folding its ground type in normalized postorder. New
  domain-neutral FCO facilities provide that postorder and bounded immutable
  sequence reads/append/concat/replace/pop; no Rust GA planner or complete
  expression name is involved. The independent gate executes 1,004 cases per
  tree bit-for-bit against separate Rust tree interpreters, then requires
  browser-valid WGSL with no runtime loop/branch/switch and no host algebra
  import. This proves structural dispatch across two trees, but it is not a
  generic GA implementation: dimension, carrier layout, output shape, and the
  five accepted constructors remain fixed in that fixture. The reusable
  provider below supersedes those first four limits; DAG sharing remains open.
- A configured-provider follow-through now eliminates the phantom program
  field for the first real QCGA consumer. `using
  CompileVectorScalarF32<GaProgram<...>>` is reflected by FCO as exact ground
  Fe type data and keyed into expansion memoization. `ga_expr` adds sparse
  symmetric metric terms and packed high-dimensional vector supports; the
  QCGA incidence path now declares its 15-generator paper-null metric and
  `ScalarProduct<Point,Quadric>` through that shared provider. The retired
  demo-local code included a 144-candidate mask, twelve-term recursive plan,
  and 24-way field-selection ladder. Independent evidence is green for 259
  scalar cases, zero host imports, the actual rank-8 pencil solve, and QCGA
  typechecking. A second substitution gate varies dimension, supports, metric
  signs/magnitudes, carrier width, and operand order through the same provider
  over 131 independent inputs. Vector leaves use their nominal Fe coefficient
  record types, eliminating author-maintained numeric slots and flat carrier
  packing. A new domain-neutral `type.fields()` reflection read lets FCO bind
  each leaf to a nested carrier field by type identity, validate its reflected
  width, and emit hygienic nested access. Concrete structs are supported;
  generic substitution and enum payloads still fail closed. This establishes
  substitution across the provider's explicitly narrow two-vector
  scalar-product contract; it does not establish generic GA. The orthogonal
  operator vocabulary is handled separately below, while off-diagonal general
  products and DAG normalization remain open. Current 41-44 second in-process
  gates make provider/dependency caching a named performance prerequisite.
- The misleading twice-wedge genericity claim is now superseded by a reusable
  bounded strict expression compiler. `CompileGaF32<GaProgram<E,M,Strict>>`
  accepts any finite ground composition inside the published provider budgets
  of nominal typed leaves, sum/difference/negation,
  geometric/outer/scalar and directed contraction,
  grade/reverse/Poincare dual, and regressive product under a signed orthogonal
  metric with at most five generators. It reflects leaf records by nominal
  type, shares their loads, infers a compact result tuple, and emits no runtime
  planner. One semantic gate substitutes four unrelated trees spanning every
  accepted operator, dimensions 2/3, distinct signatures/supports/carriers,
  and result widths 1/3/5/7 over 257 independent inputs each; separately
  authored Rust interpreters match bit-for-bit, Wasm has no host algebra
  imports, and a browser WGSL consumer has no loop/branch/switch. Unknown
  operators, duplicate leaf identities, and `AlgebraicBalanced` fail closed.
  This is real bounded strict tree genericity, not yet general GA: dense
  compile-time vectors, the five-generator limit, missing property coverage,
  missing explicit path/fuel diagnostics, and no internal-expression
  hash-consing remain. The four-program gate still takes about 32 seconds, so
  cache/sparse-arena work remains required.
- The surface fixed host now observes the canonical Wasm arena's documented
  top-level-call lifetime. Initializer, explicit resident-state replacement,
  scheduling policy, legacy control, immediate transition, and scheduled
  transition exports run in fresh `fe_cabi_reset` epochs with post-call reset
  in `finally`. Scheduled `SurfaceEvent` bytes are allocated inside that epoch;
  no arena pointer is cached across frames. This fixes the observed QCGA
  `fe_cabi_alloc -> solve_pencil -> transition_pencil_scene` exhaustion without
  enlarging the heap or teaching JavaScript about QCGA. The fixed-host gate is
  13/13 and includes a forced Wasm `unreachable` followed by a clean recovery
  call, plus the legacy per-pixel fallback's equivalent lifetime. The promoted
  QCGA suite additionally performs 256 consecutive
  allocation-heavy Fe drag/re-solves and pins identical event allocation,
  aggregate high-water mark, and Wasm page count while resident generation
  advances.
- The GA authoring surface now admits named basis and coefficient records.
  `BasisMetric<Basis>` derives positive, negative, degenerate, and typed null-
  pair products from field types; `SymmetricMatrixMetric<Basis, Matrix>` aligns
  a named symmetric matrix by reflected field name; and `Vector<Coefficients>`
  aligns vector coefficients to the basis without masks or numeric generator
  IDs. Alias-normalized `type.fields()` and the pure reflection read
  `field.same_name(other)` supply the general compiler mechanism. CGA3D, QCGA,
  QCGA Pencil, gaplay, and the PGA gate consume it. Independent differential
  gates cover the legacy sparse form, named null-pair form, named matrix form,
  4,008 PGA/Desargues cases, and the real rank-8 QCGA solve.
- Configured FCO providers may now be named through an ordinary generic type
  alias, so `CompileNamedVectorDotF32<Left, Right, Metric>` is a concise library
  facade rather than another derive declaration or target-side phantom field.
  Base-graph normalization preserves the alias's nested type/const argument
  environments. The audited provider expression surface also gains one
  domain-neutral `builder.float(literal)` node; it preserves the parsed literal
  into ordinary generated HIR without evaluating floats in the provider. This
  lets GA reductions start from literal `+0.0`, avoiding `x - x` contamination
  for infinite/NaN inputs and avoiding an inaccessible cross-ingot helper.
  Positive, fail-closed wrong-kind, ordinary type-check, generic-alias, and
  command-surface freeze gates are green. Null-pair matching now compares full
  types, so `Pair<A>` cannot silently match `Pair<B>` merely because their
  constructors coincide; a focused generic-identity regression pins this.
- A fresh primary-source GA scheduling audit is recorded in
  `docs/mb2/GA_SINGLE_INVOCATION_COMPILER.md`, with local PDF/page references
  for Elliott, Fuchs--Thery, Breuils--Nozick--Fuchs, and Leopardi. It keeps the
  two executable envelopes distinct (bounded orthogonal whole-MV trees versus
  high-dimensional named vector incidence), treats current straight-line FCO
  as instruction-level work rather than spawned lanes, and orders the next
  slices as correctness lock, shared QCGA/metric witness, exact output DAG plus
  work/depth interpreters, packed schedules, measured compute workgroups,
  general-metric products, then an optional dense transform backend.
- The QCGA Pencil renderer is consolidated rather than duplicated. The former
  `PencilRaster` GPU actor and its fake host-effect lanes are deleted; the
  iterative `PencilDistanceSurface` is the sole gallery/GPU view, while the
  old radial projection remains only as a zero-import CPU/Wasm oracle in the
  shared Fe solver library. The converted eight-part acceptance suite proves
  canonical DE initialization, interaction, drag/re-solve, rank and topology
  transitions, plus the retained independent projection evidence.
- The QCGA DE view now polishes an iterative marcher hit with four bounded
  Newton corrections along the ray. This is still implicit-field iteration,
  not the analytic quadratic intersection retained solely by the independent
  Rust oracle. It removes march-step terraces before exact-gradient shading;
  gentler AO/shadow modulation and removal of a DE-only square-root transfer
  preserve the shared raster/DE normal-and-tangent palette. The independent
  gate remains green for 500 field samples and seven analytic-root rays, and
  the browser gate emits 45,314 bytes of valid WGSL with its march loop intact.
- Gallery colour policy is now ordinary shared Fe data rather than replicated
  shader literals. `demos/sketches/gallery_palette` names the original
  Desargues midnight, guide gray, electric blue, spring green, theorem/evidence
  gold, and white roles as typed `Rgb` values. Desargues is restored exactly;
  Gradient, CGA3D, QCGA, and the canonical QCGA Pencil DE/shared material all
  consume the same ingot while mapping those roles to their own geometry.
  Mandelbrot deliberately remains outside this policy so its established
  escape palette is unchanged. Focused compile gates for all five consumers
  and a real HTML precompile are green (12 render bundles, one Fe page
  projection, two resident component projections, 68 publication assets). A
  fresh cold release publication verifies 14 Fe modules / 68 deployment files.
- The Fe-composed gallery order now leads with Gradient and TodoMVC, keeps the
  GA demonstrations together, exposes only the consolidated QCGA Pencil DE
  tile, and moves Known Color and Rollcall Pipeline to the bottom. TodoMVC's
  real Fe resident initial state is visibly seeded with `strings`,
  `review code`, and `much much more`; subsequent reducer operations treat
  those records exactly like keyboard-created items. Its independent semantic
  actor tape passes with the seeded keys and next-ID policy.
- Mandelbrot-specific native transition tapes, typed device-loss/MessagePort
  streams, pointer capture, picking/messages, resident outer-gallery
  orchestration, and runtime-manifest removal remain open.
- The first runtime-control/reactive consolidation slice is landed. `Stream`
  is now zero-state vocabulary over one explicit `mut EventSource` effect;
  imperative and stream-shaped consumers share `subscribe_event` /
  `next_event` / `cancel_event`, while pure `Signal::sample` remains unable to
  widen its effect row. A stateful non-`Copy` Fe tape proves one handler retains
  state through nested stream forwarding, and compiler lowering now transports
  mutable trait-effect witnesses by place instead of silently copying their
  concrete implementors at each call. Independent gates pin missing-handler
  diagnostics and affine double-cancel rejection. This is the synchronous
  handler boundary, not a claim that MIR suspension, browser EventSource
  adapters, or async continuation materialization are complete.
- Runtime-control implementation now has one explicit consolidation
  architecture in `docs/mb2/RUNTIME_CONTROL_EFFECTS.md`. The unused
  `fe:resumable-task/v1` JSON protocol and its seven caller-authored synthetic
  entry names are deleted. The callback-registration JSON schema, duplicated
  flattened scalar lanes/lifetime fields, and codegen-local token arena are
  deleted as well: callback transport is derived from the normalized interface
  and checked against the authored Fe MIR body, while the one host-runtime
  callback table retains generation, stale-token, reentrancy, and release
  semantics. The resumable executor now accepts a materializer-owned typed body
  key rather than parsing task/body/entry strings. All host ABI, host runtime,
  and focused callback compiler gates are green. `core::pending::TaskOutcome`
  and the executor now share the same typed `Failure(E) | Success(T) |
  Cancelled` terminal vocabulary; a Fe execution gate proves pure success
  mapping, typed failure preservation, and distinct cancellation. The browser
  host also has an explicit generation-checked `toCore` projection instead of
  implicitly passing opaque JavaScript resource objects into core-Wasm `i32`
  lanes; its Bun capstone now exercises WebIDL conversion, Fe callback/import
  execution, borrow expiry, release, and stale rejection end to end. This is
  architectural composting before the real MIR suspension/re-entry slice, not
  a claim that main-thread suspension or browser handlers already exist.
- Runtime-control phase 1 is now compiler-derived rather than merely
  architectural. `core::pending::Suspend<B, E>` is an ordinary typed Fe
  authority, `std::host::Resumable` is its downstream provider, and only the
  nominal `std::host::raw::suspend` declaration is recognized as the control
  boundary; import spellings, manifests, and caller-authored entry tables are
  not consulted. Target-neutral MIR assigns stable continuation states and
  computes exact CFG liveness at every direct suspension point. An independent
  Fe-source oracle proves that a used parameter enters the frame while a dead
  sibling does not. The executor also now delivers `Cancelled` to the Fe task
  body before terminal notification—closing a real semantic hole where the
  table became cancelled without the continuation observing the typed outcome.
  Success/failure/cancellation, exact-once notification, dead-value exclusion,
  and nominal recognition are semantic tests, not generated-byte comparisons.
  Fixed-point propagation now carries suspension through ordinary Fe helpers
  and the actual effect-provider chain without requiring a repeated annotation;
  every resumable body gets a typed union frame derived from its exact live
  values and original MIR root semantics. This work also promoted the general
  Wasm payload-enum value lane required by `TaskOutcome`, including executable
  construction/match/extraction, public argument/result flattening, invalid-tag
  traps, and the previously walled effectful `Result` flagship. Executable MIR
  block splitting now consumes direct nominal suspension calls into a verified
  target-neutral `Complete | SuspendedN` machine: every suspension variant owns
  its pending token plus only that site's live frame, while each continuation
  receives those exact locals followed by its typed `TaskOutcome` delivery.
  Wasm emits compiler-named start/resume exports with no `fe:control` import;
  independent Wasmtime gates execute success, failure, cancellation, forged-tag
  trapping, and a two-site chain whose inactive variant lanes remain zero. A
  general target-neutral CFG transform now expands acyclic resumable calls
  through ordinary helpers and the selected Fe effect-provider method before
  liveness. The executable transitive gate proves pending + one live caller
  value survive, a dead sibling does not, and private helper continuations do
  not leak into the public Wasm ABI; a separate branched-provider gate verifies
  both cloned CFG paths. Resumable cycle membership is computed before
  expansion so recursive stacks fail explicitly instead of unrolling forever.
  The target-neutral `MaterializedExecutor` now connects those generated start/
  resume functions to the existing generation-safe FIFO: it owns initial
  inputs, each exact site-frame enum, pending-operation routing, and completed
  output by task generation. The two-site Wasmtime gate runs through that
  production bridge for success, failure, and cancellation, while independent
  host-runtime gates cover traps, stale/duplicate delivery, cleanup, and equal
  raw pending ordinals under distinct typed handler identities. The compiler
  now projects each public materialized task into a concrete ES module from the
  same target-neutral machine used by Wasm lowering: exact generated start/
  resume symbols, scalar lanes, task-step union ranges, typed delivery layouts,
  and suspension-site identities are not caller-authored or recovered from a
  manifest. A fixed demo-blind browser runtime validates those lanes, keeps
  continuation frames opaque and affine, and rejects forged/stale reuse. An
  independent Bun capstone executes actual Fe Wasm through two suspension
  sites and proves success, both failure sites, both cancellation sites,
  distinct typed identities for equal raw pending ordinals, and one-shot frame
  custody. Canonical artifact publication, real standards handlers, and true
  recursive linked frames remain the active materialization slice.
- The first real browser-host realization now drives that generated machine
  without blocking. A fixed, demo-blind `HostTimer`/`Recv` broker implements
  the existing `fe:host::{sleep_begin,recv_begin,host_now}` imports with a
  monotonic clock, scheduled timers, FIFO host posts, typed receive failure,
  and `AbortSignal` cancellation; it rejects `wait` on this placement. Fe still
  owns every success/failure/cancellation match and every subsequent suspend.
  An independent capstone compiles ordinary Fe tasks using `Timer`, `Recv`,
  `Suspend`, `HostTimer`, and `Resumable`, then runs their actual Wasm through a
  real timer plus receive success/failure and both cancellation paths under
  Bun. A separate mechanics gate covers invalid delivery, timer cleanup when a
  Fe invocation traps, and non-consuming rejection of malformed posts.
  MessagePort/EventSource attachment, Worker/spawn placement, structured child
  scopes, and canonical task-package discovery remain open.
- A cross-runtime honesty pass closed a semantic mismatch in that first
  realization. `TaskOutcome::Failure(E)` is now consistently an operation
  result rather than an automatic task failure: the target-neutral executor,
  generated browser machine, and real `HostTimer`/`Recv` broker all re-enter
  the Fe continuation and honor its selected recovery/suspension/completion.
  Explicit executor/`AbortSignal` cancellation remains terminal, but reaches
  Fe exactly once for cleanup; returned values and newly minted host work from
  that cleanup step are discarded. Independent gates cover recovery after a
  first-site failure into a second suspension, terminal cancellation, cleanup
  invocation count, and cleanup-created timer disposal. The host-runtime suite
  is 23/23, the fixed browser task/broker suite is 10/10, and three actual
  compiled Fe-to-Wasm capstones pass for the two-site machine, timer/receive
  broker, and compiler-emitted browser adapter. This correctness distinction
  is behavioral evidence, not generated-byte equality.
- Resident actors now have a compiler-derived background-task authoring path.
  A self-less behavior selects the nominal target-neutral `ScopedTask` role;
  the compiler roots it beside the actor's initializer/transition/projection,
  derives its resumable MIR machine, and emits fixed start/resume adapters.
  HTML publication writes one content-addressed three-module package (generated
  adapter, fixed materialized-task runtime, fixed completion broker) and only a
  standard executable-module reference on the component script—there is no
  task JSON, hand-authored entry name, numeric task ID, or JavaScript task
  inventory. The deployment verifier pins the package digest, exact file set,
  and fixed runtime bytes. The fixed component host enumerates the generated
  machines, starts them for each connected lifetime, and aborts them on
  disconnect; reconnect gets a fresh scope. Independent evidence includes
  role/zero-input/export-shape checks, the ordinary resident actor regression,
  a lifecycle start/cancel/restart tape, and a real precompile -> static package
  -> Bun -> Fe Wasm timer execution whose wake timestamp is checked. This is
  the canonical component-task publication prerequisite, not yet the
  EventSource/surface-loader migration: typed event attachment and a Fe-owned
  race/select construction remain necessary before opcode 14's Promise/timer
  realization can be deleted honestly.
- Runtime control now has its first typed structured race on the same pending
  rail. `Race<B>` consumes two distinct affine `Pending<B, T>` values and
  returns `Pending<B, RaceOutcome<T>>`; the fixed broker only arbitrates the
  first terminal token and cancels the loser. Fe matches `Left(T)`, `Right(T)`,
  typed operation failure, or cancellation. The browser derives the nested
  enum lanes from the opaque compiler continuation schema, so neither authors
  nor JavaScript supply payload widths, winner IDs, or JSON. Unit gates cover
  schema packing, stale/duplicate ownership, both winners, loser cleanup, and
  post-race queue emptiness. The actual Fe-to-Wasm `HostTimer`/`Recv` capstone
  now also runs receive-wins, timeout-wins, receive failure, and explicit race
  cancellation under Bun. This supplies the timeout/select primitive needed by
  sequential activation; a typed surface/event begin authority is still needed
  before migrating the gallery policy itself.
- Sequential gallery poster loading now consumes that runtime-control spine.
  `std::web::SurfaceLoader<WasmBackend>` exposes only typed pending operations
  for pulling the next opaque compiler-correlated `SurfaceToken` and loading
  it; the resident SourceInspector runs the declaration loop as an actor
  `ScopedTask` and owns ordering, the `Race` against its 30-second `Timer`,
  fail-open continuation, end-of-stream handling, and disconnect cancellation.
  The fixed browser adapter owns only declaration correlation, DOM arrival
  observation, `<fe-surface>.load()` realization, and abort cleanup. The
  canonical actor's four discovery/cursor/waiting/effect state leaves and all
  opcode-14 projection are deleted. Opcode 14 remains explicitly labeled as an
  append-only legacy component adapter rather than silently becoming the new
  path. An independent compiled Fe -> generated continuation -> Bun broker
  oracle proves ordered pulls, success, typed failure recovery, end-sentinel
  consumption, and zero leaked pending work; the resident reducer decoder now
  rejects opcode 14. A fresh optimized gallery publication compiled 14 Fe
  modules / 12 render bundles and verified 71 deployment files. Its real
  Chromium tape observed every ready-or-unavailable poster settling in exact
  compiler-derived order, including fail-open progress on this machine's three
  unavailable GPU-only passes.
- A mobile-safety follow-up kept the then-current sequential policy in the
  resident Fe shell but changed its fixed opcode-14 realization from
  cold-to-live to
  poster-only loading. Poster capture and off-viewport suspension now destroy
  retained GPU buffers/pipeline references, pointer capture is unwound across
  suspension/cancellation, and the narrow gallery layout no longer overflows
  its viewport. Poster pixels are copied from the rendered WebGPU texture into
  an aligned readback buffer in the same command submission; the runtime no
  longer waits for the compositor and snapshots a possibly discarded canvas,
  which could replace a valid frame with a black poster on mobile. Until typed
  device-capability facts exist, the fixed runtime
  also applies an explicit coarse-pointer/CPU backing-store safety ceiling;
  this is marked host debt, not the final authoring model. Responsive quality
  selection must move into a Fe policy consuming browser/device capabilities
  through the runtime-control spine. Eighteen fixed render-runtime tests, six
  bootstrap tests, and a fresh optimized precompile of all 12 render bundles,
  one Fe page projection, and two resident components are green.
- A static mobile workload audit identified the next concrete quality slice.
  At the temporary 256x256 coarse-pointer ceiling, `qcga_pencil_de` can still
  execute 8,388,608 primary march iterations per frame, then four Newton
  refinements on hits; it also projects all nine control points independently
  in every fragment (589,824 projections per frame) and recomputes camera/
  pencil uniform work per fragment. `LatestPerFrame` and GPU-completion gating
  already prevent event/command buildup, so the remaining jank is real shader
  cost. Hoist camera/member/control-point projection into resident Fe state,
  move marker drawing out of the all-fragment projection loop, add a clipped
  march interval/early loop exit, and make extent/march/refinement budgets a
  typed Fe quality policy driven by raw device/viewport/frame facts. The fixed
  host may report those facts; it must not choose the quality tier.

- The first typed QCGA mobile-quality slice now executes rather than merely
  naming that debt. Actual backing-store width/height enter the complete
  resident Fe scene through `Param::extent_x/extent_y`; the shared
  `PixelSample::plane_extent` construction preserves an isotropic field of
  view on rectangular surfaces, and a zero-import Wasmtime geometry oracle
  proves landscape, portrait, and square projections independently. Ordinary
  Fe selects 64/2, 96/3, or 128/4 march/refinement budgets from those raw
  extents. The production marcher uses real bounded `break` exits on hit and
  far clip instead of spending the rest of a 128-iteration done-flag envelope.
  This exposed a genuine Sonatina structurization gap rather than a reason to
  contort the Fe source: the pinned `820f498b` revision lowers nested
  conditional exits to the canonical SPIR-V loop merge, with exact phi
  transfers and a browser-WGSL regression. Independent evidence executes 500
  field samples and seven analytic-root rays through both full and 256-square
  production policies, while the complete QCGA lifecycle/pick/drag/re-solve
  suite remains 8/8. A fresh release publication emitted 45,659 bytes of QCGA
  DE WGSL and 70,980 bytes of control Wasm, compiled 12 render bundles, and
  verified 14 Fe modules / 71 deployment files; both real Chromium gallery/
  SourceInspector and TodoMVC tapes passed. Camera/member/control projection
  hoisting, marker-pass separation, raw device/frame capability facts, and
  deletion of the temporary host ceiling remain the next mobile workload
  slice; actual GPU pixel execution retains the named no-adapter qualification.
- The control-marker portion of that mobile workload is now removed from the
  all-fragment DE path through a general Fe-authored composition mechanism,
  not a QCGA runtime hook. A GPU actor may declare ordered fullscreen,
  compute, and adjacent nominally typed vertex/fragment stages; the compiler
  lowers each adjacent pair as one raster pass and retains the unique
  fullscreen behavior as the page-facing artifact. The fixed runtime derives
  target preservation solely from that Fe order: the first render pass clears
  and subsequent render passes load the established color. No overlay flag,
  demo name, or new JSON field was added. `qcga_pencil_de` now projects its
  nine controls only for 54 small marker vertices instead of nine times in
  every fragment: at the temporary 256-square ceiling this reduces modeled
  projection evaluations from 589,824 to 54 per frame, while the independent
  point-projection/pick oracle remains unchanged. The release graph contains a
  32,109-byte browser-valid DE shader and a 17,798-byte browser-valid marker
  shader; shared-state control Wasm is 61,234 bytes, 9,746 bytes below the
  preceding checkpoint. Actor construction is 10/10, the fixed render runtime
  is 19/19 with exact clear/load and draw-count evidence, the QCGA lifecycle
  suite is 8/8, and the independent 500-field/seven-ray analytic oracle is
  green for both full and mobile policies. A fresh optimized site verifies 14
  Fe modules / 72 deployment files, and both real Chromium gallery/
  SourceInspector and TodoMVC tapes pass against it. Actual marker pixels still
  require the named external WebGPU-adapter run; camera/current-member uniform
  hoisting, clipped march intervals, typed device/frame facts, and deletion of
  the temporary host ceiling remain the next mobile workload slice.
- Camera and current-member preparation now move across the same resident Fe
  boundary rather than becoming hidden compiler or host specialization. The
  shared QCGA model exposes an orthonormal `PencilCamera` and a nominal
  `PencilRenderState { scene, camera, member }`; initialization and every
  accepted Fe transition rebuild the two pure derived values once in control
  Wasm. Both GPU passes consume them directly, so generated shaders contain no
  camera `sin`/`cos` and the DE pass no longer blends all ten pencil
  coefficients per fragment. This uncovered and closed a general nested-value
  ergonomics gap: repeated record leaves now receive the shortest unique
  compiler-derived semantic suffix (`origin.x`, `right.x`, and so on), while
  already unique leaves retain their concise names. Authors can consequently
  keep typed `Vec3` records instead of flattening coordinates or maintaining
  numeric slots. A general layered-raster fixture proves identical derived
  paths across passes. The QCGA uniform grows from 59 to 81 leaves (88 bytes)
  and control Wasm from 61,234 to 65,857 bytes, while browser-valid DE WGSL
  falls from 32,109 to 29,572 bytes and marker WGSL from 17,798 to 16,911:
  3,424 bytes, or 6.9 percent, removed from the combined GPU program. The
  independent lifecycle tape proves the prepared camera is orthonormal and
  aimed at the solved center, updates after orbit input, and preserves all
  solver leaves; it separately checks the prepared member against all nine
  control points before and after drag/re-solve. Actor construction remains
  10/10, the QCGA suite 8/8, and the 500-field/seven-ray analytic oracle green.
  A fresh optimized publication again verifies 14 Fe modules / 72 deployment
  files with both Chromium component/gallery tapes green. Clipped march
  intervals, typed device/frame capability facts, and removal of the temporary
  host ceiling remain; actual pixels retain the no-adapter qualification.
- Responsive backing-store selection is now a structurally selected Fe
  policy rather than a coarse-pointer branch in the fixed browser host.
  `SurfaceQuality<P>` selects one ordinary public Fe function by the nominal
  `SurfaceQualityFacts -> SurfaceBackingExtent` shape; the compiler roots that
  exact function privately and exposes only fixed `fe_surface_quality_v1`.
  All twelve canonical render actors select `ResponsiveBacking`, while a
  fixture-local `HalfDeclaredBacking` proves the compiler and browser cannot
  substitute the standard implementation or recognize its authored method
  name. The browser supplies untouched CSS width/height, DPR, declared extent,
  coarse-pointer, GPU-availability, and device-limit facts, validates Fe's
  complete integral decision, and realizes it at poster creation, live
  viewport resize, GPU recovery, and GPU-to-CPU fallback. The former GPU/CPU
  coarse-pointer constants and duplicate CPU clamp are deleted; the fixed
  256-pixel CPU ceiling remains only for legacy artifacts without a typed
  policy. Independent Wasmtime cases cover desktop, portrait, coarse GPU,
  CPU, device-limit, and missing-geometry facts; fixed-host tests separately
  prove raw-fact transport, exact realization beyond the deleted host ceiling,
  invalid-decision rejection, live resize, device fallback, and adopted-canvas
  recovery. The fixed render-runtime suite is 24/24, the complete codegen unit
  suite is 58/58, and all 30 serialized gallery gates pass in 2,523.58
  seconds, including the 3,520-event exact Mandelbrot tape and unchanged
  shader/work receipts. A fresh release publication compiled twelve render
  bundles in 126.1 seconds and verified 14 Fe modules / 73 deployment files;
  the real Chromium gallery/SourceInspector and TodoMVC tapes pass.
  Frame-duration feedback and clipped QCGA march intervals remain the next
  quality slices.
- QCGA rays now enter the iterative distance-estimator only inside an explicit
  Fe-authored finite scene domain. `fmath::RayInterval` supplies an arbitrary
  normalized-ray/sphere intersection plus a prepared-camera specialization;
  the latter consumes the resident camera's already-normalized forward/ray and
  distance instead of rebuilding center offsets or normalizing again in every
  fragment. Its finite-input arithmetic is branch-uniform up to the one final
  activity decision, so a missed domain skips the 64/96/128-step loop while an
  accepted ray begins at the sphere entrance and exits at its far boundary.
  This is only broad-phase clipping: production visibility still evaluates
  `abs(F) / length(gradient F)` iteratively and performs the same bounded
  Newton polish; exact ray/quadric roots remain solely in the independent Rust
  oracle. That oracle executes 500 field samples, seven analytic-root rays,
  the arbitrary interval, and six prepared-camera intervals. It measures 37
  clipped steps versus 159 unclipped, including three arbitrary and two exact
  prepared-camera whole-loop rejects. The source and browser gates retain real
  primary/shadow `break` exits and validate 31,752 bytes of DE WGSL plus 16,911
  bytes of marker WGSL under the existing 49 kB combined budget; shared-state
  control Wasm remains 67,091 bytes. The focused fmath typecheck, independent
  oracle, QCGA browser-profile compile gate, and complete eight-part QCGA
  lifecycle/solve/interaction suite are green. A fresh optimized publication
  compiled all twelve render bundles in 116.9 seconds and verified 14 Fe
  modules / 73 deployment files; the real Chromium gallery/SourceInspector
  and TodoMVC tapes pass against those artifacts. Frame-duration feedback
  remains the next quality slice; actual pixels retain the named
  external-adapter qualification.
- The first standards-shaped browser `EventSource<T>` now runs on that same
  compiler-generated suspension rail. `Event<T>` is a typed closed vocabulary
  for occurrence, absence, exhaustion, operation failure, and structured
  cancellation; `Subscription<T>` remains affine, and `Stream<T>` is still a
  zero-state Fe facade over the installed handler rather than a second
  scheduler. `BrowserSurfaceEvents` interprets the existing fixed
  `fe:web-surface::next_begin` operation into
  `Event<SurfaceToken>`, while `SurfaceLoader` has contracted to poster-load
  realization only. SourceInspector's actor-scoped gallery task consumes that
  stream, owns end/failure/cancellation policy, races each load against its Fe
  timer, and always consumes its subscription; the source contract gate
  forbids a return to `loader.next_begin`. No import name, task JSON, manifest
  field, surface count, selector, or demo case was added to the host.
- That migration exposed and fixed a general monomorphized-effect compiler
  bug. Owner-only effect enumeration could omit `EventSource<T>` on a generic
  declaration even though the concrete semantic instance had a closed
  provider resolution. Callers consequently transported a provider argument
  that the callee runtime signature could not map to a semantic local.
  Semantic and runtime MIR now enumerate bindings from the instantiated effect
  environment, and caller effect planning uses the identical instance-aware
  binding. An executable two-boundary generic-effect Wasm regression pins the
  zero-width case; the existing mutable reactive tape pins state retention and
  affine cleanup. The complete 83-test MIR unit suite, four-test Wasm export
  suite, both affine double-cancel diagnostics, missing-handler diagnostic,
  and SourceInspector Wasmtime plus generated-adapter/Bun gate are green. This
  establishes one real browser EventSource handler, not yet the general
  pointer/wheel/resize/frame/visibility/device-loss family. A fresh optimized
  gallery publication verifies 14 Fe modules / 73 deployment files and its
  real Chromium gallery/SourceInspector tape observes the compiler-derived
  surface sequence settling in order. The independently precompiled TodoMVC
  publication verifies one Fe module / three files and passes its complete
  Chromium behavior, keyed-identity, focus, UTF-8, and lifecycle tape.
- Document visibility is the second browser `EventSource` and the first one
  carrying a standards lifecycle value rather than a compiler declaration.
  `BrowserVisibilityEvents : EventSource<DocumentVisibility>` reports an
  initial typed `Visible | Hidden` observation and then waits for a distinct
  state. The affine `Subscription<DocumentVisibility>` owns its last
  observation cursor, so neither a mutable provider reference nor a permanent
  JavaScript subscription enters the suspended frame. The fixed adapter closes
  the check-to-listen race and realizes one abortable `visibilitychange`
  listener; it cannot decide whether hidden work proceeds. SourceInspector's
  scoped Fe task now waits while hidden, fails open if visibility observation
  itself fails, and only then begins the surface stream. Its generated-adapter
  gate supplies `Hidden` followed by `Visible` and rejects any surface pull
  before the Fe loop observes the latter. Separate fixed-host and standards-
  adapter tapes cover typed state retention, between-pull change detection,
  invalid states, cancellation, and exact listener cleanup. Pointer, wheel,
  resize, animation-frame, device-loss, and MessagePort handlers remain. A
  fresh optimized publication compiles all twelve render bundles and verifies
  14 Fe modules / 73 deployment files; its real Chromium gallery/SourceInspector
  tape passes. The independently rebuilt TodoMVC publication again verifies one
  module / three files and passes its complete Chromium behavior tape.
- Animation frames are the third concrete browser `EventSource` and the first
  continuously requested clock stream. `BrowserAnimationFrames` suspends one
  affine pull through the same generated continuation rail and returns an
  `AnimationFrame { timestamp: f32 }`; the fixed window adapter owns exactly
  one cancellable `requestAnimationFrame` request and supplies only the raw
  standards timestamp. SourceInspector now paces each completed, timed-out, or
  failed poster activation through that stream before pulling the next surface.
  Fe decides to continue without pacing if the frame source fails and consumes
  both subscriptions on cancellation/end. The generated-adapter oracle pins
  the exact hidden -> visible -> pull/load -> frame -> pull/load -> frame -> end
  sequence, while separate broker/bootstrap tapes cover typed f32 delivery and
  exact host/Fe cancellation. Pointer, wheel, device-loss, and
  MessagePort handlers remain. The full 33-test HTML precompiler suite is
  green. A fresh optimized publication compiles all twelve render bundles,
  verifies 14 Fe modules / 73 deployment files, and passes the real Chromium
  gallery/SourceInspector tape; standalone TodoMVC again verifies one module /
  three files and passes its Chromium tape.
- The first three real browser sources also paid back their first reusable reactive
  ergonomic. `Stream<T>::next_ready` consumes an affine subscription through
  any number of `Absent` polls and returns the sole successor with either an
  occurrence or the exact end/failure/cancellation observation. SourceInspector
  now uses that one combinator for surface, visibility, and frame streams rather
  than spelling three polling loops. A pure Fe `SparseSource` tape proves an
  absence followed by an occurrence and then exhaustion, including the final
  consumed cursor; the browser sequencing oracle remains unchanged.
- Viewport resize is the fourth concrete browser `EventSource` and the first
  state-shaped aggregate event. `Subscription<T>` now owns an explicit
  `observed` fact plus the last compiler-flattenable typed value, so neither a
  reserved numeric cursor encoding nor a JavaScript subscription graph stands
  in for Fe state. `BrowserViewportEvents` suspends an affine pull with
  `Viewport { width, height, device_pixel_ratio }`; the fixed adapter reports
  current standards values, owns one abortable `resize` listener only while
  unchanged, and closes the check-to-listen race. Broker and adapter tapes pin
  three-lane delivery, between-pull changes, and exact cancellation/cleanup.
- **Event Studio is now present as a resident Fe component and gallery tile.**
  Five concurrent actor-scoped Fe tasks own viewport, Pointer Events, wheel,
  document visibility, and paced frame/timer iteration, quantization, actor
  delivery, failure policy, resident state, DOM projection, and reconnect
  behavior. Pointer Events unify
  mouse, pen, and touch as one typed `PointerSample`; wheel retains signed XYZ
  deltas, delta mode, cursor position, control modifier, and timestamp. The
  fixed component-scoped adapter owns four pointer listeners or one wheel
  listener only while the corresponding affine pull is pending, excludes
  nested component ownership, and never performs gesture math or scheduling.
  A standalone exact publication verifies one Fe module / six files. Its real
  Chromium receipt proves initial 640x480 @ 1.5 DPR, a genuine touch-typed
  `PointerEvent`, a genuine line-mode `WheelEvent`, resize to 777x555 @ 2 DPR,
  and one fresh viewport observation after all three affine scopes cancel and
  reconnect with no host errors. An independent Wasmtime reducer checks the
  same resident viewport/pointer/wheel/lifecycle state semantics and invalid
  action-tag rejection rather than comparing generated bytes.
  The exact cache-disabled full gallery compiles 12 render bundles plus three
  page/resident projections in 158.435 seconds, verifies 15 Fe modules / 79
  deployment files, and passes both the combined Chromium gallery/inspector
  tape (including the real resize) and the complete TodoMVC tape.
  Device loss, map/filter/scan/merge/switch/latest, debounce, and bounded
  backpressure remain for this same demo.
- Event Studio now exercises document visibility and an efficient Fe-owned
  frame throttle over the shared runtime-control rail. The frame task suspends
  through the ordinary typed timer effect first and requests exactly one
  animation frame only after the interval elapses; the browser therefore does
  not deliver 60 frame callbacks per second merely for Fe to discard them.
  `std::host::sleep` centralizes timer-token minting plus continuation
  suspension without selecting a provider or adding a scheduler. The fixed
  host still realizes only `setTimeout`, `requestAnimationFrame`, and
  `visibilitychange`; Fe owns their order, rate, cancellation, failure policy,
  actor messages, and projection. A standalone Chromium tape proves initial
  visibility, hidden/visible transitions, paced timer/frame progress, existing
  pointer/wheel/resize behavior, and five-scope reconnect cleanup. The
  independent Wasmtime reducer now covers all seven task actions and exact
  20-leaf event / 16-leaf state shapes. The complete 33-test HTML precompiler
  suite is green. A cold exact gallery rebuild completed in 152.066 seconds,
  verifies 15 Fe modules / 79 deployment files, and passes the combined
  Chromium gallery/SourceInspector/Event Studio tape plus the independent
  TodoMVC browser tape. An attempted fluent `StudioTask::empty().with_*`
  authoring form type-checks but exact resident-Wasm lowering still rejects
  the aggregate `self` parameter; the explicit scalarizable constructors stay
  in the example until general aggregate-parameter scalarization lands. This
  is a compiler ergonomic gap, not permission for a JavaScript codec.
- Rich scoped-task messages no longer need to decompose one semantic value into
  a sequence of scalar `detail` messages. `ComponentEvent<A, P>` carries an
  application-defined nominal payload tail, `ActorSink` still validates its
  exact instantiated Fe identity, and the broker keeps the compiler-flattened
  lanes opaque. Browser-originated lifecycle/DOM events obtain the canonical
  zero value for that task-only tail from the Wasm transition function's own
  signature arity; scoped tasks must supply the exact complete arity. There is
  no payload JSON, field ID table, or JavaScript codec. Event Studio now sends
  one rich mailbox value per viewport/pointer/wheel occurrence instead of the
  former four-message viewport sequence. The fixed completion and bootstrap
  suites are green at 20 and 13 tests respectively.
- Actor-scoped tasks can now deliver ordinary typed values back into their
  owning resident Fe actor. `ActorSink<B, E>` is one keyed effect and
  `ActorMessage<E>::send` uses the existing affine `Pending`/`Suspend` rail;
  only the nominal `std::actor::raw::send_begin` declaration becomes the fixed
  `fe:actor` import. The resident compiler compares the sink's instantiated,
  normalized Fe event type with the actor transition's nominal event type and
  separately checks its runtime representation. A negative gate uses a
  different record with the *same* enum/u32 layout, so this contract cannot be
  satisfied by byte width or flattened-lane coincidence. The fixed broker
  keeps every event lane opaque, inherits generation/AbortSignal/stale-delivery
  rules from the canonical completion table, and invokes exactly one resident
  transition plus projection; it contains no component action or payload
  table and adds no JSON.
- SourceInspector is the first real consumer. Its scoped Fe surface-loading
  task sends typed progress, completion, and failure actions to the resident
  reducer, which owns the corresponding state. This also exposed and fixed a
  general effect-key bug: an explicit closed witness selected through a
  blanket generic provider could retain the provider impl's proof-local type
  parameter and re-generalize the authored key. The stored key now preserves
  the already-instantiated query whenever the solver result is not closed; a
  direct and inherent-method HIR regression pins the concrete type.
  Independent evidence is green for the four-part resident actor suite, the
  SourceInspector and TodoMVC semantic reducer gates, 18 fixed completion-
  broker tests, 11 bootstrap tests, and all 33 HTML precompiler tests. A fresh
  optimized publication compiled 12 render bundles in 129.475 seconds and
  verified 14 Fe modules / 73 deployment files. Its real Chromium gallery tape
  observes the exact resident sequence—connection, progress 1 through 12, then
  completion without failure—and the independent TodoMVC Chromium tape also
  passes. Pointer/wheel/device-loss/MessagePort sources, cross-actor and
  Worker/GPU sinks, and the fully combined resident gallery actor remain open.
- QCGA Pencil now exposes two Fe-owned display modes through the general
  `Param::toggle` vocabulary. The default unchecked mode marches the full
  renderer interval and restores the atmospheric fade for unbounded pencil
  members; the checked bounded mode retains the finite sphere broad phase but
  feathers its domain into the same sky instead of exposing a hard clip. The
  browser presents a checkbox and transports only the ordinary typed
  `ParamEdit`; a nominal Fe `PencilDisplayMode { bounded: bool }` selects the
  interval and fade algorithms. A fieldless enum was deliberately not replaced
  by numeric tags after the render backend correctly rejected its generated
  `Unreachable`; trap-free exhaustive shader-enum lowering is recorded as a
  compiler task below.
- The new mixed-scalar state ABI is re-proved rather than accepted by layout
  coincidence. Wasmtime establishes the unchecked default, the derived param
  index, canonical toggle normalization, exact preservation of every other
  scene leaf, and both display intervals/fades. The full eight-part QCGA suite,
  its 500-field/analytic-ray DE oracle, browser-profile compilation, 24 fixed
  render-runtime tests, 58 codegen unit tests, resident actor/SourceInspector/
  TodoMVC gates, and fixed completion/bootstrap suites are green. The generic
  runtime now materializes Fe controls before GPU acquisition and appends a
  host-failure notice without erasing them; Chromium therefore proves the
  checkbox and infinite-fade default even on this machine's adapter-unavailable
  path. A final cache-disabled optimized publication compiled all twelve
  render bundles in 120.034 seconds and verified 14 Fe modules / 73 files; its
  exact gallery/SourceInspector browser tape passes. The QCGA shaders remain
  within explicit dual-mode budgets at 34,510 bytes of DE WGSL plus 16,928
  bytes of marker WGSL and 67,617 bytes of shared-state control Wasm.
- Pure reactive bookkeeping now has executable, host-free Fe utilities rather
  than only roadmap vocabulary. `Scan<S>` applies a pure `Fn` reducer to typed
  occurrences, while `BoundedQueue<T>` makes FIFO admission, `DropNewest`,
  `KeepLatest`, and drop accounting application-visible; `Latest<T>` is the
  same one-slot protocol, initialized by `latest`. A seven-part deterministic
  Fe tape proves absence/terminal preservation, reducer state, both overflow
  policies, FIFO order, and latest-value replacement. Independent Wasmtime
  gates execute scan and the complete queue receipt with no function imports;
  the queue receipt asserts values, order, capacity, and drops rather than
  comparing generated bytes. The implementation deliberately uses four
  scalar Fe slots and portable `u32` metadata today: exact Wasm experiments
  rejected both a const-generic returned queue containing a nested array and
  target-sized `usize` metadata inside the returned generic aggregate. Those
  compiler gaps are recorded below; they are not hidden behind a JavaScript
  queue. Event Studio does not yet consume this queue, so bounded browser
  backpressure remains an integration task rather than an inferred claim.
- The first real stream-to-actor consolidation now consumes that substrate.
  `std::actor::forward_to_actor` owns affine subscription, `next_ready`
  iteration, exact-once cancellation, actor suspension, a `Scan` of successful
  deliveries, and distinct source-end/source-failure/source-cancel/sink-failure/
  sink-cancel terminals. Its observation and failure mappings are pure `Fn`
  values, so applications supply semantics without hiding authority or a
  callback graph. Event Studio's four duplicated viewport/pointer/wheel/
  visibility loops are replaced by four small nominal mappers over this one Fe
  utility; the paced frame task remains separate because its timer-before-frame
  ordering is meaningful application policy.
- Event Studio also places `BoundedQueue` and `Latest` on the real resident
  browser-Wasm path. Each Fe-owned timer tick runs a plainly labelled,
  deterministic four-value acceptance burst through a three-slot
  `DropNewest` queue and a one-slot `KeepLatest` queue, then projects cumulative
  drops and the newest value. The source says explicitly that this proves policy
  execution and is not yet a claim of concurrent pointer buffering. The
  independent resident oracle now checks a 22-leaf nominal event and 18-leaf
  state, including drop/latest semantics; standalone Chromium observes the real
  standards streams, scan forwarding, bounded drops, latest replacement, and
  five-scope reconnect cleanup. The combined gallery/SourceInspector Chromium
  tape passes as well, a warm exact publication verifies 15 Fe modules / 79
  files, and all 33 HTML-precompiler tests are green. No host queue, mapper
  table, or reactive JSON was added.
- True producer/consumer buffering is now on the real browser path rather than
  represented by that deterministic acceptance burst. `core::pending::Select`
  non-destructively arbitrates heterogeneous affine operations: success returns
  the still-live loser to Fe, child failure/cancellation is side-tagged and
  cancels the unreachable loser, and cancelling the owning scope cancels both.
  The fixed broker knows token custody and compiler-derived scalar layouts but
  no event, queue, or overflow policy. An exact generated Fe/Wasm/browser gate
  holds one actor send pending while two posted source values win in sequence,
  proving the same sink token survives both selections; materialized-runtime
  gates separately cover heterogeneous payloads, both success sides, typed
  failure/cancellation packing, and loser cleanup.
- `std::reactive::AsyncEventSource` exposes a begin-shaped affine source pull
  without choosing how it is combined. `std::actor::buffer_to_actor` uses that
  pull, `Select`, and the existing `BoundedQueue` to keep one source listener
  and one actor send live simultaneously. The in-flight value is deliberately
  separate from the bounded waiting backlog, so `KeepLatest` can never evict a
  value the sink is already accepting. Event Studio's real Pointer Events task
  now consumes this utility with a three-slot `KeepLatest` backlog. Its browser
  oracle deliberately stalls the first generic actor acceptance, delivers six
  genuine touch-typed PointerEvents, and observes four Fe deliveries, two Fe
  drops, and the newest coordinates. This also caught and fixed a generic
  materialized-task bug: inactive compiler-derived boolean lanes must use
  `false`, not a numeric zero. The resident reducer, exact heterogeneous-select
  gate, 27 fixed runtime tests, standalone precompile, real Chromium burst, and
  reconnect lifecycle are green. A fresh cold gallery publication compiles all
  12 render bundles, verifies 15 Fe modules and 79 deployment files, and its
  combined SourceInspector Chromium oracle repeats the same exact four-delivery/
  two-drop receipt. The timer retains only the independent one-slot `Latest`
  probe; bounded drops now come from actual concurrent browser traffic.
- The first rich aggregate/resumable ergonomics slice removes Event Studio's
  flattened browser-fact shadow record. Private Wasm helpers now admit an
  object-backed `mut self` only as an internal canonical-arena address; public,
  continuation, and host signatures remain recursively flattened values.
  Whole nested-record assignment writes compiler-derived target-layout leaves,
  and payload-free enum leaves round-trip between their compact memory tag and
  canonical i32 value lane. An executed heterogeneous nested-record regression
  uses a fluent owned builder, whole-record replacement, a fieldless enum,
  `u32`/`u64`/`bool` leaves, suspension, success, and cancellation; its exact
  start/resume receipts prove no pointer escapes into the continuation ABI.
  `PointerSample`, `WheelSample`, `Viewport`, `AnimationFrame`, and
  `DocumentVisibility` now derive `Default` in Fe, and Event Studio nests those
  actual nominal facts in its mailbox. This deletes 80 lines and all repeated
  13-field zero-fill constructors. The independent reducer executes the new
  34-leaf event, checks the same state semantics, and additionally rejects a
  forged nested `PointerPhase`; standalone Chromium repeats the real concurrent
  four-delivery/two-drop receipt. The complete 84-test Wasm execution suite,
  35-test runtime-handle suite, and 33-test HTML-precompiler suite are green;
  a fresh optimized publication verifies 15 Fe modules / 79 deployment files
  and passes the combined real Chromium gallery/SourceInspector/Event Studio
  tape. Payload-enum memory, general target-sized metadata, and the ideal
  const-generic queue backing remain separate work.
- The first deterministic-time and sharing policy set now executes as ordinary
  host-free Fe values. `VirtualTime` rejects rewind, leading `Throttle` records
  only admitted instants, trailing `Debounce<T>` replaces and explicitly
  consumes its pending value, generation-based `SwitchLatest` suppresses stale
  completions, and bounded `SharedReplay<T>` gives independent cursors exact
  retention/miss accounting. One zero-import Wasmtime receipt pins all five
  policies, queue indexing, terminal preservation, and the rewind trap; the
  complete 85-test Wasm execution suite also passes, including its independent
  CGA/QCGA, BN254, Poseidon, and Merkle oracles.
- Event Studio now merges Pointer Events and wheel as two distinct nominal Fe
  sources behind `MergePolicy<L, R, E>` and `merge_buffer_to_actor`. Both
  listeners remain pending while one actor delivery is held, and one Fe-owned
  three-slot `KeepLatest` queue applies overflow across the combined stream.
  Standalone and composed-gallery Chromium gates deliberately deliver six real
  touch-typed PointerEvents plus one real WheelEvent before releasing the first
  sink acceptance; the exact receipt is three pointer deliveries, one wheel
  delivery, three drops, and the wheel coordinates. The resident Wasmtime
  reducer independently checks the same cumulative drop semantics.
- That browser receipt exposed a generic fixed-runtime bug rather than an
  application workaround: nested `Select` previously reached the outer
  continuation as the raw inner winner, so the materializer assumed the wrong
  payload width. The completion broker now retains a generic race/select
  winner tree and affine-token custody, then materializes every nested envelope
  only against the outer compiler-derived continuation layout. A focused gate
  proves nested success and child failure, both side tags, both surviving loser
  tokens, and exact unreachable-loser cancellation. All 28 fixed browser-
  runtime tests, 33 HTML-precompiler tests, and 83 MIR unit tests pass. No event
  name, queue capacity, overflow policy, or merge graph entered JavaScript.
- Cargo now explicitly tracks the `core`, `core_derives`, and `std` directories
  embedded by `fe-common`; editing builtin Fe source can no longer leave a
  stale compiler binary until a manual clean. An unchanged debug build remains
  incremental at 1.39 seconds. A fresh optimized gallery publication after
  this invalidation compiled 12 render bundles and published 15 Fe modules /
  79 files in 126.165 seconds; its combined Chromium gate passes.
- Component-scoped pointer capture is now selected in authored Fe rather than
  inferred by JavaScript. `BrowserPointerEvents::new().capture_primary()`
  produces a distinct typed source; the raw source remains capture-free. The
  fixed standards adapter captures only the first primary pointer on `Down`,
  releases it on `Up`/`Cancel`, appends an honest `LostCapture` phase, and
  releases retained capture when the owning affine pull is cancelled. Event
  Studio consumes that construction directly while retaining its existing
  two-source merged Fe queue. Independent adapter evidence covers primary and
  secondary pointers, ordinary release, unexpected loss, and cancellation;
  generated-Wasm inspection proves Event Studio imports the captured source
  rather than the raw one; and a real Chromium receipt holds capture across a
  disconnect and observes exact scoped release. The 28-test fixed browser
  runtime suite, 14 bootstrap tests, Event Studio resident oracle, and focused
  deployment verifier are green. Render surfaces still use the fixed runtime's
  transitional capture/drag state and remain a consumer for the shared Fe
  interaction algebra.

This ledger records achieved evidence, not a relaxation of the phases or the
Definition of done below.

## Consolidated execution order (2026-08-13 prework)

The remaining work should be pulled through a small number of multiplying
abstractions, in this dependency order. A later item must consume the earlier
general interface rather than open a parallel demo-specific lane.

1. **Complete values and typed lifecycle at the resident boundary.** Finish
   value-correct Wasm products (deep aggregate copies, projected aggregate
   reads/references, fieldless enum state) and the append-only pointer
   down/move/up facts. This is the substrate for picking, components, native
   parity, reactive streams, persistence, and richer message payloads.
2. **One Fe interaction algebra.** Express `Pick`, `Drag`, `Orbit`, `PanZoom`,
   `RangeControl`, capture requests, and cancellation as typed state/effect
   constructions over the same lifecycle facts. QCGA is the flagship semantic
   proof: project control -> pick -> drag -> solve -> render, with the browser
   remaining geometry-blind. Mandelbrot, raymarch, and the other 3D examples
   then consume these constructions instead of owning gesture branches.
3. **One scene/camera/field vocabulary for authored raster and DE views.**
   Share camera rays, projection/inverse-drag math, implicit-field evaluation,
   gradients/normals, material identities, normal palettes, grazing/tangent
   sheen, AO/shadow, fog, and ordered sampling. The QCGA pencil's triangle view
   and companion distance-estimator raymarch must consume the same solved
   pencil value and interaction state; they are two views, not two apps or two
   solvers. Keep the vibrant normal-tracked palette and sheen as reusable Fe
   material policy rather than shader-specific constants.
4. **General generated actor/message orchestration.** Connect the outstanding
   MainThread/WebGPU provider and route typed pick/solve/view messages through
   the same Fe-derived package used by DEC's Worker lane. This removes the last
   reason to add a QCGA host hook and supplies the mechanism for proof
   submission and cross-component collaboration.
5. **Effect-backed reactive resident outer gallery.** Derive EventSource
   handlers from the typed browser capability vocabulary; interpret
   `Event`/`Stream`/`Signal` subscriptions through the canonical runtime control
   effects; and move sequential tile activation, lifecycle, timers, routing,
   backpressure, and component messages into one Fe gallery actor. The existing
   sequential activation reducer remains the compatibility test case for this
   composting, not a permanent handwritten host feature.
6. **Binary surface artifact and host-kernel contraction.** Project resource,
   pass, recovery, and presentation commands through typed exports; generate
   standards adapters; stop fetching/interpreting the render manifest. Do this
   after actor effects are real so JSON is deleted rather than replaced by a
   second temporary protocol.
7. **Compiler/library generalization passes.** Feed the same complete-value and
   FCO machinery into automatic GA expression normalization/CSE/scheduling,
   compact deep-view Mandelbrot state and policies, and the succinct orbit
   certificate. These are intentionally downstream of trustworthy value and
   message transport: no capstone should invent a bespoke browser lane.
8. **Legacy burn-down and performance budgets.** Port unique examples onto the
   general interfaces, retire duplicate JS/Rust generators, then set cold/warm
   compile, shader-size, frame-latency, and submission budgets on the smaller
   surface. Optimization may specialize compiler-derived structure, but must
   preserve independent semantic oracles rather than substituting byte matches.

The lifecycle/value, QCGA interaction, shared DE scene, first general actor
effects, true producer/consumer composition, rich aggregate payloads, merged
sources, deterministic time, switch/latest, and bounded sharing described
above are substantially landed. The current vertical slice is now the
remaining browser sources: device loss, MessagePort, fetch, and GPU
completion. This completes one multiplying axis before returning to
shared interaction and GPU/GA generalization.

## Ingot utility maturity map (2026-08-14 audit)

The campaign-facing ingots are not a bag of shims. They already contain a real
Fe-native control spine, but the polished public layer above it is fragmented
between `core`, `std`, portable/demo-local libraries, and applications. The
current completion strategy is depth-first on runtime control and reactivity:
that one axis multiplies gallery lifecycle, components, interaction, Worker
messaging, device recovery, and the future proof queue. Avoid broad cosmetic
rewrites until the underlying value/payload gaps can actually remove the
boilerplate.

The intended package layers are:

1. `core`: backend-neutral pending/outcome types, effects, actor roles, and FCO
   substrate;
2. `std`: interpreters, scoped tasks, reactive streams, browser/component/page
   interfaces, surface orchestration, and WebGPU;
3. portable libraries: math, geometry, color, precision/fields, GA, DEC, and
   cryptography/proofs; and
4. applications and evidence: gallery, TodoMVC, Event Studio, render examples,
   and independent semantic oracles.

Specialized schedules and legacy carriers belong in internal or test-support
layers, not beside reusable application utilities.

| Utility family | Proven now | Possible-now consolidation | Ideal/compiler-enabled endpoint |
| --- | --- | --- | --- |
| Runtime control | Typed `Pending`, `TaskOutcome`, nominal `Suspend`, generated continuation states, resident/scoped actors, typed actor sinks, timer suspension | One `std::runtime` facade; reusable source-to-actor forwarding; explicit scope/supervision policies | ZIO-like typed environment/exit/scope in Fe without a boxed monadic runtime; compiler-derived task handles and exactly-once structured cancellation |
| Reactive | Typed `Event`, affine `Subscription`, effect-backed `EventSource`/`AsyncEventSource`, zero-state `Stream`, `next_ready`, pure event map/filter/hold, executable `Scan`, `BoundedQueue`, `Latest`, `VirtualTime`, leading `Throttle`, trailing `Debounce`, `SwitchLatest`, bounded `SharedReplay`, non-destructive nested heterogeneous `Select`, two-source merged buffering, and exact Event Studio browser evidence | Lift the proven policies into one stream-graph surface; fuse effectful interpreters and structured shared cancellation | Static typed stream graphs with map/filter/scan/merge/switch/sample/throttle/debounce/share/replay fused by FCO into continuation machines |
| Browser sources | Render surfaces, visibility, animation frames, viewport, raw Pointer Events, Fe-selected primary-pointer capture, and wheel; host listeners are scoped and demo-blind | Device loss, MessagePort, fetch, and GPU-completion sources; move render surfaces onto the same capture/coordinate vocabulary | Standards-derived adapters generated from typed capabilities, with Fe owning combination, retry, lifetime, and gesture policy |
| Components/pages | FCO-derived action/part identity, resident reducers, keyed repeats, Fe page composition, TodoMVC/Event Studio/SourceInspector | Split browser resource effects from DOM projection; typed resident UTF-8 stores/projectors; seal raw part minting and patch buffers | Compiler derives action sums, opaque target identities, event dispatch, initial DOM, minimal projection, tasks, and resources from state/handlers/view |
| Surface interaction | FCO-derived parameter binding and cursor-aware Fe transitions; QCGA picking/drag/solve is Fe | Shared `PointerTracker`, `PanZoom`, `Orbit`, `PickDrag`, `RangeControl`, coordinate-space types, and capture requests | One typed interaction algebra consumed by every 2D/3D surface with no host gesture state |
| Scheduling/render | Latest-per-frame, sample-latest, throttle, debounce, accumulate/drop policies, responsive backing, typed pass graphs | Parameterize policies; split `std::webgpu` into program/surface/schedule/quality/resource/compute/device; shared marcher/material packages | Typed kernel values derive identity, layouts, grids, capabilities, and launch; real shared-memory/barrier/subgroup lowering with portable fallbacks |
| Math/color/precision | Shared scalar/vector/ray/sampling kernels, gallery palette, expansions/fixed arithmetic, BN254 Montgomery multiplication | Promote demo-local libraries; method-oriented vectors/cameras/colors; nominal `Fixed<L>` and `FieldElement<M,L>`; checked nonzero dimensions | Target-neutral numeric traits and compiler-supported fixed arrays remove recursive/storage and target-layout seams |
| GA/DEC | Named sparse matrix metrics, bounded expression compilation, support algebra, PGA/CGA/QCGA examples, typed DEC forms/operators | Shared `Pga2`, `Cga3`, `Qcga3` domain ingots; infer unambiguous null pairs; split portable DEC from fixed complexes and legacy sparse machinery | One arbitrary finite GA expression compiler with support inference, CSE/SSA DAGs, selectable algebraic reassociation, scalar/workgroup/multi-dispatch schedules |
| Proof/crypto | Field kernels plus strong Poseidon/Merkle fixtures and independent receipts | Promote limbs/fields, Poseidon, Merkle, transcript, claim/proof/verifier packages with malformed-input gates | Typed proof systems and WebGPU acceleration; succinct scoped Mandelbrot claims whose verifier is cheaper than orbit replay |
| Manifest/host | Fixed demo-blind browser host; compiler-generated artifacts and provenance; no caller-authored event/control JSON | Move fetch out of component opcodes, hide raw numeric task/kernel IDs, contract host modules, keep provenance build-time | One content-addressed module/artifact with typed versioned exports; no runtime render manifest or `PageRender.manifest_action` |
| Evidence | Native/Wasmtime/browser semantic tapes, independent Rust/JS/math oracles, compile and shader budgets | Reclassify specialized schedules and retained raster paths explicitly as oracle/test support | Every optimization preserves independent semantics; byte identity remains provenance evidence, never correctness evidence |

### Utilities we can make ergonomic now

- Add a narrow `std::runtime` facade without hiding the actual `uses` row, and
  consolidate repeated observe/send/cancel loops behind a pure typed mapper.
- Keep `ComponentWriter` as a low-level projector but move fetch into its own
  typed effect; expose component-local projectors instead of pointers,
  capacities, numeric opcodes, and manually correlated request IDs.
- Build the shared interaction and coordinate-space vocabulary, then migrate
  Mandelbrot, raymarch, QCGA, and the remaining surfaces onto it.
- Promote and split scalar math, vectors, cameras/rays, sampling, color, common
  raymarch policy, precision, and field arithmetic out of `demos/sketches`.
- Wrap fixed arithmetic as nominal values and use ordinary fixed arrays at the
  portable field boundary; keep recursive HList machinery private.
- Consolidate repeated PGA/CGA/QCGA metric, embedding, and incidence records
  into domain facades while retaining raw `ga_expr` for algebra authors.
- Split the portable algebra from the fixed DEC complex, `gaplay` from the
  Desargues construction, and the QCGA pencil into model/solver/interaction/
  material/oracle packages.
- Split `std::webgpu` by responsibility and centralize policy decision kernels
  before adding more GPU vocabulary.

### Utilities that need compiler work to become ideal

- Finish aggregate scalarization beyond the landed nested struct, fieldless-
  enum, whole-copy, mutable-receiver, return, and resumable value path:
  payload-enum memory, nested const-generic arrays, and portable target-sized
  metadata must remove the current four-slot queue backing without exposing
  arena pointers.
- FCO reflection/type emission rich enough to derive component action sums,
  opaque part types, view projection, task bodies, and typed task/kernel handles
  instead of public numeric constructors or parallel declarations.
- Generic `Signal<T>`, typed event errors, stream-graph fusion/sharing, and
  compiler-derived continuation/state layouts for higher-order combinators.
- A unified GA expression DAG with CSE/support planning and explicit semantic
  modes (`Strict` versus algebraically reassociated/balanced), followed by real
  workgroup/shared-memory/barrier/subgroup scheduling.
- Typed kernel launch and capability negotiation derived from Fe kernel values;
  current atomic/barrier types remain partial design scaffolding until an
  application kernel executes them.
- A binary/module surface contract that deletes runtime manifest interpretation
  from bootstrap/render code rather than replacing JSON with another schema.

### Consolidation and disposition

- Keep `core::pending`, `core::actor`, the canonical effect spine, typed browser
  sources, surface scheduling, and the FCO-derived binding/identity mechanisms
  as foundations.
- Promote `fmath`, `gallery_palette`'s reusable color substrate, `precision`,
  CGA/QCGA model definitions, and portable DEC into ordinary library ingots.
- Internalize legacy `sparse_clifford` implementation machinery, raw component
  buffers/opcodes, and specialized `canonical_cl41_schedule`-style artifacts.
- Retain the old QCGA projection and other independent implementations only as
  clearly named oracles; do not surface them as competing gallery applications.
- Compost `PageRender.manifest_action`, runtime JSON interpretation, fetch
  opcodes, raw numeric task/kernel identity, duplicated provider lists, and
  per-demo JS/Rust generators as their typed replacements land.

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

The current canonical gallery is an Fe-composed page containing Fe render
programs and resident Fe components, hosted by a substantial generic JavaScript
browser kernel. It now has a real typed Fe event/control spine, but not yet the
complete Fe-native reactive, interaction, resource, and recovery system.

### Genuinely Fe today

- Fractal, CGA, QCGA, DEC, gradient, and palette mathematics.
- GPU actor placement, stage roles, ordered pass graphs, and typed storage
  resources.
- WGSL emitted from Fe for every canonical gallery renderer.
- The brute Mandelbrot's fixed-precision orbit and adaptive precision policy.
- The perturbational Mandelbrot's `Fixed<8>` reference pass, binary32 delta
  pass, reanchoring/cancellation logic, and color policy.
- Control arithmetic in all parameterized canonical actors' typed `navigate`
  behaviors: fluent affine bindings and parameter-kind policy, plus the
  Mandelbrots' specialized pan sensitivity, zoom curves, clamps, cursor
  anchoring, and high-precision center updates. These behaviors compile from
  Fe to Wasm, and their `LatestPerFrame` choice is declared in Fe.
- CTFE-projected parameter names, ranges, initial values, kinds, and extents.
- Full canonical page composition, resident TodoMVC/Event Studio/
  SourceInspector state transitions, component projection, and typed actor
  messages.
- Typed effect-backed surface, visibility, animation-frame, viewport, raw and
  Fe-captured pointer, and wheel sources; affine subscription/cancellation and
  five concurrent Event Studio task families.
- Compiler-derived suspension/re-entry, typed timer pacing, actor delivery, and
  Fe-owned task order/failure policy. Pure `Scan`, bounded queue, and latest-
  value semantics execute in both Fe tests and Wasmtime without host imports.

### Fixed JavaScript host today

`crates/codegen/assets/render-runtime/fe-render-runtime.js` currently owns:

- DOM/custom-element construction and slider widgets;
- browser pointer/wheel listener registration;
- transitional render-surface pointer capture, active-pointer state, and drag
  delta production (resident components now select capture in Fe);
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
rule. Browser object ownership and standards callbacks remain legitimate host
work; pointer gesture state, manifest interpretation, resource recovery policy,
and transitional component opcodes are still composting targets.

### Important corrections

- Scheduling policy and its state/decisions are Fe-declared and resident in
  generated Wasm. JavaScript buffers untouched raw records, realizes Fe's
  bounded queue effect, and supplies standards facts from
  `requestAnimationFrame`/GPU completion; generated Wasm performs motion
  coalescing and invokes the authored transition when Fe admits a frame.
  Complete actor state is resident in private Wasm globals; JavaScript keeps
  only a GPU-upload mirror and supplies explicit initialization/extent/
  restoration. Slider and scripted changes are raw typed events handled by the
  same Fe transition. Browser callback registration and promise realization,
  not the presentation decision, remain host work.
- Cursor-anchored pan/zoom mathematics is Fe; cursor acquisition,
  normalization, capture, raw delta production, and timing are JavaScript.
  QCGA point projection, selection, drag geometry, and re-solving are Fe.
- The gallery is not using Rust-Wasm to fake its renderers. `fe web dev` and
  precompile are native Rust toolchain operations. Browser Wasm artifacts are
  compiler output from Fe; live GPU rendering uses Fe-generated WGSL. The
  perturbational renderer's Wasm is its generated fixed Fe surface-transition
  export; its two render passes are Fe-generated WGSL.
- `std::reactive::{Event, Stream}` and typed browser `EventSource` handlers are
  on the real gallery path through SourceInspector and Event Studio. Pure
  `Scan`, `BoundedQueue`, `Latest`, heterogeneous `Select`, and genuine bounded
  pointer producer/consumer composition execute in Event Studio;
  stream-level map/filter/scan/merge/switch/share remain open. `Signal` is
  still narrow and mostly pure vocabulary.
- The safe browser surface now exposes typed asynchronous event sources through
  runtime-control effects, including a distinct Fe-selected scoped pointer-
  capture source. Device loss, MessagePort, fetch, GPU completion, render-
  surface capture migration, and richer structured resource effects remain
  open.
- The render manifest is compiler-generated rather than hand-authored, but it
  is still a semantic runtime protocol: bootstrap/render code interprets its
  passes, resources, controls, and artifact paths, and Fe page code still
  exposes `PageRender.manifest_action`. The endpoint remains its deletion, not
  a nicer JSON schema.

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

- Thirteen figures are sourced from Fe ingots: twelve render actors plus the
  resident TodoMVC component.
- Every interactive render tile has typed Fe `SurfaceTransition` controls and
  Fe-declared `LatestPerFrame` scheduling. None emits the legacy JSON `control`
  block.
- Known-color and rollcall are pure Fe-derived GPU graphs with no Wasm module.
- Perturbational Mandelbrot is a two-pass Fe GPU graph; its Wasm is the typed
  Fe control lane only.
- No tile has its own `main.js`.
- `GalleryPage` authors the complete body structure and all render/component
  declarations in Fe. `gallery.html` retains the document shell and
  transitional CSS only. The source/WGSL/Wasm/manifest viewer is a resident Fe
  `SourceInspector`; the fixed render runtime still consumes the transitional
  render manifest today.
- The QCGA Pencil is canonical and consolidated into one iterative DE render
  actor. Its solved state, camera, projection, control-point pick/drag/re-solve,
  exact-gradient material, and scheduling are Fe; the former duplicate raster
  actor has been retired while its projection survives only as an independent
  zero-import oracle.
- DEC's Worker/message-shaped Fe functions derive an ordinary generated
  browser actor, and the gallery Chromium gate executes `d0` through its real
  Worker/Wasm lane. Its Fe-declared MainThread `submit_view` capability is
  present but explicitly unavailable until general WebGPU orchestration lands;
  the visible picture still uses the separately projected fragment actor.
- SourceInspector is also the resident outer gallery shell. Its actor-scoped
  Fe task owns sequential poster policy through typed pending surface loads and
  a Fe `Race`/`Timer`; the host performs only compiler correlation and browser
  load realization. Full routing, tile lifecycle, and component-to-component
  orchestration remain open.

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
   surface-policy default, rather than hidden demo behavior. Landed for
   resident components as the Fe-selected `capture_primary()` source policy;
   render surfaces still need to consume the shared interaction/effect form.

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

0. Close low-hanging Wasm language-parity gaps before growing more examples:
   implement boolean unary `!`/`Unary::Not`, exercise it in generated Wasm,
   audit the ordinary operator/control matrix against native, and pin every
   discovered gap with a semantic regression rather than generated-byte
   equality. Do the same for exhaustive fieldless-enum control in render
   stages: WGSL/SPIR-V lowering must prove the discriminant closed or emit a
   trap-free structured switch, rather than inserting an `Unreachable` into a
   shader ABI that intentionally has no trap channel. Until then, nominal
   policy records with boolean fields are the honest shader-safe form; demos
   must not replace the missing lowering with anonymous numeric tags.
1. Finish canonical allocator/PostReturn and rich-record transport. Landed:
   nested records and fieldless enums cross resumable Wasm as flattened values;
   private fluent `mut self` helpers use internal object storage; whole nested
   assignment is target-layout-derived; and Event Studio carries actual nominal
   browser facts with an executed rich-mailbox regression. Remaining:
   payload-enum memory, nested const-generic arrays, and portable target-sized
   metadata. Do not replace those gaps with packed integers or field IDs.
2. Connect generated WebIDL callback adapters to compiled Fe callback bodies.
3. Add the MIR suspension/re-entry transform for resumable Fe tasks.
4. Make actor state resident in a live Fe instance instead of couriered in
   JavaScript uniform arrays.
5. Complete browser `EventSource` coverage. Landed: compiler-correlated render
   surfaces, visibility, animation frame, aggregate viewport resize,
   component-scoped Pointer Events (mouse/pen/touch), Fe-selected scoped
   primary-pointer capture, and wheel. Remaining: device loss and MessagePort
   streams.
6. Re-orient `std::reactive::{Event, Stream, Signal}` around runtime control
   effects before putting it on the real gallery path. Values and combinators
   remain pure Fe descriptions/reducers; subscribe, await-next, yield, wake,
   timer/frame, cancellation, backpressure, placement, and resource lifetime
   are handled by the canonical effect system rather than a parallel reactive
   runtime or JavaScript scheduler.
7. Preserve affine subscription/cancellation semantics across the host boundary.
8. Turn runtime control into ordinary typed Fe effects rather than another
   numeric host-command protocol. Define effect families for suspend/await,
   yield, timer/frame subscription, spawn/join, cancellation, resource
   acquire/release, placement, and supervision; make the effect set visible in
   function/actor types and rejected when no handler is in scope.
9. Lower effect performance to the same MIR suspension/re-entry machinery and
   resume a continuation exactly once with a typed success, failure, or
   cancellation value. Preserve affine continuation/subscription/resource
   ownership and suppress stale or late host completions by generation.
10. Generate handler adapters for MainThread, Worker, and WebGPU placements.
    Handlers may realize browser objects, promises, clocks, and queues, while
    Fe handlers own combination, retry/backoff, timeout, cancellation,
    supervision, and algorithm-selection policy. Keep pure reducers directly
    executable with deterministic test handlers.
11. Use structured scopes for child actors/tasks: parent cancellation and
    resource release are deterministic; detached work requires an explicit
    capability; bounded mailbox/backpressure and restart budgets are typed
    policies. Prove nested-handler routing cannot capture another component's
    effects merely because an opaque handle or ordinal coincides.
12. Provide one effect-backed reactive interpreter family for synchronous
    collections, deterministic virtual-time tapes, browser `EventSource`,
    async iterators, Worker/message lanes, and WebGPU completion streams. Pin
    fusion and sharing semantics (`map`/`filter`/`scan`, merge/switch/latest,
    sample/throttle/debounce, multicast/replay) in Fe; handlers realize wakeups
    but do not choose or reconstruct the stream graph. Dropping the owning
    scope must cancel upstream work exactly once and suppress late delivery.

Current reactive status: the synchronous/effect-backed source spine,
`next_ready`, event-level map/filter/hold, pure `Scan`, explicit bounded queue,
`Latest`, deterministic virtual time, throttle/debounce, switch/latest, bounded
sharing/replay, shared source-to-actor forwarding, six real browser source
families, scoped cancellation, and paced timer/frame task are landed with
semantic and browser gates. Event Studio proves scan-based forwarding and real
concurrent merged Pointer Event/wheel buffering through nested non-destructive
heterogeneous `Select`; its timer retains a separate deterministic `Latest`
probe. Next are device loss, MessagePort, fetch, and GPU-completion sources,
followed by the unified stream-graph authoring layer. The
four-slot queue is an honest current-Wasm envelope; general nested-array/
target-sized aggregate scalarization must recover the ideal const-generic
implementation.

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

Current status: the functional page-composition exit condition is landed. One
role-selected Fe entrypoint composes the full canonical gallery body, produces
no page runtime artifact or JSON, and is exercised in a real browser. Resident
component modules now project their own static DOM as well, so `GalleryPage`
does not spell out TodoMVC/inspector internals. The remaining Phase 5 cleanup
is moving the transitional shell CSS onto a fixed selectable theme and retiring
the compatibility shell once that theme covers it.

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

#### Fe-authored GPU compute, workgroups, and proof kernels

The existing compute-surface, workgroup-size, dispatch-grid, storage-resource,
barrier, SPIR-V, and WGSL types are a starting vocabulary, not evidence that
application kernels can already use the complete model. Finish this as an
explicit compiler/runtime track:

1. Define typed Fe compute entrypoints with global/local/workgroup invocation
   identities, compile-time workgroup sizes, dispatch geometry, storage and
   uniform buffer views, storage textures, atomics, and checked address-space
   rules. Derive bindings and layouts from Fe types; do not add JSON manifests.
2. Lower workgroup memory and barriers end to end. Statically reject divergent
   barrier placement, out-of-bounds shared arenas, incompatible access modes,
   and dispatch/workgroup shape mismatches. Exercise the result in both WGSL
   and SPIR-V paths, not merely in the type vocabulary.
3. Add a capability-gated Fe subgroup vocabulary for ballot, broadcast,
   shuffle, reductions, and scans. Never assume a fixed subgroup width: query
   adapter limits, specialize only under an explicit policy, and retain a
   scalar/workgroup fallback with the same semantics.
4. Give CTFE/FCO schedulers multiple interpreters over one exact dependency
   DAG: scalar straight-line, packed-vector, output-partitioned subgroup, and
   workgroup/shared-memory. Record work, depth, communication, fanout, liveness,
   occupancy, barrier count, and shared-memory use before choosing a schedule.
5. Express capability negotiation, dispatch/await, device loss, cancellation,
   resource lifetime, and recovery through the canonical runtime control
   effects above. The fixed browser handler may acquire objects and realize
   promises; it must not choose an application algorithm or silently weaken a
   requested proof/security policy.
6. Build the cryptography path in audited layers: finite-field arithmetic and
   batch inversion; Poseidon/permutation and Merkle kernels; radix/Stockham NTT
   and FFT; multi-scalar multiplication where the curve/backend model justifies
   it; transcript/challenge derivation; and the polynomial/FRI or other proof
   kernels selected by the actual Fe proof system. Prefer reusable primitives
   shared by browser proof generation, Mandelbrot reachability, and standalone
   crypto demos over proof-specific GPU shims.
7. Keep the prover/verifier boundary honest. WebGPU may accelerate witness and
   proof generation, but canonical typed Fe owns the statement, transcript,
   field/curve parameters, proof encoding, and verification. The verifier must
   run independently in Fe Wasm/native/contract targets where supported and
   never trust a GPU-produced receipt merely because the dispatch completed.
8. Gate semantics with independent CPU/reference implementations, published
   vectors, algebraic/property tests, malformed-proof rejection, boundary and
   overflow cases, cross-backend transcript parity, and mutation tests. Add
   randomized GPU-vs-reference execution on real adapters plus browser E2E;
   Naga validation, shader bytes, and byte-for-byte compiler output are useful
   shape evidence but are not correctness proofs.
9. Publish reproducible performance envelopes by adapter and problem size:
   cold/warm compile, upload/download, dispatch, end-to-end prover time,
   occupancy, memory, and crossover against scalar/Wasm. A GPU schedule lands
   only when measured gains amortize synchronization and transfer costs.

Exit condition: a non-trivial Fe proof-generation pipeline executes reusable
Fe-authored compute kernels in browser WebGPU, produces a canonically encoded
proof accepted by an independently executed Fe verifier, rejects mutated
claims/proofs, survives capability loss through an explicit policy, and ships
no application math or schedule in Rust/JavaScript/JSON scaffolding.

### Phase 7: simplify and generalize the Fe demos

#### Geometric-algebra expression compiler

1. Define a static typed expression vocabulary for leaves and binary/unary GA
   operators (sum/subtraction, geometric and outer products, contractions,
   grade projection, reverse, and dual). "Arbitrary" means arbitrary finite
   composition of those type-level nodes known during compilation; a runtime
   AST or a leaf with unknown/dense support cannot be CTFE-sparsified.
2. Interpret the same expression twice at compile time: once over conservative
   blade support, and once into a normalized term plan carrying output blade,
   operand-product key, coefficient/sign, dependencies, and source-order
   identity. Generalize beyond the present dimension-five/192-candidate
   envelope without hiding an unbounded compile-time explosion.
3. Add an FCO `CompileGa<E, Metric, NumericPolicy>` provider which emits one
   shared straight-line SSA DAG for a shader invocation: leaf loads and common
   products occur once, provably absent terms disappear, independent outputs
   are available together, and reductions are balanced only when policy allows
   it. This exposes instruction-level parallelism; actual issue/scheduling
   remains the WebGPU implementation's job because an invocation cannot spawn
   threads.
4. Preserve two honest floating-point policies. `Strict` keeps source order and
   only prunes structural zeros. `AlgebraicBalanced` may reassociate, merge, or
   cancel terms under an explicit real-algebra contract; it must not silently
   pretend that those rewrites preserve every IEEE-754 NaN, signed-zero, and
   rounding behavior.
5. Gate the provider semantically against the dense Fuchs--Thery recurrence on
   generated expression trees, sparse supports, metrics, and adversarial
   floating-point values. Separately inspect emitted MIR/WGSL for survivor
   operation counts, single loads/CSE, balanced depth, absence of runtime plan
   branches, browser-profile validation, and executed GPU parity. Byte matches
   and shader-size reductions are supporting evidence, never correctness.
6. Keep true cross-lane collaboration as a distinct workgroup track. It would
   require several shader invocations plus workgroup memory/barriers and a
   partition/reduction mapping; it cannot satisfy a literal single-invocation
   constraint and must not be described as threads spawned inside one lane.

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
6. Add a full-stack succinct orbit-certificate capstone without overstating
   Mandelbrot membership:
   - distinguish `EscapesBy<N>` from `EntersAttractor<P>` claims; the latter
     proves entry into a certified attracting-cycle enclosure with an explicit
     contraction/error bound, rather than calling every bounded orbit
     "convergent";
   - state the numeric model in the claim (fixed-point width, rounding, escape
     radius, and iteration bound), and separately account for the interval
     bound connecting that execution to the intended complex orbit;
   - have Fe generate the execution witness, commit to the orbit trace, and
     produce a genuinely succinct reachability proof (using the existing Fe
     field/Poseidon/Merkle foundations as building blocks), so verification is
     asymptotically and concretely cheaper than replaying the full orbit;
   - encode the proof as a canonical typed Fe value, transport/submit only its
     bytes through the fixed host, and run the same Fe verifier in browser
     Wasm, native differential tests, and the contract backend where supported;
   - show the point, exact claim, witness length, proof size, prover work, and
     verifier work in the gallery instead of reducing the result to a vague
     in-set badge; and
   - gate known answers, independently replayed short traces, altered-point /
     altered-claim / altered-proof rejection, native-Wasm-verifier parity, and
     an end-to-end submitted-proof receipt. Rust/JavaScript may remain
     independent oracles and host adapters, never the shipped prover/verifier.

#### Other examples

1. Extract reusable `PanZoom`, `Orbit`, `RangeControl`, `Camera`, and
   `PickDrag` Fe constructions.
2. Make every rich example a consumer of those general forms rather than a new
   wiring implementation.
3. Require a new-demo generalization test: a new interactive actor must need
   only Fe source and no runtime/compiler change.
4. After the Phase 3 control spine is reusable, add one **Event Studio**
   acceptance demo rather than a collection of artificial operator samples.
   It visibly traces pointer/touch, wheel, resize, visibility, frame, timer,
   and device-loss sources and composes them in Fe with map/filter/scan,
   merge/switch/latest, throttle/debounce, cancellation, and bounded
   backpressure. Deterministic tapes and a real-browser receipt must establish
   behavior; the fixed host may report standards facts but may not reconstruct
   the stream graph.
5. Treat the resident gallery itself as the router/lifecycle acceptance demo:
   URL state, nested component ownership, ordered surface activation,
   cancellation, and failure recovery must use the same general Fe effects.
   Do not add a parallel router runtime merely to make a standalone sample.
6. Evolve the Mandelbrot proof capstone into a **Proof Queue** consumer of
   structured scopes and Worker/MessagePort effects: Fe owns submission,
   progress, cancellation, retry, canonical proof values, and verification;
   the host transports opaque bytes and browser facts only.
7. Once the Phase 6 primitives execute, add a CPU-oracle-checked **GPU Kernel
   Lab** over reusable NTT/MSM/GA kernels. It must exercise workgroup memory,
   barriers, capability-gated subgroups, dispatch/await, and device recovery
   through the ordinary Fe vocabulary, not laboratory-only host calls.
8. Add a compact fetch/stream consumer only when canonical URL, response-body,
   abort, timeout, and backpressure effects exist. It should prove nested
   component and network lifecycles without expanding the fixed browser host
   into an application-aware data client.

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
- the succinct Mandelbrot capstone generates, transports, and verifies its
  honestly scoped typed proof through Fe code, with a verifier demonstrably
  cheaper than full orbit replay;
- Rust remains only in the toolchain and independent gates; and
- every legacy showcase is migrated, reclassified, or retired.

## Current depth-first execution slice

The original provenance, typed surface ABI, Mandelbrot parity, Fe scheduling,
resident state, and first runtime-control slices are landed. Continue in this
order:

1. **Landed:** pure `Scan`, bounded queue, and `Latest` with Fe and exact Wasm
   semantic gates, preserving the compiler blockers discovered by the ideal
   const-generic representation.
2. **Landed:** Event Studio now exposes scan accumulation, latest replacement,
   true concurrent bounded pointer buffering, side-tagged terminals, and exact
   cancellation. The broader stream graph remains.
3. **Landed:** repeated source-to-actor loops are consolidated behind a typed
   Fe utility without hiding effects, authority, or failure policy.
4. **Landed:** rich nested struct/fieldless-enum values survive suspension;
   private fluent owned builders and whole nested assignment execute; Event
   Studio uses Fe-derived defaults and actual nominal browser facts.
5. **Landed:** two-source merge, switch/latest, bounded sharing/replay,
   debounce/throttle, deterministic virtual time, and nested-select browser
   materialization, without opening a parallel reactive runtime.
6. **Landed for resident components:** Fe-selected primary-pointer capture,
   unexpected-loss reporting, and affine cancellation release; render surfaces
   remain a shared-interaction consumer.
7. Add device loss, MessagePort, fetch, and GPU-completion sources on the same
   rail.
8. Use that completed control/reactive spine for shared interaction, Worker/GPU
   messages, device recovery, and the future proof queue.

Only then resume broad package cosmetics. Promote and split libraries when a
shared abstraction has at least two real consumers and independent evidence;
do not move boilerplate around while its compiler-level cause is still open.
