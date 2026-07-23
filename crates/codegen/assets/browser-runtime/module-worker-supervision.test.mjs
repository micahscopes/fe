import assert from "node:assert/strict";
import { actorField, actorResultSchema, exactObject } from "./actor-endpoint.js";
import { actorEnvelope } from "./actor-coordinator.js";
import { createCanonicalModuleWorkerActor } from "./module-worker-actor.js";

const schemas = {
  requestSchema: {
    render: (payload) => exactObject(payload, { args: actorField.int32Array(1) }),
  },
  resultSchema: {
    render: actorResultSchema(actorField.int32Array(1)),
  },
};
const adapter = {
  ...schemas,
  transferRequest(value) { return [value.args.buffer]; },
};

class ManualClock {
  nowMs = 0;
  nextId = 0;
  timers = new Map();
  now = () => this.nowMs;
  schedule = (callback, delay) => {
    const id = ++this.nextId;
    this.timers.set(id, { at: this.nowMs + delay, callback });
    return id;
  };
  cancel = (id) => { this.timers.delete(id); };
  advance(ms) {
    this.nowMs += ms;
    const due = [...this.timers.entries()]
      .filter(([, timer]) => timer.at <= this.nowMs)
      .sort((left, right) => left[1].at - right[1].at);
    for (const [id, timer] of due) {
      if (!this.timers.delete(id)) continue;
      timer.callback();
    }
  }
}

class ScriptedWorker {
  static script = [];
  static instances = [];
  constructor() {
    this.behavior = ScriptedWorker.script.shift() ?? "ready";
    this.listeners = new Map();
    this.requests = [];
    this.terminated = false;
    ScriptedWorker.instances.push(this);
  }
  addEventListener(type, listener) {
    if (!this.listeners.has(type)) this.listeners.set(type, new Set());
    this.listeners.get(type).add(listener);
  }
  removeEventListener(type, listener) { this.listeners.get(type)?.delete(listener); }
  emit(type) { for (const listener of this.listeners.get(type) ?? []) listener({ type }); }
  postMessage(message) {
    this.port = message.port;
    this.epoch = message.actorEpoch;
    this.port.addEventListener("message", ({ data }) => {
      if (data.type !== "request") return;
      this.requests.push(data);
      if (this.behavior !== "hold") this.reply(data, data.payload.args[0] * 2);
    });
    this.port.start();
    if (this.behavior === "startup-error") {
      queueMicrotask(() => this.emit("error"));
    } else {
      queueMicrotask(() => this.port.postMessage({ type: "ready" }));
    }
  }
  reply(request, value) {
    this.port.postMessage(actorEnvelope({
      type: "result", lane: request.lane, actorEpoch: request.actorEpoch,
      generation: request.generation, requestId: request.requestId,
      payload: { ok: true, value: new Int32Array([value]) },
    }));
  }
  terminate() { this.terminated = true; }
}

const flush = async () => {
  for (let index = 0; index < 3; index += 1) {
    await new Promise((resolve) => setTimeout(resolve, 0));
  }
};
const policy = (clock, events, overrides = {}) => ({
  maxRestarts: 3,
  windowMs: 1_000,
  backoffMs: 10,
  observe: (event) => events.push(event),
  now: clock.now,
  schedule: clock.schedule,
  cancel: clock.cancel,
  ...overrides,
});

// Initial startup failure is classified independently and can recover within
// the same explicit budget before the actor factory resolves.
ScriptedWorker.script = ["startup-error", "ready"];
ScriptedWorker.instances = [];
const startupClock = new ManualClock();
const startupEvents = [];
const starting = createCanonicalModuleWorkerActor({
  workerUrl: "startup.js", adapter, WorkerCtor: ScriptedWorker,
  supervision: policy(startupClock, startupEvents),
});
await flush();
startupClock.advance(10);
const started = await starting;
assert.equal(started.epoch(), 1);
assert.deepEqual(
  startupEvents.map(({ type, classification }) => [type, classification ?? null]),
  [
    ["failure", "startup"],
    ["backoff", "startup"],
    ["restart", "startup"],
    ["ready", null],
  ],
);
started.close();

// A runtime crash retires the old transport synchronously. Requests submitted
// during backoff either abort without crossing a port or wait for the next
// epoch; a stale old-epoch result cannot satisfy them.
ScriptedWorker.script = ["hold", "ready"];
ScriptedWorker.instances = [];
const recoveryClock = new ManualClock();
const recoveryEvents = [];
const recovering = await createCanonicalModuleWorkerActor({
  workerUrl: "recover.js", adapter, WorkerCtor: ScriptedWorker,
  supervision: policy(recoveryClock, recoveryEvents),
});
const oldWorker = ScriptedWorker.instances[0];
const oldRequest = recovering.request("render", { args: new Int32Array([3]) }, 1);
await flush();
assert.equal(oldWorker.requests.length, 1);
oldWorker.emit("error");
await assert.rejects(oldRequest, (error) => error.code === "FE_ACTOR_WORKER_RUNTIME");
assert.equal(oldWorker.terminated, true);
assert.equal(recovering.status().state, "backoff");

const cancelled = new AbortController();
const cancelledArgs = new Int32Array([4]);
const cancelledDuringBackoff = recovering.request(
  "render", { args: cancelledArgs }, 2, { signal: cancelled.signal },
);
cancelled.abort();
await assert.rejects(cancelledDuringBackoff, (error) => error.code === "FE_ACTOR_ABORTED");
assert.equal(cancelledArgs.byteLength, 4, "cancelled backoff request retains ownership");
assert.equal(oldWorker.requests.length, 1, "cancelled backoff request never reaches retiring worker");

const recoveryArgs = new Int32Array([5]);
const afterRecovery = recovering.request("render", { args: recoveryArgs }, 3);
oldWorker.reply(oldWorker.requests[0], 999);
recoveryClock.advance(10);
await flush();
assert.deepEqual(await afterRecovery, new Int32Array([10]));
assert.equal(recoveryArgs.byteLength, 0, "ownership transfers only to ready replacement");
const replacement = ScriptedWorker.instances[1];
assert.deepEqual(replacement.requests.map(({ actorEpoch }) => actorEpoch), [1]);
assert.equal(oldWorker.requests.length, 1, "queued request never reaches retiring worker");
assert.equal(recovering.epoch(), 1);
assert.equal(recovering.status().state, "ready");
assert.deepEqual(
  recoveryEvents.map(({ type, classification, epoch }) => [type, classification ?? null, epoch]),
  [
    ["ready", null, 0],
    ["failure", "runtime", 0],
    ["backoff", "runtime", 1],
    ["restart", "runtime", 1],
    ["ready", null, 1],
  ],
);
recovering.close();

// A crash racing an already-enqueued explicit restart does not start a second
// replacement or consume an automatic restart attempt.
ScriptedWorker.script = ["hold", "ready", "ready"];
ScriptedWorker.instances = [];
const raceClock = new ManualClock();
const raceEvents = [];
const racing = await createCanonicalModuleWorkerActor({
  workerUrl: "race.js", adapter, WorkerCtor: ScriptedWorker,
  supervision: policy(raceClock, raceEvents),
});
const retiring = ScriptedWorker.instances[0];
const explicitRestart = racing.restart();
retiring.emit("error");
assert.equal(await explicitRestart, 1);
assert.equal(ScriptedWorker.instances.length, 2);
assert.equal(raceClock.timers.size, 0);
assert.equal(racing.status().restartsInWindow, 0);
assert.deepEqual(
  raceEvents.filter(({ type }) => type === "restart")
    .map(({ classification }) => classification),
  ["manual"],
);
racing.close();

// Automatic recovery does not reserve itself as a manual restart. A
// replacement that crashes synchronously from the ready observation boundary
// therefore consumes another bounded attempt instead of leaving the actor
// workerless with a fulfilled transition.
ScriptedWorker.script = ["hold", "ready", "ready"];
ScriptedWorker.instances = [];
const boundaryClock = new ManualClock();
const boundaryEvents = [];
let crashReadyReplacement = true;
const boundary = await createCanonicalModuleWorkerActor({
  workerUrl: "ready-boundary.js", adapter, WorkerCtor: ScriptedWorker,
  supervision: policy(boundaryClock, boundaryEvents, {
    observe(event) {
      boundaryEvents.push(event);
      if (event.type === "ready" && event.epoch === 1 && crashReadyReplacement) {
        crashReadyReplacement = false;
        ScriptedWorker.instances.at(-1).emit("error");
      }
    },
  }),
});
ScriptedWorker.instances[0].emit("error");
boundaryClock.advance(10);
await flush();
assert.equal(boundary.status().state, "backoff");
assert.equal(ScriptedWorker.instances[1].terminated, true);
boundaryClock.advance(10);
await flush();
assert.equal(boundary.status().state, "ready");
assert.equal(boundary.epoch(), 2);
assert.equal(ScriptedWorker.instances.length, 3);
assert.deepEqual(
  boundaryEvents.filter(({ type }) => type === "failure")
    .map(({ classification }) => classification),
  ["runtime", "runtime"],
);
boundary.close();

// Startup failures consume the same bounded rolling-window budget. Exhaustion
// is terminal and observable; it never creates an unbounded timer chain.
ScriptedWorker.script = ["ready", "startup-error", "startup-error", "ready"];
ScriptedWorker.instances = [];
const loopClock = new ManualClock();
const loopEvents = [];
const looping = await createCanonicalModuleWorkerActor({
  workerUrl: "loop.js", adapter, WorkerCtor: ScriptedWorker,
  supervision: policy(loopClock, loopEvents, { maxRestarts: 2 }),
});
ScriptedWorker.instances[0].emit("error");
loopClock.advance(10);
await flush();
assert.equal(looping.status().state, "backoff");
loopClock.advance(10);
await flush();
assert.equal(looping.status().state, "terminal");
assert.equal(looping.status().terminalCode, "FE_ACTOR_WORKER_TERMINAL");
assert.equal(loopClock.timers.size, 0, "terminal crash loop leaves no timer");
assert.equal(ScriptedWorker.instances.length, 3, "restart budget bounds worker construction");
await assert.rejects(
  looping.request("render", { args: new Int32Array([1]) }),
  (error) => error.code === "FE_ACTOR_WORKER_TERMINAL",
);
assert.deepEqual(
  loopEvents.filter(({ type }) => type === "failure")
    .map(({ classification }) => classification),
  ["runtime", "startup", "startup"],
);
assert.equal(loopEvents.at(-1).type, "terminal");
looping.close();

// Closing during backoff cancels the sole scheduled timer and permanently
// rejects queued work without constructing a replacement.
ScriptedWorker.script = ["ready", "ready"];
ScriptedWorker.instances = [];
const closeClock = new ManualClock();
const closeEvents = [];
const closing = await createCanonicalModuleWorkerActor({
  workerUrl: "close.js", adapter, WorkerCtor: ScriptedWorker,
  supervision: policy(closeClock, closeEvents),
});
ScriptedWorker.instances[0].emit("error");
const queuedAtClose = closing.request("render", { args: new Int32Array([7]) });
await flush();
assert.equal(closeClock.timers.size, 1);
closing.close();
assert.equal(closeClock.timers.size, 0);
await assert.rejects(queuedAtClose, (error) => error.code === "FE_ACTOR_CLOSED");
closeClock.advance(100);
await flush();
assert.equal(ScriptedWorker.instances.length, 1);
assert.equal(closing.status().state, "closed");
assert.equal(closeEvents.at(-1).type, "close");

console.log("bounded module-worker supervision crash/backoff/epoch lifecycle: ok");
