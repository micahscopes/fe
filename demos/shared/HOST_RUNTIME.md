# Fe semantic host runtime

`host-runtime.js` is the JavaScript realization of
`fe:host-runtime/v1`. It is platform-neutral: generated adapters decide which
host objects and methods implement an interface.

The runtime owns three independent tables:

- resources: owned roots plus call-scoped borrowed handles;
- callbacks: signature-checked roots with deferred release during invocation;
- futures: exactly-once resolve, reject, or cancel state.

Handles are opaque objects. A stale, forged, double-consumed, or cross-domain
handle fails closed. `resources.withBorrowed(value, callback)` invalidates its
temporary handle when the callback returns or its promise settles, and borrowed
handles cannot be taken or dropped.

Explicit cleanup is currently required because Fe has no destructor/drop glue.
`inventory()` makes leaked roots observable to tests and development tooling.
JavaScript garbage collection is not part of correctness.

Generated Web IDL adapters accept the runtime as an ordinary dependency:

```js
const runtime = createFeHostRuntime();
const adapter = createFeHostAdapter(host, runtime);
const imports = adapter.imports;
```

This is a semantic-value boundary. Core-Wasm memory lifting/lowering and Fe
callback/future export trampolines are separate transport work; this runtime
does not claim that the scalar-only Wasm import ABI can carry rich values.
