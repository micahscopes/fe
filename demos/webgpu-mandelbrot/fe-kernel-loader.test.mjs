import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFile } from "node:fs/promises";
import test from "node:test";

import {
  MANDELBROT_SOURCE_PATH,
  loadMandelbrotFeKernel,
  rewriteFeArtifactForDevelopment,
} from "./fe-kernel-loader.js";

const html = await readFile(new URL("./index.html", import.meta.url), "utf8");
const source = await readFile(
  new URL("../capstones/mandelbrot/kernel.fe", import.meta.url),
  "utf8",
);

function frameHash(instance) {
  let hash = 0x811c9dc5;
  const pixel = instance.exports.mandel_pixel_q12;
  for (let y = 0; y < 512; y++) {
    for (let x = 0; x < 512; x++) {
      const value = pixel(x, y) >>> 0;
      for (let shift = 0; shift < 32; shift += 8) {
        hash ^= (value >>> shift) & 0xff;
        hash = Math.imul(hash, 0x01000193);
      }
    }
  }
  return hash >>> 0;
}

if (!globalThis.CustomEvent) {
  globalThis.CustomEvent = class CustomEvent {
    constructor(type, init) { this.type = type; this.detail = init?.detail; }
  };
}

test("page references one canonical source from a generic precompiled artifact block", () => {
  const tag = html.match(
    /<script\s+type="application\/fe\+wasm"[^>]*data-fe-source="\.\.\/capstones\/mandelbrot\/kernel\.fe"[^>]*>/,
  );
  assert(tag, "page must contain the generic application/fe+wasm artifact block");
  assert.match(tag[0], /data-fe-src="\.\/gen\/kernel\.wasm"/);
  assert.match(tag[0], /data-fe-manifest="\.\/gen\/kernel\.manifest\.json"/);
  assert.match(tag[0], /data-fe-autostart="false"/);
  assert.match(html, /data-fe-bootstrap src="\.\/gen\/fe-bootstrap\.js"/);
  assert.equal(MANDELBROT_SOURCE_PATH, "../capstones/mandelbrot/kernel.fe");
  assert.equal(
    createHash("sha256").update(source).digest("hex"),
    "dd9edf593b8477f2afeea3c2e4e51669d67a1a1e8f37782f2c43e1b124f8d871",
  );
});

test("Fe artifact precedes generic bootstrap and page module in document order", () => {
  const fe = html.indexOf('type="application/fe+wasm"');
  const bootstrap = html.indexOf("data-fe-bootstrap");
  const boot = html.indexOf('type="module" src="./main.js"');
  assert(fe >= 0 && bootstrap > fe && boot > bootstrap);
});

test("development rewrite preserves the single source provenance", () => {
  const attributes = new Map([
    ["type", "application/fe+wasm"],
    ["data-fe-src", "./gen/kernel.wasm"],
    ["data-fe-source", MANDELBROT_SOURCE_PATH],
    ["data-fe-manifest", "./gen/kernel.manifest.json"],
  ]);
  const element = {
    type: "application/fe+wasm",
    getAttribute: (name) => attributes.get(name) ?? null,
    setAttribute(name, value) { attributes.set(name, value); },
  };
  rewriteFeArtifactForDevelopment(element);
  assert.equal(element.type, "application/fe");
  assert.equal(attributes.get("data-fe-source"), MANDELBROT_SOURCE_PATH);
  assert.equal(attributes.get("data-fe-src"), MANDELBROT_SOURCE_PATH);
  assert.equal(attributes.get("data-fe-manifest"), "./gen/kernel.manifest.json");
});

test("production fallback verifies and executes the generated canonical artifact", async () => {
  const attributes = new Map([
    ["type", "application/fe+wasm"],
    ["data-fe-src", "./gen/kernel.wasm"],
    ["data-fe-source", MANDELBROT_SOURCE_PATH],
    ["data-fe-entry", "mandel_pixel_q12"],
    ["data-fe-autostart", "false"],
    ["data-fe-manifest", "./gen/kernel.manifest.json"],
  ]);
  const element = {
    type: "application/fe+wasm",
    baseURI: new URL("./index.html", import.meta.url).href,
    dataset: {
      feSrc: "./gen/kernel.wasm",
      feManifest: "./gen/kernel.manifest.json",
      feEntry: "mandel_pixel_q12",
      feAutostart: "false",
    },
    get attributes() {
      return Array.from(attributes, ([name, value]) => ({ name, value }));
    },
    getAttribute: (name) => attributes.get(name) ?? null,
    setAttribute(name, value) {
      attributes.set(name, value);
      if (name === "type") this.type = value;
    },
    listeners: new Map(),
    addEventListener(name, listener) { this.listeners.set(name, listener); },
    dispatchEvent(event) {
      this.lastEvent = event;
      this.listeners.get(event.type)?.(event);
    },
  };
  const baseURI = new URL("./index.html", import.meta.url).href;
  const priorFetch = globalThis.fetch;
  globalThis.fetch = async (url) => {
    const bytes = await readFile(new URL(url));
    return {
      ok: true,
      json: async () => JSON.parse(bytes.toString("utf8")),
      arrayBuffer: async () =>
        bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength),
    };
  };
  let result;
  try {
    result = await loadMandelbrotFeKernel({
      document: {
        baseURI,
        querySelector: () => element,
        querySelectorAll: () => [element],
      },
      location: { href: baseURI },
    });
  } finally {
    globalThis.fetch = priorFetch;
  }
  assert.equal(result.mode, "production-precompiled");
  assert.equal(result.sourcePath, MANDELBROT_SOURCE_PATH);
  assert.equal(result.instance.exports.mandel_pixel_q12(0, 0) >>> 0, 1);
  assert.equal(result.instance.exports.mandel_pixel_q12(256, 256) >>> 0, 100);
  assert.equal(frameHash(result.instance), 0x2d29649a);
  assert.equal(element.dataset.feState, "complete");
  assert.equal(element.lastEvent.type, "fe:load");
});

test("generated production manifest pins the exact Wasm digest and length", async () => {
  const [wasm, manifestText] = await Promise.all([
    readFile(new URL("./gen/kernel.wasm", import.meta.url)),
    readFile(new URL("./gen/kernel.manifest.json", import.meta.url), "utf8"),
  ]);
  const manifest = JSON.parse(manifestText);
  const artifact = manifest.artifacts.find(({ kind }) => kind === "wasm_module");
  assert.equal(artifact.byte_len, wasm.byteLength);
  assert.equal(artifact.sha256, createHash("sha256").update(wasm).digest("hex"));
});

test("configured development mode compiles canonical source through a Worker", async () => {
  const wasm = await readFile(new URL("./gen/kernel.wasm", import.meta.url));
  const attributes = new Map([
    ["type", "application/fe+wasm"],
    ["data-fe-src", "./gen/kernel.wasm"],
    ["data-fe-source", MANDELBROT_SOURCE_PATH],
    ["data-fe-entry", "mandel_pixel_q12"],
    ["data-fe-autostart", "false"],
    ["data-fe-dev-worker", "../fe-sandbox/fe-compiler.worker.js"],
  ]);
  const element = {
    type: "application/fe+wasm",
    dataset: { feEntry: "mandel_pixel_q12", feAutostart: "false" },
    get attributes() {
      return Array.from(attributes, ([name, value]) => ({ name, value }));
    },
    getAttribute: (name) => attributes.get(name) ?? null,
    setAttribute(name, value) { attributes.set(name, value); },
    dispatchEvent() {},
  };
  let compileRequest;
  class FakeWorker {
    constructor(url, options) {
      assert.equal(options.type, "module");
      assert(url.href.endsWith("/fe-sandbox/fe-compiler.worker.js"));
    }
    addEventListener(name, listener) {
      if (name !== "message") return;
      this.listener = listener;
      queueMicrotask(() => listener({
        data: {
          type: "ready",
          protocol: { major: 1, minor: 1 },
          compilerProtocol: { major: 1, minor: 1 },
        },
      }));
    }
    postMessage(message) {
      if (message.type !== "compile") return;
      compileRequest = message.request;
      queueMicrotask(() => this.listener({
        data: {
          type: "result",
          id: message.id,
          entry: "mandel_pixel_q12",
          result: {
            protocol: { major: 1, minor: 1 },
            diagnostics: [],
            interface: { imports: [], exports: [], resources: [] },
            artifacts: [{
              kind: "wasm_module",
              bytes: Array.from(wasm),
            }],
          },
        },
      }));
    }
  }
  const baseURI = new URL("./index.html", import.meta.url).href;
  const result = await loadMandelbrotFeKernel({
    document: { baseURI, querySelector: () => element },
    location: { href: `${baseURI}?fe-compile=worker` },
    Worker: FakeWorker,
    loaderOptions: {
      baseUrl: baseURI,
      fetch: async (url) => {
        const text = await readFile(new URL(url), "utf8");
        return { ok: true, text: async () => text };
      },
    },
  });
  assert.equal(result.mode, "development-worker");
  assert.equal(compileRequest.sources[0].text, source);
  assert.equal(compileRequest.sources[0].url, result.sourceUrl);
  assert.deepEqual(compileRequest.entries, ["mandel_pixel_q12"]);
  assert.equal(result.instance.exports.mandel_pixel_q12(0, 0) >>> 0, 1);
});
