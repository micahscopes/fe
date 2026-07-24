import assert from "node:assert/strict";
import test from "node:test";
import {
  compileCanonicalActorAdapter,
  compileCanonicalInterfaceManifest,
} from "./canonical-interface.js";

const list = (element, max) => ({
  align: 4,
  element,
  kind: "list",
  length_offset: 4,
  max,
  pointer_offset: 0,
  size: 8,
  stride: 4,
});

const manifest = (request = list("u32", 4), response = list("f32", 4)) => ({
  abi: {
    alloc_export: "fe_cabi_alloc",
    endianness: "little",
    memory_export: "memory",
    pointer_width: 32,
    reset_export: "fe_cabi_reset",
  },
  lanes: [{
    export: "fe_cabi_update",
    intent: { capabilities: [], execution: "wasm", placement: "any" },
    name: "update",
    request,
    response,
  }],
  protocol: "fe-canonical-browser-interface",
  version: 4,
});

test("bounded list codecs are typed, little-endian, copied, and bounded", () => {
  const compiled = compileCanonicalInterfaceManifest(manifest());
  const codec = compiled.lanes.update.request;
  let memory = new Uint8Array(64);
  let cursor = 8;
  const allocate = (size, align) => {
    cursor = Math.ceil(cursor / align) * align;
    const result = cursor;
    cursor += size;
    return result;
  };
  const input = new Uint32Array([0x01020304, 0xffffffff]);
  codec.write(input, { memory: () => memory, offset: 0, allocate });
  assert.deepEqual([...memory.slice(8, 16)], [4, 3, 2, 1, 255, 255, 255, 255]);
  const output = codec.read({ memory, offset: 0 });
  assert(output instanceof Uint32Array);
  assert.deepEqual([...output], [...input]);
  memory.fill(0);
  assert.deepEqual([...output], [...input], "decoded result owns a copy");
  assert.throws(
    () => codec.write(new Float32Array(1), { memory, offset: 0, allocate }),
    /Uint32Array/,
  );
  assert.throws(
    () => codec.write(new Uint32Array(5), { memory, offset: 0, allocate }),
    /maximum length 4/,
  );
});

test("f32 lists preserve values, signed zero, and NaN classification", () => {
  const codec = compileCanonicalInterfaceManifest(
    manifest(list("f32", 4), list("f32", 4)),
  ).lanes.update.request;
  const memory = new Uint8Array(64);
  let cursor = 8;
  const value = new Float32Array([1.25, -0, Number.NaN]);
  codec.write(value, {
    memory,
    offset: 0,
    allocate(size, align) {
      cursor = Math.ceil(cursor / align) * align;
      const result = cursor;
      cursor += size;
      return result;
    },
  });
  const result = codec.read({ memory, offset: 0 });
  assert.equal(result[0], 1.25);
  assert(Object.is(result[1], -0));
  assert(Number.isNaN(result[2]));
});

test("zero-capacity lists use ptr=0,len=0 and ignore the decoded pointer", () => {
  const codec = compileCanonicalInterfaceManifest(
    manifest(list("u32", 0), list("u32", 0)),
  ).lanes.update.request;
  const memory = new Uint8Array(16);
  codec.write(new Uint32Array(0), {
    memory,
    offset: 0,
    allocate: () => { throw new Error("empty list must not allocate"); },
  });
  assert.deepEqual([...memory.slice(0, 8)], [0, 0, 0, 0, 0, 0, 0, 0]);
  new DataView(memory.buffer).setUint32(0, 3, true);
  assert.equal(codec.read({ memory, offset: 0 }).length, 0);
});

test("list decode rejects oversized, unaligned, and out-of-bounds payloads", () => {
  const codec = compileCanonicalInterfaceManifest(manifest()).lanes.update.request;
  const memory = new Uint8Array(32);
  const descriptor = new DataView(memory.buffer);
  descriptor.setUint32(0, 8, true);
  descriptor.setUint32(4, 5, true);
  assert.throws(() => codec.read({ memory, offset: 0 }), /maximum length 4/);
  descriptor.setUint32(0, 3, true);
  descriptor.setUint32(4, 1, true);
  assert.throws(() => codec.read({ memory, offset: 0 }), /misaligned/);
  descriptor.setUint32(0, 28, true);
  descriptor.setUint32(4, 2, true);
  assert.throws(() => codec.read({ memory, offset: 0 }), /outside canonical memory/);
});

test("actor transfer recursively transfers only owned full-span typed arrays", () => {
  const request = {
    align: 4,
    fields: [{ layout: list("u32", 4), name: "values", offset: 0 }],
    kind: "record",
    size: 8,
  };
  const source = manifest(request, list("f32", 4));
  const compiled = compileCanonicalInterfaceManifest(source);
  const actor = compileCanonicalActorAdapter(source, compiled);
  const values = new Uint32Array([1, 2]);
  assert.deepEqual(actor.transferRequest({ values }, { lane: "update" }), [values.buffer]);
  const backing = new Uint32Array(4);
  assert.throws(
    () => actor.transferRequest({ values: backing.subarray(1, 3) }, { lane: "update" }),
    /owned full-span Uint32Array/,
  );
  const response = new Float32Array([1, 2]);
  const transfers = actor.transferResult(response, { lane: "update" });
  const clone = structuredClone(response, { transfer: transfers });
  assert.deepEqual([...clone], [1, 2]);
  assert.equal(response.byteLength, 0, "MessagePort transfer detaches sender ownership");
  assert.throws(
    () => actor.transferResult(new Uint32Array(1), { lane: "update" }),
    /owned full-span Float32Array/,
  );
});

test("recursive record and active variant traversal deduplicates shared list buffers", () => {
  const nestedList = list("u32", 4);
  const variant = {
    align: 4,
    kind: "variant",
    size: 12,
    tag_offset: 0,
    variants: [
      { fields: [], name: "none", tag: 0 },
      {
        fields: [{ layout: nestedList, name: "items", offset: 4 }],
        name: "some",
        tag: 1,
      },
    ],
  };
  const request = {
    align: 4,
    fields: [
      { layout: nestedList, name: "first", offset: 0 },
      { layout: variant, name: "nested", offset: 8 },
    ],
    kind: "record",
    size: 20,
  };
  const source = {
    ...manifest(request, list("f32", 1)),
    lanes: [{
      export: null,
      intent: { capabilities: [], execution: "host_effect", placement: "worker" },
      name: "update",
      request,
      response: list("f32", 1),
    }],
  };
  const compiled = compileCanonicalInterfaceManifest(source);
  const actor = compileCanonicalActorAdapter(source, compiled);
  const buffer = new ArrayBuffer(8);
  const value = new Uint32Array(buffer);
  const alias = new Uint32Array(buffer);
  assert.deepEqual(
    actor.transferRequest(
      { first: value, nested: { tag: "some", items: alias } },
      { lane: "update" },
    ),
    [buffer],
  );
});

test("manifest rejects unsafe list vocabulary", () => {
  assert.throws(
    () => compileCanonicalInterfaceManifest(manifest(list("u8", 4))),
    /non-canonical bounded list/,
  );
  assert.throws(
    () => compileCanonicalInterfaceManifest(manifest(list("u32", 0x40000000))),
    /non-canonical bounded list/,
  );
});
