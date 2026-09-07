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
- Canonical checkpoint/rewind bundles retain successful initializer allocations
  for the Wasm instance lifetime. Synchronous surface calls reclaim only their
  own scratch above the current cursor, preserving retained and suspended-task
  storage. Such calls cannot publish fresh persistent allocations; reserve the
  owned storage during initialization. Older scalar-only bundles retain their
  reset-based ABI.

## Task completion and presentation

A render actor can declare one `ResidentTransition` behavior accepting a named
scalar message record and returning its complete non-resource state. Its tasks
send that nominal record through `ActorMessage`; same-width records with a
different Fe identity are rejected by the same compiler check used for ordinary
resident actors. The generated fixed message export shares resident state with
the scheduled surface transition and GPU readback, if present. Non-resident
surface controls cannot be combined with this path.

For tasks that need initial state but not GPU handles, use an explicit complete
non-resource state record argument rather than `self`. The canonical task
adapter derives its layout. Taking resource-bearing `self` remains rejected;
this is not GPU resource custody for Wasm tasks. Inputs are launch-time
snapshots, not automatic subscriptions to subsequent resident state.

The fixed host transports the message's scalar lanes, invokes the Fe transition,
and mirrors its result for rendering. An aborted task cannot deliver. Every
message is applied; only the subsequent `StateChanged` presentation facts may
coalesce. The ordinary Fe scheduling policy handles visibility and backpressure.
`StateChanged` carries no message payload and must not reapply the transition.
This mechanism does not by itself implement the atlas viewer or acknowledge GPU
uploads back into Fe state.

## Evidence

`authored_raster_e2e::raster_tasks_deliver_nominal_messages_into_resident_render_state`
executes two Fe task-family instances, their messages, a scheduled surface
transition, and explicit state replacement through the shared runtime methods.
The actor owns a GPU buffer, but tasks receive only its ordinary state record.
The observed state sequence is 10 -> 13 -> 17 -> 19 -> 20 -> 22. Wrong nominal
messages, a missing receiver, resource-bearing self, and cancelled delivery are
rejected. This is Wasm/host execution evidence, not GPU pixel acceptance.

The byte-transport adapter supports both canonical `cabi_realloc` task modules
and arena `fe_cabi_alloc` modules, with compatible checkpoint/rewind or legacy
reset reclamation. The surface-event regression covers the appended StateChanged
tag and backpressure behavior; undeclared tags still trap.

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
