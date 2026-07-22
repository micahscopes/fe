import assert from "node:assert/strict";
import { actorEnvelope } from "./actor-coordinator.js";
import {
  ActorEndpointClosedError,
  ActorEndpointResetError,
  actorField,
  actorResultSchema,
  createActorEndpoint,
  createInProcessActorTransport,
  exactObject,
} from "./actor-endpoint.js";

const requestSchema = {
  render: (payload) => exactObject(payload, {
    initial: actorField.boolean,
    values: actorField.float32Array(2),
  }),
  verify: (payload) => exactObject(payload, { label: actorField.string }),
};
const resultSchema = {
  render: actorResultSchema(actorField.string),
  verify: actorResultSchema(actorField.string),
};
const request = (lane, epoch, generation, requestId, payload) => actorEnvelope({
  type: "request", lane, actorEpoch: epoch, generation, requestId, payload,
});

const delayed = [];
const transport = {
  send(message, deliver) { delayed.push({ message, deliver }); },
  close() {},
  reset() {},
};
const endpoint = createActorEndpoint({ transport, requestSchema, resultSchema });
const first = endpoint.request(request("verify", 0, 1, 1, { label: "first" }));
const second = endpoint.request(request("verify", 0, 1, 2, { label: "second" }));
assert.equal(endpoint.pendingCount(), 2);

// Reordered replies correlate independently.
delayed[1].deliver(actorEnvelope({ type: "result", lane: "verify", actorEpoch: 0,
  generation: 1, requestId: 2, payload: { ok: true, value: "second result" } }));
assert.equal((await second).payload.value, "second result");
delayed[0].deliver(actorEnvelope({ type: "result", lane: "verify", actorEpoch: 0,
  generation: 1, requestId: 1, payload: { ok: true, value: "first result" } }));
assert.equal((await first).payload.value, "first result");
assert.equal(endpoint.pendingCount(), 0);

// Duplicate results and duplicate request IDs are rejected without republishing.
assert.equal(delayed[0].deliver(actorEnvelope({ type: "result", lane: "verify", actorEpoch: 0,
  generation: 1, requestId: 1, payload: { ok: true, value: "duplicate" } })), false);
await assert.rejects(
  endpoint.request(request("verify", 0, 2, 1, { label: "duplicate request" })),
  /duplicate actor request ID/,
);

// Exact envelope and lane payload schemas reject surplus fields and wrong typed-array lengths.
assert.throws(() => endpoint.request({
  ...request("verify", 0, 1, 3, { label: "x" }), extra: true,
}), /unexpected or missing fields/);
assert.throws(() => endpoint.request(request("render", 0, 1, 4, {
  initial: true, values: new Float32Array([1]),
})), /Float32Array\(2\)/);
assert.throws(() => exactObject(new Date(), {}, "dated payload"), /plain object/);
assert.throws(() => actorEnvelope({ type: "request", lane: "verify", actorEpoch: 0,
  generation: 1, requestId: 99, payload: new Date() }), /plain object/);

// Close rejects active work and post-close sends; late replies are ignored.
const closing = endpoint.request(request("verify", 0, 2, 5, { label: "closing" }));
const closingDelivery = delayed.at(-1);
endpoint.close("test shutdown");
await assert.rejects(closing, ActorEndpointClosedError);
await assert.rejects(
  endpoint.request(request("verify", 0, 2, 6, { label: "post-close" })),
  ActorEndpointClosedError,
);
assert.equal(closingDelivery.deliver(actorEnvelope({ type: "result", lane: "verify", actorEpoch: 0,
  generation: 2, requestId: 5, payload: { ok: true, value: "late" } })), false);

// Reset reopens with a new epoch, rejects old-epoch sends, and ignores old replies.
assert.equal(endpoint.reset(), 1);
await assert.rejects(
  endpoint.request(request("verify", 0, 3, 1, { label: "old epoch" })),
  ActorEndpointResetError,
);
const afterReset = endpoint.request(request("verify", 1, 3, 1, { label: "new epoch" }));
const resetDelivery = delayed.at(-1);
closingDelivery.deliver(actorEnvelope({ type: "result", lane: "verify", actorEpoch: 0,
  generation: 2, requestId: 5, payload: { ok: true, value: "late old epoch" } }));
resetDelivery.deliver(actorEnvelope({ type: "result", lane: "verify", actorEpoch: 1,
  generation: 3, requestId: 1, payload: { ok: true, value: "new epoch result" } }));
assert.equal((await afterReset).payload.value, "new epoch result");

const interruptedByReset = endpoint.request(request("verify", 1, 4, 2, { label: "interrupt" }));
const interruptedDelivery = delayed.at(-1);
assert.equal(endpoint.reset("restart actor"), 2);
await assert.rejects(interruptedByReset, ActorEndpointResetError);
assert.equal(interruptedDelivery.deliver(actorEnvelope({ type: "result", lane: "verify", actorEpoch: 1,
  generation: 4, requestId: 2, payload: { ok: true, value: "late after reset" } })), false);
const epoch2 = endpoint.request(request("verify", 2, 5, 1, { label: "epoch two" }));
const epoch2Delivery = delayed.at(-1);
epoch2Delivery.deliver(actorEnvelope({ type: "result", lane: "verify", actorEpoch: 2,
  generation: 5, requestId: 1, payload: { ok: true, value: "epoch two result" } }));
assert.equal((await epoch2).payload.value, "epoch two result");

const malformedResult = endpoint.request(request("verify", 2, 6, 2, { label: "bad result" }));
const malformedDelivery = delayed.at(-1);
assert.equal(malformedDelivery.deliver(actorEnvelope({ type: "result", lane: "verify", actorEpoch: 2,
  generation: 6, requestId: 2, payload: { ok: true, value: "bad", extra: true } })), false);
await assert.rejects(malformedResult, /unexpected or missing fields/);

const malformedEnvelope = endpoint.request(request("verify", 2, 7, 3, { label: "bad envelope" }));
const malformedEnvelopeDelivery = delayed.at(-1);
assert.equal(malformedEnvelopeDelivery.deliver({
  ...actorEnvelope({ type: "result", lane: "verify", actorEpoch: 2,
    generation: 7, requestId: 3, payload: { ok: true, value: "bad envelope" } }),
  surplus: true,
}), false);
await assert.rejects(malformedEnvelope, /unexpected or missing fields/);

// The in-process transport implements the same callback contract without workers.
const loopback = createActorEndpoint({
  transport: createInProcessActorTransport((message) => `handled ${message.payload.label}`),
  requestSchema,
  resultSchema,
});
const loopbackResult = await loopback.request(request("verify", 0, 1, 9, { label: "locally" }));
assert.equal(loopbackResult.payload.value, "handled locally");

const failingLoopback = createActorEndpoint({
  transport: createInProcessActorTransport(() => Promise.reject(new Error("local failure"))),
  requestSchema,
  resultSchema,
});
const failureResult = await failingLoopback.request(
  request("verify", 0, 1, 10, { label: "failure" }),
);
assert.deepEqual(failureResult.payload, { ok: false, error: "Error: local failure" });

const protocolErrors = [];
const reportingEndpoint = createActorEndpoint({
  transport,
  requestSchema,
  resultSchema,
  onProtocolError: (error, message) => protocolErrors.push([error.message, message]),
});
assert.equal(reportingEndpoint.accept({ garbage: true }), false);
assert.equal(protocolErrors.length, 1);
assert.match(protocolErrors[0][0], /invalid actor protocol/);
assert.deepEqual(protocolErrors[0][1], { garbage: true });

// A malformed stale reply cannot capture a request ID reused in the new epoch.
const reusedDeliveries = [];
const reusedErrors = [];
const reused = createActorEndpoint({
  transport: { send(message, deliver) { reusedDeliveries.push({ message, deliver }); } },
  requestSchema,
  resultSchema,
  onProtocolError: (error) => reusedErrors.push(error.message),
});
reused.reset();
const reusedPending = reused.request(request("verify", 1, 1, 1, { label: "new epoch" }));
assert.equal(reused.accept({
  ...actorEnvelope({ type: "result", lane: "verify", actorEpoch: 0,
    generation: 1, requestId: 1, payload: { ok: true, value: "stale" } }),
  surplus: true,
}), false);
assert.equal(reused.pendingCount(), 1);
assert.equal(reusedErrors.length, 1);
reusedDeliveries[0].deliver(actorEnvelope({ type: "result", lane: "verify", actorEpoch: 1,
  generation: 1, requestId: 1, payload: { ok: true, value: "current" } }));
assert.equal((await reusedPending).payload.value, "current");

// Throwing lifecycle hooks cannot interrupt endpoint state transitions.
const hookJobs = [];
const hookErrors = [];
const throwingHooks = createActorEndpoint({
  transport: {
    send(message, deliver) { hookJobs.push({ message, deliver }); },
    close() { throw new Error("close hook failed"); },
    reset() { throw new Error("reset hook failed"); },
  },
  requestSchema,
  resultSchema,
  onProtocolError: (error, context) => hookErrors.push([context.hook, error.message]),
});
const closedPending = throwingHooks.request(request("verify", 0, 1, 1, { label: "close" }));
throwingHooks.close("closing despite hook");
await assert.rejects(closedPending, ActorEndpointClosedError);
assert.equal(throwingHooks.closed(), true);
assert.equal(throwingHooks.pendingCount(), 0);
assert.equal(throwingHooks.reset("reset despite hook"), 1);
assert.equal(throwingHooks.closed(), false);
assert.equal(throwingHooks.pendingCount(), 0);
assert.deepEqual(hookErrors, [
  ["close", "close hook failed"],
  ["reset", "reset hook failed"],
]);

console.log("shared actor endpoint epoch/close/reset/schema/adversarial transport: ok");
