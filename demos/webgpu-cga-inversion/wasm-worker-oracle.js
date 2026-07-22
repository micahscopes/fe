import { actorEnvelope } from "../shared/actor-coordinator.js";
import { actorField, actorResultSchema, exactObject } from "../shared/actor-endpoint.js";
import { createModuleWorkerActor } from "../shared/module-worker-actor.js";
import { createMainThreadGpuBroker } from "../shared/gpu-actor.js";

export async function createCgaWasmWorkerOracle({ wasm, exportName, width, height, gpuRender, gpuVerify }) {
  let requestId = 0;
  const actor = await createModuleWorkerActor({
    workerUrl: new URL("./wasm-oracle-worker.js", import.meta.url),
    init: { wasm, exportName, width, height },
    requestSchema: { render: (payload) => exactObject(payload,
      { values: actorField.float32Array(5) }), verify: (payload) => exactObject(payload,
      { values: actorField.float32Array(5) }) },
    resultSchema: { render: actorResultSchema((value) => exactObject(value,
        { submitted: actorField.boolean })),
      verify: actorResultSchema(actorField.uint8Array(width * height * 4)) },
    createAuxiliaryPorts(epoch) {
      const channel = new MessageChannel();
      const broker = createMainThreadGpuBroker(channel.port1, { render: gpuRender,
        verify: gpuVerify, valueCount: 5, rgbaBytes: width * height * 4, initialEpoch: epoch });
      return { message: { gpuPort: channel.port2 }, transfer: [channel.port2],
        close: () => broker.close() };
    },
  });
  const request = async (lane, values, generation) => {
    const result = await actor.request(actorEnvelope({ type: "request", lane,
      actorEpoch: actor.epoch(), generation, requestId: ++requestId,
      payload: { values: new Float32Array(values) } }));
    if (!result.payload.ok) throw new Error(result.payload.error);
    return result.payload.value;
  };
  return {
    render: (values, generation = 0) => request("verify", values, generation),
    renderGpu: (values, generation = 0) => request("render", values, generation),
    async restart() { requestId = 0; return actor.restart(); },
    close: actor.close,
    epoch: actor.epoch,
  };
}
