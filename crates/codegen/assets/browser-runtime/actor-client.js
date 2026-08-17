import { createCanonicalMainThreadGpuBroker } from "./gpu-actor.js";
import { createCanonicalModuleWorkerActor } from "./module-worker-actor.js";
import { compileActorAdapter } from "../interface.js";

export async function createCanonicalBrowserActor({
  wasm,
  handlers,
  workerUrl = new URL("./worker-host.js", import.meta.url),
  MessageChannelCtor = MessageChannel,
  ...actorOptions
}) {
  if (Object.hasOwn(actorOptions, "adapter")
      || Object.hasOwn(actorOptions, "createAuxiliaryPorts")
      || Object.hasOwn(actorOptions, "init")
      || Object.hasOwn(actorOptions, "supervision")) {
    throw new TypeError(
      "generated actor composition owns adapter, init, and auxiliary ports; supervision is Fe policy",
    );
  }
  const adapter = compileActorAdapter();
  const hasMainThreadGpu = Object.values(adapter.intents).some((intent) =>
    intent.execution === "host_effect" && intent.placement === "main_thread");
  return createCanonicalModuleWorkerActor({
    ...actorOptions,
    workerUrl,
    init: { wasm },
    adapter,
    MessageChannelCtor,
    createAuxiliaryPorts(epoch) {
      if (!hasMainThreadGpu) {
        return { message: {}, transfer: [], close() {} };
      }
      const channel = new MessageChannelCtor();
      const broker = createCanonicalMainThreadGpuBroker(channel.port1, {
        adapter,
        handlers,
        initialEpoch: epoch,
      });
      return {
        message: { gpuPort: channel.port2 },
        transfer: [channel.port2],
        close: () => broker.close(),
      };
    },
  });
}
