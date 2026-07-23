import assert from "node:assert/strict";
import {
  CANONICAL_INTERFACE_PROTOCOL,
  CANONICAL_INTERFACE_VERSION,
  compileCanonicalInterfaceManifest,
} from "./canonical-interface.js";

const scalar = (kind, size, align) => ({ kind, size, align });
const descriptor = (kind) => ({
  kind, size: 8, align: 4, pointer_offset: 0, length_offset: 4,
  ...(kind === "string" ? { encoding: "utf-8" } : {}),
});
const manifest = {
  protocol: CANONICAL_INTERFACE_PROTOCOL,
  version: CANONICAL_INTERFACE_VERSION,
  abi: {
    pointer_width: 32, endianness: "little", memory_export: "memory",
    alloc_export: "fe_cabi_alloc", reset_export: "fe_cabi_reset",
  },
  lanes: [{
    name: "echo",
    export: "echo_message",
    request: {
      kind: "record", size: 32, align: 8,
      fields: [
        { name: "tag", offset: 0, layout: scalar("u8", 1, 1) },
        { name: "sequence", offset: 8, layout: scalar("u64", 8, 8) },
        { name: "text", offset: 16, layout: descriptor("string") },
        { name: "payload", offset: 24, layout: descriptor("bytes") },
      ],
    },
    response: descriptor("bytes"),
  }],
};

const compiled = compileCanonicalInterfaceManifest(structuredClone(manifest));
const memory = new Uint8Array(256);
let cursor = 64;
const allocate = (length, align) => {
  cursor = Math.ceil(cursor / align) * align;
  const result = cursor;
  cursor += length;
  return result;
};
compiled.lanes.echo.request.write({
  tag: 7, sequence: 0x0102030405060708n, text: "Fe λ 🚀",
  payload: new Uint8Array([9, 8, 7]),
}, { memory, offset: 0, allocate });
assert.deepEqual(compiled.lanes.echo.request.read({ memory, offset: 0 }), {
  tag: 7, sequence: 0x0102030405060708n, text: "Fe λ 🚀",
  payload: new Uint8Array([9, 8, 7]),
});

const nestedManifest = structuredClone(manifest);
nestedManifest.lanes[0].request = {
  kind: "record", size: 8, align: 4, fields: [
    {
      name: "inner", offset: 0, layout: {
        kind: "record", size: 4, align: 4,
        fields: [{ name: "value", offset: 0, layout: scalar("i32", 4, 4) }],
      },
    },
    { name: "tail", offset: 4, layout: scalar("bool", 1, 1) },
  ],
};
const nested = compileCanonicalInterfaceManifest(nestedManifest);
nested.lanes.echo.request.write(
  { inner: { value: -42 }, tail: true },
  { memory, offset: 0 },
);
assert.deepEqual(nested.lanes.echo.request.read({ memory, offset: 0 }), {
  inner: { value: -42 }, tail: true,
});

const responseSource = new Uint8Array([1, 2, 3, 4]);
compiled.lanes.echo.response.write(responseSource, { memory, offset: 40, allocate });
const response = compiled.lanes.echo.response.read({ memory, offset: 40 });
memory.fill(99);
assert.deepEqual(response, new Uint8Array([1, 2, 3, 4]),
  "decoded owned bytes must not alias Wasm memory");

const invalidUtf8Memory = new Uint8Array(32);
new DataView(invalidUtf8Memory.buffer).setUint32(12, 2, true);
invalidUtf8Memory.set([0xc3, 0x28], 0);
const stringManifest = structuredClone(manifest);
stringManifest.lanes[0].response = descriptor("string");
const stringCodec = compileCanonicalInterfaceManifest(stringManifest);
assert.throws(
  () => stringCodec.lanes.echo.response.read({ memory: invalidUtf8Memory, offset: 8 }),
  /encoded data was not valid|invalid byte sequence|UTF-8/i,
);

assert.throws(
  () => compiled.lanes.echo.request.read({ memory: new Uint8Array(8), offset: 0 }),
  /outside canonical memory/,
);
assert.throws(
  () => compiled.lanes.echo.request.write({
    tag: 1, sequence: 1n, text: "x", payload: new Uint8Array(),
  }, { memory: new Uint8Array(64), offset: 0 }),
  /allocate/,
);
assert.throws(
  () => compiled.lanes.echo.request.write({
    tag: 1, sequence: 1n, text: "", payload: new Uint8Array(), surplus: true,
  }, { memory: new Uint8Array(64), offset: 0 }),
  /unexpected or missing fields/,
);
assert.throws(
  () => compiled.lanes.echo.request.write({
    tag: 1, sequence: 1n << 64n, text: "", payload: new Uint8Array(),
  }, { memory: new Uint8Array(64), offset: 0 }),
  /u64 bigint/,
);

for (const mutate of [
  (value) => { value.version = 2; },
  (value) => { value.extra = true; },
  (value) => { value.abi.pointer_width = 64; },
  (value) => { value.lanes[0].request.fields[1].offset = 4; },
  (value) => { value.lanes[0].request.size = 31; },
  (value) => { value.lanes[0].response.length_offset = 0; },
]) {
  const bad = structuredClone(manifest);
  mutate(bad);
  assert.throws(() => compileCanonicalInterfaceManifest(bad));
}

const reservedObjectName = structuredClone(manifest);
reservedObjectName.lanes[0].name = "__proto__";
assert.throws(
  () => compileCanonicalInterfaceManifest(reservedObjectName),
  /lowercase ASCII identifier/,
);

const oversizedType = structuredClone(manifest);
oversizedType.lanes[0].request = {
  kind: "record",
  size: 4096,
  align: 1,
  fields: Array.from({ length: 4096 }, (_, index) => ({
    name: `f${index}`,
    offset: index,
    layout: scalar("u8", 1, 1),
  })),
};
assert.throws(
  () => compileCanonicalInterfaceManifest(oversizedType),
  /maximum type node count/,
);

console.log("canonical interface v1 strict manifest and memory codecs: ok");
