// Generic JavaScript realization of the Fe host-runtime state machines.
//
// This module deliberately has no DOM, WebGPU, Worker, or other Web API
// knowledge. Generated adapters supply host objects and conversion functions.

export const HOST_RUNTIME_PROTOCOL = "fe:host-runtime/v1";

export class HostRuntimeError extends Error {
  constructor(code, message, details = {}) {
    super(message);
    this.name = "HostRuntimeError";
    this.code = code;
    this.details = Object.freeze({ ...details });
  }
}

class OpaqueTable {
  #kind;
  #slots = [];
  #free = [];
  #handles = new WeakMap();
  #live = 0;

  constructor(kind) {
    this.#kind = kind;
  }

  insert(value, onDrop, borrowed = false) {
    const slot = this.#free.pop() ?? this.#slots.length;
    const previous = this.#slots[slot];
    const generation = (previous?.generation ?? 0) + 1;
    const handle = Object.freeze(Object.create(null));
    this.#slots[slot] = {
      generation,
      value,
      onDrop,
      borrowed,
      live: true,
    };
    this.#handles.set(handle, { slot, generation });
    this.#live += 1;
    return handle;
  }

  #entry(handle) {
    const identity = this.#handles.get(handle);
    if (!identity) {
      throw new HostRuntimeError(
        "invalid_handle",
        `invalid or forged ${this.#kind} handle`,
        { kind: this.#kind },
      );
    }
    const entry = this.#slots[identity.slot];
    if (!entry?.live || entry.generation !== identity.generation) {
      throw new HostRuntimeError(
        "stale_handle",
        `stale ${this.#kind} handle`,
        { kind: this.#kind, slot: identity.slot, generation: identity.generation },
      );
    }
    return { entry, ...identity };
  }

  borrow(handle) {
    return this.#entry(handle).entry.value;
  }

  take(handle) {
    const current = this.#entry(handle);
    if (current.entry.borrowed) {
      throw new HostRuntimeError(
        "borrowed_handle_consumed",
        `borrowed ${this.#kind} handle cannot be consumed`,
        { kind: this.#kind },
      );
    }
    const value = current.entry.value;
    this.#retire(current, false);
    return value;
  }

  drop(handle) {
    const current = this.#entry(handle);
    if (current.entry.borrowed) {
      throw new HostRuntimeError(
        "borrowed_handle_consumed",
        `borrowed ${this.#kind} handle cannot be dropped`,
        { kind: this.#kind },
      );
    }
    this.#retire(current, true);
  }

  withBorrowed(value, callback) {
    if (typeof callback !== "function") {
      throw new TypeError("borrow scope requires a callback");
    }
    const handle = this.insert(value, undefined, true);
    let result;
    try {
      result = callback(handle);
    } catch (error) {
      this.#retire(this.#entry(handle), false);
      throw error;
    }
    if (result && typeof result.then === "function") {
      return Promise.resolve(result).finally(() => {
        this.#retire(this.#entry(handle), false);
      });
    }
    this.#retire(this.#entry(handle), false);
    return result;
  }

  #retire({ entry, slot }, notify) {
    entry.live = false;
    entry.value = undefined;
    this.#free.push(slot);
    this.#live -= 1;
    if (notify) entry.onDrop?.();
    entry.onDrop = undefined;
  }

  get liveCount() {
    return this.#live;
  }
}

function createCallbackTable() {
  const table = new OpaqueTable("callback");

  function finishInvocation(record, handle) {
    record.depth -= 1;
    if (record.depth === 0 && record.releaseRequested) table.drop(handle);
  }

  return Object.freeze({
    register(signatureId, callback) {
      if (typeof signatureId !== "string" || !signatureId) {
        throw new TypeError("callback signature id must be a non-empty string");
      }
      if (typeof callback !== "function") {
        throw new TypeError("callback must be a function");
      }
      return table.insert({
        signatureId,
        callback,
        depth: 0,
        releaseRequested: false,
      });
    },

    invoke(handle, signatureId, args = []) {
      const record = table.borrow(handle);
      if (record.releaseRequested) {
        throw new HostRuntimeError(
          "callback_released",
          "callback has been released",
          { signatureId },
        );
      }
      if (record.signatureId !== signatureId) {
        throw new HostRuntimeError(
          "callback_signature_mismatch",
          `callback expects ${record.signatureId}, received ${signatureId}`,
          { expected: record.signatureId, received: signatureId },
        );
      }
      record.depth += 1;
      let result;
      try {
        result = record.callback(...args);
      } catch (error) {
        finishInvocation(record, handle);
        throw error;
      }
      if (result && typeof result.then === "function") {
        return Promise.resolve(result).finally(() => finishInvocation(record, handle));
      }
      finishInvocation(record, handle);
      return result;
    },

    release(handle) {
      const record = table.borrow(handle);
      if (record.depth > 0) {
        record.releaseRequested = true;
      } else {
        table.drop(handle);
      }
    },

    get liveCount() {
      return table.liveCount;
    },
  });
}

function createFutureTable() {
  const table = new OpaqueTable("future");
  const defaultCancellation = () => {
    const error = new Error("The operation was cancelled");
    error.name = "AbortError";
    return error;
  };

  return Object.freeze({
    create() {
      let resolvePromise;
      let rejectPromise;
      const promise = new Promise((resolve, reject) => {
        resolvePromise = resolve;
        rejectPromise = reject;
      });
      // Tests and callers may intentionally observe cancellation rejections
      // later; prevent the host from reporting an unrelated unhandled promise.
      promise.catch(() => {});
      const record = {
        state: "pending",
        outcome: undefined,
        resolve: resolvePromise,
        reject: rejectPromise,
      };
      const token = table.insert(record);
      return Object.freeze({ token, promise });
    },

    settle(token, outcome) {
      const record = table.borrow(token);
      if (record.state !== "pending") {
        throw new HostRuntimeError(
          "future_already_completed",
          `future is already ${record.state}`,
          { state: record.state },
        );
      }
      const hasOk = Object.hasOwn(outcome || {}, "ok");
      const hasError = Object.hasOwn(outcome || {}, "error");
      if (hasOk === hasError) {
        throw new TypeError("future outcome must contain exactly one of `ok` or `error`");
      }
      record.state = hasOk ? "resolved" : "rejected";
      record.outcome = outcome;
      if (hasOk) record.resolve(outcome.ok);
      else record.reject(outcome.error);
      return true;
    },

    cancel(token, reason = defaultCancellation()) {
      const record = table.borrow(token);
      if (record.state !== "pending") {
        throw new HostRuntimeError(
          "future_already_completed",
          `future is already ${record.state}`,
          { state: record.state },
        );
      }
      record.state = "cancelled";
      record.outcome = { cancelled: reason };
      record.reject(reason);
      return true;
    },

    inspect(token) {
      const { state, outcome } = table.borrow(token);
      return Object.freeze({ state, outcome });
    },

    release(token) {
      table.drop(token);
    },

    get liveCount() {
      return table.liveCount;
    },
  });
}

function requireFutureToken(token) {
  if (!Number.isInteger(token) || token < -0x80000000 || token > 0x7fffffff) {
    throw new TypeError("future token must be a core-Wasm i32");
  }
  return token;
}

/// Report a protocol callback failure without silently swallowing it.
///
/// Injection points make both branches testable without causing an unhandled
/// exception in the test runner.
export function reportHostProtocolError(error, {
  reportError = globalThis.reportError,
  schedule = queueMicrotask,
} = {}) {
  if (typeof reportError === "function") {
    reportError(error);
  } else {
    schedule(() => { throw error; });
  }
}

// Promise settlement bridge for the transport-neutral async ABI.
//
// The caller supplies and owns each i32 token. This bridge only subscribes to
// the Promise and forwards one terminal signal to the supplied guest exports;
// it never implies Fe Future/await or a compiler resumable state machine.
export function createPromiseFutureBridge(exports, {
  onProtocolError = reportHostProtocolError,
} = {}) {
  for (const name of ["resolve", "reject", "cancel"]) {
    if (typeof exports?.[name] !== "function") {
      throw new TypeError(`future bridge requires a ${name}(token, value?) export`);
    }
  }
  if (typeof onProtocolError !== "function") {
    throw new TypeError("onProtocolError must be a function");
  }

  const subscriptions = new Map();
  const counters = {
    resolved: 0,
    rejected: 0,
    cancelled: 0,
    unsubscribed: 0,
    suppressedLate: 0,
    protocolErrors: 0,
  };

  function cleanup(token, record) {
    if (subscriptions.get(token) !== record) return false;
    subscriptions.delete(token);
    record.signal?.removeEventListener("abort", record.onAbort);
    return true;
  }

  function report(error, phase, token) {
    counters.protocolErrors += 1;
    onProtocolError(error, Object.freeze({ phase, token }));
  }

  function deliver(token, record, kind, value) {
    if (!cleanup(token, record)) {
      counters.suppressedLate += 1;
      return false;
    }
    counters[kind === "resolve" ? "resolved" : "rejected"] += 1;
    try {
      exports[kind](token, value);
    } catch (error) {
      report(error, kind, token);
    }
    return true;
  }

  function abort(token, reason) {
    requireFutureToken(token);
    const record = subscriptions.get(token);
    if (!record) {
      counters.suppressedLate += 1;
      return false;
    }
    cleanup(token, record);
    counters.cancelled += 1;
    // AbortSignal normally ends only this subscription. Underlying work is
    // cancelled exclusively when the caller supplied an owned cancellation
    // hook, making that authority explicit.
    if (record.ownedCancellation) {
      try {
        record.ownedCancellation(reason);
      } catch (error) {
        report(error, "owned_cancellation", token);
      }
    }
    try {
      exports.cancel(token, reason);
    } catch (error) {
      report(error, "cancel", token);
    }
    return true;
  }

  return Object.freeze({
    subscribe(token, promise, {
      signal,
      ownedCancellation,
    } = {}) {
      requireFutureToken(token);
      if (subscriptions.has(token)) {
        throw new HostRuntimeError(
          "future_token_in_use",
          "future token already has an active Promise subscription",
          { token },
        );
      }
      if (
        signal !== undefined
        && (typeof AbortSignal === "undefined" || !(signal instanceof AbortSignal))
      ) {
        throw new TypeError("future subscription signal must be an AbortSignal");
      }
      if (ownedCancellation !== undefined && typeof ownedCancellation !== "function") {
        throw new TypeError("ownedCancellation must be a function");
      }
      const record = {
        signal,
        ownedCancellation,
        onAbort: undefined,
      };
      record.onAbort = () => abort(token, signal.reason);
      subscriptions.set(token, record);
      if (signal?.aborted) {
        abort(token, signal.reason);
      } else {
        signal?.addEventListener("abort", record.onAbort, { once: true });
      }
      Promise.resolve(promise).then(
        (value) => deliver(token, record, "resolve", value),
        (error) => deliver(token, record, "reject", error),
      );
      return token;
    },

    // Unsubscribe is deliberately not cancellation: no guest terminal export
    // and no underlying abort hook runs. The token owner must retire or reuse
    // its own state separately.
    unsubscribe(token) {
      requireFutureToken(token);
      const record = subscriptions.get(token);
      if (!record) return false;
      cleanup(token, record);
      counters.unsubscribed += 1;
      return true;
    },

    abort,

    inventory() {
      return Object.freeze({
        active: subscriptions.size,
        ...counters,
      });
    },
  });
}

// Bounded async-iterator protocol over the existing caller-owned i32
// FutureToken/Promise bridge. Iterator resources own cancellation authority;
// exactly one `next` subscription may be active per resource.
export function createAsyncIteratorFutureBridge(resources, exports, options) {
  if (!resources || typeof resources.insert !== "function"
      || typeof resources.borrow !== "function"
      || typeof resources.drop !== "function") {
    throw new TypeError("async iterator bridge requires a resource table");
  }
  const futures = createPromiseFutureBridge(exports, options);

  function close(state, reason) {
    if (state.closed) return;
    state.closed = true;
    if (typeof state.iterator.return === "function") {
      Promise.resolve(state.iterator.return(reason)).catch(() => {});
    }
  }

  return Object.freeze({
    create(source, ...args) {
      const iterator = source?.[Symbol.asyncIterator]?.(...args);
      if (!iterator || typeof iterator.next !== "function") {
        throw new TypeError("Symbol.asyncIterator must return an async iterator");
      }
      const state = { iterator, pending: undefined, closed: false };
      return resources.insert(state, () => {
        const pending = state.pending;
        state.pending = undefined;
        if (pending !== undefined) futures.abort(pending, new Error("async iterator dropped"));
        close(state);
      });
    },

    next(handle, token, { signal } = {}) {
      const state = resources.borrow(handle);
      if (state.pending !== undefined) {
        throw new HostRuntimeError(
          "async_iterator_backpressure",
          "async iterator permits exactly one in-flight next",
          { token: state.pending },
        );
      }
      if (state.closed) {
        futures.subscribe(token, Promise.resolve(null), { signal });
        return token;
      }
      state.pending = token;
      const promise = Promise.resolve()
        .then(() => state.iterator.next())
        .then((step) => {
          if (!step || typeof step.done !== "boolean") {
            throw new TypeError("async iterator next must return an iterator result");
          }
          if (step.done) close(state);
          return step.done ? null : step.value;
        })
        .finally(() => {
          if (state.pending === token) state.pending = undefined;
        });
      futures.subscribe(token, promise, {
        signal,
        ownedCancellation: (reason) => close(state, reason),
      });
      return token;
    },

    cancel(handle, token, reason) {
      const state = resources.borrow(handle);
      if (state.pending !== token) return false;
      state.pending = undefined;
      return futures.abort(token, reason);
    },

    drop(handle) {
      resources.drop(handle);
    },

    inventory: futures.inventory,
  });
}

export function createFeHostRuntime() {
  const resources = new OpaqueTable("resource");
  const callbacks = createCallbackTable();
  const futures = createFutureTable();
  return Object.freeze({
    protocol: HOST_RUNTIME_PROTOCOL,
    resources: Object.freeze({
      insert: resources.insert.bind(resources),
      borrow: resources.borrow.bind(resources),
      take: resources.take.bind(resources),
      drop: resources.drop.bind(resources),
      withBorrowed: resources.withBorrowed.bind(resources),
      get liveCount() {
        return resources.liveCount;
      },
    }),
    callbacks,
    futures,
    createFutureBridge: createPromiseFutureBridge,
    createAsyncIteratorBridge(exports, options) {
      return createAsyncIteratorFutureBridge(this.resources, exports, options);
    },
    inventory() {
      return Object.freeze({
        resources: resources.liveCount,
        callbacks: callbacks.liveCount,
        futures: futures.liveCount,
      });
    },
  });
}
