import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { performance } from "node:perf_hooks";
import { createCgaWasmWorkerOracle } from "./wasm-worker-oracle.js";

const wasm = new Uint8Array(await readFile(
  new URL("./gen-schedule32/actor/module.wasm", import.meta.url),
));
const reference = JSON.parse(await readFile(
  new URL("./gen-schedule32/reference.json", import.meta.url),
  "utf8",
));
const values = [0, 0, Math.fround(0.0125), Math.fround(0.5), 0];
let renderGeneration = null;
let verifyGeneration = null;
const gpuBytes = new Uint8Array(128 * 128 * 4);
gpuBytes.fill(19);
const actor = await createCgaWasmWorkerOracle({
  wasm,
  gpuRender: (_payload, request) => {
    renderGeneration = request.generation;
    return { submitted: true };
  },
  gpuVerify: (_payload, request) => {
    verifyGeneration = request.generation;
    return gpuBytes.slice();
  },
});

const fnv1a32 = (bytes) => {
  let hash = 0x811c9dc5;
  for (const byte of bytes) {
    hash = Math.imul((hash ^ byte) >>> 0, 0x01000193) >>> 0;
  }
  return hash >>> 0;
};

try {
  assert.equal(actor.epoch(), 0);
  assert.equal(actor.pendingCount(), 0);
  assert.deepEqual(await actor.renderGpu(values, 3), { submitted: true });
  assert.equal(renderGeneration, 3);
  assert.equal(actor.pendingCount(), 0);

  const started = performance.now();
  const oracle = await actor.render(values, 4);
  const oracleMs = performance.now() - started;
  assert.equal(oracle.byteLength, 128 * 128 * 4);
  assert.equal(fnv1a32(oracle), reference.fnv1a32 >>> 0);
  assert.equal(verifyGeneration, null);

  assert.equal(await actor.restart(), 1);
  assert.equal(actor.epoch(), 1);
  assert.deepEqual(await actor.renderGpu(values, 5), { submitted: true });
  assert.equal(renderGeneration, 5);
  const cancelled = new AbortController();
  cancelled.abort();
  await assert.rejects(
    actor.render(values, 6, { signal: cancelled.signal }),
    (error) => error.code === "FE_ACTOR_ABORTED",
  );
  assert.equal(actor.pendingCount(), 0);
  console.log(
    `Schedule32 generated Worker/Wasm actor: ok (oracle ${oracleMs.toFixed(1)} ms)`,
  );
} finally {
  actor.close();
}
