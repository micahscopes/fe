import { instantiateWasm } from "../webgpu-keystone/wasm-runner.js";
import { attachMessagePortActorHost } from "./gen/runtime/message-port-actor.js";
import {
  createTypedGpuActorClient,
  selectActorSchemas,
} from "./gen/runtime/gpu-actor.js";
import { createExactLaneRouter } from "./gen/runtime/actor-router.js";
import {
  compiledCanonicalInterface,
  compileActorAdapter,
  createActorAdapter,
  createHostEffectAdapter,
} from "./gen/actor-interface.js";

self.addEventListener("message", async ({ data }) => {
  if (data?.type !== "init") return;
  const { port, gpuPort, wasm, actorEpoch } = data;
  try {
    const exports = await instantiateWasm(wasm);
    const wasmActor = createActorAdapter(exports);
    const schemas = compileActorAdapter();
    const gpu = createTypedGpuActorClient(gpuPort, {
      ...selectActorSchemas(schemas, ["render", "verify"]),
      initialEpoch: actorEpoch,
    });
    const hostEffects = createHostEffectAdapter({
      render: (request) => gpu.request("render", request, request.generation),
      verify: (request) => gpu.request("verify", request, request.generation),
    });
    // Placement is explicit application policy, while the complete lane set is
    // compiler-derived. Initialization fails if a newly generated Fe lane is
    // unowned or multiply owned; runtime dispatch has no fallback actor.
    const router = createExactLaneRouter(compiledCanonicalInterface.lanes, {
      host: {
        lanes: ["render", "verify"],
        dispatch: hostEffects.dispatch,
      },
      wasm: {
        lanes: ["oracle"],
        dispatch: wasmActor.dispatch,
      },
    });
    attachMessagePortActorHost(
      port,
      router.dispatch,
      { transferResult: schemas.transferResult },
    );
    port.postMessage({ type: "ready" });
  } catch (error) {
    port.postMessage({ type: "init-error", error: String(error) });
  }
});
