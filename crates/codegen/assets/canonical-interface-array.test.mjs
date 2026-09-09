import assert from "node:assert/strict";
import test from "node:test";
import { compileCanonicalInterfaceManifest, compileCanonicalActorMailbox,
  compileCanonicalActorAdapter } from "./canonical-interface.js";

const u32 = { kind: "u32", size: 4, align: 4 };
const array = (element, len) => ({ kind: "array", element, len,
  stride: element.size, size: element.size * len, align: element.align });
const draw = { kind: "record", size: 8, align: 4, fields: [
  { name: "begin", offset: 0, layout: u32 },
  { name: "end", offset: 4, layout: u32 },
] };
const manifest = layout => ({
  protocol: "fe-canonical-browser-interface", version: 4,
  abi: { alloc_export: "fe_cabi_alloc", endianness: "little", memory_export: "memory",
    pointer_width: 32, reset_export: "fe_cabi_reset" },
  lanes: [{ name: "update", export: "fe_cabi_update",
    intent: { capabilities: [], execution: "wasm", placement: "any" },
    request: layout, response: layout }],
});

test("fixed record arrays are inline, copied, and exact length", () => {
  const codec = compileCanonicalInterfaceManifest(manifest(array(draw, 16))).lanes.update.request;
  const input = Array.from({ length: 16 }, (_, i) => ({ begin: i * 3, end: i * 3 + 3 }));
  const memory = new Uint8Array(128);
  const options = { memory, offset: 0, allocate() { assert.fail("inline arrays do not allocate"); } };
  codec.write(input, options);
  assert.equal(new DataView(memory.buffer).getUint32(120, true), 45);
  const output = codec.read(options);
  assert.deepEqual(output, input);
  memory.fill(0);
  assert.deepEqual(output, input);
  for (const bad of [input.slice(1), [...input, input[0]], new Array(16), { ...input }]) {
    assert.throws(() => codec.write(bad, options), /fixed array|missing/);
  }
  assert.throws(() => codec.write(Object.assign([...input], { extra: 1 }), options), /fixed array/);
  assert.throws(() => codec.read({ memory: new Uint8Array(127), offset: 0 }), /outside canonical memory/);
  assert.throws(() => codec.write(input, { memory: new Uint8Array(127), offset: 0 }), /outside canonical memory/);
});

test("nested and empty fixed arrays roundtrip without descriptors", () => {
  for (const [layout, value] of [[array(array(u32, 2), 2), [[1, 2], [3, 4]]],
    [array(draw, 0), []]]) {
    const codec = compileCanonicalInterfaceManifest(manifest(layout)).lanes.update.request;
    const options = { memory: new Uint8Array(layout.size), offset: 0 };
    codec.write(value, options);
    assert.deepEqual(codec.read(options), value);
  }
});

test("array layout validates stride, size, alignment, and expanded node budget", () => {
  for (const patch of [{ stride: 4 }, { size: 15 }, { align: 8 }, { len: -1 }]) {
    assert.throws(() => compileCanonicalInterfaceManifest(manifest({ ...array(draw, 2), ...patch })));
  }
  assert.throws(() => compileCanonicalInterfaceManifest(manifest(array(array(u32, 64), 64))), /node count/);
});

test("mailbox record arrays retain element order", () => {
  const lane = compileCanonicalActorMailbox(manifest(array(draw, 2))).update;
  assert.deepEqual(lane.liftRequest([1, 2, 3, 4]), [{ begin: 1, end: 2 }, { begin: 3, end: 4 }]);
  assert.throws(() => lane.liftRequest([1, 2, 3]), /carrier|width|length/);
  const response = lane.createResponseSession();
  assert.deepEqual(response.lower([{ begin: 5, end: 6 }, { begin: 7, end: 8 }]), [5, 6, 7, 8]);
  assert.throws(() => lane.createResponseSession().lower([{ begin: 5, end: 6 }]), /fixed array/);
});

test("arrays in variant payloads canonicalize inactive mailbox lanes", () => {
  const layout = { kind: "variant", size: 12, align: 4, tag_offset: 0, variants: [
    { name: "none", tag: 0, fields: [] },
    { name: "some", tag: 1, fields: [{ name: "values", offset: 4, layout: array(u32, 2) }] },
  ] };
  compileCanonicalInterfaceManifest(manifest(layout));
  const lane = compileCanonicalActorMailbox(manifest(layout)).update;
  assert.deepEqual(lane.liftRequest([1, 9, 10]), { tag: "some", values: [9, 10] });
  assert.deepEqual(lane.liftRequest([0, 0, 0]), { tag: "none" });
  assert.throws(() => lane.liftRequest([0, 0, 1]), /inactive lane/);
  assert.deepEqual(lane.createResponseSession().lower({ tag: "none" }), [0, 0, 0]);
  assert.deepEqual(lane.createResponseSession().lower({ tag: "some", values: [9, 10] }), [1, 9, 10]);
});

test("fixed arrays recursively transfer descriptor ownership", () => {
  const bytes = { kind: "bytes", size: 8, align: 4, pointer_offset: 0, length_offset: 4 };
  const source = manifest(array(bytes, 2));
  const adapter = compileCanonicalActorAdapter(source, compileCanonicalInterfaceManifest(source));
  const a = new Uint8Array([1]), b = new Uint8Array([2]);
  assert.deepEqual(adapter.transferRequest([a, b], { lane: "update" }), [a.buffer, b.buffer]);
  assert.deepEqual(adapter.transferRequest([a, a], { lane: "update" }), [a.buffer]);
});
