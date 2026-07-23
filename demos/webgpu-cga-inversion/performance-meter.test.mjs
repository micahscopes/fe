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
  "interaction",
]);

meter.recordFrame(100, 2);
meter.recordFrame(116, 4);
meter.recordFrame(132, 3);
assert.equal(meter.state.interaction.count, 3);
assert.equal(meter.state.interaction.sampleCount, 3);
assert.equal(meter.state.interaction.cadenceHz, 62.5);
assert.equal(meter.state.interaction.lastSubmitCpuMs, 3);
assert.equal(meter.state.interaction.averageSubmitCpuMs, 3);
assert.equal(meter.state.interaction.maxSubmitCpuMs, 4);

meter.recordFrame(148, 1);
assert.equal(meter.state.interaction.count, 4);
assert.equal(meter.state.interaction.sampleCount, 3);
assert.equal(meter.state.interaction.cadenceHz, 62.5);
assert.equal(meter.state.interaction.averageSubmitCpuMs, 8 / 3);
assert.equal(meter.state.interaction.maxSubmitCpuMs, 4);

console.log("CGA performance meter: ok");
