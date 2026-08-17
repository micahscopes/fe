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

export async function createModuleWorkerActor(options) {
  if (!options || typeof options !== "object" || Array.isArray(options)) {
    throw new TypeError("module Worker actor options must be an object");
  }
  const {
    workerUrl, init = {}, adapter,
    requestSchema: suppliedRequestSchema,
    resultSchema: suppliedResultSchema,
    maxPending = 32, initTimeoutMs = INIT_TIMEOUT,
    createAuxiliaryPorts = () => ({ message: {}, transfer: [], close() {} }),
    WorkerCtor = Worker, MessageChannelCtor = MessageChannel,
    ...unknown
  } = options;
  if (Object.keys(unknown).length !== 0) {
    throw new TypeError(
      `module Worker actor received unsupported options: ${Object.keys(unknown).sort().join(", ")}`,
    );
  }
  let requestSchema = suppliedRequestSchema;
  let resultSchema = suppliedResultSchema;
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
  let active;
  let closed = false;
  let failure;
  let transition = Promise.resolve();
  // Manual restart calls reserve replacement ownership synchronously so a
  // same-turn crash of the retiring worker cannot publish a competing failed
  // lifecycle. JavaScript never decides whether another restart should occur.
  let queuedManualRestarts = 0;
  const retire = (instance, reason) => {
    if (!instance || instance.retired) return;
    instance.retired = true;
    if (instance.onCrash) {
      instance.worker.removeEventListener?.("error", instance.onCrash);
    }
    instance.endpoint.close(reason);
    instance.auxiliary.close?.();
    instance.worker.terminate();
    if (active === instance) active = undefined;
  };
  const start = async () => {
    const worker = new WorkerCtor(workerUrl, { type: "module" });
    const channel = new MessageChannelCtor();
    const transport = createMessagePortActorTransport(channel.port1, {
      transferRequest: adapter?.transferRequest,
    });
    const endpoint = createActorEndpoint({
      transport, initialEpoch: epoch, requestSchema, resultSchema, maxPending,
    });
    let auxiliary;
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
    const candidate = {
      worker, endpoint, transport, auxiliary, retired: false, onCrash: undefined,
    };
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
    try {
      worker.postMessage({ ...init, ...auxiliary.message, type: "init",
        port: channel.port2, actorEpoch: epoch }, [channel.port2, ...(auxiliary.transfer || [])]);
      await ready;
    } catch (error) {
      cancelReady();
      retire(candidate, "module worker initialization failed");
      throw error?.name === "ModuleWorkerActorError"
        ? error
        : runtimeError("FE_ACTOR_WORKER_INIT", "module worker initialization failed");
    }
    if (closed) {
      retire(candidate, "module worker actor is closed");
      throw runtimeError("FE_ACTOR_CLOSED", "module worker actor is closed");
    }
    candidate.onCrash = () => {
      if (candidate.retired || active !== candidate || closed) return;
      candidate.transport.fail("FE_ACTOR_WORKER_RUNTIME");
      retire(candidate, "module worker crashed");
      // An already-queued explicit restart owns the replacement lifecycle.
      if (queuedManualRestarts > 0) return;
      failure = runtimeError("FE_ACTOR_WORKER_RUNTIME", "module worker crashed");
      transition = Promise.reject(failure);
      transition.catch(() => {});
    };
    worker.addEventListener("error", candidate.onCrash);
    active = candidate;
  };

  await start();

  const awaitTransition = (signal) => {
    if (signal?.aborted) {
      return Promise.reject(runtimeError("FE_ACTOR_ABORTED", "actor request aborted"));
    }
    if (!signal) return transition;
    return new Promise((resolve, reject) => {
      const onAbort = () => {
        signal.removeEventListener("abort", onAbort);
        reject(runtimeError("FE_ACTOR_ABORTED", "actor request aborted"));
      };
      signal.addEventListener("abort", onAbort, { once: true });
      transition.then(
        (value) => { signal.removeEventListener("abort", onAbort); resolve(value); },
        (error) => { signal.removeEventListener("abort", onAbort); reject(error); },
      );
    });
  };
  const restart = () => {
    queuedManualRestarts += 1;
    const operation = transition.catch(() => {}).then(async () => {
      // This operation now owns the transition. Release only its reservation;
      // later manual restart calls remain reserved and still suppress a crash
      // race on the worker this operation is replacing.
      queuedManualRestarts -= 1;
      if (closed) throw runtimeError("FE_ACTOR_CLOSED", "module worker actor is closed");
      retire(active, "restarting module worker");
      if (epoch === Number.MAX_SAFE_INTEGER) {
        failure = runtimeError("FE_ACTOR_WORKER_TERMINAL", "module worker epoch exhausted");
        throw failure;
      }
      epoch += 1;
      failure = undefined;
      try {
        await start();
        return epoch;
      } catch (error) {
        failure = error?.name === "ModuleWorkerActorError"
          ? error
          : runtimeError("FE_ACTOR_WORKER_INIT", "module worker initialization failed");
        throw failure;
      }
    });
    transition = operation;
    transition.catch(() => {});
    return operation;
  };
  return Object.freeze({
    async ready(options = {}) {
      await awaitTransition(options.signal);
      if (closed) throw runtimeError("FE_ACTOR_CLOSED", "module worker actor is closed");
      if (failure) throw failure;
      return epoch;
    },
    async request(envelope, options = {}) {
      await awaitTransition(options.signal);
      if (closed) throw runtimeError("FE_ACTOR_CLOSED", "module worker actor is closed");
      if (failure) throw failure;
      if (!active) throw runtimeError("FE_ACTOR_WORKER_RUNTIME", "module worker is unavailable");
      return active.endpoint.request(envelope, options);
    },
    restart,
    close() {
      if (closed) return;
      closed = true;
      retire(active, "module worker actor is closed");
    },
    epoch: () => epoch,
    pendingCount: () => active?.endpoint.pendingCount() ?? 0,
    status: () => Object.freeze({
      state: closed ? "closed" : failure ? "failed" : active ? "ready" : "starting",
      epoch,
      failureCode: failure?.code ?? null,
    }),
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
  // A restart is a synchronous lifecycle fence even though replacement
  // construction is asynchronous. This distinguishes calls already in flight
  // from calls intentionally issued after restart() and waiting on its tail.
  let lifecycleVersion = 0;
  const awaitRestart = (operation, signal) => {
    if (!signal) return operation;
    if (signal.aborted) {
      return Promise.reject(runtimeError("FE_ACTOR_ABORTED", "actor request aborted"));
    }
    return new Promise((resolve, reject) => {
      const onAbort = () => {
        signal.removeEventListener("abort", onAbort);
        reject(runtimeError("FE_ACTOR_ABORTED", "actor request aborted"));
      };
      signal.addEventListener("abort", onAbort, { once: true });
      operation.then(
        (value) => { signal.removeEventListener("abort", onAbort); resolve(value); },
        (error) => { signal.removeEventListener("abort", onAbort); reject(error); },
      );
    });
  };
  return Object.freeze({
    async request(lane, payload, generation = 0, options) {
      // Capture the restart chain at call time. Requests made after restart()
      // wait for the replacement worker to become ready, while requests made
      // before restart retain their original epoch/lifecycle semantics.
      const requestLifecycle = lifecycleVersion;
      const requestRestartTail = restartTail;
      await awaitRestart(requestRestartTail, options?.signal);
      if (requestLifecycle !== lifecycleVersion) {
        throw runtimeError("FE_ACTOR_WORKER_RESTART", "restarting module worker");
      }
      await actor.ready(options);
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
      // Reserve the underlying lifecycle transition synchronously. Besides
      // preserving call order, this lets a same-turn crash observe that the
      // explicit restart already owns replacement construction.
      lifecycleVersion += 1;
      const operation = actor.restart().then((next) => {
        requestId = 0;
        return next;
      });
      restartTail = operation;
      return operation;
    },
    close: actor.close,
    epoch: actor.epoch,
    pendingCount: actor.pendingCount,
    status: actor.status,
  });
}
