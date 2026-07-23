import { actorEnvelope } from "./gen/runtime/actor-coordinator.js";
import { createModuleWorkerActor } from "./gen/runtime/module-worker-actor.js";
import {
  createTypedMainThreadGpuBroker,
  selectActorSchemas,
} from "./gen/runtime/gpu-actor.js";

export async function createQcgaActor({
  wasm,
  width,
  height,
  gpuRender,
  gpuVerify,
}) {
  let requestId = 0;
  const { compileActorAdapter } = await import("./gen/actor-interface.js");
  const schemas = compileActorAdapter();
  const gpuSchemas = selectActorSchemas(schemas, ["render", "verify"]);
  const actor = await createModuleWorkerActor({
    workerUrl: new URL("./wasm-worker.js", import.meta.url),
    init: { wasm },
    requestSchema: schemas.requestSchema,
    resultSchema: schemas.resultSchema,
    createAuxiliaryPorts(epoch) {
      const channel = new MessageChannel();
      const broker = createTypedMainThreadGpuBroker(channel.port1, {
        handlers: {
          render: (_payload, request) => gpuRender([], request),
          verify: (_payload, request) => gpuVerify([], request),
        },
        ...gpuSchemas,
        initialEpoch: epoch,
      });
      return {
        message: { gpuPort: channel.port2 },
        transfer: [channel.port2],
        close: () => broker.close(),
      };
    },
  });
  const request = async (lane, generation = 0) => {
    const result = await actor.request(actorEnvelope({
      type: "request",
      lane,
      actorEpoch: actor.epoch(),
      generation,
      requestId: ++requestId,
      payload: { generation },
    }));
    if (!result.payload.ok) throw new Error(result.payload.error);
    return result.payload.value;
  };
  return {
    render: (generation = 0) => request("render", generation),
    gpu: (generation = 0) => request("verify", generation),
    wasm: (generation = 0) => request("oracle", generation),
    async restart() {
      requestId = 0;
      return actor.restart();
    },
    close: actor.close,
    epoch: actor.epoch,
  };
}
