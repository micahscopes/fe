import assert from "node:assert/strict";
import { createPerformanceMeter } from "./performance-meter.js";

const clock = [10, 18, 20, 35];
const meter = createPerformanceMeter(() => clock.shift(), 3);
const fetchStart = meter.start();
assert.equal(meter.finish("artifactFetchMs", fetchStart), 8);
const gpuStart = meter.start();
assert.equal(meter.finish("gpuInitMs", gpuStart), 15);

const frameClock = [40, 43];
const frameMeter = createPerformanceMeter(() => frameClock.shift(), 3);
const frameStart = frameMeter.start();
assert.equal(frameMeter.elapsed(frameStart), 3);
assert.deepEqual(Object.keys(frameMeter.state), [
  "artifactFetchMs",
  "gpuInitMs",
  "firstFrameSubmitMs",
  "initialAcceptanceMs",
  "frames",
]);

meter.recordFrame(100, 2);
meter.recordFrame(116, 4);
meter.recordFrame(132, 3);
assert.equal(meter.state.frames.count, 3);
assert.equal(meter.state.frames.sampleCount, 3);
assert.equal(meter.state.frames.fps, 62.5);
assert.equal(meter.state.frames.lastSubmitCpuMs, 3);
assert.equal(meter.state.frames.averageSubmitCpuMs, 3);
assert.equal(meter.state.frames.maxSubmitCpuMs, 4);

meter.recordFrame(148, 1);
assert.equal(meter.state.frames.count, 4);
assert.equal(meter.state.frames.sampleCount, 3);
assert.equal(meter.state.frames.fps, 62.5);
assert.equal(meter.state.frames.averageSubmitCpuMs, 8 / 3);
assert.equal(meter.state.frames.maxSubmitCpuMs, 4);

console.log("CGA performance meter: ok");
