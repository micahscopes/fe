export const GPU_TIMESTAMP_FEATURE = "timestamp-query";

export function timestampFeaturePlan(features, requested) {
  const supported = Boolean(requested && features?.has?.(GPU_TIMESTAMP_FEATURE));
  return Object.freeze({
    requested: Boolean(requested),
    supported,
    requiredFeatures: Object.freeze(supported ? [GPU_TIMESTAMP_FEATURE] : []),
    ...(!requested
      ? { reason: "not requested" }
      : !supported
        ? { reason: "adapter does not expose timestamp-query" }
        : {}),
  });
}

export function decodeGpuTimestampPair(bytes) {
  if (!(bytes instanceof ArrayBuffer) || bytes.byteLength < 16) {
    throw new TypeError("GPU timestamp result must contain two u64 values");
  }
  const view = new DataView(bytes);
  const begin = view.getBigUint64(0, true);
  const end = view.getBigUint64(8, true);
  if (end < begin) throw new RangeError("GPU timestamp end precedes begin");
  const elapsedNanoseconds = end - begin;
  return Object.freeze({
    begin,
    end,
    elapsedNanoseconds,
    gpuElapsedMs: Number(elapsedNanoseconds) / 1_000_000,
  });
}
