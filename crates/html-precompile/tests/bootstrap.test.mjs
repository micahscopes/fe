import assert from "node:assert/strict";
import test from "node:test";
import {
  bootFeArtifacts,
  registerFeImportProvider,
} from "../assets/bootstrap.js";

if (!globalThis.CustomEvent) {
  globalThis.CustomEvent = class CustomEvent {
    constructor(type, init) {
      this.type = type;
      this.detail = init?.detail;
    }
  };
}

function element() {
  return {
    baseURI: "https://example.test/",
    dataset: {
      feManifest: "assets/app.json",
      feSrc: "assets/app.wasm",
      feEntry: "main",
    },
    dispatchEvent(event) {
      this.lastEvent = event;
    },
  };
}

async function digest(bytes) {
  const value = new Uint8Array(await crypto.subtle.digest("SHA-256", bytes));
  return Array.from(value, byte => byte.toString(16).padStart(2, "0")).join("");
}

function installFetch(bytes) {
  return async url => String(url).endsWith(".json")
    ? {
        ok: true,
        json: async () => ({
          entry: "main",
          artifacts: [{
            kind: "wasm_module",
            byte_len: bytes.byteLength,
            sha256: await digest(bytes),
          }],
        }),
      }
    : { ok: true, arrayBuffer: async () => bytes.buffer };
}

test("published bootstrap executes a real precompiled artifact without a compiler", async () => {
  const wasm = Uint8Array.from([
    0, 97, 115, 109, 1, 0, 0, 0,
    1, 5, 1, 96, 0, 1, 127,
    3, 2, 1, 0,
    7, 8, 1, 4, 109, 97, 105, 110, 0, 0,
    10, 6, 1, 4, 0, 65, 42, 11,
  ]);
  globalThis.fetch = installFetch(wasm);
  const script = element();
  const [result] = await bootFeArtifacts({
    querySelectorAll: () => [script],
  });
  assert.equal(result.value, 42);
  assert.equal(script.dataset.feState, "complete");
  assert.equal(script.lastEvent.type, "fe:load");
});

test("published bootstrap fails preflight before instantiation on unresolved imports", async () => {
  const wasm = Uint8Array.from([
    0, 97, 115, 109, 1, 0, 0, 0,
    1, 4, 1, 96, 0, 0,
    2, 11, 1, 3, 101, 110, 118, 3, 108, 111, 103, 0, 0,
    7, 8, 1, 4, 109, 97, 105, 110, 0, 0,
  ]);
  globalThis.fetch = installFetch(wasm);
  const script = element();
  await assert.rejects(
    bootFeArtifacts({ querySelectorAll: () => [script] }),
    /missing Wasm import: env\.log/,
  );
  assert.equal(script.dataset.feState, "error");
  assert.equal(script.lastEvent.type, "fe:error");
});

test("bootstrap registers a selected adapter before real Wasm import preflight", async () => {
  const wasm = Uint8Array.from([
    0, 97, 115, 109, 1, 0, 0, 0,
    1, 4, 1, 96, 0, 0,
    2, 11, 1, 3, 101, 110, 118, 3, 108, 111, 103, 0, 0,
    7, 8, 1, 4, 109, 97, 105, 110, 0, 0,
  ]);
  globalThis.fetch = installFetch(wasm);
  let calls = 0;
  globalThis.feAdapterEnvironment = {
    host: {
      log() {
        calls += 1;
      },
    },
    runtime: {},
  };
  const selectedModule = [
    "export function createFeHostAdapter(host) {",
    "  return { imports: { env: { log: () => host.log() } } };",
    "}",
  ].join("\n");
  const script = element();
  script.dataset.feAdapter =
    `data:text/javascript;base64,${Buffer.from(selectedModule).toString("base64")}`;
  const [result] = await bootFeArtifacts({
    querySelectorAll: () => [script],
  });
  assert.equal(result.value, undefined);
  assert.equal(calls, 1);
});

test("application import providers satisfy preflight outside compiler code", async () => {
  const wasm = Uint8Array.from([
    0, 97, 115, 109, 1, 0, 0, 0,
    1, 4, 1, 96, 0, 0,
    2, 11, 1, 3, 101, 110, 118, 3, 108, 111, 103, 0, 0,
    7, 8, 1, 4, 109, 97, 105, 110, 0, 0,
  ]);
  globalThis.fetch = installFetch(wasm);
  let calls = 0;
  registerFeImportProvider({
    env: {
      log() {
        calls += 1;
      },
    },
  });
  const [result] = await bootFeArtifacts({
    querySelectorAll: () => [element()],
  });
  assert.equal(result.value, undefined);
  assert.equal(calls, 1);
});
