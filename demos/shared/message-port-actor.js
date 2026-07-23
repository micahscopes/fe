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
} = {}) {
  if (typeof dispatch !== "function") throw new TypeError("actor dispatch required");
  if (typeof transferResult !== "function") throw new TypeError("transferResult must be a function");
  const onMessage = (event) => {
    const request = event.data;
    try {
      validateActorEnvelope(request);
      if (request.type !== "request") throw new TypeError("worker host accepts requests only");
    } catch {
      return;
    }
    Promise.resolve().then(() => dispatch(request)).then(
      (value) => {
        try {
          const transfer = transferResult(value, request);
          if (!Array.isArray(transfer)) throw new TypeError("transferResult must return an array");
          port.postMessage(actorEnvelope({
            type: "result", lane: request.lane, actorEpoch: request.actorEpoch,
            generation: request.generation, requestId: request.requestId,
            payload: { ok: true, value },
          }), transfer);
        } catch (error) {
          port.postMessage(actorEnvelope({
            type: "result", lane: request.lane, actorEpoch: request.actorEpoch,
            generation: request.generation, requestId: request.requestId,
            payload: { ok: false, error: sanitizedActorError(error, "FE_ACTOR_TRANSFER") },
          }));
        }
      },
      (error) => port.postMessage(actorEnvelope({
        type: "result", lane: request.lane, actorEpoch: request.actorEpoch,
        generation: request.generation, requestId: request.requestId,
        payload: { ok: false, error: sanitizedActorError(error, "FE_ACTOR_HOST_DISPATCH") },
      })),
    );
  };
  port.addEventListener("message", onMessage);
  port.start?.();
  return () => {
    port.removeEventListener("message", onMessage);
    port.close?.();
  };
}
