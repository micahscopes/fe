import assert from "node:assert/strict";
import { MessageChannel } from "node:worker_threads";
import {
  CANONICAL_INTERFACE_PROTOCOL,
  CANONICAL_INTERFACE_VERSION,
  compileCanonicalInterfaceManifest,
  createCanonicalInterfaceCaller,
} from "./canonical-interface.js";
import * as compilerOwnedCodec from "../../crates/codegen/assets/canonical-interface.js";
import {
  attachMessagePortActorHost,
  createMessagePortActorTransport,
} from "../../crates/codegen/assets/browser-runtime/message-port-actor.js";
import {
  actorEnvelope,
} from "../../crates/codegen/assets/browser-runtime/actor-coordinator.js";

assert.equal(
  compilerOwnedCodec.compileCanonicalInterfaceManifest,
  compileCanonicalInterfaceManifest,
  "demo compatibility module must re-export the compiler-owned codec",
);

const {
  compileCanonicalActorAdapter,
  createCanonicalActorAdapter,
  createCanonicalHostEffectAdapter,
} = compilerOwnedCodec;

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
    intent: { execution: "wasm", placement: "any", capabilities: [] },
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

const actorCompiled = compileCanonicalInterfaceManifest(structuredClone(manifest));
const actorMemory = new WebAssembly.Memory({ initial: 1 });
let actorCursor = 128;
let actorResetCount = 0;
const actorExports = {
  memory: actorMemory,
  fe_cabi_alloc(length, align) {
    actorCursor = Math.ceil(actorCursor / align) * align;
    const result = actorCursor;
    actorCursor += length;
    return result;
  },
  fe_cabi_reset() {
    actorResetCount += 1;
    actorCursor = 128;
  },
  echo_message(requestPointer) {
    const bytes = actorCompiled.lanes.echo.request.read({
      memory: () => new Uint8Array(actorMemory.buffer),
      offset: requestPointer,
    }).payload;
    const responsePointer = actorExports.fe_cabi_alloc(8, 4);
    actorCompiled.lanes.echo.response.write(bytes, {
      memory: () => new Uint8Array(actorMemory.buffer),
      offset: responsePointer,
      allocate: actorExports.fe_cabi_alloc,
    });
    return responsePointer;
  },
};
const actorShape = compileCanonicalActorAdapter(manifest, actorCompiled);
assert.deepEqual(Object.keys(actorShape.requestSchema), ["echo"]);
assert.throws(
  () => actorShape.requestSchema.echo({ nope: true }),
  /FE_ACTOR_INVALID_PAYLOAD/,
);
const actor = createCanonicalActorAdapter(manifest, actorCompiled, actorExports);
const actorRequest = (payload) => ({
  lane: "echo",
  payload: { tag: 1, sequence: 2n, text: "actor", payload },
});
const ownedRequest = actorRequest(new Uint8Array([9]));
assert.deepEqual(
  actorShape.transferRequest(ownedRequest.payload, ownedRequest),
  [ownedRequest.payload.payload.buffer],
);
assert.throws(
  () => actorShape.transferRequest(
    actorRequest(new Uint8Array(new ArrayBuffer(4), 1, 1)).payload,
    { lane: "echo" },
  ),
  /FE_ACTOR_TRANSFER/,
);
const first = actor.dispatch(actorRequest(new Uint8Array([1])));
const displaced = actor.dispatch(actorRequest(new Uint8Array([2])));
const newest = actor.dispatch(actorRequest(new Uint8Array([3])));
await assert.rejects(displaced, /FE_ACTOR_SUPERSEDED/);
assert.deepEqual(await first, new Uint8Array([1]));
const newestResult = await newest;
assert.deepEqual(newestResult, new Uint8Array([3]));
actor.resultSchema.echo({ ok: true, value: newestResult });
actor.resultSchema.echo({ ok: false, error: "FE_ACTOR_BUSY: busy" });
assert.throws(
  () => actor.resultSchema.echo({ ok: true, value: newestResult, extra: true }),
  /FE_ACTOR_INVALID_RESULT/,
);
assert.equal(actorResetCount, 2);
const transfer = actor.transferResult(newestResult, { lane: "echo" });
assert.deepEqual(transfer, [newestResult.buffer]);
assert.notEqual(transfer[0], actorMemory.buffer, "live Wasm memory must never transfer");

const channel = new MessageChannel();
attachMessagePortActorHost(channel.port2, (request) => actor.dispatch(request), {
  transferResult: actor.transferResult,
});
const workerTransport = createMessagePortActorTransport(channel.port1, {
  transferRequest: actorShape.transferRequest,
});
const workerPayload = new Uint8Array([4, 5, 6]);
const workerRequest = actorEnvelope({
  type: "request",
  lane: "echo",
  actorEpoch: 0,
  generation: 1,
  requestId: 1,
  payload: {
    tag: 1,
    sequence: 2n,
    text: "worker",
    payload: workerPayload,
  },
});
const workerReply = new Promise((resolve) => workerTransport.send(workerRequest, resolve));
assert.equal(workerPayload.byteLength, 0, "owned request bytes must detach at Worker transfer");
const resetBeforeWorkerReply = actorResetCount;
const reply = await workerReply;
assert.equal(reply.payload.ok, true);
assert.deepEqual(reply.payload.value, new Uint8Array([4, 5, 6]));
assert.equal(
  actorResetCount,
  resetBeforeWorkerReply + 1,
  "Wasm arena must reset before the owned response crosses the MessagePort",
);
assert.equal(
  reply.payload.value.byteLength,
  reply.payload.value.buffer.byteLength,
  "Worker result must remain a transferable full-span Uint8Array",
);
assert.notEqual(reply.payload.value.buffer, actorMemory.buffer);
workerTransport.close();
channel.port2.close();

await assert.rejects(
  actor.dispatch({ lane: "missing", payload: {} }),
  /FE_ACTOR_UNKNOWN_LANE: unknown canonical actor lane/,
);

const hostManifest = structuredClone(manifest);
hostManifest.lanes[0].export = null;
hostManifest.lanes[0].intent = {
  execution: "host_effect",
  placement: "main_thread",
  capabilities: [{ capability: "webgpu_dispatch", mutable: true }],
};
hostManifest.lanes.push({
  ...structuredClone(hostManifest.lanes[0]),
  name: "gpu_submit",
  intent: {
    execution: "host_effect",
    placement: "worker",
    capabilities: [{ capability: "webgpu_dispatch", mutable: true }],
  },
});
const hostCompiled = compileCanonicalInterfaceManifest(hostManifest);
const hostRequest = (tag) => ({
  tag, sequence: BigInt(tag), text: `host ${tag}`,
  payload: new Uint8Array([tag]),
});
assert.throws(
  () => createCanonicalHostEffectAdapter(hostManifest, hostCompiled, null),
  /canonical host-effect handlers must be an object/,
);
assert.throws(
  () => createCanonicalHostEffectAdapter(hostManifest, hostCompiled, {}),
  /missing canonical host-effect handlers: echo/,
);
assert.throws(
  () => createCanonicalHostEffectAdapter(hostManifest, hostCompiled, { missing() {} }),
  /unknown canonical host-effect lane missing/,
);
assert.throws(
  () => createCanonicalHostEffectAdapter(hostManifest, hostCompiled, { echo: true }),
  /canonical host-effect handler echo must be a function/,
);
const wrongExecutionAdapter = createCanonicalActorAdapter(
  hostManifest,
  hostCompiled,
  actorExports,
);
await assert.rejects(
  wrongExecutionAdapter.dispatch({ lane: "echo", payload: hostRequest(1) }),
  /FE_ACTOR_WRONG_EXECUTION: echo is not owned by this adapter/,
);

let releaseFirstHostEffect;
const hostEffectCalls = [];
const hostEffects = createCanonicalHostEffectAdapter(
  hostManifest,
  hostCompiled,
  {
    echo: async (request) => {
      hostEffectCalls.push(request.tag);
      if (request.tag === 1) {
        await new Promise((resolve) => { releaseFirstHostEffect = resolve; });
      }
      if (request.tag === 9) throw new Error("sensitive host detail");
      return new Uint8Array([request.tag]);
    },
  },
  { maxPendingPerLane: 1 },
);
const wrongPlacementEffects = createCanonicalHostEffectAdapter(
  hostManifest,
  hostCompiled,
  {
    gpu_submit: async (request) => new Uint8Array([request.tag]),
  },
  { placement: "worker" },
);
await assert.rejects(
  wrongPlacementEffects.dispatch({ lane: "echo", payload: hostRequest(1) }),
  /FE_ACTOR_WRONG_EXECUTION: echo is not owned by this adapter/,
);
await assert.rejects(
  hostEffects.dispatch({ lane: "echo", payload: { nope: true } }),
  /FE_ACTOR_INVALID_PAYLOAD/,
);
const activeHostEffect = hostEffects.dispatch({ lane: "echo", payload: hostRequest(1) });
while (!releaseFirstHostEffect) await Promise.resolve();
const displacedHostEffect = hostEffects.dispatch({ lane: "echo", payload: hostRequest(2) });
const newestHostEffect = hostEffects.dispatch({ lane: "echo", payload: hostRequest(3) });
await assert.rejects(displacedHostEffect, /FE_ACTOR_SUPERSEDED/);
releaseFirstHostEffect();
assert.deepEqual(await activeHostEffect, new Uint8Array([1]));
const hostBytes = await newestHostEffect;
assert.deepEqual(hostBytes, new Uint8Array([3]));
assert.deepEqual(hostEffectCalls, [1, 3]);
assert.deepEqual(hostEffects.transferResult(hostBytes, { lane: "echo" }), [hostBytes.buffer]);
await assert.rejects(
  hostEffects.dispatch({ lane: "echo", payload: hostRequest(9) }),
  (error) => String(error).includes("FE_ACTOR_HOST_EFFECT: echo host-effect handler failed")
    && !String(error).includes("sensitive host detail"),
  "host exceptions must be normalized instead of crossing the actor boundary",
);
const invalidHostResponse = createCanonicalHostEffectAdapter(
  hostManifest,
  hostCompiled,
  {
    echo: () => "not bytes",
  },
);
await assert.rejects(
  invalidHostResponse.dispatch({ lane: "echo", payload: hostRequest(4) }),
  /FE_ACTOR_INVALID_RESPONSE: echo result does not match its canonical layout/,
);
assert.throws(
  () => compiled.lanes.echo.request.write({
    tag: 1, sequence: 1n << 64n, text: "", payload: new Uint8Array(),
  }, { memory: new Uint8Array(64), offset: 0 }),
  /u64 bigint/,
);

for (const mutate of [
  (value) => { value.version = 3; },
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

const wasmMemory = new WebAssembly.Memory({ initial: 1 });
const initialBuffer = wasmMemory.buffer;
let arenaCursor = 1024;
let resetCount = 0;
let allocationCount = 0;
const growingAllocate = (length, align) => {
  allocationCount += 1;
  // Deliberately invalidate every previously-created view.
  wasmMemory.grow(1);
  arenaCursor = Math.ceil(arenaCursor / align) * align;
  const pointer = arenaCursor;
  arenaCursor += length;
  return pointer;
};
const mockExports = {
  memory: wasmMemory,
  fe_cabi_alloc: growingAllocate,
  fe_cabi_reset() {
    resetCount += 1;
    arenaCursor = 1024;
  },
  echo_message(requestPointer) {
    const request = compiled.lanes.echo.request.read({
      memory: () => new Uint8Array(wasmMemory.buffer),
      offset: requestPointer,
    });
    const responsePointer = growingAllocate(
      compiled.lanes.echo.response.size,
      compiled.lanes.echo.response.align,
    );
    compiled.lanes.echo.response.write(
      new Uint8Array([...request.payload, request.tag]),
      {
        memory: () => new Uint8Array(wasmMemory.buffer),
        offset: responsePointer,
        allocate: growingAllocate,
      },
    );
    return responsePointer;
  },
};
const caller = createCanonicalInterfaceCaller(compiled, mockExports);
const callValue = (tag) => ({
  tag, sequence: BigInt(tag), text: `growth ${tag}`,
  payload: new Uint8Array([tag + 1, tag + 2]),
});
const firstCall = await caller.call("echo", callValue(4));
assert.deepEqual(firstCall, new Uint8Array([5, 6, 4]));
assert.equal(initialBuffer.byteLength, 0, "memory.grow must detach the initially captured view");
new Uint8Array(wasmMemory.buffer).fill(77);
assert.deepEqual(firstCall, new Uint8Array([5, 6, 4]),
  "arena caller result must remain copied after memory mutation/reset");

const callOrder = [];
const orderedExports = {
  ...mockExports,
  echo_message(requestPointer) {
    const tag = compiled.lanes.echo.request.read({
      memory: () => new Uint8Array(wasmMemory.buffer),
      offset: requestPointer,
    }).tag;
    callOrder.push(tag);
    return mockExports.echo_message(requestPointer);
  },
};
const orderedCaller = createCanonicalInterfaceCaller(compiled, orderedExports);
const [orderedA, orderedB] = await Promise.all([
  orderedCaller.call("echo", callValue(10)),
  orderedCaller.call("echo", callValue(20)),
]);
assert.deepEqual(callOrder, [10, 20]);
assert.deepEqual(orderedA, new Uint8Array([11, 12, 10]));
assert.deepEqual(orderedB, new Uint8Array([21, 22, 20]));

const resetsBeforeFailure = resetCount;
const failingCaller = createCanonicalInterfaceCaller(compiled, {
  ...mockExports,
  echo_message() { throw new Error("mock lane failure"); },
});
await assert.rejects(failingCaller.call("echo", callValue(1)), /mock lane failure/);
assert.equal(resetCount, resetsBeforeFailure + 1, "lane failure must reset the arena");
assert.ok(allocationCount > 0);

console.log("canonical interface v2 intent routing and memory codecs: ok");
