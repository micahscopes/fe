import assert from "node:assert/strict";
import { actorEnvelope } from "./actor-coordinator.js";
import { createActorEndpoint, actorField, actorResultSchema, exactObject } from "./actor-endpoint.js";
import {
  attachMessagePortActorHost,
  createMessagePortActorTransport,
  transferOwnedTypedArray,
} from "./message-port-actor.js";

const channel = new MessageChannel();
const detach = attachMessagePortActorHost(channel.port2, ({ payload }) => payload.value * 2);
const transport = createMessagePortActorTransport(channel.port1);
const schema = {
  render: (payload) => exactObject(payload, { value: actorField.finiteNumber }),
  verify: (payload) => exactObject(payload, { value: actorField.finiteNumber }),
};
const results = { render: actorResultSchema(actorField.finiteNumber),
  verify: actorResultSchema(actorField.finiteNumber) };
const endpoint = createActorEndpoint({ transport, requestSchema: schema, resultSchema: results });
const request = actorEnvelope({ type: "request", lane: "verify", actorEpoch: 0,
  generation: 1, requestId: 1, payload: { value: 21 } });
assert.equal((await endpoint.request(request)).payload.value, 42);

const pending = endpoint.request(actorEnvelope({ type: "request", lane: "verify", actorEpoch: 0,
  generation: 2, requestId: 2, payload: { value: 7 } }));
transport.fail("simulated worker crash");
const failed = await pending;
assert.equal(failed.payload.ok, false);
assert.match(failed.payload.error, /simulated worker crash/);
endpoint.close();
detach();

const malformedChannel = new MessageChannel();
const malformedEndpoint = createActorEndpoint({
  transport: createMessagePortActorTransport(malformedChannel.port1),
  requestSchema: schema,
  resultSchema: results,
});
const malformedPending = malformedEndpoint.request(actorEnvelope({ type: "request", lane: "verify",
  actorEpoch: 0, generation: 1, requestId: 3, payload: { value: 1 } }));
malformedChannel.port2.postMessage({
  ...actorEnvelope({ type: "result", lane: "verify", actorEpoch: 0,
    generation: 1, requestId: 3, payload: { ok: true, value: 2 } }),
  surplus: true,
});
await assert.rejects(malformedPending, /unexpected or missing fields/);
malformedEndpoint.close();
malformedChannel.port2.close();

const transferChannel = new MessageChannel();
let workerOwnedBytes;
const detachTransfer = attachMessagePortActorHost(transferChannel.port2, () => {
  workerOwnedBytes = new Uint8Array([1, 2, 3, 4]);
  return workerOwnedBytes;
}, { transferResult: transferOwnedTypedArray });
const transferEndpoint = createActorEndpoint({
  transport: createMessagePortActorTransport(transferChannel.port1),
  requestSchema: schema,
  resultSchema: { ...results, verify: actorResultSchema(actorField.uint8Array(4)) },
});
const transferred = await transferEndpoint.request(actorEnvelope({
  type: "request", lane: "verify", actorEpoch: 0,
  generation: 1, requestId: 4, payload: { value: 1 },
}));
assert.deepEqual([...transferred.payload.value], [1, 2, 3, 4]);
assert.equal(workerOwnedBytes.byteLength, 0, "successful transfer detaches worker-owned buffer");
assert.throws(
  () => transferOwnedTypedArray(new Uint8Array(new ArrayBuffer(8), 2, 4)),
  /full-span owned typed array/,
);
transferEndpoint.close();
detachTransfer();

console.log("protocol-v2 MessagePort actor transport/host: ok");
