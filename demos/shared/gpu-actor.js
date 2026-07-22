import { actorEnvelope, validateActorEnvelope } from "./actor-coordinator.js";
import { actorField, actorResultSchema, createActorEndpoint, exactObject } from "./actor-endpoint.js";
import { createMessagePortActorTransport } from "./message-port-actor.js";

export const gpuActorSchemas = (valueCount, rgbaBytes) => Object.freeze({
  request: Object.freeze({
    render: (payload) => exactObject(payload, { values: actorField.float32Array(valueCount) }),
    verify: (payload) => exactObject(payload, { values: actorField.float32Array(valueCount) }),
  }),
  result: Object.freeze({
    render: actorResultSchema((value) => exactObject(value, { submitted: actorField.boolean })),
    verify: actorResultSchema(actorField.uint8Array(rgbaBytes)),
  }),
});

export function createGpuActorClient(port, { valueCount, rgbaBytes, initialEpoch = 0 }) {
  const schemas = gpuActorSchemas(valueCount, rgbaBytes);
  const endpoint = createActorEndpoint({ transport: createMessagePortActorTransport(port),
    initialEpoch, requestSchema: schemas.request, resultSchema: schemas.result });
  let requestId = 0;
  const request = async (lane, values, generation) => {
    const result = await endpoint.request(actorEnvelope({ type: "request", lane,
      actorEpoch: endpoint.epoch(), generation, requestId: ++requestId,
      payload: { values: new Float32Array(values) } }));
    if (!result.payload.ok) throw new Error(result.payload.error);
    return result.payload.value;
  };
  return Object.freeze({
    render: (values, generation = 0) => request("render", values, generation),
    verify: (values, generation = 0) => request("verify", values, generation),
    restart(reason = "GPU actor client restarted") { requestId = 0; return endpoint.reset(reason); },
    close: endpoint.close, epoch: endpoint.epoch,
  });
}

// Main-thread owner for the GPU device. Each lane is bounded to one active and
// one latest pending request; superseded and stale requests receive explicit
// dropped results so remote promises never leak.
export function createMainThreadGpuBroker(port, { render, verify, valueCount, rgbaBytes, initialEpoch = 0 }) {
  if (typeof render !== "function" || typeof verify !== "function") {
    throw new TypeError("GPU render and verify handlers are required");
  }
  const schemas = gpuActorSchemas(valueCount, rgbaBytes);
  let epoch = initialEpoch;
  let latestGeneration = 0;
  let closed = false;
  const lanes = { render: { active: null, pending: null, run: render },
    verify: { active: null, pending: null, run: verify } };
  const reply = (request, payload) => {
    if (closed) return;
    port.postMessage(actorEnvelope({ type: "result", lane: request.lane,
      actorEpoch: request.actorEpoch, generation: request.generation,
      requestId: request.requestId, payload }));
  };
  const start = (request) => {
    const lane = lanes[request.lane];
    lane.active = request;
    Promise.resolve().then(() => lane.run(Array.from(request.payload.values), request)).then(
      (value) => reply(request, { ok: true, value }),
      (error) => reply(request, { ok: false, error: String(error) }),
    ).finally(() => {
      lane.active = null;
      const next = lane.pending;
      lane.pending = null;
      if (next) start(next);
    });
  };
  const onMessage = (event) => {
    const request = event.data;
    try {
      validateActorEnvelope(request);
      if (request.type !== "request" || request.actorEpoch !== epoch) return;
      schemas.request[request.lane](request.payload);
    } catch { return; }
    if (request.generation < latestGeneration) {
      reply(request, { ok: false, error: "stale GPU generation" });
      return;
    }
    latestGeneration = request.generation;
    const lane = lanes[request.lane];
    if (lane.active) {
      if (lane.pending) reply(lane.pending,
        { ok: false, error: "superseded by newer GPU request" });
      lane.pending = request;
    } else start(request);
  };
  port.addEventListener("message", onMessage);
  port.start?.();
  return Object.freeze({
    close() { if (closed) return; closed = true; port.removeEventListener("message", onMessage); port.close?.(); },
    restart(nextEpoch = epoch + 1) {
      for (const lane of Object.values(lanes)) {
        if (lane.pending) reply(lane.pending, { ok: false, error: "GPU actor restarted" });
        lane.pending = null;
      }
      epoch = nextEpoch;
      latestGeneration = 0;
      return epoch;
    },
    epoch: () => epoch,
    state: () => ({ render: { active: lanes.render.active?.requestId ?? null,
      pending: lanes.render.pending?.requestId ?? null }, verify: {
      active: lanes.verify.active?.requestId ?? null, pending: lanes.verify.pending?.requestId ?? null } }),
  });
}
