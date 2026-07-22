import assert from "node:assert/strict";
import {
  encodeShaderScalar,
  isShaderParamOffset,
  validateShaderParamArity,
} from "./webgpu-runner.js";

function bytes(view) {
  return [...new Uint8Array(view.buffer, view.byteOffset, view.byteLength)];
}

const fractional = encodeShaderScalar(0.0125, { scalar: "F32", width: 4 });
assert.ok(fractional instanceof Float32Array);
assert.deepEqual(bytes(fractional), [205, 204, 76, 60]);
assert.equal(fractional[0], Math.fround(0.0125));

const signed = encodeShaderScalar(-123, { scalar: "I32", width: 4 });
assert.ok(signed instanceof Int32Array);
assert.deepEqual(bytes(signed), [133, 255, 255, 255]);

const unsigned = encodeShaderScalar(0xfedcba98, { scalar: "U32", width: 4 });
assert.ok(unsigned instanceof Uint32Array);
assert.deepEqual(bytes(unsigned), [152, 186, 220, 254]);

const legacy = encodeShaderScalar(-7, { width: 4 });
assert.ok(legacy instanceof Int32Array, "missing scalar must preserve the legacy I32 default");
assert.deepEqual(bytes(legacy), [249, 255, 255, 255]);

for (const offset of [0, 4, 12]) assert.equal(isShaderParamOffset(offset), true);
for (const offset of [-4, 2, 1.5, NaN, "4"]) assert.equal(isShaderParamOffset(offset), false);

assert.doesNotThrow(() => validateShaderParamArity([{}, {}], [1, 2]));
assert.throws(
  () => validateShaderParamArity([{}], [1, 2]),
  /layout names 1 parameters but caller supplied 2/,
);
assert.throws(() => encodeShaderScalar(1, { scalar: "F32", width: 8 }), /width 8/);
assert.throws(() => encodeShaderScalar(1, { scalar: "I1", width: 4 }), /unsupported.*I1/);

console.log("typed shader parameter transport: ok");
