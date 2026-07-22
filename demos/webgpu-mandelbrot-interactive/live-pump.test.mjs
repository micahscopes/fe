import assert from "node:assert/strict";
import { createLivePump } from "./live-pump.js";

globalThis.requestAnimationFrame = () => 1;
const canvas = {
  style: {}, addEventListener() {}, removeEventListener() {},
};
const deferred = () => {
  let resolve;
  const promise = new Promise((done) => { resolve = done; });
  return { promise, resolve };
};
const calls = [];
const updateView = (...args) => {
  const job = deferred();
  calls.push({ args, job });
  return job.promise;
};
const ctlMeta = {
  args: ["cr", "ci", "sq", "dx"],
  view_init: [10, 20, 30],
  event_map: {
    cr: { source: "view", index: 0 }, ci: { source: "view", index: 1 },
    sq: { source: "view", index: 2 }, dx: { source: "pointer", field: "movementX", when: "drag" },
  },
};
const pump = createLivePump({ canvas, updateView, ctlMeta, renderFn() {} });

const first = pump.applyFields({ movementX: 1 }, "drag");
const superseded = pump.applyFields({ movementX: 2 }, "drag");
const latest = pump.applyFields({ movementX: 3 }, "drag");
assert.equal(calls.length, 1, "only one control request may be active");
assert.deepEqual(await superseded, { dropped: true });
calls[0].job.resolve([11, 21, 31]);
assert.deepEqual(await first, [11, 21, 31]);
await Promise.resolve();
assert.equal(calls.length, 2, "only the latest pending request is promoted");
assert.deepEqual(calls[1].args, [11, 21, 31, 3], "pending input marshals from the prior reply");
calls[1].job.resolve([12, 22, 32]);
assert.deepEqual(await latest, [12, 22, 32]);

const stale = pump.applyFields({ movementX: 4 }, "drag");
pump.setView([100, 200, 300]);
calls[2].job.resolve([13, 23, 33]);
assert.deepEqual(await stale, { dropped: true });
assert.deepEqual(pump.getView(), [100, 200, 300], "an old worker reply cannot overwrite a newer explicit view");
pump.destroy();

console.log("Mandelbrot async live pump scheduling and stale-result safety: ok");
