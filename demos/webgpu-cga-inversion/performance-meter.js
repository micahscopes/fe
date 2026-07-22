export function createPerformanceMeter(now = () => performance.now(), windowSize = 120) {
  const frameTimes = [];
  const submitTimes = [];
  const state = {
    artifactFetchMs: null,
    gpuInitMs: null,
    firstFrameSubmitMs: null,
    initialAcceptanceMs: null,
    frames: {
      count: 0,
      sampleCount: 0,
      fps: null,
      lastSubmitCpuMs: null,
      averageSubmitCpuMs: null,
      maxSubmitCpuMs: null,
    },
  };

  const start = () => now();
  const elapsed = (started) => Math.max(0, now() - started);
  const finish = (field, started) => {
    const duration = elapsed(started);
    state[field] = duration;
    return duration;
  };
  const recordFrame = (rafTime, submitCpuMs) => {
    state.frames.count += 1;
    frameTimes.push(rafTime);
    submitTimes.push(Math.max(0, submitCpuMs));
    if (frameTimes.length > windowSize) frameTimes.shift();
    if (submitTimes.length > windowSize) submitTimes.shift();
    state.frames.sampleCount = submitTimes.length;
    state.frames.lastSubmitCpuMs = submitTimes.at(-1);
    state.frames.averageSubmitCpuMs =
      submitTimes.reduce((sum, value) => sum + value, 0) / submitTimes.length;
    state.frames.maxSubmitCpuMs = Math.max(...submitTimes);
    if (frameTimes.length > 1) {
      const span = frameTimes.at(-1) - frameTimes[0];
      state.frames.fps = span > 0 ? (frameTimes.length - 1) * 1000 / span : null;
    }
    return state.frames;
  };

  return Object.freeze({ state, start, elapsed, finish, recordFrame });
}
