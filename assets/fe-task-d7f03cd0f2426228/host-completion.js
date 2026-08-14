// Fixed browser realization for std::host's broker-completed Timer/Recv rail.
//
// Fe owns the task body, outcome matching, retry/cancellation policy, and every
// subsequent suspension. This module owns only standards clocks/timers, the
// finite pending-token table, FIFO receive delivery, and driving the opaque
// compiler-generated continuation machine.

import {
  raceTaskOutcome,
  taskCancelled,
  taskFailure,
  taskSuccess,
} from "./materialized-task.js";

const MAX_U32 = 0xffff_ffff;
const MAX_U64 = (1n << 64n) - 1n;
const MAX_TIMER_CHUNK_MS = 0x7fff_ffffn;

export class FeTaskCancelled extends Error {
  constructor() {
    super("Fe task was cancelled");
    this.name = "AbortError";
  }
}

function u32(value, name) {
  if (!Number.isInteger(value) || value < 0 || value > MAX_U32) {
    throw new TypeError(`${name} must be a u32`);
  }
  return value;
}

function u64(value, name) {
  if (typeof value !== "bigint" || value < 0n || value > MAX_U64) {
    throw new TypeError(`${name} must be a u64 bigint`);
  }
  return value;
}

function defaultClock() {
  if (typeof performance === "undefined" || typeof performance.now !== "function") {
    throw new Error("fe host completion runtime requires a monotonic performance.now clock");
  }
  return BigInt(Math.max(0, Math.trunc(performance.now())));
}

function pendingToken(pending) {
  if (!pending || typeof pending !== "object" || Array.isArray(pending)) {
    throw new TypeError("Fe host pending operation must be an object");
  }
  const keys = Object.keys(pending).sort();
  if (keys.join("\0") !== "handler\0lanes"
      || !pending.handler || typeof pending.handler !== "object"
      || !Array.isArray(pending.lanes) || pending.lanes.length !== 1) {
    throw new TypeError("Fe host pending operation has the wrong compiler-derived shape");
  }
  return u32(pending.lanes[0], "Fe host pending token");
}

function abortSignal(value) {
  if (value === undefined) return undefined;
  if (!value || typeof value !== "object"
      || typeof value.addEventListener !== "function"
      || typeof value.removeEventListener !== "function"
      || typeof value.aborted !== "boolean") {
    throw new TypeError("signal must implement the AbortSignal interface");
  }
  return value;
}

export function createHostCompletionBroker(options = {}) {
  if (!options || typeof options !== "object" || Array.isArray(options)) {
    throw new TypeError("host completion options must be an object");
  }
  const clock = options.clock ?? defaultClock;
  const schedule = options.schedule ?? setTimeout;
  const cancelSchedule = options.cancelSchedule ?? clearTimeout;
  const surface = options.surface;
  const documentEvents = options.documentEvents;
  if (typeof clock !== "function" || typeof schedule !== "function"
      || typeof cancelSchedule !== "function") {
    throw new TypeError("host completion clock and scheduling hooks must be callable");
  }
  if (surface !== undefined && (!surface || typeof surface !== "object"
      || typeof surface.next !== "function" || typeof surface.load !== "function")) {
    throw new TypeError("host completion surface hooks must provide next and load");
  }
  if (documentEvents !== undefined && (!documentEvents
      || typeof documentEvents !== "object"
      || typeof documentEvents.visibility !== "function")) {
    throw new TypeError("host completion document hooks must provide visibility");
  }

  let nextToken = 0;
  const slots = new Map();
  const receives = [];

  const readClock = () => u64(clock(), "monotonic clock result");

  const allocate = (kind) => {
    if (nextToken > MAX_U32) {
      throw new RangeError("Fe host pending-token space is exhausted");
    }
    const token = nextToken;
    nextToken += 1;
    let resolve;
    const settled = new Promise((value) => { resolve = value; });
    const slot = {
      token,
      kind,
      state: "pending",
      claimed: false,
      handler: undefined,
      cancelWork: undefined,
      resolve,
      settled,
    };
    slots.set(token, slot);
    if (kind === "receive") receives.push(token);
    return slot;
  };

  const settle = (slot, outcome, cancelled = false, raceSide = undefined) => {
    if (slot.state !== "pending") return false;
    slot.state = "settled";
    if (slot.cancelWork !== undefined) slot.cancelWork();
    slot.resolve(Object.freeze({ outcome, cancelled, raceSide }));
    return true;
  };

  const beginTimer = (rawDelay) => {
    if (typeof rawDelay !== "bigint") {
      throw new TypeError("fe:host::sleep_begin requires an i64 Wasm carrier");
    }
    const delay = BigInt.asUintN(64, rawDelay);
    const started = readClock();
    if (delay > MAX_U64 - started) {
      throw new RangeError("Fe timer deadline exceeds u64 monotonic time");
    }
    const deadline = started + delay;
    const slot = allocate("timer");
    let handle;
    const arm = () => {
      const now = readClock();
      if (now >= deadline) {
        settle(slot, taskSuccess([now]));
        return;
      }
      const remaining = deadline - now;
      const chunk = remaining > MAX_TIMER_CHUNK_MS ? MAX_TIMER_CHUNK_MS : remaining;
      handle = schedule(arm, Number(chunk));
    };
    slot.cancelWork = () => {
      if (handle !== undefined) cancelSchedule(handle);
    };
    try {
      const initial = delay > MAX_TIMER_CHUNK_MS ? MAX_TIMER_CHUNK_MS : delay;
      handle = schedule(arm, Number(initial));
    } catch (error) {
      slots.delete(slot.token);
      throw error;
    }
    return slot.token | 0;
  };

  const beginReceive = () => allocate("receive").token | 0;

  const beginBrowserOperation = (kind, invoke, successLanes) => {
    const slot = allocate(kind);
    const controller = new AbortController();
    slot.cancelWork = () => controller.abort();
    Promise.resolve().then(() => invoke(controller.signal)).then(value => {
      settle(slot, taskSuccess(successLanes(value)));
    }).catch(() => {
      // The browser boundary deliberately reports only a stable typed failure
      // fact. Error strings, DOM identities, and retry policy do not become a
      // second application protocol; Fe decides how a failed operation affects
      // the task.
      settle(slot, taskFailure([1]));
    });
    return slot.token | 0;
  };

  const beginSurfaceOperation = (kind, invoke) => {
    if (surface === undefined) {
      throw new Error(`fe:web-surface::${kind}_begin requires a surface capability`);
    }
    return beginBrowserOperation(
      `surface-${kind}`,
      invoke,
      value => [u64(value, `surface ${kind} result`)],
    );
  };

  const beginSurfaceNext = () => beginSurfaceOperation(
    "next",
    signal => surface.next(signal),
  );

  const beginSurfaceLoad = (rawSurface) => {
    if (typeof rawSurface !== "bigint") {
      throw new TypeError("fe:web-surface::load_begin requires an i64 Wasm carrier");
    }
    const checked = BigInt.asUintN(64, rawSurface);
    return beginSurfaceOperation("load", signal => surface.load(checked, signal));
  };

  const beginDocumentVisibility = (rawSeen, rawPreviousHidden) => {
    if (documentEvents === undefined) {
      throw new Error("fe:web-document::visibility_begin requires a document capability");
    }
    if (rawSeen !== 0 && rawSeen !== 1) {
      throw new TypeError("fe:web-document::visibility_begin seen flag must be a Fe bool");
    }
    if (rawPreviousHidden !== 0 && rawPreviousHidden !== 1) {
      throw new TypeError(
        "fe:web-document::visibility_begin previous-hidden flag must be a Fe bool",
      );
    }
    return beginBrowserOperation(
      "document-visibility",
      signal => documentEvents.visibility(rawSeen === 1, rawPreviousHidden === 1, signal),
      value => {
        const visibility = u32(value, "document visibility result");
        if (visibility > 1) {
          throw new TypeError("document visibility result is not a declared Fe variant");
        }
        return [visibility];
      },
    );
  };

  const beginRace = (rawLeft, rawRight) => {
    if (!Number.isInteger(rawLeft) || !Number.isInteger(rawRight)) {
      throw new TypeError("fe:host::race_begin requires two i32 Wasm carriers");
    }
    const leftToken = rawLeft >>> 0;
    const rightToken = rawRight >>> 0;
    if (leftToken === rightToken) {
      throw new TypeError("Fe race inputs must be distinct affine pending tokens");
    }
    const left = slots.get(leftToken);
    const right = slots.get(rightToken);
    for (const child of [left, right]) {
      if (child === undefined || child.state !== "pending" || child.claimed) {
        throw new TypeError("Fe race input is stale, settled, foreign, or already claimed");
      }
    }
    const race = allocate("race");
    left.claimed = true;
    right.claimed = true;
    race.cancelWork = () => {
      for (const child of [left, right]) {
        if (child.state === "pending") {
          child.state = "settled";
          if (child.cancelWork !== undefined) child.cancelWork();
          child.resolve(Object.freeze({
            outcome: taskCancelled(),
            cancelled: true,
            raceSide: undefined,
          }));
        }
        slots.delete(child.token);
      }
    };
    left.settled.then(delivery => {
      settle(race, delivery.outcome, delivery.cancelled, "left");
    });
    right.settled.then(delivery => {
      settle(race, delivery.outcome, delivery.cancelled, "right");
    });
    return race.token | 0;
  };

  const firstPendingReceive = () => {
    while (receives.length > 0) {
      const slot = slots.get(receives.shift());
      if (slot !== undefined && slot.kind === "receive" && slot.state === "pending") {
        return slot;
      }
    }
    return undefined;
  };

  const awaitPending = async (pending, signal) => {
    const token = pendingToken(pending);
    const slot = slots.get(token);
    if (slot === undefined) {
      throw new TypeError("Fe host pending token is stale, foreign, or unknown");
    }
    if (slot.claimed) {
      throw new TypeError("Fe host pending token was already claimed by a continuation");
    }
    slot.claimed = true;
    slot.handler = pending.handler;
    const onAbort = () => settle(slot, taskCancelled(), true);
    if (signal !== undefined) {
      signal.addEventListener("abort", onAbort, { once: true });
      if (signal.aborted) onAbort();
    }
    try {
      const delivery = await slot.settled;
      if (delivery.raceSide !== undefined) {
        return Object.freeze({
          outcome: raceTaskOutcome(pending, delivery.outcome, delivery.raceSide),
          cancelled: delivery.cancelled,
          raceSide: undefined,
        });
      }
      return delivery;
    } finally {
      if (signal !== undefined) signal.removeEventListener("abort", onAbort);
      slots.delete(token);
    }
  };

  const discardSince = (tokenCheckpoint) => {
    for (const [token, slot] of slots) {
      if (token >= tokenCheckpoint) {
        if (slot.cancelWork !== undefined) slot.cancelWork();
        slots.delete(token);
      }
    }
  };

  const invokeMachine = (invoke) => {
    const tokenCheckpoint = nextToken;
    try {
      const step = invoke();
      if (!step || typeof step !== "object"
          || (step.kind !== "suspended" && step.kind !== "complete")
          || (step.kind === "complete" && !Array.isArray(step.output))) {
        throw new TypeError("materialized Fe task returned an invalid step");
      }
      return step;
    } catch (error) {
      discardSince(tokenCheckpoint);
      throw error;
    }
  };

  const run = async (machine, input, runOptions = {}) => {
    if (!machine || typeof machine.start !== "function" || typeof machine.resume !== "function") {
      throw new TypeError("host completion runner requires a materialized Fe task machine");
    }
    if (!runOptions || typeof runOptions !== "object" || Array.isArray(runOptions)) {
      throw new TypeError("host completion run options must be an object");
    }
    const signal = abortSignal(runOptions.signal);
    let step = invokeMachine(() => machine.start(input));
    while (step.kind === "suspended") {
      const delivery = await awaitPending(step.pending, signal);
      const tokenCheckpoint = nextToken;
      step = invokeMachine(() => machine.resume(step.frame, delivery.outcome));
      if (delivery.cancelled) {
        // Cancellation is delivered exactly once so Fe can release its owned
        // state. It remains the task's terminal verdict even if that cleanup
        // continuation returns a value or attempts another suspension.
        discardSince(tokenCheckpoint);
        throw new FeTaskCancelled();
      }
    }
    return step.output;
  };

  const post = (value) => {
    const checked = u64(value, "receive value");
    const slot = firstPendingReceive();
    return slot === undefined ? false : settle(slot, taskSuccess([checked]));
  };

  const failNextReceive = (error) => {
    const checked = u32(error, "receive error");
    const slot = firstPendingReceive();
    return slot === undefined ? false : settle(slot, taskFailure([checked]));
  };

  const cancelAll = () => {
    let cancelled = 0;
    for (const slot of slots.values()) {
      if (settle(slot, taskCancelled(), true)) cancelled += 1;
    }
    return cancelled;
  };

  const host = Object.freeze({
    host_now: () => BigInt.asIntN(64, readClock()),
    sleep_begin: beginTimer,
    recv_begin: beginReceive,
    race_begin: beginRace,
    wait: () => {
      throw new Error("fe:host::wait is unavailable in the non-blocking browser broker");
    },
  });

  const surfaceImports = Object.freeze({
    next_begin: beginSurfaceNext,
    load_begin: beginSurfaceLoad,
  });
  const documentImports = Object.freeze({
    visibility_begin: beginDocumentVisibility,
  });

  const imports = { "fe:host": host };
  if (surface !== undefined) imports["fe:web-surface"] = surfaceImports;
  if (documentEvents !== undefined) imports["fe:web-document"] = documentImports;

  return Object.freeze({
    imports: Object.freeze(imports),
    run,
    post,
    failNextReceive,
    cancelAll,
    activeCount: () => slots.size,
  });
}
