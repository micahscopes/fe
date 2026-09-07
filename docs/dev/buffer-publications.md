# Typed runtime buffer publications

`PublishBuffer<P>` on a GPU stage selects a public inherent function on `P`
from the complete non-resource actor state to `BufferPublication<Resource>`.
The compiler resolves `Resource` against the actor's resource field types.
Missing/ambiguous targets and competing policies for one target are rejected.
Repeated use of the same policy for the same target is deduplicated.

The current destination must be actor-resident storage without an initial
artifact. Its compiler-derived physical usages include COPY_DST. The existing
WebIDL-generated GPUQueue.writeBuffer operation performs the transfer; no
application-authored binding number, byte manifest or JavaScript loader is
required. The result contains a version (epoch/revision), borrowed source bytes,
and a destination byte offset. Packed u32 lists have a checked, zero-copy Fe
constructor; arbitrary Wasm structs are not assumed to match GPU storage layout.

The compiler publishes dense fixed policy and binding exports. The render
runtime evaluates each policy with current state before pass execution and
synchronously consumes the returned range. Unchanged revisions are not uploaded
again. Physical buffer replacement invalidates the submission receipt. Range
bounds, alignment, stale revisions and ownership changes fail explicitly.
An empty source denotes no work; it does not replace an earlier submission
receipt. This permits a producer to report pending or caught-up state.

## Limits and ownership

- A transfer receipt means queued, not completed GPU execution. Asynchronous
  GPU validation and device loss remain part of surface recovery.
- A policy must keep its borrowed source alive and unchanged through transfer.
  Constructing BufferPublication does not confer ownership of its memory.
- Full recovery needs a replay of all required bytes, not merely the most
  recent appended range. Fe's consumer cursor must reset with physical storage.
- Same-typed resource fields are currently ambiguous. Do not invent numeric
  selectors in application JavaScript to bypass this check.
- Connecting long-lived worker-owned arenas still needs typed completion
  delivery and verification that surface initialization/transition reclamation
  cannot reset retained atlas storage. This change alone does not finish that
  integration.

## Evidence

`wasm_buffer_publication` executes generated policy exports and checks nominal
target resolution, ambiguity/missing-target rejection, packed-word identity,
bounded lengths and overflow. The fixed render-runtime suite checks submission
deduplication, memory growth, failed-write retry and physical replacement.

Set `FE_PUBLICATION_TEST_SITE` to a **new** output directory when running the
release `wasm_buffer_publication` test to publish its generated browser fixture.
The Quilting browser acceptance script seeds that fixture's Wasm source and
checks actual WebGPU pixel readback: source changes with the same revision do
not alter pixels; the next Fe-selected revision does. This is controlled-source
integration evidence, not yet a worker-atlas rendering or performance claim.
