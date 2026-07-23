import {
  createCanonicalModuleWorkerActor,
} from "./gen/runtime/module-worker-actor.js";
import {
  createCanonicalMainThreadGpuBroker,
} from "./gen/runtime/gpu-actor.js";

export async function createQcgaActor({
  wasm,
  width,
  height,
  params,
  gpuRender,
  gpuVerify,
}) {
  const { compileActorAdapter } = await import("./gen/actor-interface.js");
  const adapter = compileActorAdapter();
  const actor = await createCanonicalModuleWorkerActor({
    workerUrl: new URL("./wasm-worker.js", import.meta.url),
    init: { wasm },
    adapter,
    createAuxiliaryPorts(epoch) {
      const channel = new MessageChannel();
      const broker = createCanonicalMainThreadGpuBroker(channel.port1, {
        adapter,
        handlers: {
          render: (request) =>
            gpuRender(params.map(({ name }) => request[name]), request),
          verify: (request) =>
            gpuVerify(params.map(({ name }) => request[name]), request),
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
  const request = (lane, payload, options) =>
    actor.request(lane, payload, payload.generation, options);
  return {
    render: (payload, options) => request("render", payload, options),
    gpu: (payload, options) => request("verify", payload, options),
    wasm: (payload, options) => request("oracle", payload, options),
    restart: actor.restart,
    close: actor.close,
    epoch: actor.epoch,
    pendingCount: actor.pendingCount,
    status: actor.status,
  };
}
