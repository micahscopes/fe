import assert from "node:assert/strict";
import { createMandelbrotActorRuntime, MANDELBROT_ACTOR_SCHEMAS } from "./actor-runtime.js";
import { createCanonicalIntentRouter } from "../shared/actor-router.js";
import { compileActorAdapter } from "./gen/ctl-interface.js";

assert.deepEqual(Object.keys(MANDELBROT_ACTOR_SCHEMAS.request), ["render", "verify"]);
assert.deepEqual(Object.keys(MANDELBROT_ACTOR_SCHEMAS.result), ["render", "verify"]);
const intentRouter = createCanonicalIntentRouter(compileActorAdapter(), {
  main_thread_host() {},
  wasm() {},
});
assert.equal(intentRouter.ownerOf("render"), "main_thread_host");
assert.equal(intentRouter.ownerOf("verify"), "main_thread_host");
assert.equal(intentRouter.ownerOf("update_view_message"), "wasm");

const renders = [];
const verifications = [];
const runtime = createMandelbrotActorRuntime({
  render(view) {
    renders.push(view);
    return { submitted: true };
  },
  verify(view) {
    verifications.push(view);
    return { gpuHash: 123, wasmHash: 123, referenceHash: 123 };
  },
});

assert.deepEqual(await runtime.render([1, -2, 384]), { submitted: true });
assert.deepEqual(renders, [[1, -2, 384]]);
assert.deepEqual(await runtime.verify(new Int32Array([3, 4, 128])), {
  gpuHash: 123, wasmHash: 123, referenceHash: 123,
});
assert.deepEqual(verifications, [[3, 4, 128]]);
assert.throws(() => MANDELBROT_ACTOR_SCHEMAS.request.render({
  view: new Float32Array(3),
}), /FE_ACTOR_INVALID_PAYLOAD/);
assert.throws(() => runtime.verify([1, 2]), /three-word vector/);

let reported = null;
const failing = createMandelbrotActorRuntime({
  render: () => ({ submitted: true }),
  verify: () => Promise.reject(new Error("readback failed")),
  onError: (error) => { reported = error; },
});
await assert.rejects(failing.verify([0, 0, 384]), /FE_ACTOR_GPU_EFFECT/);
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
  verify: () => ({ gpuHash: 1, wasmHash: 1, referenceHash: 1 }),
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

const restartingRuns = [];
const restarting = createMandelbrotActorRuntime({
  render(view) {
    const job = deferred();
    restartingRuns.push({ view, job });
    return job.promise;
  },
  verify: () => ({ gpuHash: 1, wasmHash: 1, referenceHash: 1 }),
});
const interrupted = restarting.render([5, 0, 384]);
await tick();
assert.equal(restarting.epoch(), 0);
assert.equal(restarting.reset(), 1);
await assert.rejects(interrupted, /Mandelbrot actor restarted/);
assert.equal(restarting.epoch(), 1);
restartingRuns[0].job.resolve({ submitted: true });
await tick();
assert.deepEqual(restarting.gpuState().render, { active: null, pending: null });
const afterRestart = restarting.render([6, 0, 384]);
await tick();
restartingRuns[1].job.resolve({ submitted: true });
assert.deepEqual(await afterRestart, { submitted: true });
restarting.close();

runtime.close();
failing.close();
burst.close();
console.log("Mandelbrot generated canonical GPU actor lifecycle: ok");
