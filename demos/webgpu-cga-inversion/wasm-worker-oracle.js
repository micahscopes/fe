import { createCanonicalModuleWorkerActor } from "../shared/module-worker-actor.js";
import {
  createCanonicalMainThreadGpuBroker,
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
  const { compileActorAdapter } =
    await import("./gen-schedule32/actor-interface.js");
  const adapter = compileActorAdapter();
  const actor = await createCanonicalModuleWorkerActor({
    workerUrl: new URL("./wasm-oracle-worker.js", import.meta.url),
    init: { wasm },
    adapter,
    maxPending: 8,
    createAuxiliaryPorts(epoch) {
      const channel = new MessageChannel();
      const broker = createCanonicalMainThreadGpuBroker(channel.port1, {
        adapter,
        handlers: {
          render: (payload, request) => gpuRender(payload, request),
          verify: (payload, request) => gpuVerify(payload, request),
        },
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
    return actor.request(lane, requestPayload(values, generation), generation);
  };
  return {
    render: (values, generation = 0) => request("oracle", values, generation),
    renderGpu: (values, generation = 0) => request("render", values, generation),
    restart: actor.restart,
    close: actor.close,
    epoch: actor.epoch,
  };
}
