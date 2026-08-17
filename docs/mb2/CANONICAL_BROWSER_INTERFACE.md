# Fe canonical browser interface: milestone 1

Status: milestone 1 implemented. The legacy post-Wasm `actor_manifest`
derivation described by the original plan has been removed.

This milestone supplies the first compiler-owned ABI between Fe Wasm actors and
JavaScript Workers. It is intentionally smaller than the Component Model, but
it establishes the type, layout, ownership, and codec seams that later resource,
async, and capability work can extend.

## Current boundary

Canonical lane manifests now derive nominal request and response records from
Fe semantic signatures, cross-check the emitted Wasm arena ABI, and generate
the JavaScript codecs and actor/host-effect adapters. Callers select lane names
but do not restate record fields or typed-array schemas. Actor protocol-v3
envelopes remain transport framing, separate from the canonical payload ABI.
Protocol v3 adds correlated `cancel` messages and propagates an `AbortSignal`
into Worker and WebGPU host dispatch.

The remaining boundary is deliberately narrower than the Component Model.
Versions 1–2 established fixed-layout records, scalar leaves, owned bytes, and
UTF-8 strings. Version 3 added the bounded host-effect variant family below.
Version 4 adds the exact nominal bounded-list transport described below.
Recursively scalar record variants now also cross Wasm lanes. Variants
containing bytes, strings, or lists remain schema values only until the
post-return memory bridge can preserve their ownership. Unbounded and nested
lists, resources, futures, streams, and shared-memory zero-copy remain later
work.

## Protocol

Protocol name: `fe-canonical-browser-interface`

## Version 4: bounded typed-list transport

The exact nominal Fe descriptor `BrowserList<T, MAX>` is admitted when `T` is
`u32` or `f32` and `MAX` is a concrete `usize` constant whose four-byte payload
fits in wasm32. Its wire layout is `{ ptr: u32, len: u32 }`, size 8, alignment
4; `len` is an element count. JavaScript values are respectively `Uint32Array`
and `Float32Array`.

Codecs enforce the element-specific typed-array class, `len <= MAX`, four-byte
alignment for non-empty descriptors, and checked Wasm bounds. Encoding and
decoding copy the payload. Actor transfer recursively finds active list values
inside records and variants, transfers only owned full-span buffers, and
deduplicates shared buffers. Empty encodes use `{ ptr: 0, len: 0 }`; decoders
and Wasm response copying ignore the pointer when `len == 0`. Wasm wrappers
validate Fe-produced descriptors and copy exactly `len * 4` bytes into an
aligned canonical-arena result before publishing it.

This remains a bounded borrowed view, not a general Fe collection API.
`BrowserList<T, MAX>` preserves its address as `MemPtr<T>` in Fe, so a lane can
read the pointed-to element with ordinary provider semantics; the canonical
wasm32 boundary carries that memory `RawAddr` as one physical `i32` word.
Read-only aggregate parameters are reified only when every use is a static
field path ending in one scalar/address leaf. Whole-aggregate reads,
multi-leaf projections, dynamic paths, stores, and address-taking fail closed.
There is not yet typed pointer arithmetic/indexing or a typed list allocator,
so v4 does not claim general iteration or Fe-minted lists.

## Version 3: bounded tagged variants

Version 3 adds compiler-derived enum metadata for actor and host-effect
messages. It intentionally does not claim general serde or a Wasm component
model.

Only unit variants and record variants with named fields are admitted. Tuple
variants fail closed so the wire API never invents positional JavaScript field
names. Variant names are deterministically converted from Fe `UpperCamelCase`
to lowercase snake case. The JavaScript and generated TypeScript value is a
discriminated object:

```ts
{ readonly tag: "empty" }
| { readonly tag: "data"; code: number; payload: Uint8Array }
```

The wasm32 wire envelope is pinned:

- a little-endian `u32` tag at offset 0;
- tags are dense declaration-order indices starting at zero;
- each case payload starts after the tag and follows the existing canonical
  field alignment rules;
- all case payloads overlay one union region;
- union alignment is `max(4, field alignments)` and size is the aligned maximum
  case end;
- encoders zero the complete inactive payload region;
- unknown tags, non-dense manifests, bad offsets, invalid nested descriptors,
  unexpected fields, and non-owned byte transfers fail closed.

Bytes nested in the active case retain the v2 ownership rule: codecs copy them,
and actor transfer is zero-copy only for an owned full-span `Uint8Array`.
Strings remain copied UTF-8 values. The one-call arena still resets in
`finally`; every decoded descriptor is copied before reset.

Fe payload enums and canonical memory deliberately have different physical
forms. Canonical memory is the tagged union above. The Fe Wasm value ABI is a
tag followed by every variant payload lane in declaration order. The generated
wrapper is the only bridge between them:

- request lowering validates the memory tag, loads only the active union
  member, and supplies canonical zero values for every inactive Fe payload
  lane;
- response lowering validates the Fe tag, zeros the complete reusable response
  record, and stores only the active payload into the union region;
- the compiler-derived parent mailbox codec uses the same value-lane order and
  rejects nonzero inactive lanes rather than letting JavaScript infer enum
  meaning; and
- nested scalar variants and records recurse through the same plan.

This path admits only recursively scalar trees. A lane that combines any
variant with bytes, strings, or lists fails closed with an explicit
post-return-bridge diagnostic. Host-effect schemas may still use those rich
variants because their values do not cross a Wasm function boundary.

Version: `4`

Each canonical lane has one uniform exported signature:

```text
(request_ptr: i32) -> response_ptr: i32
```

The module also exports:

```text
memory
fe_cabi_alloc(size: i32, align: i32) -> i32
fe_cabi_reset() -> ()
```

The ABI is wasm32, little-endian. Request and response records use
compiler-emitted sizes, alignments, and field offsets.

Milestone 1 supports:

- booleans and fixed-width integer/f32 scalars;
- nested fixed-layout records;
- unit and named-record variants whose complete Wasm value tree is scalar;
- byte strings represented by `{ ptr: u32, len: u32 }`;
- UTF-8 strings represented by the same physical descriptor and distinct
  nominal interface metadata;
- bounded `BrowserList<u32, MAX>` and `BrowserList<f32, MAX>` descriptors.

Unbounded or nested lists, descriptor-bearing Wasm variants, resources,
futures, and streams are not part of version 4.

## Actor selection and generated identities

The actor interface has separate semantic and ABI naming layers. Keeping them
separate prevents a browser adapter from becoming an application router.

1. An authored behavior has an ordinary Fe source name, such as `scale` or
   `receive`.
2. Nominal values in its `uses` row select execution roles. For example,
   `Worker`, `ResidentTransition`, `InitialState`, `ProjectState`, and
   `ScopedTask` are compiler-recognized through their type metadata, not their
   source spelling.
3. Actor lowering derives `C: Handles<M, R>` for a self-less behavior on child
   `C` taking request `M` and returning response `R`. `ActorMailbox<B, C>` can
   therefore select the edge from the nominal child and message types without
   a numeric operation ID or behavior-name table.
4. A structured-child transport name such as `request_<hash>` is a private,
   compiler-generated identity over `(C, M, R)`. Its child export is
   `fe_cabi_request_<hash>`. Child lifecycle imports similarly use
   `spawn_<hash>`, `failure_<hash>`, and `close_<hash>` derived from `C`.
5. Resident roles are published behind fixed protocol exports such as
   `fe_actor_initialize_v1`, `fe_actor_transition_v1`, and
   `fe_actor_project_v1`. Their authored behavior names do not reach the host.
6. Direct public canonical lanes currently retain the authored Fe behavior
   name as their friendly lane label and use `fe_cabi_<name>` for a Wasm
   implementation. Selection into that set still comes from nominal placement,
   execution, or capability markers rather than a name scan.

The type hash is a rebuild-together package identity, not a durable public ABI.
Renaming a nominal type may change it. The compiler generates and collision
checks the parent import, child lane, publication path, and fixed adapter in one
closed package, so application JavaScript never interprets the hash.

Generated module functions such as `createInterfaceCaller`,
`compileActorMailbox`, and `createActorAdapter` are fixed transport mechanics.
They consume compiler-derived lanes and layouts; they are not application
operations and accept no caller-authored routing table.

Two render compatibility paths remain spelling-sensitive and should not be
mistaken for the general actor model. The projected const behavior is currently
named `view`, and legacy `UpdateSurface` uses the reserved `dx`, `dy`, `dzoom`,
`mx`, and `my` argument vocabulary. Typed `SurfaceTransition` already replaces
the latter with one nominal event record. A future projection role should
remove the remaining `view` name convention.

Singleton role-selected behaviors and typed mailbox endpoints do not
semantically require a public operation name. A future authoring surface may
therefore permit an omitted behavior name while retaining a compiler-owned
source identity for diagnostics, stack traces, and explicit intra-Fe calls.
That refinement does not require changing the type-directed transport model.

## Arena and lifetime

Calls are serialized through an actor mailbox. Before invoking a lane, the
generated JavaScript adapter allocates the request record and payload in a
per-instance bump arena. The lane allocates its response in the same arena.

The adapter decodes the response and copies owned byte/string results into
standalone JavaScript storage. It invokes `fe_cabi_reset` in `finally`, whether
the lane or decoding succeeds or fails.

Every pointer becomes invalid at reset. The arena:

- resets to a pinned heap base after each call;
- grows linear memory by checked whole pages;
- uses checked 32-bit size/alignment arithmetic;
- permits only one active canonical call per instance;
- does not claim general `free`, concurrent borrows, or linear ownership.

## Transfer rule

A live `WebAssembly.Memory` view is never a transferable actor payload. Its
buffer is engine-owned and remains the address space of the running instance;
it cannot be treated as a detached, uniquely owned result buffer.

The Worker must:

1. decode/copy response bytes out of Wasm memory;
2. reset the arena;
3. transfer the new full-span standalone typed-array buffer with
   `transferOwnedTypedArray`.

This introduces one necessary Wasm-to-JavaScript copy while eliminating the
subsequent Worker-to-main structured clone.

## Compiler-owned manifest

Add `crates/codegen/src/canonical_interface.rs` with typed representations for:

```text
CanonicalType
CanonicalField
CanonicalLayout
CanonicalLane
CanonicalInterfaceManifest
```

The compiler derives field names, types, and layouts from each selected entry's
semantic signature and runtime layout. Direct lanes use their selected source
entry as a friendly label. Structured-child lane and export identities derive
from the nominal child/request/response edge. Callers do not restate record
schemas or child routes.

Bundle manifests embed:

```text
interface {
  protocol,
  version,
  memory,
  arena,
  lanes,
  types
}
```

The generated interface module derives actor request/result validators and
Wasm/host-effect adapters directly from this interface. Existing actor
protocol-v3 envelopes remain the transport framing.

## Implementation surfaces

1. `crates/codegen/src/canonical_interface.rs`
   - semantic type and deterministic layout derivation;
   - name/collision validation;
   - emitted-Wasm signature cross-checks;
   - manifest serialization.
2. `crates/codegen/src/sonatina/wasm_lower.rs`
   - raw-address arithmetic and aligned scalar load/store;
   - memory copy/fill and arena allocation;
   - fail-closed unsupported classes, alignments, and overflow.
3. Sonatina Wasm emission
   - one exported memory;
   - allocator/reset globals and exports.
4. `ingots/std/src/wasm/cabi.fe`
   - nominal `BrowserBytes` and `BrowserString` descriptors;
   - bounded byte/length primitives;
   - no reuse of Solidity/EVM `Bytes` or `DynString`.
5. `crates/codegen/assets/canonical-interface.js`
   - strict manifest validation;
   - `DataView` record codec;
   - fatal UTF-8 decode and validated encode;
   - arena invocation with reset-in-finally.
6. First migrations
   - Mandelbrot control uses its generated canonical interface;
   - QCGA uses generated multi-lane actor, Wasm caller, and host-effect
     adapters;
   - the Schedule32 CGA showcase should use the canonical path from inception.

## Acceptance gates

- A Fe fixture accepts a record containing a tag, `BrowserString`, and
  `BrowserBytes`, and returns a typed response through the uniform entry.
- Wasmtime verifies offsets, bytes, UTF-8, reset/reused base, checked malformed
  lengths, and 10,000 repeated calls without unbounded memory growth.
- `wasmparser` confirms the exported memory, allocator/reset, and uniform lane
  signatures.
- Layout tests cover deterministic nested records, invalid semantic types,
  collisions, manifest round-trips, and emitted-signature mismatch.
- JavaScript tests cover Unicode/non-BMP text, invalid UTF-8, exact fields,
  reset on success/error, and output storage that does not alias Wasm memory.
- A real parent actor sends a nested scalar request variant through its
  type-derived mailbox to a separately compiled child, receives a scalar result
  variant, resumes its Fe continuation, and checks semantic state plus inactive
  response-union scrubbing after arena reuse.
- A MessageChannel integration test builds a lane solely from the generated
  manifest and proves that the standalone response buffer transfers and detaches
  in the sender while Wasm memory is never put in a transfer list.

## Explicitly later

- general allocation/free and concurrent outstanding borrows;
- affine/linear ownership and resource destructors;
- shared-memory zero-copy;
- descriptor-bearing variants, general or nested lists, and recursive values;
- async, futures, streams, and cancellation;
- generated supervision, placement, and backpressure policy;
- WebGPU resource ownership;
- Component Model binary ABI compatibility.

Milestone 1 must leave extension points for these features without claiming to
implement them.
