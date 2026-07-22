// Environment-neutral phase-0 actor protocol for demo work scheduling.
// It deliberately owns no timers, animation frames, workers, or DOM state.

export const ACTOR_PROTOCOL_VERSION = 2;

// Protocol lanes are compiler-addressable identifiers, not a closed rendering
// enum. Policy layers (such as createActorCoordinator below) may expose a fixed
// subset. Keep the wire grammar ASCII, bounded, and safe as an object key.
const ACTOR_LANE_PATTERN = /^[a-z][a-z0-9]*(?:[._-][a-z0-9]+)*$/;
export function validateActorLaneName(lane) {
  if (typeof lane !== "string" || lane.length > 64 || !ACTOR_LANE_PATTERN.test(lane)) {
    throw new TypeError("invalid actor lane name");
  }
  return lane;
}

function nonNegativeInteger(value, name) {
  if (!Number.isSafeInteger(value) || value < 0) {
    throw new TypeError(`${name} must be a non-negative safe integer`);
  }
}

function cloneSafe(value, path = "payload", seen = new Set()) {
  if (value === undefined) return;
  if (value === null || ["string", "boolean", "number", "bigint"].includes(typeof value)) {
    return;
  }
  if (typeof value !== "object") {
    throw new TypeError(`${path} is not structured-clone-safe`);
  }
  if (seen.has(value)) throw new TypeError(`${path} is cyclic`);
  seen.add(value);
  if (Array.isArray(value)) {
    value.forEach((item, index) => cloneSafe(item, `${path}[${index}]`, seen));
  } else if (ArrayBuffer.isView(value) || value instanceof ArrayBuffer) {
    // Typed buffers are part of the structured clone algorithm.
  } else {
    const prototype = Object.getPrototypeOf(value);
    if (prototype !== Object.prototype && prototype !== null) {
      throw new TypeError(`${path} must be a plain object, array, or typed buffer`);
    }
    for (const [key, item] of Object.entries(value)) cloneSafe(item, `${path}.${key}`, seen);
  }
  seen.delete(value);
}

export function actorEnvelope({ type, lane, actorEpoch = 0, generation, requestId, payload = null }) {
  if (type !== "request" && type !== "result") throw new TypeError("invalid envelope type");
  validateActorLaneName(lane);
  nonNegativeInteger(actorEpoch, "actorEpoch");
  nonNegativeInteger(generation, "generation");
  nonNegativeInteger(requestId, "requestId");
  cloneSafe(payload);
  return {
    protocol: "fe-demo-actor",
    version: ACTOR_PROTOCOL_VERSION,
    type,
    lane,
    actorEpoch,
    generation,
    requestId,
    payload,
  };
}

export function validateActorEnvelope(envelope) {
  if (!envelope || envelope.protocol !== "fe-demo-actor") throw new TypeError("invalid actor protocol");
  if (envelope.version !== ACTOR_PROTOCOL_VERSION) throw new TypeError("unsupported actor protocol version");
  const expectedKeys = [
    "actorEpoch", "generation", "lane", "payload", "protocol", "requestId", "type", "version",
  ];
  if (Object.keys(envelope).sort().join("\0") !== expectedKeys.join("\0")) {
    throw new TypeError("actor envelope has unexpected or missing fields");
  }
  actorEnvelope(envelope);
  return envelope;
}

/**
 * Two bounded latest-wins mailboxes.
 *
 * Each lane has at most one active request and one pending request. Enqueueing
 * while active replaces the pending request. Results are published only when
 * their generation is still current; request IDs make the rule explicit and
 * survive a future postMessage/worker boundary unchanged.
 */
export function createActorCoordinator({
  render,
  verify,
  onRenderResult = () => {},
  onVerificationResult = () => {},
  onRenderSettled = () => {},
  onVerificationSettled = () => {},
  onCallbackError = () => {},
}) {
  if (typeof render !== "function" || typeof verify !== "function") {
    throw new TypeError("render and verify handlers are required");
  }

  let generation = 0;
  let nextRequestId = 1;
  const lanes = {
    render: { active: null, pending: null, latestRequestId: null, run: render,
      publish: onRenderResult, settled: onRenderSettled },
    verify: { active: null, pending: null, latestRequestId: null, run: verify,
      publish: onVerificationResult, settled: onVerificationSettled },
  };

  const callSafely = (callback, callbackName, laneName, request, result, ...args) => {
    try {
      callback(result, ...args);
    } catch (error) {
      try {
        onCallbackError(error, { callback: callbackName, lane: laneName, request, result });
      } catch {
        // Error reporting is also outside the coordinator's scheduling core.
      }
    }
  };

  const dropPending = (laneName, reason) => {
    const lane = lanes[laneName];
    const request = lane.pending;
    lane.pending = null;
    if (!request) return;
    const result = actorEnvelope({
      type: "result", lane: laneName, actorEpoch: request.actorEpoch,
      generation: request.generation, requestId: request.requestId,
      payload: { ok: false, dropped: true, error: reason },
    });
    callSafely(lane.settled, "settled", laneName, request, result,
      { fresh: false, dropped: true, reason, request });
  };

  const finish = (laneName, request, outcome) => {
    const lane = lanes[laneName];
    if (lane.active?.requestId !== request.requestId) return;
    lane.active = null;
    const fresh = request.generation === generation && request.requestId === lane.latestRequestId;
    const payload = outcome.ok
      ? { ok: true, value: outcome.value }
      : { ok: false, error: String(outcome.error) };
    const result = actorEnvelope({
      type: "result", lane: laneName, actorEpoch: request.actorEpoch, generation: request.generation,
      requestId: request.requestId, payload,
    });
    const pending = lane.pending;
    lane.pending = null;
    if (pending) start(laneName, pending);
    callSafely(lane.settled, "settled", laneName, request, result, { fresh, request });
    if (fresh) callSafely(lane.publish, "publish", laneName, request, result);
  };

  const start = (laneName, request) => {
    const lane = lanes[laneName];
    lane.active = request;
    Promise.resolve()
      .then(() => lane.run(request))
      .then(
        (value) => finish(laneName, request, { ok: true, value }),
        (error) => finish(laneName, request, { ok: false, error }),
      );
  };

  const enqueue = (laneName, payload, requestedGeneration) => {
    const requestGeneration = requestedGeneration ?? generation;
    nonNegativeInteger(requestGeneration, "generation");
    if (requestGeneration !== generation) {
      throw new RangeError("requests must target the current generation");
    }
    const request = actorEnvelope({
      type: "request", lane: laneName, generation: requestGeneration,
      requestId: nextRequestId++, payload,
    });
    const lane = lanes[laneName];
    lane.latestRequestId = request.requestId;
    if (lane.active) {
      dropPending(laneName, "superseded by a newer request");
      lane.pending = request;
    }
    else start(laneName, request);
    return request;
  };

  return {
    nextGeneration() {
      if (generation === Number.MAX_SAFE_INTEGER) {
        throw new RangeError("actor generation exhausted the safe integer range");
      }
      generation += 1;
      // Active work may be impossible to cancel, but queued work from an older
      // generation must never start after the generation boundary.
      dropPending("render", "superseded by a newer generation");
      dropPending("verify", "superseded by a newer generation");
      return generation;
    },
    generation: () => generation,
    enqueueRender: (payload, atGeneration) => enqueue("render", payload, atGeneration),
    enqueueVerification: (payload, atGeneration) => enqueue("verify", payload, atGeneration),
    state() {
      return {
        generation,
        render: { active: lanes.render.active?.requestId ?? null, pending: lanes.render.pending?.requestId ?? null },
        verify: { active: lanes.verify.active?.requestId ?? null, pending: lanes.verify.pending?.requestId ?? null },
      };
    },
  };
}
