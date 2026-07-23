import assert from "node:assert/strict";
import {
  createCanonicalMainThreadGpuChannel,
  createGpuActorClient,
  createMainThreadGpuBroker,
  createTypedGpuActorClient,
  createTypedMainThreadGpuBroker,
} from "./gpu-actor.js";
import { actorField, actorResultSchema, exactObject } from "./actor-endpoint.js";

const deferred = () => { let resolve; const promise = new Promise((done) => { resolve = done; });
  return { promise, resolve }; };
const tick = () => new Promise((resolve) => setTimeout(resolve, 0));
const channel = new MessageChannel();
const runs = [];
const broker = createMainThreadGpuBroker(channel.port1, {
  valueCount: 5, rgbaBytes: 4,
  render(values) { const job = deferred(); runs.push({ values, job }); return job.promise; },
  verify: () => new Uint8Array([1, 2, 3, 4]),
});
const client = createGpuActorClient(channel.port2, { valueCount: 5, rgbaBytes: 4 });
const a = client.render([1, 0, 0, 0, 0], 1);
const b = client.render([2, 0, 0, 0, 0], 1);
const bRejected = assert.rejects(b, /superseded/);
const c = client.render([3, 0, 0, 0, 0], 1);
await tick();
assert.equal(runs.length, 1);
await bRejected;
runs[0].job.resolve({ submitted: true });
assert.deepEqual(await a, { submitted: true });
await tick();
assert.deepEqual(runs[1].values, [3, 0, 0, 0, 0]);
runs[1].job.resolve({ submitted: true });
assert.deepEqual(await c, { submitted: true });
assert.deepEqual(await client.verify([0, 0, 0, 0, 0], 2), new Uint8Array([1, 2, 3, 4]));
await assert.rejects(client.render([4, 0, 0, 0, 0], 1), /stale GPU generation/);
client.close();
broker.close();

const generatedSchemas = Object.freeze({
  requestSchema: Object.freeze({
    draw: (payload) => exactObject(payload, { value: actorField.finiteNumber }),
    effect: (payload) => exactObject(payload, { value: actorField.finiteNumber }),
    malformed: (payload) => exactObject(payload, { value: actorField.finiteNumber }),
  }),
  resultSchema: Object.freeze({
    draw: actorResultSchema((value) =>
      exactObject(value, { doubled: actorField.finiteNumber })),
    effect: actorResultSchema((value) =>
      exactObject(value, { doubled: actorField.finiteNumber })),
    malformed: actorResultSchema((value) =>
      exactObject(value, { doubled: actorField.finiteNumber })),
  }),
});
const canonicalAdapter = Object.freeze({
  ...generatedSchemas,
  intents: Object.freeze({
    draw: Object.freeze({
      execution: "host_effect",
      placement: "main_thread",
      capabilities: Object.freeze([
        Object.freeze({ capability: "webgpu_dispatch", mutable: true }),
      ]),
    }),
    effect: Object.freeze({
      execution: "host_effect",
      placement: "main_thread",
      capabilities: Object.freeze([
        Object.freeze({ capability: "webgpu_dispatch", mutable: true }),
      ]),
    }),
    malformed: Object.freeze({
      execution: "host_effect",
      placement: "worker",
      capabilities: Object.freeze([]),
    }),
  }),
});
const canonicalRuns = [];
const canonical = createCanonicalMainThreadGpuChannel({
  adapter: canonicalAdapter,
  handlers: {
    draw: ({ value }) => { canonicalRuns.push(value); return { doubled: value * 2 }; },
    effect: ({ value }) => ({ doubled: value }),
  },
});
assert.deepEqual(await canonical.client.request("draw", { value: 7 }, 1), { doubled: 14 });
assert.deepEqual(canonicalRuns, [7]);
await assert.rejects(
  canonical.client.request("malformed", { value: 1 }, 1),
  /no request schema for actor lane malformed/,
);
assert.throws(
  () => createCanonicalMainThreadGpuChannel({
    adapter: canonicalAdapter,
    handlers: { draw() {} },
  }),
  /must exactly cover actor lanes: draw, effect/,
);
assert.throws(
  () => createCanonicalMainThreadGpuChannel({
    adapter: canonicalAdapter,
    handlers: { draw() {}, effect() {}, malformed() {} },
  }),
  /must exactly cover actor lanes: draw, effect/,
);
canonical.client.close();
canonical.broker.close();

const typedChannel = new MessageChannel();
const typedRuns = [];
const typedBroker = createTypedMainThreadGpuBroker(typedChannel.port1, {
  handlers: {
    draw(payload) {
      const job = deferred();
      typedRuns.push({ payload, job });
      return job.promise;
    },
    effect: () => {
      throw new Error("secret GPU device and host details");
    },
    malformed: () => ({ wrong: true }),
  },
  ...generatedSchemas,
});
const typedClient = createTypedGpuActorClient(typedChannel.port2, generatedSchemas);
const first = typedClient.request("draw", { value: 1 }, 4);
const superseded = typedClient.request("draw", { value: 2 }, 4);
const supersededRejected = assert.rejects(superseded, /superseded/);
const latest = typedClient.request("draw", { value: 3 }, 4);
await tick();
assert.deepEqual(typedBroker.state(), {
  draw: { active: 1, pending: 3 },
  effect: { active: null, pending: null },
  malformed: { active: null, pending: null },
});
await supersededRejected;
typedRuns[0].job.resolve({ doubled: 2 });
assert.deepEqual(await first, { doubled: 2 });
await tick();
assert.deepEqual(typedRuns[1].payload, { value: 3 });
typedRuns[1].job.resolve({ doubled: 6 });
assert.deepEqual(await latest, { doubled: 6 });
await assert.rejects(typedClient.request("effect", { value: 1 }, 4), (error) => {
  assert.equal(error.message, "FE_ACTOR_GPU_EFFECT");
  assert.doesNotMatch(error.message, /secret|device|host/);
  return true;
});
await assert.rejects(typedClient.request("malformed", { value: 1 }, 4), (error) => {
  assert.equal(error.message, "FE_ACTOR_INVALID_GPU_RESULT");
  assert.doesNotMatch(error.message, /wrong|doubled|object/);
  return true;
});
await assert.rejects(
  typedClient.request("unknown", { value: 1 }, 4),
  /no request schema for actor lane unknown/,
);

const interrupted = typedClient.request("draw", { value: 4 }, 5);
const pendingAtRestart = typedClient.request("draw", { value: 5 }, 5);
await tick();
assert.equal(typedBroker.restart(1), 1);
await assert.rejects(pendingAtRestart, /GPU actor restarted/);
assert.equal(typedClient.restart(), 1);
await assert.rejects(interrupted, /GPU actor client restarted/);
typedRuns.at(-1).job.resolve({ doubled: 8 });
await tick();
assert.equal(typedClient.pendingCount(), 0);

assert.throws(
  () => createTypedMainThreadGpuBroker(new MessageChannel().port1, {
    handlers: { draw() {} },
    ...generatedSchemas,
  }),
  /must exactly cover actor lanes/,
);
assert.throws(
  () => createTypedGpuActorClient(new MessageChannel().port1, {
    requestSchema: generatedSchemas.requestSchema,
    resultSchema: { draw: generatedSchemas.resultSchema.draw },
  }),
  /must exactly cover actor lanes/,
);
typedClient.close();
typedBroker.close();

const abortGpuChannel = new MessageChannel();
const abortGpuJobs = [];
const abortGpuBroker = createTypedMainThreadGpuBroker(abortGpuChannel.port1, {
  handlers: {
    draw(payload, _request, { signal }) {
      const job = deferred();
      abortGpuJobs.push({ payload, signal, job });
      return job.promise;
    },
    effect: () => ({ doubled: 0 }),
    malformed: () => ({ doubled: 0 }),
  },
  ...generatedSchemas,
});
const abortGpuClient = createTypedGpuActorClient(abortGpuChannel.port2, generatedSchemas);
const activeGpuAbort = new AbortController();
const activeGpu = abortGpuClient.request(
  "draw", { value: 10 }, 1, { signal: activeGpuAbort.signal },
);
await tick();
activeGpuAbort.abort();
await assert.rejects(activeGpu, (error) => error.code === "FE_ACTOR_ABORTED");
await tick();
assert.equal(abortGpuJobs[0].signal.aborted, true);
const queuedGpuAbort = new AbortController();
const queuedGpu = abortGpuClient.request(
  "draw", { value: 11 }, 1, { signal: queuedGpuAbort.signal },
);
queuedGpuAbort.abort();
await assert.rejects(queuedGpu, (error) => error.code === "FE_ACTOR_ABORTED");
await tick();
abortGpuJobs[0].job.resolve({ doubled: 20 });
await tick();
assert.equal(abortGpuJobs.length, 1, "cancelled pending GPU request never reaches handler");
assert.deepEqual(abortGpuBroker.state(), {
  draw: { active: null, pending: null },
  effect: { active: null, pending: null },
  malformed: { active: null, pending: null },
});
abortGpuClient.close();
abortGpuBroker.close();

console.log("shared GPU actor bounded lanes, typed replies, cancellation, and stale generations: ok");
