# Inline Fe script spike

The sandbox now contains an executable loader convention for inert HTML:

```html
<script type="application/fe" data-fe-entry="main">
pub fn main() -> u32 {
    42
}
</script>
<script type="module">
import { createFeScriptLoader } from "./fe-script-loader.js";
import { compiler } from "./fe-compiler.js";
import { createWebImports } from "./generated/web-adapter.js";

await createFeScriptLoader({
  compiler,
  importProviders: [createWebImports()],
}).boot();
</script>
```

The boundary is intentionally small. `compiler.compile(request)` asynchronously
returns Wasm (or a `WebAssembly.Module`), an optional entry name, and optional
additional imports. Web bindings use the native Wasm import-object shape, so a
Web IDL generated adapter plugs in without compiler knowledge of the DOM.
`compiler-adapter.example.js` shows the worker transport expected by a future
browser build of the compiler. That build now exists as
`fe-browser-compiler`; `fe-compiler.worker.js` connects its wasm-bindgen exports
to the same adapter.

Inline scripts and external source referenced by `data-fe-src="/app.fe"` are
supported. The custom data attribute is deliberate: an unknown script MIME
type is an HTML data block, for which the standard `src` attribute does not
apply. Scripts execute in document order, expose `data-fe-state`, and dispatch
`fe:load` or `fe:error`. `boot()` on a still-loading `Document` waits for
`DOMContentLoaded` before taking its document-order snapshot. Concurrent
`run()` calls for one element share the same lifecycle promise.

External URLs resolve against the configured loader base or the element's
`baseURI`. Fetches use CORS mode and map `crossorigin="use-credentials"` to
included credentials, another present `crossorigin` value to omitted
credentials, and an absent attribute to same-origin credentials.
`referrerpolicy` is forwarded to Fetch. Source `integrity` and artifact
`data-fe-integrity` values are forwarded as Fetch SRI metadata; precompiled
Wasm is additionally checked with Web Crypto against the manifest's canonical
lowercase SHA-256, and `data-fe-integrity` must name that same digest.
`data-fe-manifest-integrity` optionally protects the manifest fetch itself.

Execution defaults explicitly to the main realm. A block marked
`data-fe-execution="worker"` is delegated only to an injected
`workerExecutor.run(request)`; it fails closed when no executor exists and
never silently instantiates on the main thread. The executor owns Worker
creation, import transport, and termination because JavaScript functions in a
Wasm import object are not generally structured-cloneable.

`run(element, { signal })` and `boot(root, { signal })` accept an
`AbortSignal`. Cancellation is forwarded to source/artifact fetches and the
compiler request, checked again between asynchronous phases, sets
`data-fe-state="cancelled"`, and dispatches `fe:cancel`. Compilation inside the
current compiler Wasm is synchronous once entered, so cancellation can suppress
publication and execution but cannot yet preempt that CPU work.

## Optional compiler pool

`compiler-pool.js` exports `FeCompilerPool`, which implements the same
`compiler.compile(request)` shape accepted by `createFeScriptLoader`:

```js
const compiler = new FeCompilerPool({
  size: 2,
  capacity: 32,
  workerFactory: () => new Worker("./fe-compiler.worker.js", { type: "module" }),
});
await createFeScriptLoader({ compiler, importProviders }).boot();
```

The pool hashes canonical protocol inputs (source URL/content, target, entries,
and options), coalesces identical in-flight work, and retains a bounded LRU of
successful results. Cancellation belongs to each caller: cancelling one
subscriber does not cancel another subscriber's shared compilation. A Worker
`error` or `messageerror` rejects that job and causes the slot to create a
replacement Worker for its next job. This layer schedules protocol requests
only; it has no DOM, Web IDL, or compiler-semantic policy.

## Browser-enforced policy gates

The loader does not bypass browser security policy. Cross-origin responses must
satisfy CORS, credentials still follow cookie and SameSite rules, SRI support is
enforced by the browser's Fetch implementation, and `connect-src`,
`worker-src`, `script-src`, and `wasm-unsafe-eval`/equivalent CSP gates may
reject fetching, Worker creation, loader execution, or Wasm compilation. These
checks require a real browser and response headers; the runtime-neutral unit
tests verify request construction, digest agreement, ordering, and placement
dispatch but do not claim CSP coverage.

## Browser compiler boundary

The browser build uses `fe-browser-compiler`, not the native `fe` CLI. Its
source-in/diagnostics-and-Wasm-out facade is backed by an in-memory compiler
database and is built for `wasm32-unknown-unknown`; CLI, filesystem, Git
resolution, reporting, and server entry points are outside that boundary.
The Worker adapter speaks the versioned protocol used by this loader.

The remaining production concern is payload size and startup cost, not whether
the path is real. Further splitting should remove unused EVM/SPIR-V and tooling
dependencies without changing the loader or generated Web IDL import surface.

## Production precompilation

The generic native entrypoint consumes HTML rather than a shader-specific
compiler mode:

```sh
fe web precompile site/index.html --out dist
```

It parses HTML5, resolves `data-fe-src` against the document and its first
`<base href>`, invokes the same compiler facade as the Worker, and rewrites Fe
data blocks to digest-verified `application/fe+wasm` manifests. Publication is
staged and renamed as one new directory; an existing destination is never
merged or overwritten. The older `fe web build` command remains only for the
existing Wasm+WebGPU demo-bundle format and is not the generic Web interface.

For development, `fe web dev site/index.html` serves immutable last-good
snapshots on loopback, rebuilds from compiler-reported source dependencies, and
exposes structured JSON events at `/.fe/events`. It does not inject a reload
client into application HTML; a dev client may consume that SSE endpoint and
choose whether to reload or render diagnostics.

Browser threads, shared memory, and other APIs guarded by
`crossOriginIsolated` can be enabled explicitly:

```sh
fe web dev site/index.html --isolation
```

Isolation mode adds `Cross-Origin-Opener-Policy: same-origin` and
`Cross-Origin-Embedder-Policy: require-corp`, and marks every locally served
HTML, generated asset, static asset, error, and SSE response with
`Cross-Origin-Resource-Policy: same-origin`. It is off by default because
`require-corp` blocks cross-origin scripts, images, fonts, workers, and other
subresources unless those servers opt in with an appropriate CORS response or
`Cross-Origin-Resource-Policy` header. Proxy or self-host such assets when the
remote origin cannot provide those headers.

Run the loader contract tests with the JavaScript runtime used by the browser
demos in this checkout:

```sh
bun test demos/fe-sandbox/fe-script-loader.test.mjs
```

Build and execute the real browser-hosted compiler smoke test with:

```sh
demos/fe-sandbox/build-browser-compiler.sh
```

The verifier loads the compiler Wasm in JavaScript, compiles virtual Fe source
entirely inside that Wasm module, instantiates the produced program Wasm, and
asserts that `main()` returns `42`. The current optimized compiler artifact is
still large (17,160,465 bytes after `wasm-opt`), and the smoke test does not
substitute a precompiled program for the compiler result.
