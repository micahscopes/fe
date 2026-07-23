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
but do not restate record fields or typed-array schemas. Actor protocol-v2
envelopes remain transport framing, separate from the canonical payload ABI.

The remaining boundary is deliberately narrower than the Component Model:
canonical version 1 supports fixed-layout records, scalar leaves, owned bytes,
and UTF-8 strings. General lists, variants, resources, futures, streams, and
shared-memory zero-copy remain later work.

## Protocol

Protocol name: `fe-canonical-browser-interface`

Version: `1`

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
- byte strings represented by `{ ptr: u32, len: u32 }`;
- UTF-8 strings represented by the same physical descriptor and distinct
  nominal interface metadata.

Lists, options, enums, resources, futures, and streams are not part of version
1.

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
semantic signature and runtime layout. Lane/export names are explicit inputs;
callers do not restate record schemas.

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
protocol-v2 envelopes remain the transport framing.

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
- A MessageChannel integration test builds a lane solely from the generated
  manifest and proves that the standalone response buffer transfers and detaches
  in the sender while Wasm memory is never put in a transfer list.

## Explicitly later

- general allocation/free and concurrent outstanding borrows;
- affine/linear ownership and resource destructors;
- shared-memory zero-copy;
- options, variants, general lists, and recursive values;
- async, futures, streams, and cancellation;
- generated supervision, placement, and backpressure policy;
- WebGPU resource ownership;
- Component Model binary ABI compatibility.

Milestone 1 must leave extension points for these features without claiming to
implement them.
