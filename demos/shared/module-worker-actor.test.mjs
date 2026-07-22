import assert from "node:assert/strict";
import { actorField, actorResultSchema, exactObject } from "./actor-endpoint.js";
import { createModuleWorkerActor } from "./module-worker-actor.js";

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

FakeWorker.replies = [{ type: "init-error", error: "bad wasm" }];
await assert.rejects(createModuleWorkerActor({ workerUrl: "bad.js", ...schemas,
  WorkerCtor: FakeWorker }), /bad wasm/);
assert.equal(FakeWorker.instances.at(-1).terminated, true);

FakeWorker.replies = [{ type: "ready", extra: true }];
await assert.rejects(createModuleWorkerActor({ workerUrl: "malformed.js", ...schemas,
  WorkerCtor: FakeWorker }), /malformed module worker readiness/);

console.log("shared module worker actor restart/init-failure/malformed-ready/auxiliary ports: ok");
