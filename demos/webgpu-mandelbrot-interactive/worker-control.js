import { actorEnvelope } from "../shared/actor-coordinator.js";
import { actorField, actorResultSchema, createActorEndpoint, exactObject } from "../shared/actor-endpoint.js";
import { createMessagePortActorTransport } from "../shared/message-port-actor.js";

export async function createMandelbrotWorkerControl({ wasm, exportName }) {
  let epoch = 0;
  let requestId = 0;
  let worker;
  let endpoint;
  let transport;
  const start = async () => {
    worker = new Worker(new URL("./control-worker.js", import.meta.url), { type: "module" });
    const channel = new MessageChannel();
    transport = createMessagePortActorTransport(channel.port1);
    endpoint = createActorEndpoint({
      transport,
      initialEpoch: epoch,
      requestSchema: { render: (payload) => exactObject(payload,
        { args: actorField.int32Array(8) }), verify: (payload) => exactObject(payload,
        { args: actorField.int32Array(8) }) },
      resultSchema: { render: actorResultSchema(actorField.int32Array(3)),
        verify: actorResultSchema(actorField.int32Array(3)) },
    });
    const ready = new Promise((resolve, reject) => {
      const onMessage = (event) => {
        if (event.data?.type === "ready") { channel.port1.removeEventListener("message", onMessage); resolve(); }
        if (event.data?.type === "init-error") {
          channel.port1.removeEventListener("message", onMessage);
          reject(new Error(event.data.error));
        }
      };
      channel.port1.addEventListener("message", onMessage);
      worker.addEventListener("error", reject, { once: true });
    });
    worker.addEventListener("error", (event) => transport.fail(event.message || "control worker error"));
    worker.postMessage({ type: "init", port: channel.port2, wasm, exportName }, [channel.port2]);
    await ready;
  };
  await start();
  return {
    async update(args, generation = 0) {
      requestId += 1;
      const result = await endpoint.request(actorEnvelope({
        type: "request", lane: "render", actorEpoch: epoch, generation, requestId,
        payload: { args: new Int32Array(args) },
      }));
      if (!result.payload.ok) throw new Error(result.payload.error);
      return Array.from(result.payload.value);
    },
    async restart() {
      endpoint.close("restarting control worker");
      worker.terminate();
      epoch += 1;
      requestId = 0;
      await start();
      return epoch;
    },
    close() { endpoint.close(); worker.terminate(); },
    epoch: () => epoch,
  };
}
