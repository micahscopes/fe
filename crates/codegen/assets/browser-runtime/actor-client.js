import { createCanonicalMainThreadGpuBroker } from "./gpu-actor.js";
import {
  createCanonicalModuleWorkerActor,
  createModuleWorkerScope,
} from "./module-worker-actor.js";
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

// Construct the policy-free browser capability consumed by Fe's
// `ChildPlacement<WasmBackend>` handler. Fe supplies every epoch and decides
// when this capability is called.
export function createCanonicalBrowserWorkerScope(options) {
  if (!options || typeof options !== "object" || Array.isArray(options)) {
    throw new TypeError("canonical browser Worker scope options must be an object");
  }
  if (Object.hasOwn(options, "initialEpoch") || Object.hasOwn(options, "signal")) {
    throw new TypeError("the owning Fe scope supplies Worker epoch and cancellation");
  }
  return createModuleWorkerScope({
    createActor: ({ initialEpoch, signal }) => createCanonicalBrowserActor({
      ...options,
      initialEpoch,
      signal,
    }),
  });
}
