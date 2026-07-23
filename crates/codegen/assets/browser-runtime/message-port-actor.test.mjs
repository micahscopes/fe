import assert from "node:assert/strict";
import { actorEnvelope } from "./actor-coordinator.js";
import { createActorEndpoint, actorField, actorResultSchema, exactObject } from "./actor-endpoint.js";
import {
  attachMessagePortActorHost,
  createMessagePortActorTransport,
  transferOwnedTypedArray,
} from "./message-port-actor.js";
import { createExactLaneRouter } from "./actor-router.js";

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

const sanitizedChannel = new MessageChannel();
const detachSanitized = attachMessagePortActorHost(
  sanitizedChannel.port2,
  () => Promise.reject(new Error("private host detail")),
);
const sanitizedEndpoint = createActorEndpoint({
  transport: createMessagePortActorTransport(sanitizedChannel.port1),
  requestSchema: schema,
  resultSchema: results,
});
const sanitized = await sanitizedEndpoint.request(actorEnvelope({
  type: "request", lane: "verify", actorEpoch: 0,
  generation: 1, requestId: 5, payload: { value: 1 },
}));
assert.deepEqual(sanitized.payload, { ok: false, error: "FE_ACTOR_HOST_DISPATCH" });
sanitizedEndpoint.close();
detachSanitized();

const malformedRequestChannel = new MessageChannel();
let malformedDispatches = 0;
const detachMalformedRequest = attachMessagePortActorHost(
  malformedRequestChannel.port2,
  () => { malformedDispatches += 1; },
);
const malformedRequestReply = new Promise((resolve) => {
  malformedRequestChannel.port1.addEventListener("message", ({ data }) => resolve(data), {
    once: true,
  });
});
malformedRequestChannel.port1.start();
malformedRequestChannel.port1.postMessage({
  ...actorEnvelope({
    type: "request", lane: "verify", actorEpoch: 0,
    generation: 1, requestId: 6, payload: { value: 1 },
  }),
  surplus: true,
});
assert.deepEqual((await malformedRequestReply).payload, {
  ok: false,
  error: "FE_ACTOR_PROTOCOL",
});
assert.equal(malformedDispatches, 0, "malformed request never reaches host dispatch");
const malformedCancelReply = new Promise((resolve) => {
  malformedRequestChannel.port1.addEventListener("message", ({ data }) => resolve(data), {
    once: true,
  });
});
malformedRequestChannel.port1.postMessage({
  ...actorEnvelope({
    type: "cancel", lane: "verify", actorEpoch: 0,
    generation: 1, requestId: 6, payload: null,
  }),
  surplus: true,
});
assert.deepEqual((await malformedCancelReply).payload, {
  ok: false,
  error: "FE_ACTOR_PROTOCOL",
});
assert.equal(malformedDispatches, 0, "malformed cancel never reaches host dispatch");
malformedRequestChannel.port1.close();
detachMalformedRequest();

const abortChannel = new MessageChannel();
let hostAbortSignal;
let settleAbortedHost;
const abortedHostWork = new Promise((resolve) => { settleAbortedHost = resolve; });
const abortRouter = createExactLaneRouter({ verify: {} }, {
  worker: {
    lanes: ["verify"],
    dispatch(_request, { signal }) {
      hostAbortSignal = signal;
      return abortedHostWork;
    },
  },
});
const detachAbort = attachMessagePortActorHost(
  abortChannel.port2,
  abortRouter.dispatch,
  { maxInFlight: 1 },
);
const abortEndpoint = createActorEndpoint({
  transport: createMessagePortActorTransport(abortChannel.port1),
  requestSchema: schema,
  resultSchema: results,
});
const abortController = new AbortController();
const abortPending = abortEndpoint.request(actorEnvelope({
  type: "request", lane: "verify", actorEpoch: 0,
  generation: 1, requestId: 7, payload: { value: 1 },
}), { signal: abortController.signal });
await new Promise((resolve) => setTimeout(resolve, 0));
assert.equal(hostAbortSignal.aborted, false);
abortController.abort();
await assert.rejects(abortPending, (error) => error.code === "FE_ACTOR_ABORTED");
await new Promise((resolve) => setTimeout(resolve, 0));
assert.equal(hostAbortSignal.aborted, true, "cancel envelope aborts host dispatch signal");
assert.equal(abortEndpoint.pendingCount(), 0);
settleAbortedHost(2);
await new Promise((resolve) => setTimeout(resolve, 0));
assert.equal(abortEndpoint.pendingCount(), 0, "late cancelled host value is suppressed");
abortEndpoint.close();
detachAbort();

const boundedHostChannel = new MessageChannel();
let releaseBounded;
const boundedHostWork = new Promise((resolve) => { releaseBounded = resolve; });
const detachBoundedHost = attachMessagePortActorHost(
  boundedHostChannel.port2,
  () => boundedHostWork,
  { maxInFlight: 1 },
);
const boundedReplies = [];
boundedHostChannel.port1.addEventListener("message", ({ data }) => boundedReplies.push(data));
boundedHostChannel.port1.start();
for (const requestId of [8, 9]) {
  boundedHostChannel.port1.postMessage(actorEnvelope({
    type: "request", lane: "verify", actorEpoch: 0,
    generation: 1, requestId, payload: { value: requestId },
  }));
}
await new Promise((resolve) => setTimeout(resolve, 0));
assert.deepEqual(boundedReplies.map(({ requestId, payload }) => [requestId, payload]), [
  [9, { ok: false, error: "FE_ACTOR_BUSY" }],
]);
boundedHostChannel.port1.postMessage(actorEnvelope({
  type: "cancel", lane: "verify", actorEpoch: 0,
  generation: 1, requestId: 8, payload: null,
}));
await new Promise((resolve) => setTimeout(resolve, 0));
releaseBounded(16);
await new Promise((resolve) => setTimeout(resolve, 0));
assert.equal(boundedReplies.length, 1, "cancelled host work publishes no late result");
boundedHostChannel.port1.close();
detachBoundedHost();

console.log("protocol-v3 MessagePort actor cancellation/transport/host: ok");
