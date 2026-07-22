import assert from "node:assert/strict";
import { createMandelbrotActorRuntime, MANDELBROT_ACTOR_SCHEMAS } from "./actor-runtime.js";

const renders = [];
const verifications = [];
const runtime = createMandelbrotActorRuntime({
  render(view) {
    renders.push(view);
    return { submitted: true };
  },
  verify(view) {
    verifications.push(view);
    return { gpuHash: 123, referenceHash: 123 };
  },
});

assert.deepEqual(await runtime.render([1, -2, 384]), { submitted: true });
assert.deepEqual(renders, [[1, -2, 384]]);
assert.deepEqual(await runtime.verify(new Int32Array([3, 4, 128])), {
  gpuHash: 123, referenceHash: 123,
});
assert.deepEqual(verifications, [[3, 4, 128]]);
assert.throws(() => MANDELBROT_ACTOR_SCHEMAS.request.render({
  view: new Float32Array(3),
}), /Int32Array\(3\)/);
await assert.rejects(runtime.verify([1, 2]), /Int32Array\(3\)/);

let reported = null;
const failing = createMandelbrotActorRuntime({
  render: () => ({ submitted: true }),
  verify: () => Promise.reject(new Error("readback failed")),
  onError: (error) => { reported = error; },
});
await assert.rejects(failing.verify([0, 0, 384]), /readback failed/);
assert.equal(reported, null, "awaited verification errors belong to the caller");

const deferred = () => {
  let resolve;
  const promise = new Promise((done) => { resolve = done; });
  return { promise, resolve };
};
const tick = () => new Promise((resolve) => setTimeout(resolve, 0));
const burstRuns = [];
const burst = createMandelbrotActorRuntime({
  render(view) {
    const job = deferred();
    burstRuns.push({ view, job });
    return job.promise;
  },
  verify: () => ({ gpuHash: 1, referenceHash: 1 }),
});
const burstPromises = [
  burst.render([1, 0, 384]),
  burst.render([2, 0, 384]),
  burst.render([3, 0, 384]),
  burst.render([4, 0, 384]),
];
await tick();
assert.equal(burstRuns.length, 1, "only the active render crosses the endpoint");
assert.deepEqual(burst.state().render, { active: 1, pending: 4 });
burstRuns[0].job.resolve({ submitted: true });
await tick();
assert.equal(burstRuns.length, 2, "only the latest pending render is promoted");
assert.deepEqual(burstRuns[1].view, [4, 0, 384]);
burstRuns[1].job.resolve({ submitted: true });
const burstResults = await Promise.all(burstPromises);
assert.deepEqual(burstResults, [
  { submitted: true }, { dropped: true }, { dropped: true }, { submitted: true },
]);
assert.deepEqual(burst.state().render, { active: null, pending: null });

console.log("Mandelbrot protocol-v2 in-process actor runtime: ok");
