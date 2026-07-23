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
  params,
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
          render: (request) =>
            gpuRender(params.map(({ name }) => request[name]), request),
          verify: (request) =>
            gpuVerify(params.map(({ name }) => request[name]), request),
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
  const request = async (lane, payload) => {
    const generation = payload.generation;
    const result = await actor.request(actorEnvelope({
      type: "request",
      lane,
      actorEpoch: actor.epoch(),
      generation,
      requestId: ++requestId,
      payload,
    }));
    if (!result.payload.ok) throw new Error(result.payload.error);
    return result.payload.value;
  };
  return {
    render: (payload) => request("render", payload),
    gpu: (payload) => request("verify", payload),
    wasm: (payload) => request("oracle", payload),
    async restart() {
      requestId = 0;
      return actor.restart();
    },
    close: actor.close,
    epoch: actor.epoch,
  };
}
