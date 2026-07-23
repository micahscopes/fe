export const BENCHMARK_RESOLUTIONS = Object.freeze([128, 256, 512, 768]);

export function parseBenchmarkResolution(value) {
  if (value === null || value === "") return null;
  const resolution = Number(value);
  if (!Number.isInteger(resolution) || !BENCHMARK_RESOLUTIONS.includes(resolution)) {
    throw new TypeError(
      `resolution must be one of ${BENCHMARK_RESOLUTIONS.join("|")}`,
    );
  }
  return resolution;
}

export function createContinuousBenchmark({
  requestFrame,
  now,
  submit,
  width,
  height,
  warmupFrames = 30,
  sampleFrames = 120,
  path = "direct",
}) {
  if (typeof requestFrame !== "function" || typeof now !== "function"
      || typeof submit !== "function") {
    throw new TypeError("continuous benchmark requires frame, clock, and submit functions");
  }
  for (const [name, value] of Object.entries({
    width, height, warmupFrames, sampleFrames,
  })) {
    if (!Number.isSafeInteger(value) || value <= 0) {
      throw new TypeError(`${name} must be a positive safe integer`);
    }
  }
  if (!["direct", "actor"].includes(path)) {
    throw new TypeError("benchmark path must be direct or actor");
  }

  return Object.freeze({
    run() {
      return new Promise((resolve, reject) => {
        let frame = 0;
        const cadence = [];
        const cpu = [];
        const tick = (rafTime) => {
          const started = now();
          Promise.resolve().then(submit).then(() => {
            const submitCpuMs = Math.max(0, now() - started);
            if (frame >= warmupFrames) {
              cadence.push(rafTime);
              cpu.push(submitCpuMs);
            }
            frame += 1;
            if (frame < warmupFrames + sampleFrames) {
              requestFrame(tick);
              return;
            }
            const span = cadence.length > 1 ? cadence.at(-1) - cadence[0] : 0;
            resolve(Object.freeze({
              mode: "continuous_no_readback",
              path,
              resolution: Object.freeze({ width, height, pixels: width * height }),
              warmupFrames,
              sampleFrames,
              submittedFrameCadenceHz:
                span > 0 ? (cadence.length - 1) * 1000 / span : null,
              averageSubmitCpuMs: cpu.reduce((sum, value) => sum + value, 0) / cpu.length,
              maxSubmitCpuMs: Math.max(...cpu),
              gpuCompletionMeasured: false,
              timestampQuery: Object.freeze({
                available: false,
                reason: "not requested; benchmark performs no GPU readback",
              }),
            }));
          }, reject);
        };
        requestFrame(tick);
      });
    },
  });
}
