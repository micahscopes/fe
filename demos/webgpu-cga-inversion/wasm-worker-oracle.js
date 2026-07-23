import { actorEnvelope } from "../shared/actor-coordinator.js";
import { createModuleWorkerActor } from "../shared/module-worker-actor.js";
import {
  createTypedMainThreadGpuBroker,
  selectActorSchemas,
} from "../shared/gpu-actor.js";

const requestPayload = (values, generation) => ({
  generation,
  cam_x: values[0],
  cam_y: values[1],
  zoom: values[2],
  inv_cx: values[3],
  inv_cy: values[4],
});

export async function createCgaWasmWorkerOracle({
  wasm,
  gpuRender,
  gpuVerify,
}) {
  let requestId = 0;
  const { compileActorAdapter, compiledCanonicalInterface } =
    await import("./gen-schedule32/actor-interface.js");
  const schemas = compileActorAdapter();
  const gpuLanes = Object.entries(compiledCanonicalInterface.lanes)
    .filter(([, lane]) => lane.intent.capabilities
      .some(({ capability }) => capability === "webgpu_dispatch"))
    .map(([name]) => name);
  const gpuSchemas = selectActorSchemas(schemas, gpuLanes);
  const actor = await createModuleWorkerActor({
    workerUrl: new URL("./wasm-oracle-worker.js", import.meta.url),
    init: { wasm },
    requestSchema: schemas.requestSchema,
    resultSchema: schemas.resultSchema,
    createAuxiliaryPorts(epoch) {
      const channel = new MessageChannel();
      const broker = createTypedMainThreadGpuBroker(channel.port1, {
        handlers: {
          render: (payload, request) => gpuRender(payload, request),
          verify: (payload, request) => gpuVerify(payload, request),
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
  const request = async (lane, values, generation = 0) => {
    const result = await actor.request(actorEnvelope({
      type: "request",
      lane,
      actorEpoch: actor.epoch(),
      generation,
      requestId: ++requestId,
      payload: requestPayload(values, generation),
    }));
    if (!result.payload.ok) throw new Error(result.payload.error);
    return result.payload.value;
  };
  return {
    render: (values, generation = 0) => request("oracle", values, generation),
    renderGpu: (values, generation = 0) => request("render", values, generation),
    async restart() {
      requestId = 0;
      return actor.restart();
    },
    close: actor.close,
    epoch: actor.epoch,
  };
}
