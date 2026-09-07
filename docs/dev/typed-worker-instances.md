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

## Const-indexed task families

Use `ScopedTaskFamily<N>` when one behavior must run for each member of a finite
type-indexed family. The behavior declares exactly one `const I: u32` parameter:

```fe
fn supervision<const I: u32>() -> u32 uses (ScopedTaskFamily<WORKERS>) {
    supervise(ActorInstance<AtlasWorker, Slot<I>> {})
}
fn work<const I: u32>(self) -> u32 uses (ScopedTaskFamily<WORKERS>) {
    run<ActorInstance<AtlasWorker, Slot<I>>>(self.address, I)
}
```

Here `supervise` and `run` are ordinary shared Fe functions; only their generic
call sites are shown. `WORKERS` is a const u32. The compiler evaluates the count,
specializes the authored behavior over `0..WORKERS`, and uses the existing
continuation and structured-child machinery. It emits neither source wrappers
nor an application-authored task table. Generic parameters are real semantic
substitutions, so every slot's mailbox remains type-branded. Ordinary
`ScopedTask` behavior is unchanged. Families on structured children retain the
existing self-less-task requirement.

The task registry exposes each generated machine's `inputWidth`; generic hosts
can start self-less tasks with no input and state-capturing tasks with the actor
state, without reconstructing a list of numbered source method names.
