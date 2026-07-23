import assert from "node:assert/strict";
import {
  BENCHMARK_RESOLUTIONS,
  createContinuousBenchmark,
  gpuTimingUnavailableResult,
  parseBenchmarkResolution,
  parseBenchmarkTiming,
} from "./continuous-benchmark.js";

assert.deepEqual(BENCHMARK_RESOLUTIONS, [128, 256, 512, 768]);
assert.equal(parseBenchmarkResolution(null), null);
assert.equal(parseBenchmarkResolution("256"), 256);
for (const bad of ["127", "129", "1024", "256.5", "garbage"]) {
  assert.throws(() => parseBenchmarkResolution(bad), /resolution must be one of/);
}
assert.equal(parseBenchmarkTiming(null), "submit");
assert.equal(parseBenchmarkTiming("gpu"), "gpu");
assert.throws(() => parseBenchmarkTiming("cpu"), /timing must be gpu/);
assert.deepEqual(gpuTimingUnavailableResult({
  width: 256,
  height: 256,
  reason: "adapter does not expose timestamp-query",
}), {
  mode: "gpu_timestamp_unsupported",
  path: "direct",
  resolution: { width: 256, height: 256, pixels: 65536 },
  gpuCompletionMeasured: false,
  timestampQuery: {
    available: false,
    reason: "adapter does not expose timestamp-query",
  },
});

async function run(path, submit) {
  let raf = 0;
  let clock = 0;
  const benchmark = createContinuousBenchmark({
    requestFrame(callback) {
      raf += 16;
      queueMicrotask(() => callback(raf));
    },
    now() {
      return clock;
    },
    submit() {
      clock += path === "direct" ? 0.25 : 0.75;
      return submit();
    },
    width: 256,
    height: 256,
    warmupFrames: 2,
    sampleFrames: 4,
    path,
  });
  return benchmark.run();
}

const direct = await run("direct", () => undefined);
const actor = await run("actor", () => Promise.resolve());
assert.equal(direct.submittedFrameCadenceHz, 62.5);
assert.equal(direct.averageSubmitCpuMs, 0.25);
assert.equal(actor.averageSubmitCpuMs, 0.75);
assert.equal(direct.resolution.pixels, 256 * 256);
assert.equal(direct.gpuCompletionMeasured, false);
assert.equal(direct.timestampQuery.available, false);
assert.equal(actor.path, "actor");

let gpuRaf = 0;
const gpu = await createContinuousBenchmark({
  requestFrame(callback) {
    gpuRaf += 20;
    queueMicrotask(() => callback(gpuRaf));
  },
  now: () => 0,
  submit: async () => ({ cpuSubmitMs: 0.1, gpuElapsedMs: 2.5 }),
  width: 256,
  height: 256,
  warmupFrames: 1,
  sampleFrames: 3,
  timing: "gpu",
}).run();
assert.equal(gpu.mode, "continuous_gpu_timestamp");
assert.equal(gpu.gpuCompletionMeasured, true);
assert.equal(gpu.submittedFrameCadenceHz, null);
assert.equal(gpu.completedFrameCadenceHz, 50);
assert.equal(gpu.timestampQuery.available, true);
assert.equal(gpu.timestampQuery.unit, "nanoseconds");
assert.equal(gpu.timestampQuery.averageGpuElapsedMs, 2.5);
assert.equal(gpu.timestampQuery.samples, 3);

console.log("CGA continuous no-readback benchmark: ok");
