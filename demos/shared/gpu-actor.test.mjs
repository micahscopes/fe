import assert from "node:assert/strict";
import { createGpuActorClient, createMainThreadGpuBroker } from "./gpu-actor.js";

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
console.log("shared GPU actor bounded lanes, typed replies, and stale generations: ok");
