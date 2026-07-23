import { createActorEndpoint } from "./actor-endpoint.js";
import { createMessagePortActorTransport } from "./message-port-actor.js";
import { actorEnvelope } from "./actor-coordinator.js";

const INIT_TIMEOUT = 10_000;
const runtimeError = (code, message) => {
  const error = new Error(`${code}: ${message}`);
  error.name = "ModuleWorkerActorError";
  error.code = code;
  return error;
};

export async function createModuleWorkerActor({
  workerUrl, init = {}, adapter, requestSchema, resultSchema,
  maxPending = 32, initTimeoutMs = INIT_TIMEOUT,
  createAuxiliaryPorts = () => ({ message: {}, transfer: [], close() {} }),
  WorkerCtor = Worker, MessageChannelCtor = MessageChannel,
}) {
  if (adapter !== undefined) {
    if (!adapter?.requestSchema || !adapter?.resultSchema
        || typeof adapter.transferRequest !== "function") {
      throw new TypeError("canonical actor adapter with explicit transfer policy required");
    }
    requestSchema = adapter.requestSchema;
    resultSchema = adapter.resultSchema;
  }
  if (!requestSchema || !resultSchema) throw new TypeError("actor schemas required");
  if (!Number.isSafeInteger(initTimeoutMs) || initTimeoutMs < 1) {
    throw new TypeError("initTimeoutMs must be a positive safe integer");
  }
  let epoch = 0;
  let worker;
  let endpoint;
  let auxiliary;
  let closed = false;
  let transition = Promise.resolve();
  const start = async () => {
    worker = new WorkerCtor(workerUrl, { type: "module" });
    const channel = new MessageChannelCtor();
    const transport = createMessagePortActorTransport(channel.port1, {
      transferRequest: adapter?.transferRequest,
    });
    endpoint = createActorEndpoint({
      transport, initialEpoch: epoch, requestSchema, resultSchema, maxPending,
    });
    try {
      auxiliary = createAuxiliaryPorts(epoch);
      if (!auxiliary || typeof auxiliary !== "object"
          || !Array.isArray(auxiliary.transfer ?? [])) {
        throw new TypeError("invalid auxiliary port bundle");
      }
    } catch {
      endpoint.close("module worker initialization failed");
      worker.terminate();
      throw runtimeError("FE_ACTOR_WORKER_INIT", "module worker initialization failed");
    }
    let cancelReady = () => {};
    const ready = new Promise((resolve, reject) => {
      const cleanup = () => {
        clearTimeout(timeout);
        channel.port1.removeEventListener("message", onMessage);
        worker.removeEventListener?.("error", onError);
      };
      cancelReady = cleanup;
      const rejectCleanly = (error) => { cleanup(); reject(error); };
      const onMessage = (event) => {
        const message = event.data;
        if (message?.type === "ready" && Object.keys(message).length === 1) {
          cleanup(); resolve(); return;
        }
        if (message?.type === "init-error"
            && message.error === "FE_ACTOR_WORKER_INIT"
            && Object.keys(message).sort().join("\0") === "error\0type") {
          rejectCleanly(runtimeError("FE_ACTOR_WORKER_INIT", "module worker initialization failed"));
          return;
        }
        rejectCleanly(runtimeError("FE_ACTOR_WORKER_PROTOCOL", "malformed readiness message"));
      };
      const onError = () => rejectCleanly(
        runtimeError("FE_ACTOR_WORKER_INIT", "module worker initialization failed"),
      );
      const timeout = setTimeout(() => rejectCleanly(
        runtimeError("FE_ACTOR_WORKER_TIMEOUT", "module worker readiness timed out"),
      ), initTimeoutMs);
      channel.port1.addEventListener("message", onMessage);
      worker.addEventListener("error", onError);
    });
    worker.addEventListener("error", () => transport.fail("FE_ACTOR_WORKER_RUNTIME"));
    try {
      worker.postMessage({ ...init, ...auxiliary.message, type: "init",
        port: channel.port2, actorEpoch: epoch }, [channel.port2, ...(auxiliary.transfer || [])]);
      await ready;
    } catch (error) {
      cancelReady();
      endpoint.close("module worker initialization failed");
      auxiliary.close?.();
      worker.terminate();
      throw error?.name === "ModuleWorkerActorError"
        ? error
        : runtimeError("FE_ACTOR_WORKER_INIT", "module worker initialization failed");
    }
  };
  await start();
  const restart = () => {
    const operation = transition.catch(() => {}).then(async () => {
      if (closed) throw runtimeError("FE_ACTOR_CLOSED", "module worker actor is closed");
      endpoint.close("restarting module worker");
      auxiliary.close?.();
      worker.terminate();
      epoch += 1;
      await start();
      return epoch;
    });
    transition = operation;
    return operation;
  };
  return Object.freeze({
    request: (envelope, options) => endpoint.request(envelope, options),
    restart,
    close() {
      if (closed) return;
      closed = true;
      endpoint.close();
      auxiliary.close?.();
      worker.terminate();
    },
    epoch: () => epoch,
    pendingCount: () => endpoint.pendingCount(),
  });
}

// Canonical applications provide the compiler-derived actor shape. This layer
// owns wire IDs/epochs and never exposes worker error text across the boundary.
export async function createCanonicalModuleWorkerActor(options) {
  if (!options?.adapter) {
    throw new TypeError("compiler-derived canonical actor adapter required");
  }
  const actor = await createModuleWorkerActor(options);
  let requestId = 0;
  let restartTail = Promise.resolve();
  return Object.freeze({
    async request(lane, payload, generation = 0, options) {
      // Capture the restart chain at call time. Requests made after restart()
      // wait for the replacement worker to become ready, while requests made
      // before restart retain their original epoch/lifecycle semantics.
      await restartTail;
      const result = await actor.request(actorEnvelope({
        type: "request", lane, payload, generation,
        actorEpoch: actor.epoch(), requestId: ++requestId,
      }), options);
      if (result.payload.ok) return result.payload.value;
      const match = /^FE_ACTOR_[A-Z_]+/.exec(result.payload.error);
      const code = match?.[0] ?? "FE_ACTOR_REMOTE";
      throw runtimeError(code, "canonical worker request failed");
    },
    restart() {
      const operation = restartTail.catch(() => {}).then(async () => {
        const next = await actor.restart();
        requestId = 0;
        return next;
      });
      restartTail = operation;
      return operation;
    },
    close: actor.close,
    epoch: actor.epoch,
    pendingCount: actor.pendingCount,
  });
}
