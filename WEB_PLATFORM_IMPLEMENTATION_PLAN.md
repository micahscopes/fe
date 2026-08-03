# Fe standards-driven web platform implementation plan

Status: ratified implementation plan  
Capstone: one canonical Fe Q12 Mandelbrot computation executed and
cross-verified on EVM, Wasm, browser WebGPU, and native Cranelift.

## 0. Current implementation checkpoint

Implemented and exercised:

- versioned in-memory compiler protocol/database/facade and a real browser
  compiler Worker;
- HTML5-parsed inline/external Fe loading plus content-addressed production
  rewriting through `fe web precompile`;
- a target-neutral `#[host_import]` compiler path;
- generic resource, callback-token, future-token, ownership, and structured
  error runtime contracts;
- linked WebIDL interfaces, partials, mixins, namespaces, typedefs,
  dictionaries, unions, callbacks, constants, constructors, and retained
  iterable/maplike/setlike forms, with deterministic host/adapter plans;
- live iterable/maplike/setlike resource protocols with explicit iterator
  ownership, completion, error, mutation, drop, and stale-handle behavior;
- the checked-in `fe:host-wasm-codec/v1` rich-value layout and JavaScript plan
  interpreter;
- a scalar/resource `std::web` Window → Document → Element vertical;
- host-neutral EventSource → Stream → FRP layering with affine cleanup;
- canonical-source Mandelbrot evidence across revm/EVM, Wasm, validated
  browser-profile WGSL, and native/Cranelift.

Still deliberately gated:

- safe owned rich results until the reviewed Sonatina stack-memory candidate is
  present in a reachable pinned dependency;
- executable guest callback trampolines and resumable async state machines;
- rich/event/promise methods in `std::web`;
- live WebGPU execution on a host with a browser GPU adapter;
- async collection execution and raw Fe rich iterator emission;
- a complete watcher/reload server above the implemented semantic dependency
  inventory and immutable last-good publication core.

The gates above are product boundaries, not aliases or simulated support.

## 1. Product statement

Fe will support ordinary web programming without teaching the compiler the
names or semantics of DOM, HTML, WebGPU, events, promises, workers, or FRP.

The stack is:

```text
WHATWG/Web standards snapshots (Web IDL)
                  |
                  v
       linked interface graph
                  |
          +-------+--------+
          |                |
          v                v
   generated raw Fe   generated JS adapters
          |                |
          +-------+--------+
                  |
             generic host ABI
                  |
      +-----------+------------+
      |                        |
      v                        v
  std::web wrappers       module manifest
      |                        |
  Future / Stream         script loader
      |                        |
 optional FRP             dev or build
```

Development mode compiles inert `<script type="application/fe">` source in a
browser Worker. Production mode standards-parses the HTML and rewrites the same
scripts to content-addressed precompiled artifacts. Both modes consume the same
compiler protocol, module manifest, generated imports, entry semantics, and
lifecycle contract.

## 2. Non-negotiable design rules

### 2.1 Compiler boundary

Compiler support is limited to general language-wide interop and target
selection:

- imports and exports;
- target-neutral value and resource ABI;
- structured compilation inputs, artifacts, and diagnostics;
- target triples, layouts, and capability queries.

The compiler must not recognize names such as `Window`, `EventTarget`,
`GPUDevice`, `WebGpuBackend`, `MainThread`, `Worker`, or FRP concepts.

### 2.2 Two-sided idiomaticity

Every phase requires both an Fe review and a web-platform review.

| Concern | Fe-idiomatic requirement | Web-idiomatic requirement |
|---|---|---|
| Interfaces | Nominal types, traits, effects, `Option`/`Result` | Exact Web IDL types, conversion, overload, exposure, and exception semantics |
| Resources | Unforgeable own/borrow/drop and explicit affinity | JavaScript identity, GC rooting, realms, and thread affinity |
| Async | `Future`, `Stream`, cancellation, typed failure | Promise jobs, events, `AbortSignal`, and callback lifetime |
| Modules | Virtual ingots and explicit import graph | URL/base resolution, CORS, credentials, CSP, and caching |
| Diagnostics | Stable spans and structured diagnostics | Virtual source URLs, source maps, Worker transport, DevTools |
| Builds | Deterministic target/entry/interface manifests | Standards-based HTML parsing, integrity, content types, cache headers |
| Targets | Explicit semantic target and capabilities | Runtime feature detection and provider availability |

Generated raw bindings may be mechanical. Public `std::web` APIs must feel like
Fe. JavaScript adapters may implement Web IDL algorithms, but arbitrary
JavaScript objects or `any` must not masquerade as typed Fe values.

### 2.3 Honest multi-backend abstraction

The shared abstraction is pure authored computation, not a fictional universal
execution environment.

- EVM has contract-call adapters and persistent execution semantics.
- Wasm has exports, linear memory, and host imports.
- native has a target ABI and JIT/AOT execution.
- WebGPU has grid/render entry envelopes and GPU resource submission.

Adapters remain visibly distinct ordinary wrappers. The shared kernel contains
no target-selection effect and is byte-for-byte/source-digest identical between
legs.

## 3. Existing foundation and debt

Reusable foundations:

- `extern` plus `#[wasm_import(module = "...")]` already produces real generic
  Wasm imports.
- `DriverDataBase::default()`, a virtual touched source, and `BackendKind`
  already form an in-memory compilation seed.
- `fe-webidl-bindgen` parses and links an initial Web IDL subset and emits raw
  Fe imports plus a JavaScript handle-table adapter.
- `demos/fe-sandbox/fe-script-loader.js` implements an injected compiler
  contract, inline and `src` source, ordered execution, entry selection,
  provider composition, and lifecycle events.
- existing tests execute one Fe computation on EVM, Wasm, and SPIR-V/WebGPU.
- current web bundle code contains reusable deterministic manifest,
  hashing/materialization, browser-WGSL validation, and atomic-publication
  mechanics.

Debt to retire or quarantine:

- `canonical_interface.rs` contains exact compiler-owned browser type and
  capability recognition.
- `WebBundle` combines generic artifact mechanics with fixed browser actor and
  render policy.
- raw `u32` handle structs are forgeable and only suitable for the initial ABI
  spike.
- the current generator ignores exposure metadata and lacks most Web IDL
  definition kinds and rich values.
- the existing `fe` CLI, resolver, reporting, and codegen graph contain native
  filesystem, Git, terminal, server, and temporary-file dependencies.
- the historical “20-site wasm32 port” is an unaudited estimate, not an
  existing port or artifact.

Migration policy: extract reusable target-neutral mechanics, add replacements
and parity tests, migrate consumers, then delete recognizers. Do not grow the
old compiler-owned browser seam.

## 4. Phase plan

### Phase 0 — versioned contracts and architecture gates

Deliver:

- a versioned target-neutral `CompileRequest`;
- virtual source-file and ingot inputs;
- structured diagnostics with stable source spans;
- a versioned `CompileResult`;
- artifact and interface manifests with SHA-256 identity;
- compatibility and deterministic serialization rules;
- documented development and production script conventions.

Initial targets in the protocol are semantic names: `evm`, `wasm`, `webgpu`,
and `native`. Unsupported targets fail explicitly; listing a target is not a
claim that its backend exists.

Gates:

- JSON golden round trips;
- stable ordering and deterministic hashes;
- unknown major-version rejection;
- artifact digest mismatch rejection;
- no browser API names in compiler-protocol or compiler-core crates.

### Phase 1 — in-memory compiler core

Extract a narrow compiler library around:

```text
virtual sources -> parse/analyze -> MIR package -> structured diagnostics
```

Deliver:

- virtual source and package providers;
- embedded `core` and `std` ingots;
- no filesystem discovery, Git, current directory, terminal reporter, server,
  subprocess, or network dependency;
- a facade callable natively and from wasm32.

Gates:

- native and `wasm32-unknown-unknown` checks;
- valid source produces a runtime package;
- invalid source returns structured diagnostics without panic;
- deterministic source/package identity;
- denylist checks for filesystem/network/process imports in the browser graph.

### Phase 2 — narrow Wasm emitter

Separate or feature-isolate the Wasm emitter from EVM, SPIR-V, WebBundle,
temporary publication, and native-only dependencies.

Deliver:

- MIR runtime package to validated Wasm bytes;
- target-neutral import/export/interface inventory;
- deterministic output at the supported optimization level;
- Wasm artifact manifest generation.

Gates:

- the emitter itself checks on wasm32;
- a browser wasm test compiles, validates, instantiates, and executes a tiny Fe
  source;
- invalid or unsupported MIR fails closed;
- generated WebIDL imports pass through unchanged.

### Phase 3 — browser compiler Worker

Deliver:

- a wasm-bindgen compiler facade;
- a versioned Worker request/response protocol;
- request correlation, cancellation, and protocol/compiler handshake;
- transferable Wasm artifact bytes;
- structured diagnostics with virtual URLs and spans;
- bounded worker pooling and compilation cache keys.

Gates:

- a real browser compiles edited source rather than substituting a fixture;
- the compiled module returns `42`;
- two concurrent requests correlate correctly;
- cancellation and worker failure are deterministic;
- compilation never blocks the main browser thread.

### Phase 4 — general host ABI

Build the reusable ABI before broad Web API generation.

Ordered value/resource rungs:

1. scalars and nominal opaque resources;
2. owned, borrowed, nullable, and dropped resources;
3. strings and byte buffers with explicit encoding/ownership;
4. records, enums, variants, options, and results;
5. lists and typed array/buffer views;
6. callbacks with lifetime and cancellation;
7. async results and host exceptions.

The value model should align with the Wasm Component Model/WIT where that
preserves semantics. JavaScript adapters separately implement Web IDL
conversion and overload behavior.

Gates per rung:

- round trip through a JavaScript host;
- round trip through a non-browser fake host;
- malformed value rejection;
- resource use-after-drop/double-drop/leak tests;
- no forgeable public handle construction;
- no Web-specific names in ABI machinery.

### Phase 5 — comprehensive Web IDL frontend and generator

Ordered linker rungs:

1. typedefs and enums;
2. dictionaries and partial dictionaries;
3. interface mixins and `includes`;
4. callbacks and callback interfaces;
5. namespaces, constructors, constants, and special operations;
6. iterable, async iterable, maplike, and setlike;
7. nullable/optional/variadic, unions, sequences, records, buffers, promises;
8. extended attributes and exposure graph.

Deliver:

- source-located diagnostics;
- deterministic linked interface graph;
- pinned standards snapshot and provenance;
- interface/feature selection;
- raw Fe declarations;
- JavaScript Web IDL adapters;
- interface dependency and exposure metadata;
- generated conformance fixtures.

Gates:

- representative real upstream Web IDL fixtures;
- deterministic snapshots;
- correct `Window`, worker, worklet, and secure-context exposure;
- overload and conversion conformance;
- unsupported constructs fail at their source location;
- generated raw Fe compiles through the generic host ABI.

### Phase 6 — raw DOM vertical and `std::web`

First useful interface set:

- `Window`, `Document`, `Node`, `Element`;
- `EventTarget`, `Event`, listener options, `AbortSignal`;
- console;
- timers and animation frames;
- minimal fetch primitives required by the loader/runtime.

Raw bindings remain literal. `std::web` supplies:

- typed, unforgeable handles;
- explicit feature detection;
- owned subscriptions with an explicit consuming `unsubscribe` operation;
- exceptions as typed errors;
- ordinary Fe effects/providers for host access and affinity.

Gate: a real inline Fe program changes the DOM, installs an event listener,
receives an event, and proves listener removal on consuming unsubscribe.
Automatic unregister-on-drop becomes an additional gate when Fe has destructor
or drop glue; JavaScript finalization may be a leak backstop, never correctness.

### Phase 7 — hardened inline Fe loader

Define exact semantics for inert Fe elements:

```html
<script type="application/fe" data-fe-entry="main">...</script>
<script type="application/fe" data-fe-src="./app.fe"></script>
```

Deliver:

- compiler worker pool and cache;
- diagnostics rendering hook;
- manifest/import preflight before instantiation;
- documented document-order and DOM-readiness rules;
- URL/base, CORS, credentials, CSP, and integrity behavior;
- error isolation, cancellation, and lifecycle events;
- main-thread and Worker execution placement where explicitly requested.

Gates:

- real inline and external sources;
- order, error, cancellation, and missing-import tests;
- hostile duplicate-provider rejection;
- CSP configurations documented and browser-tested;
- development reload preserves resource cleanup.

### Phase 8 — Trunk-style production precompiler

Extend `fe web build` around a standards-compliant HTML parser/serializer. Do
not rewrite HTML with regular expressions.

Input:

```html
<script type="application/fe" data-fe-src="./app.fe"></script>
```

Representative output:

```html
<script
  type="application/fe+wasm"
  data-fe-src="/assets/app.<hash>.wasm"
  data-fe-manifest="/assets/app.<hash>.json"
  data-fe-integrity="sha256-...">
</script>
```

Deliver:

- inline and `data-fe-src` extraction with correct base-URL resolution;
- compilation through the same facade used by the browser Worker;
- content-addressed Wasm, WGSL where requested, adapters, manifests, and maps;
- only-needed WebIDL interface selection;
- atomic publication;
- loader/bootstrap injection;
- reproducible build graph and dependency inventory.

Gates:

- unrelated HTML is preserved semantically;
- inline, external, `<base>`, malformed attributes, and duplicate cases;
- clean rebuilds are byte-identical where promised;
- deployed output contains no compiler Wasm;
- integrity hashes are verified;
- development and production entry/lifecycle behavior is identical.

### Phase 9 — development server integration

Reuse the existing safe serving, immutable snapshot, isolation-header, and
last-good-build concepts without retaining fixed WebBundle semantics.

Deliver:

- HTML/Fe/interface dependency graph;
- incremental affected-artifact rebuilding;
- structured build diagnostics;
- browser reload or hot replacement;
- correct COOP/COEP/CORP headers for worker/shared-memory configurations.

Gates:

- changing one Fe dependency rebuilds only affected artifacts;
- failed compilation retains the last good immutable site;
- diagnostics reach the browser;
- path traversal and partial-publication tests.

### Phase 10 — async, streams, and FRP

Layer in this order:

```text
raw generated binding
    -> safe std::web operation
    -> Future / Stream
    -> optional Event / Behavior / Signal
```

Deliver:

- first, callback/export-driven Promise and event delivery whose Fe handlers
  run to completion without blocking the browser main thread;
- cancellation subscriptions distinct from aborting an underlying operation;
- a generic, host-neutral resumable task/executor ABI before exposing an
  awaitable `Future` on the main thread;
- Promise-to-`Future` adaptation with Web cancellation semantics only after
  that resumable runtime exists;
- EventTarget-to-`Stream` with owned subscription;
- async iterables and stream backpressure policy;
- FRP combinators as an ordinary library consuming the same streams.

The existing `Pending<B, T>`/`Wait<B>` rail remains a blocking Worker
completion mechanism. It must not be renamed or presented as a main-thread
future: browsers forbid blocking the main event-loop agent, and Fe does not yet
have resumable async state machines. Generic suspension is language/runtime
infrastructure; Promise, EventTarget, and DOM names remain generated-library
concerns.

Gate: one underlying event subscription is exercised imperatively, as a
`Stream`, and through FRP, with identical cancellation and cleanup. No FRP
concept appears in compiler or raw binding generator.

### Phase 11 — native target implemented by Cranelift

Cranelift is the implementation, not the language-level target name.

Deliver:

- feature-gated `native-backend`;
- semantic `Native` backend kind;
- generalized portable scalar lowering shared by Wasm and native;
- honest x86-64/AArch64 triples and 64-bit pointer layout;
- typed safe execution harness around the backend JIT artifact;
- no Cranelift dependency in browser compiler builds.

Do not clone `wasm_lower.rs` and relabel its Wasm32 ISA. Extract its portable
operation lowering while retaining target-specific ISA, layout, ABI, and
artifact construction.

Gates:

- x86-64 and AArch64 where CI runners exist;
- differential scalar/control-flow suite against Wasm;
- memory/layout tests before claiming general native support;
- unsupported host architectures fail explicitly.

### Phase 12 — four-backend capstone

Canonical source:

`demos/capstones/mandelbrot/kernel.fe`

The kernel is a pure `(px: i32, py: i32) -> u32` fixed-point Q12 Mandelbrot
iteration using bounded signed arithmetic, comparison, arithmetic shift,
loops, branches, and early return. It avoids allocation, imports, callbacks,
`u64` GPU incompatibility, and floating-point differences.

Build one capstone ingot and page. Each leg consumes the exact canonical source
digest; target adapters contain no duplicated Mandelbrot mathematics.

| Leg | Runtime | Verification |
|---|---|---|
| EVM | revm | deterministic contract-call probe set equals independent oracle |
| Wasm | browser and wasmtime | all 512×512 pixels equal independent oracle |
| Native | Cranelift JIT | all 512×512 pixels equal oracle and Wasm |
| WebGPU | Chromium WebGPU | browser-profile WGSL; all pixels equal Wasm in verification mode |

Presentation mode submits and renders directly through WebGPU without a Wasm
readback oracle. Verification mode explicitly reads back and compares.

The page itself is authored with `<script type="application/fe">`, uses
generated WebIDL DOM/WebGPU bindings, compiles in a Worker during development,
and is rewritten to hashed production artifacts by `fe web build`.

Capstone gates:

- canonical source SHA-256 recorded for every artifact;
- no generated or handwritten duplicate kernel;
- real target runtime execution on every claimed leg;
- independent oracle agreement;
- browser-default WGSL reparsing/validation and no unsupported integer types;
- fail-closed negative capability test per backend;
- artifact manifest records compiler, source, interface snapshot, target,
  imports, exports, and hashes;
- Fe and web idiomaticity reviews both pass.

## 5. Definitions of done

### Development inline compilation

A real browser loads inert inline Fe source, sends its virtual URL and text to a
versioned Worker, compiles entirely client-side, preflights generated imports
from the manifest, instantiates the resulting Wasm, mutates the DOM, recompiles
an edit, and maps structured diagnostics back to the inline source. No server
compilation, precompiled program substitution, or compiler browser-name check
is present.

### Production rewriting

`fe web build index.html` standards-parses HTML, compiles every inline and
external Fe script through the same facade, emits content-addressed artifacts
and source maps, selects only required generated adapters, rewrites the
elements, and atomically publishes. The deployed site contains no compiler
artifact and preserves development entry, import, order, and lifecycle
semantics.

### Comprehensive web bindings

The pinned Web IDL snapshot links every supported definition with correct
exposure and conversion behavior; representative DOM, fetch/streams, workers,
storage, canvas, WebGPU, and audio interfaces generate deterministically.
Unsupported definitions are explicit, source-located failures. Safe Fe wrappers
own resource lifetime and typed errors without altering standard observable
behavior.

### Capstone

One authored Fe Q12 Mandelbrot kernel is executed through four honest target
adapters and produces independently verified equal results. The interactive
browser artifact is created by the same inline/precompile pipeline and uses
generated web-standard bindings. No part of the success claim depends on an
exact-name compiler seam.

## 6. Implementation discipline

Each phase lands as a vertical slice:

1. contract or normalized representation;
2. implementation;
3. positive execution test;
4. fail-closed negative test;
5. deterministic artifact/provenance test;
6. Fe idiomaticity review;
7. web idiomaticity review;
8. migration note and deleted/reduced debt.

Later phases may begin behind stable interfaces, but a phase is not called
complete until its execution and negative gates pass.
