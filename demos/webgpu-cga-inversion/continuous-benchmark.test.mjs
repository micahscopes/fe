import assert from "node:assert/strict";
import {
  BENCHMARK_RESOLUTIONS,
  createContinuousBenchmark,
  parseBenchmarkResolution,
} from "./continuous-benchmark.js";

assert.deepEqual(BENCHMARK_RESOLUTIONS, [128, 256, 512, 768]);
assert.equal(parseBenchmarkResolution(null), null);
assert.equal(parseBenchmarkResolution("256"), 256);
for (const bad of ["127", "129", "1024", "256.5", "garbage"]) {
  assert.throws(() => parseBenchmarkResolution(bad), /resolution must be one of/);
}

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

console.log("CGA continuous no-readback benchmark: ok");
