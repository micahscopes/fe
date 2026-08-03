import assert from "node:assert/strict";
import test from "node:test";

import { compileRequest } from "./compiler-protocol.js";
import { createCompilerWorkerRuntime } from "./compiler-worker-runtime.js";

function fixtureRequest() {
  return compileRequest({
    source: "pub fn main() -> u32 { 42 }",
    sourceUrl: "fe-memory:///inline.fe",
    entries: ["main"],
  });
}

function runtime(compileJson = () => JSON.stringify({
  protocol: { major: 1, minor: 1 },
  compiler: { name: "fe", version: "test", build: "test" },
  target: "wasm",
  source_set_sha256: "fixture",
  diagnostics: [],
  artifacts: [{
    name: "module.wasm",
    kind: "wasm_module",
    media_type: "application/wasm",
    sha256: "fixture",
    bytes: [0, 97, 115, 109],
  }],
  interface: { imports: [], exports: [], resources: [] },
})) {
  const messages = [];
  const worker = createCompilerWorkerRuntime({
    compileJson,
    compilerProtocolMajor: () => 1,
    compilerProtocolMinor: () => 0,
    postMessage(message, transfers = []) {
      messages.push({ message, transfers });
    },
  });
  return { worker, messages };
}

test("handshakes then returns transferable artifact bytes", async () => {
  const { worker, messages } = runtime();
  assert.equal(messages[0].message.type, "ready");
  await worker.receive({
    data: { type: "compile", id: 7, request: fixtureRequest() },
  });
  const response = messages[1];
  assert.equal(response.message.type, "result");
  assert.equal(response.message.id, 7);
  assert(
    response.message.result.artifacts[0].bytes instanceof Uint8Array,
  );
  assert.equal(response.transfers.length, 1);
});

test("rejects incompatible request versions without compiling", async () => {
  let compiled = false;
  const { worker, messages } = runtime(() => {
    compiled = true;
    return "{}";
  });
  const request = fixtureRequest();
  request.protocol = { major: 2, minor: 0 };
  await worker.receive({ data: { type: "compile", id: 8, request } });
  assert.equal(compiled, false);
  assert.equal(messages[1].message.type, "error");
  assert.match(messages[1].message.error, /expected major 1/);
});

test("queued cancellation suppresses compilation and response", async () => {
  let compiled = false;
  const { worker, messages } = runtime(() => {
    compiled = true;
    return "{}";
  });
  await worker.receive({ data: { type: "cancel", id: 9 } });
  await worker.receive({
    data: { type: "compile", id: 9, request: fixtureRequest() },
  });
  assert.equal(compiled, false);
  assert.equal(messages.length, 1, "only the ready handshake is emitted");
});
