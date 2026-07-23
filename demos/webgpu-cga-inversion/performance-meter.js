export function createPerformanceMeter(now = () => performance.now(), windowSize = 120) {
  const frameTimes = [];
  const submitTimes = [];
  const state = {
    artifactFetchMs: null,
    gpuInitMs: null,
    firstFrameSubmitMs: null,
    initialAcceptanceMs: null,
    interaction: {
      count: 0,
      sampleCount: 0,
      cadenceHz: null,
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
    state.interaction.count += 1;
    frameTimes.push(rafTime);
    submitTimes.push(Math.max(0, submitCpuMs));
    if (frameTimes.length > windowSize) frameTimes.shift();
    if (submitTimes.length > windowSize) submitTimes.shift();
    state.interaction.sampleCount = submitTimes.length;
    state.interaction.lastSubmitCpuMs = submitTimes.at(-1);
    state.interaction.averageSubmitCpuMs =
      submitTimes.reduce((sum, value) => sum + value, 0) / submitTimes.length;
    state.interaction.maxSubmitCpuMs = Math.max(...submitTimes);
    if (frameTimes.length > 1) {
      const span = frameTimes.at(-1) - frameTimes[0];
      state.interaction.cadenceHz =
        span > 0 ? (frameTimes.length - 1) * 1000 / span : null;
    }
    return state.interaction;
  };

  return Object.freeze({ state, start, elapsed, finish, recordFrame });
}
