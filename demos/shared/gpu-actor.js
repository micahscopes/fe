import { actorEnvelope, validateActorEnvelope } from "./actor-coordinator.js";
import { actorField, actorResultSchema, createActorEndpoint, exactObject } from "./actor-endpoint.js";
import { createMessagePortActorTransport } from "./message-port-actor.js";

function plainObject(value, name) {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    throw new TypeError(`${name} must be a plain object`);
  }
  const prototype = Object.getPrototypeOf(value);
  if (prototype !== Object.prototype && prototype !== null) {
    throw new TypeError(`${name} must be a plain object`);
  }
  return value;
}

function exactFunctionMap(value, expectedKeys, name) {
  plainObject(value, name);
  const actual = Object.keys(value).sort();
  const expected = [...expectedKeys].sort();
  if (actual.join("\0") !== expected.join("\0")) {
    throw new TypeError(`${name} must exactly cover actor lanes: ${expected.join(", ")}`);
  }
  for (const lane of expected) {
    if (typeof value[lane] !== "function") {
      throw new TypeError(`${name}.${lane} must be a function`);
    }
  }
  return value;
}

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

export function selectActorSchemas(schemas, lanes) {
  plainObject(schemas, "actor schemas");
  if (!Array.isArray(lanes) || lanes.length === 0 || new Set(lanes).size !== lanes.length) {
    throw new TypeError("selected actor lanes must be a non-empty unique array");
  }
  const requestSchema = plainObject(schemas.requestSchema, "actor request schemas");
  const resultSchema = plainObject(schemas.resultSchema, "actor result schemas");
  const request = Object.create(null);
  const result = Object.create(null);
  for (const lane of lanes) {
    if (typeof lane !== "string"
        || typeof requestSchema[lane] !== "function"
        || typeof resultSchema[lane] !== "function") {
      throw new TypeError(`generated actor schemas do not contain lane ${String(lane)}`);
    }
    request[lane] = requestSchema[lane];
    result[lane] = resultSchema[lane];
  }
  return Object.freeze({
    requestSchema: Object.freeze(request),
    resultSchema: Object.freeze(result),
  });
}

export function createTypedGpuActorClient(port, {
  requestSchema,
  resultSchema,
  initialEpoch = 0,
}) {
  const lanes = Object.keys(plainObject(requestSchema, "GPU actor request schemas")).sort();
  if (lanes.length === 0) throw new TypeError("GPU actor schemas must not be empty");
  exactFunctionMap(requestSchema, lanes, "GPU actor request schemas");
  exactFunctionMap(resultSchema, lanes, "GPU actor result schemas");
  const endpoint = createActorEndpoint({ transport: createMessagePortActorTransport(port),
    initialEpoch, requestSchema, resultSchema });
  let requestId = 0;
  const request = async (lane, payload, generation = 0) => {
    const result = await endpoint.request(actorEnvelope({ type: "request", lane,
      actorEpoch: endpoint.epoch(), generation, requestId: ++requestId,
      payload }));
    if (!result.payload.ok) throw new Error(result.payload.error);
    return result.payload.value;
  };
  return Object.freeze({
    request,
    restart(reason = "GPU actor client restarted") { requestId = 0; return endpoint.reset(reason); },
    close: endpoint.close,
    epoch: endpoint.epoch,
    pendingCount: endpoint.pendingCount,
  });
}

// Main-thread owner for a GPU device. Every explicitly handled lane is bounded
// to one active and one latest pending request. Payload schemas and the exact
// lane set come from the compiler-generated canonical adapter.
export function createTypedMainThreadGpuBroker(port, {
  handlers,
  requestSchema,
  resultSchema,
  initialEpoch = 0,
}) {
  const laneNames = Object.keys(plainObject(handlers, "GPU actor handlers")).sort();
  if (laneNames.length === 0) throw new TypeError("GPU actor handlers must not be empty");
  exactFunctionMap(handlers, laneNames, "GPU actor handlers");
  exactFunctionMap(requestSchema, laneNames, "GPU actor request schemas");
  exactFunctionMap(resultSchema, laneNames, "GPU actor result schemas");
  let epoch = initialEpoch;
  let latestGeneration = 0;
  let closed = false;
  const lanes = Object.fromEntries(laneNames.map((lane) => [
    lane, { active: null, pending: null, run: handlers[lane] },
  ]));
  const reply = (request, payload) => {
    if (closed) return;
    try {
      resultSchema[request.lane](payload);
    } catch {
      payload = { ok: false, error: "FE_ACTOR_INVALID_GPU_RESULT" };
      resultSchema[request.lane](payload);
    }
    port.postMessage(actorEnvelope({ type: "result", lane: request.lane,
      actorEpoch: request.actorEpoch, generation: request.generation,
      requestId: request.requestId, payload }));
  };
  const start = (request) => {
    const lane = lanes[request.lane];
    lane.active = request;
    Promise.resolve().then(() => lane.run(request.payload, request)).then(
      (value) => reply(request, { ok: true, value }),
      () => reply(request, { ok: false, error: "FE_ACTOR_GPU_EFFECT" }),
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
      if (!Object.hasOwn(lanes, request.lane)) return;
      requestSchema[request.lane](request.payload);
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
    state: () => Object.fromEntries(laneNames.map((lane) => [lane, {
      active: lanes[lane].active?.requestId ?? null,
      pending: lanes[lane].pending?.requestId ?? null,
    }])),
  });
}

// Compatibility wrappers for the older CGA demo. New canonical applications
// pass compiler-generated validators into the typed APIs above.
export function createGpuActorClient(port, { valueCount, rgbaBytes, initialEpoch = 0 }) {
  const schemas = gpuActorSchemas(valueCount, rgbaBytes);
  const client = createTypedGpuActorClient(port, {
    requestSchema: schemas.request,
    resultSchema: schemas.result,
    initialEpoch,
  });
  return Object.freeze({
    render: (values, generation = 0) =>
      client.request("render", { values: new Float32Array(values) }, generation),
    verify: (values, generation = 0) =>
      client.request("verify", { values: new Float32Array(values) }, generation),
    restart: client.restart,
    close: client.close,
    epoch: client.epoch,
    pendingCount: client.pendingCount,
  });
}

export function createMainThreadGpuBroker(port, {
  render,
  verify,
  valueCount,
  rgbaBytes,
  initialEpoch = 0,
}) {
  if (typeof render !== "function" || typeof verify !== "function") {
    throw new TypeError("GPU render and verify handlers are required");
  }
  const schemas = gpuActorSchemas(valueCount, rgbaBytes);
  return createTypedMainThreadGpuBroker(port, {
    handlers: {
      render: ({ values }, request) => render(Array.from(values), request),
      verify: ({ values }, request) => verify(Array.from(values), request),
    },
    requestSchema: schemas.request,
    resultSchema: schemas.result,
    initialEpoch,
  });
}
