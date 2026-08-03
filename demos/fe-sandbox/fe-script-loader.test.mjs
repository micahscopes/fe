import assert from "node:assert/strict";
import test from "node:test";
import {
  createFeScriptLoader,
  verifyArtifactDigest,
} from "./fe-script-loader.js";

if (!globalThis.CustomEvent) {
  globalThis.CustomEvent = class CustomEvent {
    constructor(type, init) { this.type = type; this.detail = init?.detail; }
  };
}

function script({
  source = "",
  src,
  entry,
  type = "application/fe",
  manifest,
  attributes = {},
  baseURI,
} = {}) {
  const values = new Map([["type", type]]);
  if (src) values.set("data-fe-src", src);
  if (entry) values.set("data-fe-entry", entry);
  if (manifest) values.set("data-fe-manifest", manifest);
  for (const [name, value] of Object.entries(attributes)) values.set(name, value);
  return {
    type,
    baseURI,
    textContent: source,
    dataset: {
      ...(entry ? { feEntry: entry } : {}),
      ...(attributes["data-fe-execution"]
        ? { feExecution: attributes["data-fe-execution"] }
        : {}),
    },
    get attributes() {
      return Array.from(values, ([name, value]) => ({ name, value }));
    },
    getAttribute(name) { return values.get(name) ?? null; },
    dispatchEvent(event) { this.lastEvent = event; },
  };
}

test("compiles and runs inline Fe with generated imports", async () => {
  const element = script({ source: "pub fn main() -> u32 { 42 }" });
  let compileRequest;
  let seenImports;
  const loader = createFeScriptLoader({
    compiler: {
      async compile(request) {
        compileRequest = request;
        return { wasm: new Uint8Array([0, 97, 115, 109]), entry: "start" };
      },
    },
    importProviders: [
      () => ({ "fe:web": { window_inner_width: () => 800 } }),
    ],
    fetch: async () => assert.fail("inline source must not fetch"),
    baseUrl: "https://example.test/playground/",
    instantiate: async (_wasm, imports) => {
      seenImports = imports;
      return { instance: { exports: { start: () => 42 } } };
    },
    compile: async (bytes) => bytes,
    moduleImports: () => [],
  });

  const result = await loader.run(element);
  assert.equal(compileRequest.source, element.textContent);
  assert.equal(compileRequest.sourceUrl, "https://example.test/playground/");
  assert.equal(seenImports["fe:web"].window_inner_width(), 800);
  assert.equal(result.value, 42);
  assert.equal(element.dataset.feState, "complete");
  assert.equal(element.lastEvent.type, "fe:load");
});

test("loads external Fe and preserves document order", async () => {
  const first = script({ src: "./first.fe" });
  const second = script({ source: "second", entry: "custom" });
  const compiled = [];
  const loader = createFeScriptLoader({
    compiler: {
      async compile(request) {
        compiled.push(request.source);
        return { wasm: new Uint8Array(), entry: request.source === "first" ? "main" : undefined };
      },
    },
    fetch: async (url) => {
      assert.equal(url, "https://example.test/app/first.fe");
      return { ok: true, text: async () => "first" };
    },
    baseUrl: "https://example.test/app/",
    instantiate: async () => ({
      instance: { exports: { main: () => 1, custom: () => 2 } },
    }),
    compile: async (bytes) => bytes,
    moduleImports: () => [],
  });
  const root = {
    querySelectorAll() { return [first, second]; },
  };

  const results = await loader.boot(root);
  assert.deepEqual(compiled, ["first", "second"]);
  assert.deepEqual(results.map(({ value }) => value), [1, 2]);
});

test("resolves against the element base and forwards explicit Fetch policy", async () => {
  const element = script({
    src: "../src/app.fe",
    baseURI: "https://cdn.example/site/nested/",
    attributes: {
      crossorigin: "use-credentials",
      referrerpolicy: "strict-origin",
      integrity: "sha256-source",
    },
  });
  let fetched;
  const loader = createFeScriptLoader({
    compiler: { async compile() { return { wasm: new Uint8Array() }; } },
    fetch: async (url, options) => {
      fetched = { url, options };
      return { ok: true, text: async () => "pub fn main() {}" };
    },
    compile: async (bytes) => bytes,
    moduleImports: () => [],
    instantiate: async () => ({ exports: { main: () => {} } }),
    baseUrl: undefined,
  });
  await loader.run(element);
  assert.equal(fetched.url, "https://cdn.example/site/src/app.fe");
  assert.equal(fetched.options.mode, "cors");
  assert.equal(fetched.options.credentials, "include");
  assert.equal(fetched.options.referrerPolicy, "strict-origin");
  assert.equal(fetched.options.integrity, "sha256-source");
});

test("crossorigin anonymous omits credentials", async () => {
  const element = script({
    src: "app.fe",
    attributes: { crossorigin: "" },
  });
  let options;
  const loader = createFeScriptLoader({
    compiler: { async compile() { return { wasm: new Uint8Array() }; } },
    fetch: async (_url, init) => {
      options = init;
      return { ok: true, text: async () => "pub fn main() {}" };
    },
    baseUrl: "https://example.test/",
    compile: async (bytes) => bytes,
    moduleImports: () => [],
    instantiate: async () => ({ exports: { main: () => {} } }),
  });
  await loader.run(element);
  assert.equal(options.credentials, "omit");
});

test("non-autostart Fe data blocks instantiate without invoking their entry", async () => {
  const element = script({ source: "pub fn pixel(x: i32, y: i32) -> u32 { 0 }", entry: "pixel" });
  element.dataset.feAutostart = "false";
  let calls = 0;
  const loader = createFeScriptLoader({
    compiler: { async compile() { return { wasm: new Uint8Array() }; } },
    fetch: async () => assert.fail("inline source must not fetch"),
    baseUrl: "https://example.test/",
    compile: async (bytes) => bytes,
    moduleImports: () => [],
    instantiate: async () => ({
      instance: { exports: { pixel: () => { calls += 1; } } },
    }),
  });
  const result = await loader.run(element);
  assert.equal(calls, 0);
  assert.equal(typeof result.instance.exports.pixel, "function");
  assert.equal(element.dataset.feState, "complete");
});

test("Worker placement is explicit and never falls through to main-realm instantiation", async () => {
  const element = script({
    source: "pub fn main() {}",
    attributes: { "data-fe-execution": "worker" },
  });
  let request;
  const loader = createFeScriptLoader({
    compiler: { async compile() { return { wasm: new Uint8Array(), entry: "main" }; } },
    fetch: async () => assert.fail("inline source must not fetch"),
    baseUrl: "https://example.test/",
    instantiate: async () => assert.fail("Worker placement must not instantiate on main"),
    workerExecutor: {
      async run(value) {
        request = value;
        return { value: 9, placement: "worker" };
      },
    },
  });
  const result = await loader.run(element);
  assert.equal(request.entry, "main");
  assert.equal(request.autostart, true);
  assert.equal(result.placement, "worker");
});

test("Worker placement fails closed without an executor", async () => {
  const element = script({
    attributes: { "data-fe-execution": "worker" },
  });
  const loader = createFeScriptLoader({
    compiler: { async compile() { return { wasm: new Uint8Array() }; } },
    fetch: async () => assert.fail("inline source must not fetch"),
    baseUrl: "https://example.test/",
  });
  await assert.rejects(loader.run(element), /requires a workerExecutor/);
  assert.equal(element.dataset.feState, "error");
});

test("fails closed on conflicting import providers", async () => {
  const element = script({ source: "fn main() {}" });
  const loader = createFeScriptLoader({
    compiler: { async compile() { return { wasm: new Uint8Array() }; } },
    importProviders: [
      { env: { log: () => 1 } },
      { env: { log: () => 2 } },
    ],
    fetch: async () => { throw new Error("unused"); },
    baseUrl: "https://example.test/",
    instantiate: async () => assert.fail("conflicting imports must not instantiate"),
    compile: async (bytes) => bytes,
    moduleImports: () => [],
  });

  await assert.rejects(loader.run(element), /duplicate Wasm import: env\.log/);
  assert.equal(element.dataset.feState, "error");
});

test("AbortSignal cancels before fetch or compilation and reports its lifecycle", async () => {
  const element = script({ src: "app.fe" });
  const controller = new AbortController();
  controller.abort(new DOMException("cancelled by test", "AbortError"));
  const loader = createFeScriptLoader({
    compiler: { async compile() { assert.fail("cancelled source must not compile"); } },
    fetch: async () => assert.fail("cancelled source must not fetch"),
    baseUrl: "https://example.test/",
  });

  await assert.rejects(loader.run(element, { signal: controller.signal }), /cancelled by test/);
  assert.equal(element.dataset.feState, "cancelled");
  assert.equal(element.lastEvent.type, "fe:cancel");
});

test("AbortSignal wins when compilation completes after cancellation", async () => {
  const element = script({ source: "fn main() {}" });
  const controller = new AbortController();
  const loader = createFeScriptLoader({
    compiler: {
      async compile() {
        controller.abort(new DOMException("late cancellation", "AbortError"));
        return { wasm: new Uint8Array() };
      },
    },
    fetch: async () => assert.fail("inline source must not fetch"),
    baseUrl: "https://example.test/",
    compile: async () => assert.fail("cancelled artifact must not compile"),
  });

  await assert.rejects(loader.run(element, { signal: controller.signal }), /late cancellation/);
  assert.equal(element.dataset.feState, "cancelled");
  assert.equal(element.lastEvent.type, "fe:cancel");
});

test("loads a precompiled artifact without a compiler and preflights imports", async () => {
  const element = script({
    type: "application/fe+wasm",
    src: "assets/app.wasm",
    manifest: "assets/app.json",
  });
  const wasm = new Uint8Array([0, 97, 115, 109]);
  const manifest = {
    protocol: { major: 1, minor: 1 },
    entry: "main",
    interface: { imports: [], exports: [], resources: [] },
    artifacts: [{
      kind: "wasm_module",
      byte_len: wasm.byteLength,
      sha256: "fixture",
    }],
  };
  const fetched = [];
  const loader = createFeScriptLoader({
    fetch: async (url) => {
      fetched.push(url);
      if (url.endsWith(".json")) {
        return { ok: true, json: async () => manifest };
      }
      return { ok: true, arrayBuffer: async () => wasm.buffer };
    },
    baseUrl: "https://example.test/site/",
    verifyDigest: async (bytes, digest) => {
      assert.deepEqual(bytes, wasm);
      assert.equal(digest, "fixture");
    },
    compile: async () => ({ compiled: true }),
    moduleImports: () => [{ module: "fe:web", name: "console_log" }],
    importProviders: [{ "fe:web": { console_log: () => {} } }],
    instantiate: async (_module, imports) => {
      assert.equal(typeof imports["fe:web"].console_log, "function");
      return { exports: { main: () => 42 } };
    },
  });
  const result = await loader.run(element);
  assert.equal(result.value, 42);
  assert.deepEqual(fetched, [
    "https://example.test/site/assets/app.json",
    "https://example.test/site/assets/app.wasm",
  ]);
});

test("precompiled integrity is forwarded and must agree with the signed manifest digest", async () => {
  const digest = "00".repeat(32);
  const sri = `sha256-${Buffer.alloc(32).toString("base64")}`;
  const element = script({
    type: "application/fe+wasm",
    src: "app.wasm",
    manifest: "app.json",
    attributes: {
      "data-fe-integrity": sri,
      "data-fe-manifest-integrity": "sha256-manifest",
    },
  });
  const fetches = [];
  const loader = createFeScriptLoader({
    fetch: async (url, options) => {
      fetches.push({ url, options });
      return url.endsWith(".json")
        ? {
            ok: true,
            json: async () => ({
              protocol: { major: 1, minor: 1 },
              entry: "main",
              artifacts: [{ kind: "wasm_module", byte_len: 0, sha256: digest }],
            }),
          }
        : { ok: true, arrayBuffer: async () => new ArrayBuffer() };
    },
    baseUrl: "https://example.test/",
    verifyDigest: async () => {},
    compile: async () => ({}),
    moduleImports: () => [],
    instantiate: async () => ({ exports: { main: () => {} } }),
  });
  await loader.run(element);
  assert.equal(fetches[0].options.integrity, "sha256-manifest");
  assert.equal(fetches[1].options.integrity, sri);

  const mismatch = script({
    type: "application/fe+wasm",
    src: "app.wasm",
    manifest: "app.json",
    attributes: { "data-fe-integrity": `sha256-${Buffer.alloc(32, 1).toString("base64")}` },
  });
  await assert.rejects(loader.run(mismatch), /integrity does not match manifest/);
});

test("precompiled artifacts fail before instantiation when an import is missing", async () => {
  const element = script({
    type: "application/fe+wasm",
    src: "app.wasm",
    manifest: "app.json",
  });
  const loader = createFeScriptLoader({
    fetch: async (url) => url.endsWith(".json")
      ? {
          ok: true,
          json: async () => ({
            protocol: { major: 1, minor: 1 },
            entry: "main",
            artifacts: [{ kind: "wasm_module", byte_len: 0, sha256: "empty" }],
          }),
        }
      : { ok: true, arrayBuffer: async () => new ArrayBuffer() },
    baseUrl: "https://example.test/",
    verifyDigest: async () => {},
    compile: async () => ({}),
    moduleImports: () => [{ module: "fe:web", name: "window" }],
    instantiate: async () => assert.fail("missing imports must not instantiate"),
  });
  await assert.rejects(loader.run(element), /missing Wasm import: fe:web\.window/);
});

test("default artifact digest verification detects tampering", async () => {
  const bytes = new Uint8Array([0, 97, 115, 109]);
  const digest = await crypto.subtle.digest("SHA-256", bytes);
  const expected = Array.from(
    new Uint8Array(digest),
    (byte) => byte.toString(16).padStart(2, "0"),
  ).join("");
  await verifyArtifactDigest(bytes, expected);
  await assert.rejects(
    verifyArtifactDigest(new Uint8Array([1, 2, 3]), expected),
    /SHA-256 mismatch/,
  );
});

test("boot waits for DOMContentLoaded before taking its document-order snapshot", async () => {
  const element = script({ source: "pub fn main() {}" });
  let ready;
  const root = {
    nodeType: 9,
    readyState: "loading",
    addEventListener(type, callback) {
      assert.equal(type, "DOMContentLoaded");
      ready = callback;
    },
    removeEventListener() {},
    querySelectorAll() { return [element]; },
  };
  const loader = createFeScriptLoader({
    compiler: { async compile() { return { wasm: new Uint8Array() }; } },
    fetch: async () => assert.fail("inline source must not fetch"),
    baseUrl: "https://example.test/",
    compile: async (bytes) => bytes,
    moduleImports: () => [],
    instantiate: async () => ({ exports: { main: () => 1 } }),
  });
  let settled = false;
  const boot = loader.boot(root).then((value) => {
    settled = true;
    return value;
  });
  await Promise.resolve();
  assert.equal(settled, false);
  ready();
  assert.equal((await boot)[0].value, 1);
});

test("concurrent run calls share one lifecycle promise", async () => {
  const element = script({ source: "pub fn main() {}" });
  let release;
  let compilations = 0;
  const loader = createFeScriptLoader({
    compiler: {
      async compile() {
        compilations += 1;
        await new Promise((resolve) => { release = resolve; });
        return { wasm: new Uint8Array() };
      },
    },
    fetch: async () => assert.fail("inline source must not fetch"),
    baseUrl: "https://example.test/",
    compile: async (bytes) => bytes,
    moduleImports: () => [],
    instantiate: async () => ({ exports: { main: () => 1 } }),
  });
  const first = loader.run(element);
  const second = loader.run(element);
  await Promise.resolve();
  release();
  assert.equal((await first).value, 1);
  assert.equal((await second).value, 1);
  assert.equal(compilations, 1);
});
