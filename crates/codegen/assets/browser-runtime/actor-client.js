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

// Bind compiler-derived scalar mailbox edges to the same structured Worker
// scope. The opaque lane names and codecs come from the child interface, while
// this fixed adapter owns only Promise completion and cancellation mechanics.
export function createCanonicalWorkerMailboxImports({ scope, completions, mailbox }) {
  if (!scope || typeof scope.request !== "function") {
    throw new TypeError("canonical Worker mailbox requires a structured scope");
  }
  if (completions?.protocol !== "fe:generated-completion/v1"
      || typeof completions.begin !== "function") {
    throw new TypeError("canonical Worker mailbox requires the generated completion rail");
  }
  if (!mailbox || typeof mailbox !== "object" || Array.isArray(mailbox)) {
    throw new TypeError("canonical Worker mailbox requires compiler-derived lanes");
  }
  const imports = Object.create(null);
  for (const [lane, codec] of Object.entries(mailbox)) {
    if (!/^request_[0-9a-f]{16}$/.test(lane)
        || !Number.isSafeInteger(codec?.requestWidth) || codec.requestWidth < 0
        || !Number.isSafeInteger(codec?.responseWidth) || codec.responseWidth < 0
        || typeof codec.liftRequest !== "function"
        || typeof codec.lowerResponse !== "function") {
      throw new TypeError("canonical Worker mailbox lane is malformed");
    }
    imports[lane] = (...carriers) => {
      const request = codec.liftRequest(carriers);
      return completions.begin(
        `worker-mailbox/${lane}`,
        signal => scope.request(lane, request, signal),
        codec.responseWidth,
        value => codec.lowerResponse(value),
        () => {},
      );
    };
  }
  return Object.freeze({ "fe:worker-mailbox": Object.freeze(imports) });
}
