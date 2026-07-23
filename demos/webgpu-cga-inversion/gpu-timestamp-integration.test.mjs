import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

const main = readFileSync(new URL("./main.js", import.meta.url), "utf8");
const runner = readFileSync(
  new URL("../webgpu-keystone/webgpu-runner.js", import.meta.url),
  "utf8",
);

assert.match(main, /timing=gpu requires benchmark=continuous/);
assert.match(
  main,
  /timestampQuery: continuousBenchmark && benchmarkTiming === "gpu"/,
);
assert.match(main, /gpuTimingUnavailableResult/);
assert.match(main, /presentationEvidence\.gpuTimestampReadbackCount \+= 1/);

const ordinaryStart = runner.indexOf("export function renderFrame(");
const timedStart = runner.indexOf("export async function renderFrameGpuTimed(");
const ordinaryEnd = runner.indexOf("\n}\n", ordinaryStart) + 3;
const ordinaryRender = runner.slice(ordinaryStart, ordinaryEnd);
assert.doesNotMatch(ordinaryRender, /timestamp|mapAsync|resolveQuerySet/);

const timedEnd = runner.indexOf(
  "// Submit one offscreen frame",
  timedStart,
);
const timedRender = runner.slice(timedStart, timedEnd);
assert.match(timedRender, /timestampWrites/);
assert.match(timedRender, /resolveQuerySet/);
assert.match(timedRender, /mapAsync/);

console.log("CGA opt-in GPU timestamp integration: ok");
