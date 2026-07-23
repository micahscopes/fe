import { createActorEndpoint } from "./actor-endpoint.js";
import { createMessagePortActorTransport } from "./message-port-actor.js";
import { actorEnvelope } from "./actor-coordinator.js";

const INIT_TIMEOUT = 10_000;
const MAX_TIMER_DELAY = 2_147_483_647;
const runtimeError = (code, message) => {
  const error = new Error(`${code}: ${message}`);
  error.name = "ModuleWorkerActorError";
  error.code = code;
  return error;
};

const supervisionPolicy = (value) => {
  if (value === undefined) {
    return Object.freeze({
      maxRestarts: 0, windowMs: 1, backoffMs: 0, observe() {},
      now: Date.now, schedule: setTimeout, cancel: clearTimeout, configured: false,
    });
  }
  if (!value || typeof value !== "object") {
    throw new TypeError("supervision must be an object");
  }
  const {
    maxRestarts, windowMs, backoffMs, observe = () => {},
    now = Date.now, schedule = setTimeout, cancel = clearTimeout,
  } = value;
  if (!Number.isSafeInteger(maxRestarts) || maxRestarts < 0) {
    throw new TypeError("supervision.maxRestarts must be a non-negative safe integer");
  }
  if (!Number.isSafeInteger(windowMs) || windowMs < 1) {
    throw new TypeError("supervision.windowMs must be a positive safe integer");
  }
  if (!Number.isSafeInteger(backoffMs) || backoffMs < 0 || backoffMs > MAX_TIMER_DELAY) {
    throw new TypeError("supervision.backoffMs must be a bounded non-negative safe integer");
  }
  if (typeof observe !== "function" || typeof now !== "function"
      || typeof schedule !== "function" || typeof cancel !== "function") {
    throw new TypeError("supervision hooks must be functions");
  }
  return Object.freeze({
    maxRestarts, windowMs, backoffMs, observe, now, schedule, cancel, configured: true,
  });
};

export async function createModuleWorkerActor({
  workerUrl, init = {}, adapter, requestSchema, resultSchema,
  maxPending = 32, initTimeoutMs = INIT_TIMEOUT,
  supervision,
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
  const policy = supervisionPolicy(supervision);
  let epoch = 0;
  let active;
  let closed = false;
  let terminal;
  let transition = Promise.resolve();
  // Manual restart calls reserve replacement ownership synchronously so a
  // same-turn crash of the retiring worker cannot also launch automatic
  // recovery. Automatic recovery must never count itself here: a replacement
  // can fail at the ready boundary and still needs another bounded attempt.
  let queuedManualRestarts = 0;
  let pendingDelay;
  const attempts = [];
  const emit = (type, fields = {}) => {
    const event = Object.freeze({ type, epoch, ...fields });
    const observe = policy.observe;
    try { observe(event); } catch { /* observation cannot affect supervision */ }
  };
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
      if (candidate.retired || active !== candidate || closed || terminal) return;
      candidate.transport.fail("FE_ACTOR_WORKER_RUNTIME");
      retire(candidate, "module worker crashed");
      emit("failure", { classification: "runtime", code: "FE_ACTOR_WORKER_RUNTIME" });
      // An already-queued explicit restart owns the replacement. Starting a
      // second recovery here would construct two workers for one epoch.
      if (queuedManualRestarts > 0) return;
      transition = recover("runtime");
      transition.catch(() => {});
    };
    worker.addEventListener("error", candidate.onCrash);
    active = candidate;
    emit("ready");
  };

  const waitBackoff = () => {
    if (policy.backoffMs === 0) return Promise.resolve();
    return new Promise((resolve, reject) => {
      const schedule = policy.schedule;
      const timer = schedule(() => {
        if (pendingDelay?.timer !== timer) return;
        pendingDelay = undefined;
        resolve();
      }, policy.backoffMs);
      pendingDelay = { timer, reject };
    });
  };
  const nextRestart = () => {
    const nowFn = policy.now;
    const now = nowFn();
    if (!Number.isFinite(now)) throw new TypeError("supervision.now must return a finite number");
    while (attempts.length > 0 && now - attempts[0] >= policy.windowMs) attempts.shift();
    if (attempts.length >= policy.maxRestarts) return null;
    attempts.push(now);
    return attempts.length;
  };
  const recover = async (classification) => {
    let failureClass = classification;
    while (!closed) {
      const attempt = nextRestart();
      if (attempt === null) {
        terminal = runtimeError(
          "FE_ACTOR_WORKER_TERMINAL",
          "module worker restart policy exhausted",
        );
        emit("terminal", {
          classification: failureClass,
          code: terminal.code,
          attempts: attempts.length,
        });
        throw terminal;
      }
      if (epoch === Number.MAX_SAFE_INTEGER) {
        terminal = runtimeError("FE_ACTOR_WORKER_TERMINAL", "module worker epoch exhausted");
        emit("terminal", {
          classification: failureClass, code: terminal.code, attempts: attempts.length,
        });
        throw terminal;
      }
      epoch += 1;
      emit("backoff", { classification: failureClass, attempt, delayMs: policy.backoffMs });
      await waitBackoff();
      if (closed) throw runtimeError("FE_ACTOR_CLOSED", "module worker actor is closed");
      emit("restart", { classification: failureClass, attempt });
      try {
        await start();
        return epoch;
      } catch (error) {
        if (closed) throw runtimeError("FE_ACTOR_CLOSED", "module worker actor is closed");
        failureClass = "startup";
        emit("failure", {
          classification: "startup",
          code: error?.code ?? "FE_ACTOR_WORKER_INIT",
        });
      }
    }
    throw runtimeError("FE_ACTOR_CLOSED", "module worker actor is closed");
  };

  try {
    await start();
  } catch (error) {
    emit("failure", {
      classification: "startup", code: error?.code ?? "FE_ACTOR_WORKER_INIT",
    });
    if (!policy.configured) throw error;
    transition = recover("startup");
    await transition;
  }

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
      if (terminal) throw terminal;
      retire(active, "restarting module worker");
      if (epoch === Number.MAX_SAFE_INTEGER) {
        throw runtimeError("FE_ACTOR_WORKER_TERMINAL", "module worker epoch exhausted");
      }
      epoch += 1;
      emit("restart", { classification: "manual", attempt: 0 });
      try {
        await start();
        return epoch;
      } catch (error) {
        emit("failure", {
          classification: "startup", code: error?.code ?? "FE_ACTOR_WORKER_INIT",
        });
        if (!policy.configured) throw error;
        return recover("startup");
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
      if (terminal) throw terminal;
      return epoch;
    },
    async request(envelope, options = {}) {
      await awaitTransition(options.signal);
      if (closed) throw runtimeError("FE_ACTOR_CLOSED", "module worker actor is closed");
      if (terminal) throw terminal;
      return active.endpoint.request(envelope, options);
    },
    restart,
    close() {
      if (closed) return;
      closed = true;
      if (pendingDelay) {
        const { timer, reject } = pendingDelay;
        pendingDelay = undefined;
        const cancel = policy.cancel;
        cancel(timer);
        reject(runtimeError("FE_ACTOR_CLOSED", "module worker actor is closed"));
      }
      retire(active, "module worker actor is closed");
      emit("close");
    },
    epoch: () => epoch,
    pendingCount: () => active?.endpoint.pendingCount() ?? 0,
    status: () => Object.freeze({
      state: closed ? "closed" : terminal ? "terminal"
        : active ? "ready" : pendingDelay ? "backoff" : "starting",
      epoch,
      restartsInWindow: attempts.length,
      terminalCode: terminal?.code ?? null,
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
      await awaitRestart(restartTail, options?.signal);
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
