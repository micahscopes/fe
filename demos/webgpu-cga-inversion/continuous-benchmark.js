export const BENCHMARK_RESOLUTIONS = Object.freeze([128, 256, 512, 768]);

export function parseBenchmarkTiming(value) {
  if (value === null || value === "") return "submit";
  if (value !== "gpu") throw new TypeError("timing must be gpu");
  return value;
}

export function gpuTimingUnavailableResult({ width, height, path = "direct", reason }) {
  if (!Number.isSafeInteger(width) || width <= 0
      || !Number.isSafeInteger(height) || height <= 0) {
    throw new TypeError("GPU timing resolution must be positive safe integers");
  }
  if (typeof reason !== "string" || reason.length === 0) {
    throw new TypeError("GPU timing unavailability requires a reason");
  }
  return Object.freeze({
    mode: "gpu_timestamp_unsupported",
    path,
    resolution: Object.freeze({ width, height, pixels: width * height }),
    gpuCompletionMeasured: false,
    timestampQuery: Object.freeze({ available: false, reason }),
  });
}

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
  timing = "submit",
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
  if (!["submit", "gpu"].includes(timing)) {
    throw new TypeError("benchmark timing must be submit or gpu");
  }

  return Object.freeze({
    run() {
      return new Promise((resolve, reject) => {
        let frame = 0;
        const cadence = [];
        const cpu = [];
        const gpu = [];
        const tick = (rafTime) => {
          const started = now();
          Promise.resolve().then(submit).then((measurement) => {
            const submitCpuMs = timing === "gpu"
              ? measurement?.cpuSubmitMs
              : Math.max(0, now() - started);
            if (!Number.isFinite(submitCpuMs) || submitCpuMs < 0) {
              throw new TypeError("benchmark submit must report a non-negative cpuSubmitMs");
            }
            if (timing === "gpu") {
              if (!Number.isFinite(measurement?.gpuElapsedMs)
                  || measurement.gpuElapsedMs < 0) {
                throw new TypeError(
                  "GPU timestamp benchmark submit must report a non-negative gpuElapsedMs",
                );
              }
            }
            if (frame >= warmupFrames) {
              cadence.push(rafTime);
              cpu.push(submitCpuMs);
              if (timing === "gpu") gpu.push(measurement.gpuElapsedMs);
            }
            frame += 1;
            if (frame < warmupFrames + sampleFrames) {
              requestFrame(tick);
              return;
            }
            const span = cadence.length > 1 ? cadence.at(-1) - cadence[0] : 0;
            const cadenceHz =
              span > 0 ? (cadence.length - 1) * 1000 / span : null;
            const gpuTiming = timing === "gpu";
            resolve(Object.freeze({
              mode: gpuTiming ? "continuous_gpu_timestamp" : "continuous_no_readback",
              path,
              resolution: Object.freeze({ width, height, pixels: width * height }),
              warmupFrames,
              sampleFrames,
              submittedFrameCadenceHz: gpuTiming ? null : cadenceHz,
              completedFrameCadenceHz: gpuTiming ? cadenceHz : null,
              averageSubmitCpuMs: cpu.reduce((sum, value) => sum + value, 0) / cpu.length,
              maxSubmitCpuMs: Math.max(...cpu),
              gpuCompletionMeasured: gpuTiming,
              timestampQuery: Object.freeze({
                available: gpuTiming,
                ...(gpuTiming
                  ? {
                      unit: "nanoseconds",
                      averageGpuElapsedMs:
                        gpu.reduce((sum, value) => sum + value, 0) / gpu.length,
                      minGpuElapsedMs: Math.min(...gpu),
                      maxGpuElapsedMs: Math.max(...gpu),
                      samples: gpu.length,
                    }
                  : {
                      reason: "not requested; benchmark performs no GPU readback",
                    }),
              }),
            }));
          }).catch(reject);
        };
        requestFrame(tick);
      });
    },
  });
}
