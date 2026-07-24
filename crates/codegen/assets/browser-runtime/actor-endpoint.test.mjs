import assert from "node:assert/strict";
import { actorEnvelope } from "./actor-coordinator.js";
import {
  ActorEndpointClosedError,
  ActorEndpointAbortError,
  ActorEndpointBusyError,
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
requestSchema["compile.tile"] = (payload) => exactObject(payload, { label: actorField.string });
resultSchema["compile.tile"] = actorResultSchema(actorField.string);
const request = (lane, epoch, generation, requestId, payload) => actorEnvelope({
  type: "request", lane, actorEpoch: epoch, generation, requestId, payload,
});

const delayed = [];
const cancelled = [];
const transport = {
  send(message, deliver) { delayed.push({ message, deliver }); },
  cancel(message) { cancelled.push(message); },
  close() {},
  reset() {},
};
const endpoint = createActorEndpoint({ transport, requestSchema, resultSchema });
const arbitraryLane = endpoint.request(request("compile.tile", 0, 0, 100, { label: "tile" }));
delayed.at(-1).deliver(actorEnvelope({ type: "result", lane: "compile.tile", actorEpoch: 0,
  generation: 0, requestId: 100, payload: { ok: true, value: "compiled" } }));
assert.equal((await arbitraryLane).payload.value, "compiled");
assert.throws(() => endpoint.request(request("manifest.unknown", 0, 0, 101, {})),
  /no request schema for actor lane manifest.unknown/);
assert.throws(() => endpoint.request(request("constructor", 0, 0, 102, {})),
  /no request schema for actor lane constructor/);
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

// Abort rejects locally, emits one protocol cancellation, and ignores a late result.
const abortController = new AbortController();
const aborted = endpoint.request(
  request("verify", 0, 2, 7, { label: "abort me" }),
  { signal: abortController.signal },
);
const abortedDelivery = delayed.at(-1);
abortController.abort();
await assert.rejects(aborted, (error) =>
  error instanceof ActorEndpointAbortError && error.code === "FE_ACTOR_ABORTED");
assert.equal(endpoint.pendingCount(), 0);
assert.deepEqual(cancelled, [request("verify", 0, 2, 7, { label: "abort me" })]);
assert.equal(abortedDelivery.deliver(actorEnvelope({
  type: "result", lane: "verify", actorEpoch: 0,
  generation: 2, requestId: 7, payload: { ok: true, value: "too late" },
})), false);
const alreadyAborted = new AbortController();
alreadyAborted.abort();
const sendsBeforePreAbort = delayed.length;
await assert.rejects(
  endpoint.request(
    request("verify", 0, 2, 8, { label: "never sent" }),
    { signal: alreadyAborted.signal },
  ),
  ActorEndpointAbortError,
);
assert.equal(delayed.length, sendsBeforePreAbort, "pre-aborted request is never transferred");

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

const boundedJobs = [];
const bounded = createActorEndpoint({
  transport: { send(message, deliver) { boundedJobs.push({ message, deliver }); } },
  requestSchema,
  resultSchema,
  maxPending: 1,
});
const firstBounded = bounded.request(request("verify", 0, 1, 1, { label: "first" }));
await assert.rejects(
  bounded.request(request("verify", 0, 1, 2, { label: "second" })),
  ActorEndpointBusyError,
);
boundedJobs[0].deliver(actorEnvelope({ type: "result", lane: "verify", actorEpoch: 0,
  generation: 1, requestId: 1, payload: { ok: true, value: "done" } }));
assert.equal((await firstBounded).payload.value, "done");

// Opt-in wait saturation provides bounded FIFO admission. Transferable payloads
// remain owned by the caller until their request actually enters an active slot.
const admitted = [];
const admissionCancels = [];
const admissionTransport = {
  send(message, deliver) {
    structuredClone(message, { transfer: [message.payload.values.buffer] });
    admitted.push({ message, deliver });
  },
  cancel(message) { admissionCancels.push(message.requestId); },
};
const waiting = createActorEndpoint({
  transport: admissionTransport,
  requestSchema,
  resultSchema,
  maxPending: 1,
  maxQueued: 2,
  saturation: "wait",
});
const values1 = new Float32Array([1, 1]);
const values2 = new Float32Array([2, 2]);
const values3 = new Float32Array([3, 3]);
const values4 = new Float32Array([4, 4]);
const wait1 = waiting.request(request("render", 0, 8, 1, {
  initial: true, values: values1,
}));
const wait2 = waiting.request(request("render", 0, 8, 2, {
  initial: false, values: values2,
}));
const queuedAbort = new AbortController();
const wait3 = waiting.request(request("render", 0, 8, 3, {
  initial: false, values: values3,
}), { signal: queuedAbort.signal });
await assert.rejects(
  waiting.request(request("render", 0, 8, 4, {
    initial: false, values: values4,
  })),
  (error) => error instanceof ActorEndpointBusyError && error.code === "FE_ACTOR_BUSY",
);
assert.equal(values1.byteLength, 0, "active request transfers immediately");
assert.equal(values2.byteLength, 8, "queued request retains ownership");
assert.equal(values3.byteLength, 8, "second queued request retains ownership");
assert.equal(values4.byteLength, 8, "overflow rejection retains ownership");
assert.equal(waiting.pendingCount(), 1);
assert.equal(waiting.queuedCount(), 2);

queuedAbort.abort();
await assert.rejects(wait3, ActorEndpointAbortError);
assert.deepEqual(admissionCancels, [], "queued abort never emits protocol cancellation");
assert.equal(admitted.length, 1, "queued abort is never sent");
assert.equal(values3.byteLength, 8, "queued abort retains transferable ownership");
assert.equal(waiting.queuedCount(), 1);

admitted[0].deliver(actorEnvelope({ type: "result", lane: "render", actorEpoch: 0,
  generation: 8, requestId: 1, payload: { ok: true, value: "first admitted" } }));
assert.equal((await wait1).payload.value, "first admitted");
assert.equal(admitted.length, 2);
assert.equal(admitted[1].message.requestId, 2, "admission order is FIFO");
assert.equal(values2.byteLength, 0, "ownership transfers exactly at admission");
admitted[1].deliver(actorEnvelope({ type: "result", lane: "render", actorEpoch: 0,
  generation: 8, requestId: 2, payload: { ok: true, value: "second admitted" } }));
assert.equal((await wait2).payload.value, "second admitted");

const abortJobs = [];
const abortCancels = [];
const abortWaiting = createActorEndpoint({
  transport: {
    send(message, deliver) { abortJobs.push({ message, deliver }); },
    cancel(message) { abortCancels.push(message.requestId); },
  },
  requestSchema,
  resultSchema,
  maxPending: 1,
  maxQueued: 1,
  saturation: "wait",
});
const activeAbortController = new AbortController();
const activeAbort = abortWaiting.request(
  request("verify", 0, 8, 1, { label: "abort active" }),
  { signal: activeAbortController.signal },
);
const afterActiveAbort = abortWaiting.request(
  request("verify", 0, 8, 2, { label: "admit after abort" }),
);
activeAbortController.abort();
await assert.rejects(activeAbort, ActorEndpointAbortError);
assert.deepEqual(abortCancels, [1], "active abort emits exactly one cancellation");
assert.equal(abortJobs.length, 2, "active abort drains the next queued request");
assert.equal(abortJobs[1].message.requestId, 2);
abortJobs[1].deliver(actorEnvelope({ type: "result", lane: "verify", actorEpoch: 0,
  generation: 8, requestId: 2, payload: { ok: true, value: "after abort" } }));
assert.equal((await afterActiveAbort).payload.value, "after abort");

// A synchronous send failure rejects that admission and immediately gives the
// released active slot to the next queued request.
const failureJobs = [];
const failingRequestIds = new Set([2, 3]);
const failureWaiting = createActorEndpoint({
  transport: {
    send(message, deliver) {
      if (failingRequestIds.has(message.requestId)) throw new Error("synchronous send failure");
      failureJobs.push({ message, deliver });
    },
  },
  requestSchema,
  resultSchema,
  maxPending: 1,
  maxQueued: 3,
  saturation: "wait",
});
const failure1 = failureWaiting.request(request("verify", 0, 9, 1, { label: "active" }));
const failure2 = failureWaiting.request(request("verify", 0, 9, 2, { label: "will fail" }));
const failure3 = failureWaiting.request(request("verify", 0, 9, 3, { label: "also fails" }));
const failure4 = failureWaiting.request(request("verify", 0, 9, 4, { label: "must drain" }));
failureJobs[0].deliver(actorEnvelope({ type: "result", lane: "verify", actorEpoch: 0,
  generation: 9, requestId: 1, payload: { ok: true, value: "released" } }));
assert.equal((await failure1).payload.value, "released");
await assert.rejects(failure2, /synchronous send failure/);
await assert.rejects(failure3, /synchronous send failure/);
assert.equal(failureJobs.at(-1).message.requestId, 4);
failureJobs.at(-1).deliver(actorEnvelope({ type: "result", lane: "verify", actorEpoch: 0,
  generation: 9, requestId: 4, payload: { ok: true, value: "drained" } }));
assert.equal((await failure4).payload.value, "drained");
assert.equal(failureWaiting.pendingCount(), 0);
assert.equal(failureWaiting.queuedCount(), 0);

// Close and reset reject unsent work without transfer or cancellation.
for (const lifecycle of ["close", "reset"]) {
  const lifecycleJobs = [];
  const lifecycleCancels = [];
  const lifecycleEndpoint = createActorEndpoint({
    transport: {
      send(message, deliver) { lifecycleJobs.push({ message, deliver }); },
      cancel(message) { lifecycleCancels.push(message.requestId); },
    },
    requestSchema,
    resultSchema,
    maxPending: 1,
    maxQueued: 1,
    saturation: "wait",
  });
  const active = lifecycleEndpoint.request(
    request("verify", 0, 10, 1, { label: `${lifecycle} active` }),
  );
  const queued = lifecycleEndpoint.request(
    request("verify", 0, 10, 2, { label: `${lifecycle} queued` }),
  );
  lifecycleEndpoint[lifecycle](`${lifecycle} test`);
  const ExpectedError = lifecycle === "close"
    ? ActorEndpointClosedError
    : ActorEndpointResetError;
  await assert.rejects(active, ExpectedError);
  await assert.rejects(queued, ExpectedError);
  assert.equal(lifecycleJobs.length, 1, `${lifecycle} never sends queued work`);
  assert.deepEqual(lifecycleCancels, [], `${lifecycle} does not cancel queued work`);
  assert.equal(lifecycleEndpoint.pendingCount(), 0);
  assert.equal(lifecycleEndpoint.queuedCount(), 0);
}

console.log("shared actor endpoint bounded/epoch/close/reset/schema/adversarial transport: ok");
