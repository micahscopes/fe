# Multiple workers from one actor

`core::actor::ActorInstance<C, Tag>` gives a supervised actor a nominal instance
identity without duplicating its authored behaviors. For example:

```fe
struct Slot<const I: u32> {}

// Two types, two scopes, one authored AtlasWorker implementation:
// ActorInstance<AtlasWorker, Slot<0>>
// ActorInstance<AtlasWorker, Slot<1>>
```

Use either complete type as `C` in `BrowserActorMailbox<C>` and pass its empty
constructor to `supervise_browser_child`. Ordinary Fe generic functions can
share the request/supervision code. Reusing the same type within a parent scope
addresses the same instance; it does not allocate another worker.

The compiler keeps the complete tagged type when deriving lifecycle imports,
request lanes and package paths. Only behavior lookup unwraps the trusted core
constructor. A user-defined same-named struct cannot borrow another actor's
endpoints. `Handles` is forwarded from the underlying actor, and the compiler
still verifies each mailbox request against an actual Worker behavior rather
than trusting a manually asserted trait implementation.

This is static instance identity, not a pool scheduler. Queue policy, batch
epochs, cancellation, backpressure and worker-count policy belong in Fe library
or application code. The browser uses the existing structured-worker host;
there is no new JavaScript dispatch table or pool implementation. Worker
artifacts are currently compiled separately per instance. This does not claim
binary deduplication, dynamic spawning of arbitrarily many instances, or nested
`ActorInstance` support: the first parameter must be an authored actor.

Regression: `tagged_instances_share_behavior_but_not_worker_identity` in
`crates/codegen/tests/resident_actor.rs`. It checks scope/lane/package identity,
executes each emitted Wasm child, checks same-tag aliasing and rejects forged
types/endpoint claims.
