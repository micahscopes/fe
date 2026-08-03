import assert from "node:assert/strict";
import test from "node:test";

import {
  FE_COMPILER_PROTOCOL,
  assertCompatibleProtocol,
  compileRequest,
  wasmArtifact,
} from "./compiler-protocol.js";

test("builds the versioned virtual-source compile request", () => {
  const request = compileRequest({
    source: "pub fn main() -> u32 { 42 }",
    sourceUrl: "fe-memory:///app/src/lib.fe",
    entries: ["main"],
  });
  assert.deepEqual(request.protocol, FE_COMPILER_PROTOCOL);
  assert.equal(request.root, request.sources[0].url);
  assert.equal(request.target, "wasm");
  assert.deepEqual(request.entries, ["main"]);
});

test("rejects incompatible compiler results", () => {
  assert.throws(
    () => assertCompatibleProtocol({ major: 2, minor: 0 }),
    /expected major 1, received 2/,
  );
  assert.throws(
    () => wasmArtifact({ protocol: FE_COMPILER_PROTOCOL, artifacts: [] }),
    /no Wasm module artifact/,
  );
});

test("selects a Wasm artifact without depending on artifact order", () => {
  const wasm = {
    name: "module.wasm",
    kind: "wasm_module",
    media_type: "application/wasm",
    sha256: "test-fixture",
    bytes: [0, 97, 115, 109],
  };
  assert.equal(
    wasmArtifact({
      protocol: FE_COMPILER_PROTOCOL,
      artifacts: [
        { name: "module.map", kind: "source_map", bytes: [] },
        wasm,
      ],
    }),
    wasm,
  );
});
