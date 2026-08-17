import assert from "node:assert/strict";
import { actorField, actorResultSchema, exactObject } from "./actor-endpoint.js";
import { actorEnvelope } from "./actor-coordinator.js";
import {
  createCanonicalModuleWorkerActor,
  createModuleWorkerActor,
} from "./module-worker-actor.js";

class FakeWorker {
  static replies = [];
  static instances = [];
  constructor(url, options) { this.url = url; this.options = options; this.listeners = new Map();
    this.terminated = false; FakeWorker.instances.push(this); }
  addEventListener(type, listener) { this.listeners.set(type, listener); }
  postMessage(message, transfer) {
    this.message = message; this.transfer = transfer;
    queueMicrotask(() => message.port.postMessage(FakeWorker.replies.shift() || { type: "ready" }));
  }
  terminate() { this.terminated = true; }
}
const schemas = {
  requestSchema: { render: (payload) => exactObject(payload, { args: actorField.int32Array(1) }),
    verify: (payload) => exactObject(payload, { args: actorField.int32Array(1) }) },
  resultSchema: { render: actorResultSchema(actorField.int32Array(1)),
    verify: actorResultSchema(actorField.int32Array(1)) },
};

FakeWorker.replies = [{ type: "ready" }, { type: "ready" }];
let auxiliaryCloses = 0;
const actor = await createModuleWorkerActor({ workerUrl: "worker.js", ...schemas,
  WorkerCtor: FakeWorker, createAuxiliaryPorts: (epoch) => {
    const channel = new MessageChannel();
    return { message: { extraPort: channel.port2 }, transfer: [channel.port2],
      close() { auxiliaryCloses += 1; channel.port1.close(); } };
  } });
assert.equal(actor.epoch(), 0);
assert.equal(FakeWorker.instances[0].transfer.length, 2, "control and auxiliary ports transfer");
assert.equal(await actor.restart(), 1);
assert.equal(FakeWorker.instances[0].terminated, true);
assert.equal(auxiliaryCloses, 1);
actor.close();
assert.equal(auxiliaryCloses, 2);

class CanonicalWorker extends FakeWorker {
  postMessage(message, transfer) {
    this.message = message; this.transfer = transfer;
    message.port.addEventListener("message", ({ data: request }) => {
      message.port.postMessage(actorEnvelope({
        type: "result", lane: request.lane, actorEpoch: request.actorEpoch,
        generation: request.generation, requestId: request.requestId,
        payload: request.payload.args[0] < 0
          ? { ok: false, error: "private worker detail" }
          : { ok: true, value: new Int32Array([request.payload.args[0] * 2]) },
      }));
    });
    message.port.start();
    queueMicrotask(() => message.port.postMessage({ type: "ready" }));
  }
}
const canonicalAdapter = {
  ...schemas,
  transferRequest(value) { return [value.args.buffer]; },
};
const canonical = await createCanonicalModuleWorkerActor({
  workerUrl: "canonical.js", adapter: canonicalAdapter, WorkerCtor: CanonicalWorker,
});
const ownedArgs = new Int32Array([4]);
assert.deepEqual(await canonical.request("render", { args: ownedArgs }, 3), new Int32Array([8]));
assert.equal(ownedArgs.byteLength, 0, "canonical request policy transfers owned request bytes");
await assert.rejects(
  canonical.request("render", { args: new Int32Array([-1]) }, 4),
  (error) => error.code === "FE_ACTOR_REMOTE"
    && !error.message.includes("private worker detail"),
);
const interrupted = canonical.request("render", { args: new Int32Array([7]) }, 5);
const interruptingRestart = canonical.restart();
await assert.rejects(
  interrupted,
  (error) => error.code === "FE_ACTOR_WORKER_RESTART"
    && error.message.includes("restarting module worker"),
  "a request started before restart must not slip onto the replacement epoch",
);
assert.equal(await interruptingRestart, 1);
assert.deepEqual(await Promise.all([canonical.restart(), canonical.restart()]), [2, 3]);
assert.equal(canonical.epoch(), 3);
canonical.close();

class GatedRestartWorker extends FakeWorker {
  static readyPort;
  static requestEpochs = [];
  postMessage(message, transfer) {
    this.message = message; this.transfer = transfer;
    message.port.addEventListener("message", ({ data: request }) => {
      GatedRestartWorker.requestEpochs.push(request.actorEpoch);
      message.port.postMessage(actorEnvelope({
        type: "result", lane: request.lane, actorEpoch: request.actorEpoch,
        generation: request.generation, requestId: request.requestId,
        payload: { ok: true, value: new Int32Array([request.payload.args[0]]) },
      }));
    });
    message.port.start();
    if (message.actorEpoch === 0) {
      queueMicrotask(() => message.port.postMessage({ type: "ready" }));
    } else {
      GatedRestartWorker.readyPort = message.port;
    }
  }
}
const gated = await createCanonicalModuleWorkerActor({
  workerUrl: "gated.js", adapter: canonicalAdapter, WorkerCtor: GatedRestartWorker,
});
const gatedRestart = gated.restart();
const gatedRequest = gated.request("render", { args: new Int32Array([9]) }, 1);
await new Promise((resolve) => setTimeout(resolve, 0));
assert.deepEqual(
  GatedRestartWorker.requestEpochs,
  [],
  "request issued after restart waits for replacement worker readiness",
);
GatedRestartWorker.readyPort.postMessage({ type: "ready" });
assert.equal(await gatedRestart, 1);
assert.deepEqual(await gatedRequest, new Int32Array([9]));
assert.deepEqual(GatedRestartWorker.requestEpochs, [1]);
gated.close();

class CancelWorker extends FakeWorker {
  static messages = [];
  postMessage(message, transfer) {
    this.message = message; this.transfer = transfer;
    message.port.addEventListener("message", ({ data }) => {
      CancelWorker.messages.push(data);
    });
    message.port.start();
    queueMicrotask(() => message.port.postMessage({ type: "ready" }));
  }
}
const cancellable = await createCanonicalModuleWorkerActor({
  workerUrl: "cancel.js", adapter: canonicalAdapter, WorkerCtor: CancelWorker,
});
const cancelController = new AbortController();
const cancelArgs = new Int32Array([12]);
const cancelledRequest = cancellable.request(
  "render",
  { args: cancelArgs },
  2,
  { signal: cancelController.signal },
);
await new Promise((resolve) => setTimeout(resolve, 0));
assert.equal(cancelArgs.byteLength, 0, "request ownership transfers before later cancellation");
cancelController.abort();
await assert.rejects(cancelledRequest, (error) => error.code === "FE_ACTOR_ABORTED");
await new Promise((resolve) => setTimeout(resolve, 0));
assert.deepEqual(CancelWorker.messages.map(({ type, requestId }) => [type, requestId]), [
  ["request", 1],
  ["cancel", 1],
]);
cancellable.close();

FakeWorker.replies = [{ type: "init-error", error: "bad wasm" }];
await assert.rejects(createModuleWorkerActor({ workerUrl: "bad.js", ...schemas,
  WorkerCtor: FakeWorker }), /FE_ACTOR_WORKER_PROTOCOL/);
assert.equal(FakeWorker.instances.at(-1).terminated, true);

FakeWorker.replies = [{ type: "init-error", error: "FE_ACTOR_WORKER_INIT" }];
await assert.rejects(createModuleWorkerActor({ workerUrl: "init.js", ...schemas,
  WorkerCtor: FakeWorker }), (error) =>
  error.code === "FE_ACTOR_WORKER_INIT" && !error.message.includes("bad wasm"));

FakeWorker.replies = [{ type: "ready", extra: true }];
await assert.rejects(createModuleWorkerActor({ workerUrl: "malformed.js", ...schemas,
  WorkerCtor: FakeWorker }), /FE_ACTOR_WORKER_PROTOCOL/);

class SilentWorker extends FakeWorker {
  postMessage(message, transfer) { this.message = message; this.transfer = transfer; }
}
const silentController = new AbortController();
const silentStart = createModuleWorkerActor({ workerUrl: "silent.js", ...schemas,
  WorkerCtor: SilentWorker, signal: silentController.signal });
await new Promise((resolve) => setTimeout(resolve, 0));
silentController.abort();
await assert.rejects(silentStart, (error) => error.code === "FE_ACTOR_ABORTED");
assert.equal(FakeWorker.instances.at(-1).terminated, true);

console.log("canonical module worker transfer/restart/errors plus external cancellation: ok");
