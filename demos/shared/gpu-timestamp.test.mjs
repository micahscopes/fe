import assert from "node:assert/strict";
import {
  decodeGpuTimestampPair,
  timestampFeaturePlan,
} from "./gpu-timestamp.js";

const available = new Set(["timestamp-query"]);
assert.deepEqual(timestampFeaturePlan(available, false), {
  requested: false,
  supported: false,
  requiredFeatures: [],
  reason: "not requested",
});
assert.deepEqual(timestampFeaturePlan(new Set(), true), {
  requested: true,
  supported: false,
  requiredFeatures: [],
  reason: "adapter does not expose timestamp-query",
});
assert.deepEqual(timestampFeaturePlan(available, true), {
  requested: true,
  supported: true,
  requiredFeatures: ["timestamp-query"],
});

const bytes = new ArrayBuffer(16);
const view = new DataView(bytes);
view.setBigUint64(0, 1_000_000n, true);
view.setBigUint64(8, 3_500_000n, true);
assert.equal(decodeGpuTimestampPair(bytes).gpuElapsedMs, 2.5);
view.setBigUint64(8, 999_999n, true);
assert.throws(() => decodeGpuTimestampPair(bytes), /end precedes begin/);
assert.throws(() => decodeGpuTimestampPair(new ArrayBuffer(8)), /two u64/);

console.log("shared GPU timestamp capability/result semantics: ok");
