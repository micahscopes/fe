import { actorEnvelope, validateActorEnvelope } from "./actor-coordinator.js";

const keyOf = ({ actorEpoch, requestId }) => `${actorEpoch}:${requestId}`;

export function createMessagePortActorTransport(port) {
  if (!port || typeof port.postMessage !== "function") throw new TypeError("MessagePort required");
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
      deliveries.set(keyOf(request), { request, deliver });
      try { port.postMessage(request); } catch (error) {
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

export function attachMessagePortActorHost(port, dispatch) {
  if (typeof dispatch !== "function") throw new TypeError("actor dispatch required");
  const onMessage = (event) => {
    const request = event.data;
    try {
      validateActorEnvelope(request);
      if (request.type !== "request") throw new TypeError("worker host accepts requests only");
    } catch {
      return;
    }
    Promise.resolve().then(() => dispatch(request)).then(
      (value) => port.postMessage(actorEnvelope({
        type: "result", lane: request.lane, actorEpoch: request.actorEpoch,
        generation: request.generation, requestId: request.requestId,
        payload: { ok: true, value },
      })),
      (error) => port.postMessage(actorEnvelope({
        type: "result", lane: request.lane, actorEpoch: request.actorEpoch,
        generation: request.generation, requestId: request.requestId,
        payload: { ok: false, error: String(error) },
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
