import assert from "node:assert/strict";
import { actorField, actorResultSchema, exactObject } from "./actor-endpoint.js";
import { actorEnvelope } from "./actor-coordinator.js";
import {
  createCanonicalModuleWorkerActor,
  createModuleWorkerScope,
} from "./module-worker-actor.js";

const schemas = {
  requestSchema: {
    render: payload => exactObject(payload, { args: actorField.int32Array(1) }),
  },
  resultSchema: {
    render: actorResultSchema(actorField.int32Array(1)),
  },
};
const adapter = {
  ...schemas,
  transferRequest(value) { return [value.args.buffer]; },
};

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

  emit(type) {
    for (const listener of this.listeners.get(type) ?? []) listener({ type });
  }

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
    } else if (this.behavior !== "silent") {
      queueMicrotask(() => this.port.postMessage({ type: "ready" }));
    }
  }

  reply(request, value) {
    this.port.postMessage(actorEnvelope({
      type: "result",
      lane: request.lane,
      actorEpoch: request.actorEpoch,
      generation: request.generation,
      requestId: request.requestId,
      payload: { ok: true, value: new Int32Array([value]) },
    }));
  }

  terminate() { this.terminated = true; }
}

const flush = async () => {
  for (let index = 0; index < 3; index += 1) {
    await new Promise(resolve => setTimeout(resolve, 0));
  }
};

const actor = (overrides = {}) => createCanonicalModuleWorkerActor({
  workerUrl: "worker.js",
  adapter,
  WorkerCtor: ScriptedWorker,
  ...overrides,
});

// Restart windows, clocks, and backoff are deliberately not browser-runtime
// options. They are application policy and must be supplied by Fe through the
// structured-scope effect rail.
await assert.rejects(
  actor({ supervision: { maxRestarts: 3, windowMs: 1_000, backoffMs: 10 } }),
  /unsupported options: supervision/,
);

// Startup failure retires the partially initialized Worker exactly once. The
// fixed runtime never chooses to construct a replacement.
ScriptedWorker.script = ["startup-error", "ready"];
ScriptedWorker.instances = [];
await assert.rejects(actor(), error => error.code === "FE_ACTOR_WORKER_INIT");
await flush();
assert.equal(ScriptedWorker.instances.length, 1);
assert.equal(ScriptedWorker.instances[0].terminated, true);

// A runtime crash fails in-flight work, retires the Worker synchronously, and
// remains failed until an explicit caller-owned restart effect is realized.
ScriptedWorker.script = ["hold", "ready"];
ScriptedWorker.instances = [];
const recovering = await actor();
const oldWorker = ScriptedWorker.instances[0];
const oldRequest = recovering.request("render", { args: new Int32Array([3]) }, 1);
await flush();
assert.equal(oldWorker.requests.length, 1);
oldWorker.emit("error");
await assert.rejects(oldRequest, error => error.code === "FE_ACTOR_WORKER_RUNTIME");
assert.equal(oldWorker.terminated, true);
assert.deepEqual(recovering.status(), {
  state: "failed", epoch: 0, failureCode: "FE_ACTOR_WORKER_RUNTIME",
});
await flush();
assert.equal(ScriptedWorker.instances.length, 1, "failure must not auto-spawn a Worker");

const retained = new Int32Array([4]);
await assert.rejects(
  recovering.request("render", { args: retained }, 2),
  error => error.code === "FE_ACTOR_WORKER_RUNTIME",
);
assert.equal(retained.byteLength, 4, "failed admission retains request ownership");

assert.equal(await recovering.restart(), 1);
assert.deepEqual(recovering.status(), { state: "ready", epoch: 1, failureCode: null });
const replacementArgs = new Int32Array([5]);
assert.deepEqual(
  await recovering.request("render", { args: replacementArgs }, 3),
  new Int32Array([10]),
);
assert.equal(replacementArgs.byteLength, 0, "ready Worker receives request ownership");
assert.deepEqual(ScriptedWorker.instances[1].requests.map(request => request.actorEpoch), [1]);
recovering.close();

// A crash racing an already-enqueued explicit restart cannot publish a second
// lifecycle or construct a second replacement.
ScriptedWorker.script = ["hold", "ready", "ready"];
ScriptedWorker.instances = [];
const racing = await actor();
const retiring = ScriptedWorker.instances[0];
const explicitRestart = racing.restart();
retiring.emit("error");
assert.equal(await explicitRestart, 1);
assert.equal(ScriptedWorker.instances.length, 2);
assert.deepEqual(racing.status(), { state: "ready", epoch: 1, failureCode: null });
racing.close();

// Failed replacement construction is a raw lifecycle fact. A second restart
// occurs only because the caller explicitly asks for it.
ScriptedWorker.script = ["ready", "startup-error", "ready"];
ScriptedWorker.instances = [];
const retrying = await actor();
await assert.rejects(retrying.restart(), error => error.code === "FE_ACTOR_WORKER_INIT");
assert.deepEqual(retrying.status(), {
  state: "failed", epoch: 1, failureCode: "FE_ACTOR_WORKER_INIT",
});
await flush();
assert.equal(ScriptedWorker.instances.length, 2);
assert.equal(await retrying.restart(), 2);
assert.equal(ScriptedWorker.instances.length, 3);
assert.deepEqual(retrying.status(), { state: "ready", epoch: 2, failureCode: null });
retrying.close();
assert.deepEqual(retrying.status(), { state: "closed", epoch: 2, failureCode: null });
await assert.rejects(retrying.restart(), error => error.code === "FE_ACTOR_CLOSED");

// The capability adapter executes only Fe-selected epochs. A failed initial
// construction does not invent a replacement; the next explicit epoch creates
// one, runtime failure observation is abortable, and one successor epoch maps
// to exactly one mechanical restart.
ScriptedWorker.script = ["startup-error", "hold", "ready"];
ScriptedWorker.instances = [];
const scope = createModuleWorkerScope({
  createActor: ({ initialEpoch, signal }) => actor({ initialEpoch, signal }),
});
await assert.rejects(scope.spawn(0), error => error.code === "FE_ACTOR_WORKER_INIT");
await flush();
assert.equal(ScriptedWorker.instances.length, 1, "scope must not auto-retry startup");
assert.equal(ScriptedWorker.instances[0].terminated, true);
await scope.spawn(1);
assert.deepEqual(scope.status(), { state: "ready", epoch: 1 });
const observedFailure = scope.failure(1);
ScriptedWorker.instances[1].emit("error");
await observedFailure;
assert.deepEqual(scope.status(), { state: "failed", epoch: 1 });
await assert.rejects(scope.spawn(3), error => error.code === "FE_ACTOR_WORKER_RESTART");
assert.equal(ScriptedWorker.instances.length, 2);
await scope.spawn(2);
assert.deepEqual(scope.status(), { state: "ready", epoch: 2 });
assert.equal(ScriptedWorker.instances.length, 3);
scope.close(2);
assert.deepEqual(scope.status(), { state: "closed", epoch: 2 });

// Cancellation during readiness promptly retires the partially constructed
// Worker and leaves the mechanics adapter idle for Fe's terminal close.
ScriptedWorker.script = ["silent"];
ScriptedWorker.instances = [];
const cancelledScope = createModuleWorkerScope({
  createActor: ({ initialEpoch, signal }) => actor({ initialEpoch, signal }),
});
const spawnController = new AbortController();
const cancelledSpawn = cancelledScope.spawn(0, spawnController.signal);
spawnController.abort();
await assert.rejects(cancelledSpawn, error => error.code === "FE_ACTOR_ABORTED");
assert.equal(ScriptedWorker.instances[0].terminated, true);
cancelledScope.close(0);

console.log("policy-free module-worker lifecycle and explicit restart mechanics: ok");
