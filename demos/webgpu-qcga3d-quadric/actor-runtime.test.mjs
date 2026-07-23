import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { performance } from "node:perf_hooks";
import { createQcgaActor } from "./worker-client.js";

const workerSource = await readFile(
  new URL("./wasm-worker.js", import.meta.url),
  "utf8",
);
assert.match(
  workerSource,
  /postMessage\(\{ type: "init-error", error: "FE_ACTOR_WORKER_INIT" \}\)/,
  "the Worker readiness boundary must use the canonical non-leaky init failure",
);
assert.doesNotMatch(
  workerSource,
  /postMessage\(\{ type: "init-error", error: String\(error\) \}\)/,
  "arbitrary Worker error strings must not become malformed protocol messages",
);
assert.match(
  workerSource,
  /createHostEffectAdapter\([\s\S]*\{ placement: "main_thread" \}\)/,
  "the Worker proxy must select the handlers' main-thread intent explicitly",
);

const wasm = new Uint8Array(await readFile(
  new URL("./gen/actor-canonical.wasm", import.meta.url),
));
const reference = JSON.parse(await readFile(
  new URL("./gen/reference.json", import.meta.url),
  "utf8",
));
const gpuBytes = new Uint8Array(128 * 128 * 4);
gpuBytes.fill(19);
let renderGeneration = null;
let verifyGeneration = null;
const actor = await createQcgaActor({
  wasm,
  width: 128,
  height: 128,
  gpuRender: (_values, request) => {
    renderGeneration = request.generation;
    return { submitted: true };
  },
  gpuVerify: (_values, request) => {
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
  assert.deepEqual(await actor.render(3), { submitted: true });
  assert.equal(renderGeneration, 3);
  assert.deepEqual(await actor.gpu(4), gpuBytes);
  assert.equal(verifyGeneration, 4);

  const started = performance.now();
  const oracle = await actor.wasm(5);
  const oracleMs = performance.now() - started;
  assert.equal(oracle.byteLength, 128 * 128 * 4);
  assert.equal(fnv1a32(oracle), reference.fnv1a32 >>> 0);

  const interrupted = actor.wasm(6);
  const restarted = actor.restart();
  await assert.rejects(interrupted, /restarting module worker/);
  assert.equal(await restarted, 1);
  assert.equal(actor.epoch(), 1);
  assert.deepEqual(await actor.render(7), { submitted: true });
  assert.equal(renderGeneration, 7);
  console.log(`QCGA generated canonical host/Wasm actors: ok (oracle ${oracleMs.toFixed(1)} ms)`);
} finally {
  actor.close();
}
