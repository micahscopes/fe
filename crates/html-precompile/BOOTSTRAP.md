# Precompiled browser bootstrap

`precompile_html` publishes a small content-addressed JavaScript module and
injects exactly one external `<script type="module" data-fe-bootstrap>` when a
document contains precompiled Fe artifacts. There is no inline JavaScript and
no browser compiler dependency. The module verifies each Wasm artifact against
its manifest, preflights native `WebAssembly.Module.imports`, instantiates in
document order, and invokes the declared entry unless
`data-fe-autostart="false"`.

Application-specific Web IDL adapters register ordinary Wasm import objects or
async provider functions before bootstrap execution:

```js
globalThis.feImportProviders ??= [];
globalThis.feImportProviders.push(async ({ element, manifest, module }) => ({
  "fe:web": {
    console_log(value) {
      console.log(value);
    },
  },
}));
```

The bootstrap also exposes `globalThis.registerFeImportProvider(provider)`.
Providers remain host code: the compiler and precompiler know nothing about the
DOM or a particular generated adapter. Duplicate or unresolved imports fail
before instantiation and dispatch `fe:error`.

Tooling with generated Web IDL adapter metadata can call
`precompile_html_with_adapter_metadata`. It matches the compiled module's exact
function import inventory against generated operation metadata, fails on a
missing or ambiguous provider, and publishes a content-addressed
`fe-adapter-selection-*.json` referenced by `data-fe-adapter-selection`.
Selection manifest v1 contains the deterministic transitive operation,
resource, named-type, and exposure closure.

`precompile_html_with_adapter_plan` additionally emits a content-addressed
semantic JavaScript adapter containing only selected operations and conversion
definitions. The bootstrap loads it from `data-fe-adapter` and calls
`createFeHostAdapter` with `globalThis.feAdapterEnvironment` (`{ host,
runtime }`, or an async function returning it) before Wasm import preflight.
Iterator, async-iterator, and collection operations share ownership state and
must currently be selected as complete groups; partial groups fail closed.

To use an application-owned bootstrap, place this in the source document:

```html
<meta name="fe-bootstrap" content="/assets/my-fe-bootstrap.js">
```

To publish artifacts without injecting a startup module:

```html
<meta name="fe-bootstrap" content="none">
```

The default module is CSP-friendly as an external module, but the deployment
must still allow its URL through `script-src`, artifact fetches through
`connect-src`, and Wasm compilation under the browser's applicable CSP rule.
The content-addressed path supports a narrow source allowlist; no nonce or
inline-script exception is required.
