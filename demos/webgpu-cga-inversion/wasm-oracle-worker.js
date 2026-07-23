import { instantiateWasm } from "../webgpu-keystone/wasm-runner.js";
import { attachMessagePortActorHost } from "./gen-schedule32/actor/runtime/message-port-actor.js";
import {
  createCanonicalMainThreadGpuClient,
} from "./gen-schedule32/actor/runtime/gpu-actor.js";
import { createCanonicalIntentRouter } from "./gen-schedule32/actor/runtime/actor-router.js";
import {
  compileActorAdapter,
  createActorAdapter,
} from "./gen-schedule32/actor/interface.js";

self.addEventListener("message", async ({ data }) => {
  if (data?.type !== "init") return;
  const { port, gpuPort, wasm, actorEpoch } = data;
  try {
    const exports = await instantiateWasm(wasm);
    const wasmActor = createActorAdapter(exports, { placement: "worker" });
    const schemas = compileActorAdapter();
    const gpu = createCanonicalMainThreadGpuClient(gpuPort, {
      adapter: schemas,
      initialEpoch: actorEpoch,
    });
    const router = createCanonicalIntentRouter(schemas, {
      main_thread_host: (request, { signal } = {}) => gpu.request(
          request.lane,
          request.payload,
          request.generation,
          { signal },
        ),
      wasm: (request, context) => wasmActor.dispatch(request, context),
    });
    attachMessagePortActorHost(port, router.dispatch, {
      transferResult: schemas.transferResult,
    });
    port.postMessage({ type: "ready" });
  } catch (error) {
    port.postMessage({ type: "init-error", error: "FE_ACTOR_WORKER_INIT" });
  }
});
