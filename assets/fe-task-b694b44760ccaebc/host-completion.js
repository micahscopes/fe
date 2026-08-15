// Fixed browser realization for broker-completed Fe runtime-control effects.
//
// Fe owns the task body, outcome matching, retry/cancellation policy, and every
// subsequent suspension. This module owns only standards clocks/timers, the
// finite pending-token table, FIFO receive delivery, and driving the opaque
// compiler-generated continuation machine.

import {
  raceTaskOutcome,
  selectTaskOutcome,
  taskCancelled,
  taskFailure,
  taskOutcomeKind,
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

function finiteF32(value, name) {
  if (typeof value !== "number" || !Number.isFinite(value) || value < 0) {
    throw new TypeError(`${name} must be a non-negative finite number`);
  }
  return Math.fround(value);
}

function signedF32(value, name) {
  if (typeof value !== "number" || !Number.isFinite(value)) {
    throw new TypeError(`${name} must be a finite number`);
  }
  return Math.fround(value);
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
  const windowEvents = options.windowEvents;
  const componentEvents = options.componentEvents;
  const actorEvents = options.actorEvents;
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
  if (windowEvents !== undefined && (!windowEvents
      || typeof windowEvents !== "object"
      || typeof windowEvents.animationFrame !== "function"
      || typeof windowEvents.viewport !== "function")) {
    throw new TypeError("host completion window hooks must provide animationFrame and viewport");
  }
  if (componentEvents !== undefined && (!componentEvents
      || typeof componentEvents !== "object"
      || typeof componentEvents.pointer !== "function"
      || typeof componentEvents.wheel !== "function")) {
    throw new TypeError("host completion component hooks must provide pointer and wheel");
  }
  if (actorEvents !== undefined && (!actorEvents
      || typeof actorEvents !== "object"
      || typeof actorEvents.send !== "function")) {
    throw new TypeError("host completion actor hooks must provide send");
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

  const settle = (
    slot,
    outcome,
    cancelled = false,
    raceSide = undefined,
    loserToken = undefined,
  ) => {
    if (slot.state !== "pending") return false;
    slot.state = "settled";
    if (slot.cancelWork !== undefined) slot.cancelWork();
    slot.resolve(Object.freeze({ outcome, cancelled, raceSide, loserToken }));
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

  const beginAnimationFrame = () => {
    if (windowEvents === undefined) {
      throw new Error("fe:web-window::animation_frame_begin requires a window capability");
    }
    return beginBrowserOperation(
      "window-animation-frame",
      signal => windowEvents.animationFrame(signal),
      value => [finiteF32(value, "animation frame timestamp")],
    );
  };

  const beginViewport = (
    rawSeen,
    rawPreviousWidth,
    rawPreviousHeight,
    rawPreviousDevicePixelRatio,
  ) => {
    if (windowEvents === undefined) {
      throw new Error("fe:web-window::viewport_begin requires a window capability");
    }
    if (rawSeen !== 0 && rawSeen !== 1) {
      throw new TypeError("fe:web-window::viewport_begin seen flag must be a Fe bool");
    }
    const previousWidth = finiteF32(rawPreviousWidth, "previous viewport width");
    const previousHeight = finiteF32(rawPreviousHeight, "previous viewport height");
    const previousDevicePixelRatio = finiteF32(
      rawPreviousDevicePixelRatio,
      "previous viewport device pixel ratio",
    );
    return beginBrowserOperation(
      "window-viewport",
      signal => windowEvents.viewport(
        rawSeen === 1,
        previousWidth,
        previousHeight,
        previousDevicePixelRatio,
        signal,
      ),
      value => {
        if (!value || typeof value !== "object" || Array.isArray(value)) {
          throw new TypeError("window viewport result must be an object");
        }
        return [
          finiteF32(value.width, "viewport width"),
          finiteF32(value.height, "viewport height"),
          finiteF32(value.devicePixelRatio, "viewport device pixel ratio"),
        ];
      },
    );
  };

  const beginPointer = () => {
    if (componentEvents === undefined) {
      throw new Error(
        "fe:web-component-events::pointer_begin requires a component capability",
      );
    }
    return beginBrowserOperation(
      "component-pointer",
      signal => componentEvents.pointer(signal),
      value => {
        if (!value || typeof value !== "object" || Array.isArray(value)) {
          throw new TypeError("component pointer result must be an object");
        }
        const phase = u32(value.phase, "pointer phase");
        const device = u32(value.device, "pointer device");
        const pressure = finiteF32(value.pressure, "pointer pressure");
        if (phase > 3 || device > 3 || pressure > 1) {
          throw new TypeError("component pointer result is outside its declared Fe vocabulary");
        }
        if (typeof value.primary !== "boolean") {
          throw new TypeError("pointer primary flag must be boolean");
        }
        return [
          phase,
          device,
          u32(value.pointerId, "pointer identity"),
          signedF32(value.clientX, "pointer client x"),
          signedF32(value.clientY, "pointer client y"),
          u32(value.buttons, "pointer buttons"),
          value.primary,
          pressure,
          finiteF32(value.timestamp, "pointer timestamp"),
        ];
      },
    );
  };

  const beginWheel = () => {
    if (componentEvents === undefined) {
      throw new Error(
        "fe:web-component-events::wheel_begin requires a component capability",
      );
    }
    return beginBrowserOperation(
      "component-wheel",
      signal => componentEvents.wheel(signal),
      value => {
        if (!value || typeof value !== "object" || Array.isArray(value)) {
          throw new TypeError("component wheel result must be an object");
        }
        const mode = u32(value.mode, "wheel delta mode");
        if (mode > 3) {
          throw new TypeError("component wheel result is outside its declared Fe vocabulary");
        }
        if (typeof value.control !== "boolean") {
          throw new TypeError("wheel control flag must be boolean");
        }
        return [
          signedF32(value.deltaX, "wheel delta x"),
          signedF32(value.deltaY, "wheel delta y"),
          signedF32(value.deltaZ, "wheel delta z"),
          mode,
          signedF32(value.clientX, "wheel client x"),
          signedF32(value.clientY, "wheel client y"),
          value.control,
          finiteF32(value.timestamp, "wheel timestamp"),
        ];
      },
    );
  };

  const beginActorSend = (...lanes) => {
    if (actorEvents === undefined) {
      throw new Error("fe:actor::send_begin requires a resident actor capability");
    }
    // Wasm has already enforced the compiler-derived scalar signature. Keep
    // the values opaque: the fixed broker neither decodes an event nor knows
    // which resident transition will consume it.
    const event = Object.freeze([...lanes]);
    return beginBrowserOperation(
      "actor-send",
      signal => actorEvents.send(event, signal),
      () => [],
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

  const beginSelect = (rawLeft, rawRight) => {
    if (!Number.isInteger(rawLeft) || !Number.isInteger(rawRight)) {
      throw new TypeError("fe:host::select_begin requires two i32 Wasm carriers");
    }
    const leftToken = rawLeft >>> 0;
    const rightToken = rawRight >>> 0;
    if (leftToken === rightToken) {
      throw new TypeError("Fe select inputs must be distinct affine pending tokens");
    }
    const left = slots.get(leftToken);
    const right = slots.get(rightToken);
    for (const child of [left, right]) {
      // A token returned as the loser of an earlier select may already have
      // settled before Fe selects it again. It is still a valid unconsumed
      // Pending value; awaiting its promise simply chooses it immediately.
      if (child === undefined || (child.state !== "pending" && child.state !== "settled")
          || child.claimed) {
        throw new TypeError("Fe select input is stale, foreign, or already claimed");
      }
    }
    const selected = allocate("select");
    left.claimed = true;
    right.claimed = true;
    selected.cancelWork = () => {
      for (const child of [left, right]) {
        if (child.state === "pending") {
          child.state = "settled";
          if (child.cancelWork !== undefined) child.cancelWork();
          child.resolve(Object.freeze({
            outcome: taskCancelled(),
            cancelled: true,
            raceSide: undefined,
            loserToken: undefined,
          }));
        }
        slots.delete(child.token);
      }
    };
    const choose = (winner, loser, side, delivery) => {
      if (selected.state !== "pending") return;
      if (taskOutcomeKind(delivery.outcome) === "success") {
        // Transfer custody of the loser back into Fe. Clearing cancelWork is
        // essential because `settle` ordinarily uses it to cancel children.
        loser.claimed = false;
        selected.cancelWork = undefined;
        slots.delete(winner.token);
        settle(
          selected,
          delivery.outcome,
          false,
          side,
          loser.token,
        );
      } else {
        // Failure/cancellation is terminal and has no payload position in
        // which an affine loser could be returned. The normal cancelWork path
        // therefore cancels and removes it exactly once.
        // Child terminals are typed SelectOutcome variants, not cancellation
        // of the owning scoped task. The broker cancels the unreachable loser
        // but lets Fe observe and classify the winning side.
        settle(selected, delivery.outcome, false, side);
      }
    };
    left.settled.then(delivery => choose(left, right, "left", delivery));
    right.settled.then(delivery => choose(right, left, "right", delivery));
    return selected.token | 0;
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
      if (delivery.raceSide !== undefined && slot.kind === "race") {
        return Object.freeze({
          outcome: raceTaskOutcome(pending, delivery.outcome, delivery.raceSide),
          cancelled: delivery.cancelled,
          raceSide: undefined,
          loserToken: undefined,
        });
      }
      if (delivery.raceSide !== undefined && slot.kind === "select") {
        return Object.freeze({
          outcome: selectTaskOutcome(
            pending,
            delivery.outcome,
            delivery.raceSide,
            delivery.loserToken,
          ),
          cancelled: delivery.cancelled,
          raceSide: undefined,
          loserToken: undefined,
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
    select_begin: beginSelect,
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
  const windowImports = Object.freeze({
    animation_frame_begin: beginAnimationFrame,
    viewport_begin: beginViewport,
  });
  const componentEventImports = Object.freeze({
    pointer_begin: beginPointer,
    wheel_begin: beginWheel,
  });
  const actorImports = Object.freeze({
    send_begin: beginActorSend,
  });

  const imports = { "fe:host": host };
  if (surface !== undefined) imports["fe:web-surface"] = surfaceImports;
  if (documentEvents !== undefined) imports["fe:web-document"] = documentImports;
  if (windowEvents !== undefined) imports["fe:web-window"] = windowImports;
  if (componentEvents !== undefined) {
    imports["fe:web-component-events"] = componentEventImports;
  }
  if (actorEvents !== undefined) imports["fe:actor"] = actorImports;

  return Object.freeze({
    imports: Object.freeze(imports),
    run,
    post,
    failNextReceive,
    cancelAll,
    activeCount: () => slots.size,
  });
}
