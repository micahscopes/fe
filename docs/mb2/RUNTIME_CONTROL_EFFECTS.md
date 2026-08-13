# Runtime control effects: one spine

This is the consolidation architecture for Fe browser, Worker, and GPU
asynchrony. It is subordinate to `FE_NATIVE_GALLERY_PLAN.md`; the goal and its
independent semantic gates remain authoritative.

## Decision

Fe has one runtime-control model:

1. Fe traits in `core`/`std` name authority (`Timer`, `Recv`, `Spawn`, GPU
   dispatch, cancellation, placement, and later supervision/resource scopes).
2. A function's `uses` row is the complete, checked set of authorities it may
   exercise. Missing handlers are ordinary type errors.
3. Begin-shaped operations mint affine `Pending<B, T>` values.
4. The consumer chooses a legal interpretation:
   - `Wait<B>` blocks only on placements that may block;
   - the resumable interpretation suspends the current Fe continuation and
     later receives one typed success, failure, or cancellation outcome.
5. The compiler derives body identity, value lanes, placement, continuation
   state, and fixed exports from Fe semantic/MIR types. None of those facts is
   supplied by JSON or duplicated as handwritten numeric/string tables.
6. The fixed host realizes standards objects, clocks, queues, promises,
   Workers, and WebGPU callbacks. It cannot reconstruct a stream graph, select
   retry policy, choose backpressure, or interpret demo-specific behavior.

`std::reactive` is a pure/combinator vocabulary interpreted through these
effects. It is not another runtime.

## The convergence point

```text
browser event / timer / Worker message / WebGPU completion
                          |
                 fixed standards adapter
                          |
             generation-tagged runtime delivery
                          |
          typed success | failure | cancellation
                          |
        compiler-generated Fe continuation re-entry
                          |
      resident actor / reactive interpreter / task scope
```

The existing resident actor transition is the first callback-style instance of
this shape: the host supplies a typed fact and Fe resumes its resident state
machine. The MIR suspension transform generalizes that boundary so a direct Fe
effect operation can pause and re-enter without hand-writing the state machine.
The resident path remains a deterministic interpreter and test oracle; it is
not thrown away when direct suspension lands.

## Ownership and race rules

- `Pending<B, T>`, subscriptions, continuations, cancellation rights, and owned
  resources are affine.
- Every host-visible token is slot-and-generation checked. A numeric slot alone
  is never authority.
- Browser adapters retain opaque JavaScript authorities and explicitly project
  them through `toCore` only at a core-Wasm `i32` boundary. The projected token
  carries slot and generation and resolves through the same table, so callback
  borrows cannot escape and stale numeric re-entry is rejected.
- Exactly one terminal outcome wins. Cancellation replaces an undelivered
  value/error, and late/stale completion is rejected.
- Parent scope cancellation deterministically cancels children and releases
  resources. Detached work requires an explicit capability.
- Placement is nominal Fe evidence (`MainThread`, `Worker`, and GPU roles), not
  a host guess or a string in an application manifest.
- Fairness and backpressure are bounded Fe policies. The host owns finite queue
  mechanics but not policy selection.

## Compiler materialization

The resumable MIR slice must derive one internal task description from the
authored Fe body and its effects. At each suspending operation it records:

- a compiler-owned continuation state;
- live affine values and their ownership state;
- the expected typed delivery;
- executor placement and scope ownership; and
- the next MIR block.

Generated start/resume/poll/suspend/complete exports are backend implementation
details. Authors and host tooling do not name seven sibling functions. Scalar
versus indirect value transport comes from the canonical type layout. The host
executor accepts a typed materializer-owned body key and has no string parser
or entry-name list.

## What was removed in the first consolidation cut

- `fe:resumable-task/v1`, an unused serializable schema which required callers
  to repeat the authored body, seven synthetic entry identities, three lane
  descriptions, state ownership, and placement.
- The callback-registration JSON schema, including caller-authored flattened
  scalar lanes and fixed lifetime policy fields. Callback lanes now come from
  the normalized interface signature and are checked against the Fe body's MIR
  signature.
- A second callback token arena in codegen. Runtime token ownership,
  generations, stale rejection, reentrant invocation, and deferred release
  remain centralized in `fe-host-runtime`.
- Stringly resumable task descriptors. `ResumableExecutor<K, V, E>` now accepts
  a materializer-owned typed `K` and retains the same exact-once, cancellation,
  FIFO, routing, and stale-token gates.
- The implicit object-to-`i32` callback handoff. The fixed browser runtime now
  exposes an explicit generation-checked core projection; the Bun capstone
  executes generated WebIDL conversion, Fe Wasm callback dispatch, a borrowed
  Event import, return conversion, release, and stale-borrow rejection.

These removals do not claim MIR suspension or browser effect adapters are
complete. They remove the architecture that would have made those features
manifest-driven.

## Ordered implementation slices

1. [done] Add the typed terminal outcome and compiler-recognized suspend
   operation. `Suspend<B, E>` and its downstream provider are ordinary Fe
   effects; nominal recognition and exact direct-site CFG liveness are pinned
   by independent tests. Cancellation is delivered to the continuation before
   terminal notification.
2. [in progress] Split MIR at suspension points and persist/reconstruct live
   state. Exact live sets, stable continuation-state assignment, typed frame
   layouts, and fixed-point propagation through ordinary helpers/effect
   providers are landed. Direct suspensions now split into verified executable
   segments: a compiler-created `Complete | SuspendedN` payload enum carries
   each site's pending token and exact live frame, and typed re-entry parameters
   reconstruct those locals. Non-recursive helper/provider calls are now
   expanded as complete target-neutral CFGs before liveness, independent of
   inline hints; both a real selected `Resumable` provider stack and a branched
   provider body materialize successfully. Recursive resumable SCCs remain an
   explicit linked-frame boundary, and host-owned frame persistence remains.
3. [in progress] Materialize fixed Wasm re-entry exports and connect them to the
   existing generation-safe executor. Direct tasks now emit compiler-named
   `__fe_task_start_*` and `__fe_task_resume_*_N` exports with no `fe:control`
   import. Wasmtime gates execute success, failure, cancellation, invalid-tag
   trapping, a two-site suspension chain, and a transitive Fe
   helper/effect-provider stack whose dead caller value is absent. Private
   helper continuations stay private; only the authored public task's generated
   entry points are exported. Recursive stacks fail explicitly instead of
   unrolling or degrading to an import. Executor frame/result wiring remains.
4. [todo] Interpret timer/receive/spawn and callback completion through that path on
   MainThread and Worker placements; retain blocking `Wait` only where legal.
5. [todo] Derive browser `EventSource` handlers and move the gallery activation/timer
   loop onto the same resident/reactive interpreter.
6. [todo] Move Worker admission, cancellation, restart/backoff, and supervision policy
   from handwritten JavaScript into Fe handlers and structured scopes.
7. [todo] Route WebGPU completion, device loss/recovery, and resource lifetime through
   the same outcome/scope machinery.
8. [todo] Expose typed device/viewport capability facts so Fe owns responsive render
   quality; delete the fixed host's temporary coarse-pointer/CPU safety cap.
9. [todo] Delete remaining semantic render-manifest fetch/interpretation after typed
   exports carry the complete contract.

Each slice needs an independent semantic oracle. Generated-byte equality is
never sufficient evidence of continuation, cancellation, or race correctness.
