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
assert.doesNotMatch(
  workerSource,
  /placement: "main_thread"/,
  "the Worker proxy must not restate Fe-declared placement",
);
assert.match(
  workerSource,
  /createCanonicalIntentRouter\(adapter,/,
  "Worker routing must be partitioned from compiler-derived lane intents",
);
assert.match(
  workerSource,
  /createCanonicalMainThreadGpuClient\(gpuPort,/,
  "Worker GPU schemas must be selected from compiler-derived lane intents",
);
assert.doesNotMatch(
  workerSource,
  /createExactLaneRouter|selectActorSchemas|lanes:\s*\[/,
  "the Worker must not restate the compiler-owned lane set",
);

const clientSource = await readFile(
  new URL("./worker-client.js", import.meta.url),
  "utf8",
);
assert.match(
  clientSource,
  /createCanonicalModuleWorkerActor\(\{/,
  "the client must delegate request IDs, epochs, restarts, and failures to the canonical runtime",
);
assert.match(
  clientSource,
  /createCanonicalMainThreadGpuBroker\(channel\.port1,/,
  "the main-thread broker must derive its GPU lane schemas from compiler intent",
);
assert.doesNotMatch(
  clientSource,
  /actorEnvelope|createModuleWorkerActor|selectActorSchemas|requestId/,
  "the application client must not duplicate canonical actor protocol machinery",
);

const wasm = new Uint8Array(await readFile(
  new URL("./gen/actor-canonical.wasm", import.meta.url),
));
const reference = JSON.parse(await readFile(
  new URL("./gen/reference.json", import.meta.url),
  "utf8",
));
const layout = JSON.parse(await readFile(
  new URL("./gen/layout.json", import.meta.url),
  "utf8",
));
const defaults = {
  origin_x: 0, origin_y: 0, origin_z: -4,
  projection_norm_squared: 3.24, pixel_scale: 0.018,
  a: 0.85, b: 1.25, c: 0.65, d: 0.55, e: -0.40,
  f: 0.30, g: -0.16, h: 0.1375, i: -0.04, j: -0.979125,
};
const frame = (generation) => ({ ...defaults, generation });
const expectedValues = layout.params.map(({ name }) => defaults[name]);
const gpuBytes = new Uint8Array(128 * 128 * 4);
gpuBytes.fill(19);
let renderGeneration = null;
let verifyGeneration = null;
const actor = await createQcgaActor({
  wasm,
  width: 128,
  height: 128,
  params: layout.params,
  gpuRender: (values, request) => {
    assert.deepEqual(values, expectedValues);
    renderGeneration = request.generation;
    return { submitted: true };
  },
  gpuVerify: (values, request) => {
    assert.deepEqual(values, expectedValues);
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
  assert.deepEqual(await actor.render(frame(3)), { submitted: true });
  assert.equal(renderGeneration, 3);
  assert.deepEqual(await actor.gpu(frame(4)), gpuBytes);
  assert.equal(verifyGeneration, 4);

  const started = performance.now();
  const oracle = await actor.wasm(frame(5));
  const oracleMs = performance.now() - started;
  assert.equal(oracle.byteLength, 128 * 128 * 4);
  assert.equal(fnv1a32(oracle), reference.fnv1a32 >>> 0);

  const interrupted = actor.wasm(frame(6));
  const restarted = actor.restart();
  await assert.rejects(interrupted, /restarting module worker/);
  assert.equal(await restarted, 1);
  assert.equal(actor.epoch(), 1);
  assert.deepEqual(await actor.render(frame(7)), { submitted: true });
  assert.equal(renderGeneration, 7);
  console.log(`QCGA generated canonical host/Wasm actors: ok (oracle ${oracleMs.toFixed(1)} ms)`);
} finally {
  actor.close();
}
