import { instantiateWasm } from "../webgpu-keystone/wasm-runner.js";
import { attachMessagePortActorHost } from "./gen/runtime/message-port-actor.js";
import {
  createCanonicalMainThreadGpuClient,
} from "./gen/runtime/gpu-actor.js";
import { createCanonicalIntentRouter } from "./gen/runtime/actor-router.js";
import {
  compileActorAdapter,
  createActorAdapter,
} from "./gen/actor-interface.js";

self.addEventListener("message", async ({ data }) => {
  if (data?.type !== "init") return;
  const { port, gpuPort, wasm, actorEpoch } = data;
  try {
    const exports = await instantiateWasm(wasm);
    const wasmActor = createActorAdapter(exports, { placement: "worker" });
    const adapter = compileActorAdapter();
    const gpu = createCanonicalMainThreadGpuClient(gpuPort, {
      adapter,
      initialEpoch: actorEpoch,
    });
    const router = createCanonicalIntentRouter(adapter, {
      main_thread_host: (request, { signal } = {}) => gpu.request(
        request.lane,
        request.payload,
        request.generation,
        { signal },
      ),
      wasm: (request, context) => wasmActor.dispatch(request, context),
    });
    attachMessagePortActorHost(
      port,
      router.dispatch,
      { transferResult: adapter.transferResult },
    );
    port.postMessage({ type: "ready" });
  } catch (error) {
    // Keep the wire protocol stable and non-leaky. The owning realm gets the
    // detailed diagnostic in its developer console; the actor boundary carries
    // only the canonical failure code accepted by ModuleWorkerActor.
    console.error("QCGA module worker initialization failed", error);
    port.postMessage({ type: "init-error", error: "FE_ACTOR_WORKER_INIT" });
  }
});
