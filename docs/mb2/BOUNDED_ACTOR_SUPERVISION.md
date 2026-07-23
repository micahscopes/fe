# Bounded browser actor supervision

`createModuleWorkerActor` and `createCanonicalModuleWorkerActor` have an
explicit, opt-in supervision boundary. It is deliberately a lifecycle policy
for one generated Module Worker, not a persistence layer, a supervision tree,
or a multi-worker scheduler.

With no `supervision` option, startup errors retain their original stable error
codes and a runtime crash is terminal. An application opts into deterministic
restart by supplying all three bounds:

```js
const oracle = await createCanonicalModuleWorkerActor({
  workerUrl,
  adapter,
  supervision: {
    maxRestarts: 3,
    windowMs: 10_000,
    backoffMs: 25,
    observe(event) {
      actorTelemetry.record(event);
    },
  },
});
```

`maxRestarts` counts replacement constructions in the rolling `windowMs`
window. `backoffMs` is a fixed bounded delay, so the runtime owns at most one
restart timer and never grows an implicit retry queue. Zero restarts is a
useful explicit fail-fast policy. The observer receives frozen, sanitized
events (`ready`, `failure`, `backoff`, `restart`, `terminal`, and `close`);
exceptions in telemetry cannot alter lifecycle behavior. `status()` exposes
only the bounded state, epoch, current-window count, and stable terminal code.

A runtime error immediately closes the retiring endpoint, fails its in-flight
requests with `FE_ACTOR_WORKER_RUNTIME`, removes its listener, closes auxiliary
capabilities, and terminates it. Requests made afterward wait behind the one
serialized lifecycle transition. Their canonical envelopes are created only
after the replacement is ready, so they carry the recovery epoch and cannot
cross the retiring port. Cancellation remains prompt during backoff and closing
the actor cancels the sole timer. Startup failures during recovery are
classified separately and consume the same budget. Exhaustion is sticky and
propagates `FE_ACTOR_WORKER_TERMINAL`; callers must construct a new actor to
apply a new policy.

Tests use injected clock hooks only to make time deterministic. Production
defaults are `Date.now`, `setTimeout`, and `clearTimeout`; these are browser
capabilities supplied explicitly by the JavaScript boundary rather than
claimed as Fe or Worker effects. The policy does not broaden generated host
capabilities, transfer ownership rules, protocol-v3 cancellation, mailbox
bounds, or GPU placement.

`module-worker-supervision.test.mjs` proves:

- adversarial runtime/startup crash loops stop at the configured budget;
- cancel and close during backoff do not construct or contact a replacement;
- no post-crash request reaches a retiring Worker;
- a crash racing an explicit restart constructs exactly one replacement;
- an automatically restarted Worker that crashes at the ready-observation
  boundary consumes another bounded attempt instead of being mistaken for a
  queued manual restart;
- stale old-epoch results cannot satisfy recovery work;
- successful recovery sends work only after readiness with the next epoch;
- terminal state leaves no timer and returns a stable sanitized error.
