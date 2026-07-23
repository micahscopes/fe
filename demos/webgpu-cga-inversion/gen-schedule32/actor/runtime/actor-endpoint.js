import { actorEnvelope, validateActorEnvelope } from "./actor-coordinator.js";

export class ActorEndpointClosedError extends Error {
  constructor(message = "actor endpoint is closed") {
    super(message);
    this.name = "ActorEndpointClosedError";
  }
}

export class ActorEndpointResetError extends Error {
  constructor(message = "actor endpoint epoch was reset") {
    super(message);
    this.name = "ActorEndpointResetError";
  }
}

export class ActorEndpointBusyError extends Error {
  constructor(message = "actor endpoint pending limit reached") {
    super(message);
    this.name = "ActorEndpointBusyError";
    this.code = "FE_ACTOR_BUSY";
  }
}

function validatePayload(schema, envelope, direction) {
  const validator = schema && Object.hasOwn(schema, envelope.lane)
    ? schema[envelope.lane]
    : undefined;
  if (typeof validator !== "function") {
    throw new TypeError(`no ${direction} schema for actor lane ${envelope.lane}`);
  }
  validator(envelope.payload);
}

export function exactObject(value, fields, name = "payload") {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    throw new TypeError(`${name} must be a plain object`);
  }
  const prototype = Object.getPrototypeOf(value);
  if (prototype !== Object.prototype && prototype !== null) {
    throw new TypeError(`${name} must be a plain object`);
  }
  const actual = Object.keys(value).sort();
  const expected = Object.keys(fields).sort();
  if (actual.join("\0") !== expected.join("\0")) {
    throw new TypeError(`${name} has unexpected or missing fields`);
  }
  for (const key of expected) fields[key](value[key], `${name}.${key}`);
  return value;
}

export function actorResultSchema(validateValue) {
  if (typeof validateValue !== "function") throw new TypeError("result value validator is required");
  return (payload) => {
    if (payload?.ok === true) {
      return exactObject(payload, { ok: actorField.boolean, value: validateValue }, "result payload");
    }
    if (payload?.ok === false) {
      return exactObject(payload, { error: actorField.string, ok: actorField.boolean }, "result payload");
    }
    throw new TypeError("result payload must be a discriminated ok result");
  };
}

export const actorField = Object.freeze({
  boolean(value, name) {
    if (typeof value !== "boolean") throw new TypeError(`${name} must be boolean`);
  },
  string(value, name) {
    if (typeof value !== "string") throw new TypeError(`${name} must be string`);
  },
  finiteNumber(value, name) {
    if (typeof value !== "number" || !Number.isFinite(value)) {
      throw new TypeError(`${name} must be a finite number`);
    }
  },
  float32Array(length) {
    return (value, name) => {
      if (!(value instanceof Float32Array) || value.length !== length) {
        throw new TypeError(`${name} must be Float32Array(${length})`);
      }
    };
  },
  int32Array(length) {
    return (value, name) => {
      if (!(value instanceof Int32Array) || value.length !== length) {
        throw new TypeError(`${name} must be Int32Array(${length})`);
      }
    };
  },
  uint8Array(length) {
    return (value, name) => {
      if (!(value instanceof Uint8Array) || value.length !== length) {
        throw new TypeError(`${name} must be Uint8Array(${length})`);
      }
    };
  },
});

/** A transport with the same callback boundary a future MessagePort adapter uses. */
export function createInProcessActorTransport(dispatch) {
  if (typeof dispatch !== "function") throw new TypeError("dispatch handler is required");
  let closed = false;
  return {
    send(request, deliver) {
      if (closed) throw new ActorEndpointClosedError("actor transport is closed");
      Promise.resolve()
        .then(() => dispatch(request))
        .then(
          (value) => deliver(actorEnvelope({
            type: "result", lane: request.lane, actorEpoch: request.actorEpoch,
            generation: request.generation, requestId: request.requestId,
            payload: { ok: true, value },
          })),
          (error) => deliver(actorEnvelope({
            type: "result", lane: request.lane, actorEpoch: request.actorEpoch,
            generation: request.generation, requestId: request.requestId,
            payload: { ok: false, error: String(error) },
          })),
        );
    },
    close() { closed = true; },
    reset() { closed = false; },
  };
}

export function createActorEndpoint({
  transport,
  requestSchema,
  resultSchema,
  initialEpoch = 0,
  maxPending = 32,
  onProtocolError = () => {},
}) {
  if (!transport || typeof transport.send !== "function") {
    throw new TypeError("actor transport must provide send(request, deliver)");
  }
  if (!Number.isSafeInteger(initialEpoch) || initialEpoch < 0) {
    throw new TypeError("initialEpoch must be a non-negative safe integer");
  }
  if (!Number.isSafeInteger(maxPending) || maxPending < 1) {
    throw new TypeError("maxPending must be a positive safe integer");
  }
  let epoch = initialEpoch;
  let closed = false;
  const pending = new Map();
  const seen = new Set();

  const rejectPending = (error) => {
    for (const entry of pending.values()) entry.reject(error);
    pending.clear();
  };

  const accept = (result) => {
    const candidateId = result && typeof result === "object"
      && Number.isSafeInteger(result.requestId) ? result.requestId : null;
    const candidateEpoch = result && typeof result === "object"
      && Number.isSafeInteger(result.actorEpoch) ? result.actorEpoch : null;
    const candidateEntry = candidateId === null || candidateEpoch !== epoch
      ? null
      : pending.get(candidateId);
    try {
      validateActorEnvelope(result);
      if (result.type !== "result") throw new TypeError("endpoint accepts only result envelopes");
    } catch (error) {
      if (candidateEntry) {
        pending.delete(candidateId);
        candidateEntry.reject(error);
      } else {
        try { onProtocolError(error, result); } catch { /* reporting cannot break transport */ }
      }
      return false;
    }
    if (closed || result.actorEpoch !== epoch) return false;
    const entry = pending.get(result.requestId);
    if (!entry) return false;
    try {
      if (result.lane !== entry.request.lane || result.generation !== entry.request.generation) {
        throw new TypeError("actor result does not correlate with its request");
      }
      validatePayload(resultSchema, result, "result");
    } catch (error) {
      pending.delete(result.requestId);
      entry.reject(error);
      return false;
    }
    pending.delete(result.requestId);
    entry.resolve(result);
    return true;
  };

  return {
    epoch: () => epoch,
    closed: () => closed,
    pendingCount: () => pending.size,
    accept,
    request(request) {
      validateActorEnvelope(request);
      if (request.type !== "request") throw new TypeError("endpoint sends only request envelopes");
      if (closed) return Promise.reject(new ActorEndpointClosedError());
      if (request.actorEpoch !== epoch) return Promise.reject(new ActorEndpointResetError());
      validatePayload(requestSchema, request, "request");
      if (pending.size >= maxPending) {
        return Promise.reject(new ActorEndpointBusyError());
      }
      if (seen.has(request.requestId)) {
        return Promise.reject(new TypeError("duplicate actor request ID in this epoch"));
      }
      seen.add(request.requestId);
      return new Promise((resolve, reject) => {
        pending.set(request.requestId, { request, resolve, reject });
        try {
          transport.send(request, accept);
        } catch (error) {
          pending.delete(request.requestId);
          reject(error);
        }
      });
    },
    close(reason = "actor endpoint closed") {
      if (closed) return;
      closed = true;
      rejectPending(new ActorEndpointClosedError(reason));
      try {
        transport.close?.(reason);
      } catch (error) {
        try { onProtocolError(error, { hook: "close", reason }); } catch { /* reporting is isolated */ }
      }
    },
    reset(reason = "actor endpoint reset") {
      if (epoch === Number.MAX_SAFE_INTEGER) throw new RangeError("actor epoch exhausted");
      rejectPending(new ActorEndpointResetError(reason));
      epoch += 1;
      closed = false;
      seen.clear();
      try {
        transport.reset?.(epoch);
      } catch (error) {
        try { onProtocolError(error, { hook: "reset", reason, epoch }); } catch { /* reporting is isolated */ }
      }
      return epoch;
    },
  };
}
