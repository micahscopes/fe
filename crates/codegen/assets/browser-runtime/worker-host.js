import { createCanonicalIntentRouter } from "./actor-router.js";
import { createCanonicalMainThreadGpuClient } from "./gpu-actor.js";
import { attachMessagePortActorHost } from "./message-port-actor.js";
import {
  compileActorAdapter,
  createActorAdapter,
} from "../interface.js";

const INIT_ERROR = "FE_ACTOR_WORKER_INIT";

const instantiateCanonicalWasm = async (wasm) => {
  const instantiated = await WebAssembly.instantiate(wasm, {});
  return instantiated.instance?.exports ?? instantiated.exports;
};

const canonicalDispatchers = (adapter, wasmActor, gpu) => {
  const owners = new Set();
  for (const intent of Object.values(adapter.intents)) {
    if (intent.execution === "wasm") owners.add("wasm");
    else if (intent.execution === "host_effect" && intent.placement === "main_thread") {
      owners.add("main_thread_host");
    } else {
      throw new TypeError("generated Worker host cannot own this canonical lane intent");
    }
  }
  const dispatchers = {};
  if (owners.has("wasm")) {
    dispatchers.wasm = (request, context) => wasmActor.dispatch(request, context);
  }
  if (owners.has("main_thread_host")) {
    if (!gpu) throw new TypeError("canonical main-thread host lanes require a GPU port");
    dispatchers.main_thread_host = (request, { signal } = {}) => gpu.request(
      request.lane,
      request.payload,
      request.generation,
      { signal },
    );
  }
  return dispatchers;
};

export async function attachCanonicalWorkerHost({
  port,
  gpuPort,
  wasm,
  actorEpoch,
}) {
  const exports = await instantiateCanonicalWasm(wasm);
  const adapter = compileActorAdapter();
  const wasmActor = createActorAdapter(exports, { placement: "worker" });
  const hasMainThreadGpu = Object.values(adapter.intents).some((intent) =>
    intent.execution === "host_effect" && intent.placement === "main_thread");
  const gpu = hasMainThreadGpu
    ? createCanonicalMainThreadGpuClient(gpuPort, { adapter, initialEpoch: actorEpoch })
    : null;
  const router = createCanonicalIntentRouter(
    adapter,
    canonicalDispatchers(adapter, wasmActor, gpu),
  );
  attachMessagePortActorHost(port, router.dispatch, {
    transferResult: adapter.transferResult,
  });
  port.postMessage({ type: "ready" });
}

export function installCanonicalWorkerHost(scope = globalThis) {
  scope.addEventListener("message", async ({ data }) => {
    if (data?.type !== "init") return;
    try {
      await attachCanonicalWorkerHost(data);
    } catch (error) {
      console.error("canonical Worker initialization failed", error);
      data?.port?.postMessage({ type: "init-error", error: INIT_ERROR });
    }
  });
}

installCanonicalWorkerHost();
