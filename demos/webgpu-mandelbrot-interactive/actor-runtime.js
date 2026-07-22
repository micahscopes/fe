import { createActorCoordinator } from "../shared/actor-coordinator.js";
import { actorField, actorResultSchema, createActorEndpoint, createInProcessActorTransport, exactObject } from "../shared/actor-endpoint.js";

const viewPayload = (payload) => exactObject(payload, {
  view: actorField.int32Array(3),
});
const renderValue = (value) => exactObject(value, {
  submitted: actorField.boolean,
}, "render result");
const verifyValue = (value) => exactObject(value, {
  gpuHash: actorField.finiteNumber,
  wasmHash: actorField.finiteNumber,
  referenceHash: actorField.finiteNumber,
}, "verification result");

export const MANDELBROT_ACTOR_SCHEMAS = Object.freeze({
  request: Object.freeze({ render: viewPayload, verify: viewPayload }),
  result: Object.freeze({
    render: actorResultSchema(renderValue),
    verify: actorResultSchema(verifyValue),
  }),
});

export function createMandelbrotActorRuntime({ render, verify, onError = () => {} }) {
  if (typeof render !== "function" || typeof verify !== "function") {
    throw new TypeError("Mandelbrot render and verify handlers are required");
  }
  let generation = 0;
  const waiters = new Map();
  const endpoint = createActorEndpoint({
    transport: createInProcessActorTransport(({ lane, payload }) => {
      const view = Array.from(payload.view);
      return lane === "render" ? render(view) : verify(view);
    }),
    requestSchema: MANDELBROT_ACTOR_SCHEMAS.request,
    resultSchema: MANDELBROT_ACTOR_SCHEMAS.result,
    onProtocolError: onError,
  });

  const execute = async (request) => {
    const result = await endpoint.request({ ...request, actorEpoch: endpoint.epoch() });
    if (!result.payload.ok) throw new Error(result.payload.error);
    return result.payload.value;
  };
  const settle = (result) => {
    const waiter = waiters.get(result.requestId);
    if (!waiter) return;
    waiters.delete(result.requestId);
    if (result.payload.dropped) waiter.resolve({ dropped: true });
    else if (result.payload.ok) waiter.resolve(result.payload.value);
    else waiter.reject(new Error(result.payload.error));
  };
  const coordinator = createActorCoordinator({
    render: execute,
    verify: execute,
    onRenderSettled: settle,
    onVerificationSettled: settle,
    onCallbackError: onError,
  });

  const send = (lane, view, nextGeneration) => {
    if (!Array.isArray(view) && !(view instanceof Int32Array)) {
      throw new TypeError("Mandelbrot view must be a three-word vector");
    }
    if (nextGeneration) generation = coordinator.nextGeneration();
    const request = lane === "render"
      ? coordinator.enqueueRender({ view: new Int32Array(view) }, generation)
      : coordinator.enqueueVerification({ view: new Int32Array(view) }, generation);
    return new Promise((resolve, reject) => {
      waiters.set(request.requestId, { resolve, reject });
    });
  };

  return Object.freeze({
    render(view) {
      const work = send("render", view, true);
      work.catch(onError);
      return work;
    },
    verify: (view) => send("verify", view, false),
    close: endpoint.close,
    reset: endpoint.reset,
    epoch: endpoint.epoch,
    state: coordinator.state,
  });
}
