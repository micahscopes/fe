import { actorEnvelope, validateActorEnvelope } from "./actor-coordinator.js";

const keyOf = ({ actorEpoch, requestId }) => `${actorEpoch}:${requestId}`;
const sanitizedActorError = (error, fallback) => {
  const match = /^FE_ACTOR_[A-Z_]+/.exec(
    typeof error?.message === "string" ? error.message : "",
  );
  return match?.[0] ?? fallback;
};

export function createMessagePortActorTransport(port, {
  transferRequest = () => [],
} = {}) {
  if (!port || typeof port.postMessage !== "function") throw new TypeError("MessagePort required");
  if (typeof transferRequest !== "function") {
    throw new TypeError("transferRequest must be a function");
  }
  const deliveries = new Map();
  let closed = false;
  const failAll = (reason) => {
    for (const [key, { request, deliver }] of deliveries) {
      deliveries.delete(key);
      deliver(actorEnvelope({
        type: "result", lane: request.lane, actorEpoch: request.actorEpoch,
        generation: request.generation, requestId: request.requestId,
        payload: { ok: false, error: String(reason) },
      }));
    }
  };
  const onMessage = (event) => {
    const result = event.data;
    const correlatable = result && typeof result === "object"
      && Number.isSafeInteger(result.actorEpoch) && Number.isSafeInteger(result.requestId);
    if (!correlatable) return;
    const entry = deliveries.get(keyOf(result));
    if (!entry) return;
    deliveries.delete(keyOf(result));
    entry.deliver(result);
  };
  const onMessageError = () => failAll("MessagePort messageerror");
  port.addEventListener("message", onMessage);
  port.addEventListener("messageerror", onMessageError);
  port.start?.();
  return {
    send(request, deliver) {
      if (closed) throw new Error("MessagePort actor transport is closed");
      const key = keyOf(request);
      if (deliveries.has(key)) throw new TypeError("duplicate in-flight actor request");
      const transfer = transferRequest(request.payload, request);
      if (!Array.isArray(transfer)) throw new TypeError("transferRequest must return an array");
      deliveries.set(key, { request, deliver });
      try { port.postMessage(request, transfer); } catch (error) {
        deliveries.delete(keyOf(request));
        throw error;
      }
    },
    cancel(request) {
      if (closed) return;
      port.postMessage(actorEnvelope({
        type: "cancel", lane: request.lane, actorEpoch: request.actorEpoch,
        generation: request.generation, requestId: request.requestId, payload: null,
      }));
    },
    fail: failAll,
    close(reason = "MessagePort actor transport closed") {
      if (closed) return;
      closed = true;
      failAll(reason);
      port.removeEventListener("message", onMessage);
      port.removeEventListener("messageerror", onMessageError);
      port.close?.();
    },
  };
}

// Transfer a typed array only when its view owns the whole backing buffer.
// Transferring a subview could expose unrelated bytes and detach storage still
// owned by another value, so callers must copy such views before opting in.
export function transferOwnedTypedArray(value) {
  if (!ArrayBuffer.isView(value) || value instanceof DataView
      || !(value.buffer instanceof ArrayBuffer)
      || value.byteOffset !== 0 || value.byteLength !== value.buffer.byteLength) {
    throw new TypeError("transfer result must be a full-span owned typed array");
  }
  return [value.buffer];
}

export function attachMessagePortActorHost(port, dispatch, {
  transferResult = () => [],
  maxInFlight = 32,
} = {}) {
  if (typeof dispatch !== "function") throw new TypeError("actor dispatch required");
  if (typeof transferResult !== "function") throw new TypeError("transferResult must be a function");
  if (!Number.isSafeInteger(maxInFlight) || maxInFlight < 1) {
    throw new TypeError("maxInFlight must be a positive safe integer");
  }
  const inFlight = new Map();
  const reply = (request, payload, transfer = []) => {
    port.postMessage(actorEnvelope({
      type: "result", lane: request.lane, actorEpoch: request.actorEpoch,
      generation: request.generation, requestId: request.requestId, payload,
    }), transfer);
  };
  const onMessage = (event) => {
    const request = event.data;
    try {
      validateActorEnvelope(request);
      if (request.type === "cancel") {
        const entry = inFlight.get(keyOf(request));
        if (entry
            && entry.request.lane === request.lane
            && entry.request.generation === request.generation) {
          entry.cancelled = true;
          entry.controller.abort();
        }
        return;
      }
      if (request.type !== "request") throw new TypeError("worker host accepts requests only");
    } catch {
      // A structurally valid correlation tuple may still arrive in a malformed
      // envelope (for example, with surplus fields). Reply when it is safe to
      // do so instead of silently leaving the remote endpoint pending forever.
      // actorEnvelope is the gate: wholly uncorrelatable input remains ignored.
      try {
        port.postMessage(actorEnvelope({
          type: "result",
          lane: request?.lane,
          actorEpoch: request?.actorEpoch,
          generation: request?.generation,
          requestId: request?.requestId,
          payload: { ok: false, error: "FE_ACTOR_PROTOCOL" },
        }));
      } catch {
        // There is no trustworthy request identity to answer.
      }
      return;
    }
    if (inFlight.size >= maxInFlight) {
      reply(request, { ok: false, error: "FE_ACTOR_BUSY" });
      return;
    }
    const key = keyOf(request);
    if (inFlight.has(key)) {
      reply(request, { ok: false, error: "FE_ACTOR_PROTOCOL" });
      return;
    }
    const entry = { request, controller: new AbortController(), cancelled: false };
    inFlight.set(key, entry);
    Promise.resolve().then(() => dispatch(request, {
      signal: entry.controller.signal,
    })).then(
      (value) => {
        inFlight.delete(key);
        if (entry.cancelled) return;
        try {
          const transfer = transferResult(value, request);
          if (!Array.isArray(transfer)) throw new TypeError("transferResult must return an array");
          reply(request, { ok: true, value }, transfer);
        } catch (error) {
          reply(request, {
            ok: false, error: sanitizedActorError(error, "FE_ACTOR_TRANSFER"),
          });
        }
      },
      (error) => {
        inFlight.delete(key);
        if (!entry.cancelled) {
          reply(request, {
            ok: false, error: sanitizedActorError(error, "FE_ACTOR_HOST_DISPATCH"),
          });
        }
      },
    );
  };
  port.addEventListener("message", onMessage);
  port.start?.();
  return () => {
    for (const entry of inFlight.values()) entry.controller.abort();
    inFlight.clear();
    port.removeEventListener("message", onMessage);
    port.close?.();
  };
}
