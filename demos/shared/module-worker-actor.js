import { createActorEndpoint } from "./actor-endpoint.js";
import { createMessagePortActorTransport } from "./message-port-actor.js";

export async function createModuleWorkerActor({
  workerUrl, init = {}, requestSchema, resultSchema,
  createAuxiliaryPorts = () => ({ message: {}, transfer: [], close() {} }),
  WorkerCtor = Worker, MessageChannelCtor = MessageChannel,
}) {
  let epoch = 0;
  let worker;
  let endpoint;
  let auxiliary;
  const start = async () => {
    worker = new WorkerCtor(workerUrl, { type: "module" });
    const channel = new MessageChannelCtor();
    const transport = createMessagePortActorTransport(channel.port1);
    endpoint = createActorEndpoint({ transport, initialEpoch: epoch, requestSchema, resultSchema });
    auxiliary = createAuxiliaryPorts(epoch);
    const ready = new Promise((resolve, reject) => {
      const cleanup = () => channel.port1.removeEventListener("message", onMessage);
      const onMessage = (event) => {
        const message = event.data;
        if (message?.type === "ready" && Object.keys(message).length === 1) {
          cleanup(); resolve(); return;
        }
        if (message?.type === "init-error" && typeof message.error === "string") {
          cleanup(); reject(new Error(message.error)); return;
        }
        cleanup(); reject(new TypeError("malformed module worker readiness message"));
      };
      channel.port1.addEventListener("message", onMessage);
      worker.addEventListener("error", (event) => reject(event.error || new Error(event.message)), { once: true });
    });
    worker.addEventListener("error", (event) => transport.fail(event.message || "module worker error"));
    worker.postMessage({ ...init, ...auxiliary.message, type: "init",
      port: channel.port2, actorEpoch: epoch }, [channel.port2, ...(auxiliary.transfer || [])]);
    try { await ready; } catch (error) {
      endpoint.close("module worker initialization failed");
      auxiliary.close?.();
      worker.terminate();
      throw error;
    }
  };
  await start();
  return Object.freeze({
    request: (envelope) => endpoint.request(envelope),
    async restart() {
      endpoint.close("restarting module worker");
      auxiliary.close?.();
      worker.terminate();
      epoch += 1;
      await start();
      return epoch;
    },
    close() { endpoint.close(); auxiliary.close?.(); worker.terminate(); },
    epoch: () => epoch,
  });
}
