import assert from "node:assert/strict";
import {
  ACTOR_PROTOCOL_VERSION,
  actorEnvelope,
  createActorCoordinator,
  validateActorLaneName,
  validateActorEnvelope,
} from "./actor-coordinator.js";

const deferred = () => {
  let resolve;
  const promise = new Promise((done) => { resolve = done; });
  return { promise, resolve };
};
const tick = () => new Promise((resolve) => setTimeout(resolve, 0));

const sample = actorEnvelope({
  type: "request", lane: "render", generation: 3, requestId: 7,
  payload: { view: new Float32Array([1, 2, 3]) },
});
assert.equal(sample.version, ACTOR_PROTOCOL_VERSION);
assert.deepEqual(validateActorEnvelope(structuredClone(sample)), sample);
const cancel = actorEnvelope({
  type: "cancel", lane: "render", actorEpoch: 0,
  generation: 3, requestId: 7, payload: null,
});
assert.deepEqual(validateActorEnvelope(structuredClone(cancel)), cancel);
assert.throws(() => actorEnvelope({
  type: "cancel", lane: "render", actorEpoch: 0,
  generation: 3, requestId: 7, payload: {},
}), /payload must be null/);
assert.equal(validateActorLaneName("compile.tile-2"), "compile.tile-2");
for (const lane of ["", "Render", "2render", "render/now", "render..now", "render_", "a".repeat(65)]) {
  assert.throws(() => actorEnvelope({ type: "request", lane, generation: 0,
    requestId: 1 }), /invalid actor lane name/);
}
assert.throws(() => actorEnvelope({
  type: "request", lane: "render", generation: 0, requestId: 1, payload: { bad() {} },
}), /structured-clone-safe/);
assert.throws(() => validateActorEnvelope({ ...sample, version: 1 }), /unsupported/);

const renderRuns = [];
const verifyRuns = [];
const renderResults = [];
const verifyResults = [];
const renderSettlements = [];
const coordinator = createActorCoordinator({
  render(request) {
    const job = deferred();
    renderRuns.push({ request, job });
    return job.promise;
  },
  verify(request) {
    const job = deferred();
    verifyRuns.push({ request, job });
    return job.promise;
  },
  onRenderResult: (result) => renderResults.push(result),
  onVerificationResult: (result) => verifyResults.push(result),
  onRenderSettled: (result, status) => renderSettlements.push({ result, status }),
});

// Latest-wins is bounded: one active render and exactly the newest pending one.
const generation1 = coordinator.nextGeneration();
const render1 = coordinator.enqueueRender({ frame: 1 }, generation1);
coordinator.enqueueRender({ frame: 2 }, generation1);
const render3 = coordinator.enqueueRender({ frame: 3 }, generation1);
assert.equal(renderSettlements.length, 1);
assert.equal(renderSettlements[0].result.payload.dropped, true);
assert.equal(renderSettlements[0].status.request.payload.frame, 2);
await tick();
assert.equal(renderRuns.length, 1);
assert.deepEqual(coordinator.state().render, { active: render1.requestId, pending: render3.requestId });
renderRuns[0].job.resolve("old frame");
await tick();
assert.equal(renderRuns.length, 2);
assert.equal(renderRuns[1].request.requestId, render3.requestId);
renderRuns[1].job.resolve("latest frame");
await tick();
assert.deepEqual(renderResults.map((result) => result.payload.value), ["latest frame"]);

// Verification has the same one-active/one-latest-pending bound.
const verify1 = coordinator.enqueueVerification({ view: 1 }, generation1);
coordinator.enqueueVerification({ view: 2 }, generation1);
const verify3 = coordinator.enqueueVerification({ view: 3 }, generation1);
await tick();
assert.deepEqual(coordinator.state().verify, { active: verify1.requestId, pending: verify3.requestId });

// A newer generation makes the delayed active result stale. It is discarded,
// and only the newest pending request is executed and published.
const generation2 = coordinator.nextGeneration();
const verify4 = coordinator.enqueueVerification({ view: 4 }, generation2);
verifyRuns[0].job.resolve("stale verification");
await tick();
assert.equal(verifyResults.length, 0);
assert.equal(verifyRuns.length, 2);
assert.equal(verifyRuns[1].request.requestId, verify4.requestId);
verifyRuns[1].job.resolve("current verification");
await tick();
assert.deepEqual(verifyResults.map((result) => result.payload.value), ["current verification"]);

// Advancing a generation without replacing a pending request discards that
// queued work. The already-active request may finish but cannot publish.
const staleActive = coordinator.enqueueRender({ frame: 4 }, generation2);
coordinator.enqueueRender({ frame: "must not run" }, generation2);
await tick();
const generation3 = coordinator.nextGeneration();
const staleRun = renderRuns.find((run) => run.request.requestId === staleActive.requestId);
staleRun.job.resolve("stale active frame");
await tick();
assert.equal(renderRuns.some((run) => run.request.payload.frame === "must not run"), false);
assert.equal(renderResults.some((result) => result.payload.value === "stale active frame"), false);

// Independent lanes may complete out of order; freshness, not completion order,
// decides publication.
coordinator.enqueueRender({ frame: 5 }, generation3);
coordinator.enqueueVerification({ view: 5 }, generation3);
await tick();
const lastRender = renderRuns.at(-1);
const lastVerify = verifyRuns.at(-1);
lastVerify.job.resolve("verify first");
await tick();
lastRender.job.resolve("render second");
await tick();
assert.equal(verifyResults.at(-1).payload.value, "verify first");
assert.equal(renderResults.at(-1).payload.value, "render second");
assert.equal(verifyResults.at(-1).generation, generation3);
assert.equal(renderResults.at(-1).generation, generation3);

const rejectionSettlements = [];
const rejecting = createActorCoordinator({
  render: () => null,
  verify: () => Promise.reject(new Error("device lost")),
  onVerificationSettled: (result, status) => rejectionSettlements.push({ result, status }),
});
const rejectionGeneration = rejecting.nextGeneration();
const rejectedRequest = rejecting.enqueueVerification({ initial: true }, rejectionGeneration);
await tick();
assert.equal(rejectionSettlements.length, 1);
assert.equal(rejectionSettlements[0].result.payload.ok, false);
assert.match(rejectionSettlements[0].result.payload.error, /device lost/);
assert.equal(rejectionSettlements[0].status.request.requestId, rejectedRequest.requestId);
assert.equal(rejectionSettlements[0].status.request.payload.initial, true);

const throwingRuns = [];
const callbackErrors = [];
const throwing = createActorCoordinator({
  render: () => null,
  verify(request) {
    const job = deferred();
    throwingRuns.push({ request, job });
    return job.promise;
  },
  onVerificationSettled: () => { throw new Error("settled callback failed"); },
  onVerificationResult: () => { throw new Error("publish callback failed"); },
  onCallbackError: (error, context) => callbackErrors.push([context.callback, error.message]),
});
const throwingGeneration = throwing.nextGeneration();
throwing.enqueueVerification({ view: 1 }, throwingGeneration);
await tick();
throwing.enqueueVerification({ view: 2 }, throwingGeneration);
const throwingLatest = throwing.enqueueVerification({ view: 3 }, throwingGeneration);
throwingRuns[0].job.resolve("superseded result");
await tick();
assert.equal(throwingRuns.length, 2);
assert.equal(throwingRuns[1].request.requestId, throwingLatest.requestId);
throwingRuns[1].job.resolve("latest result");
await tick();
assert.deepEqual(callbackErrors, [
  ["settled", "settled callback failed"],
  ["settled", "settled callback failed"],
  ["settled", "settled callback failed"],
  ["publish", "publish callback failed"],
]);
assert.deepEqual(throwing.state().verify, { active: null, pending: null });

console.log("shared actor coordinator delay/reorder/coalescing: ok");
