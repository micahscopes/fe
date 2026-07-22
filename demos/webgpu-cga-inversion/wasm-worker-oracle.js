import { actorEnvelope } from "../shared/actor-coordinator.js";
import { actorField, actorResultSchema, createActorEndpoint, exactObject } from "../shared/actor-endpoint.js";
import { createMessagePortActorTransport } from "../shared/message-port-actor.js";

export async function createCgaWasmWorkerOracle({ wasm, exportName, width, height }) {
  let epoch = 0;
  let requestId = 0;
  let worker;
  let endpoint;
  let transport;
  const start = async () => {
    worker = new Worker(new URL("./wasm-oracle-worker.js", import.meta.url), { type: "module" });
    const channel = new MessageChannel();
    transport = createMessagePortActorTransport(channel.port1);
    endpoint = createActorEndpoint({
      transport,
      initialEpoch: epoch,
      requestSchema: { render: (payload) => exactObject(payload,
        { values: actorField.float32Array(5) }) ,
        verify: (payload) => exactObject(payload, { values: actorField.float32Array(5) }) },
      resultSchema: { render: actorResultSchema(actorField.uint8Array(width * height * 4)),
        verify: actorResultSchema(actorField.uint8Array(width * height * 4)) },
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
    worker.addEventListener("error", (event) => transport.fail(event.message || "Wasm worker error"));
    worker.postMessage({ type: "init", port: channel.port2, wasm, exportName, width, height }, [channel.port2]);
    await ready;
  };
  await start();
  return {
    async render(values, generation = 0) {
      requestId += 1;
      const result = await endpoint.request(actorEnvelope({
        type: "request", lane: "verify", actorEpoch: epoch, generation, requestId,
        payload: { values: new Float32Array(values) },
      }));
      if (!result.payload.ok) throw new Error(result.payload.error);
      return result.payload.value;
    },
    async restart() {
      endpoint.close("restarting Wasm worker");
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
