import { instantiateWasm } from "../webgpu-keystone/wasm-runner.js";
import { attachMessagePortActorHost } from "./gen-schedule32/actor/runtime/message-port-actor.js";
import {
  createCanonicalMainThreadGpuClient,
} from "./gen-schedule32/actor/runtime/gpu-actor.js";
import { createCanonicalIntentRouter } from "./gen-schedule32/actor/runtime/actor-router.js";
import {
  compileActorAdapter,
  createActorAdapter,
  createHostEffectAdapter,
  createInterfaceCaller,
} from "./gen-schedule32/actor/interface.js";

const WIDTH = 128;
const HEIGHT = 128;

self.addEventListener("message", async ({ data }) => {
  if (data?.type !== "init") return;
  const { port, gpuPort, wasm, actorEpoch } = data;
  try {
    const exports = await instantiateWasm(wasm);
    const wasmCaller = createInterfaceCaller(exports);
    const wasmActor = createActorAdapter(exports, { placement: "worker" });
    const schemas = compileActorAdapter();
    const gpu = createCanonicalMainThreadGpuClient(gpuPort, {
      adapter: schemas,
      initialEpoch: actorEpoch,
    });
    const hostEffects = createHostEffectAdapter({
      oracle: async (request) => {
        const frame = new Uint8Array(WIDTH * HEIGHT * 4);
        const words = new DataView(frame.buffer);
        const { generation: _, ...view } = request;
        for (let y = 0; y < HEIGHT; y += 1) {
          for (let x = 0; x < WIDTH; x += 1) {
            const { rgba } = await wasmCaller.call("oracle_pixel", { x, y, ...view });
            words.setUint32((y * WIDTH + x) * 4, rgba, true);
          }
        }
        return frame;
      },
    }, { placement: "worker" });
    const router = createCanonicalIntentRouter(schemas, {
      main_thread_host: (request) => gpu.request(
          request.lane,
          request.payload,
          request.generation,
        ),
      worker_host: hostEffects.dispatch,
      wasm: wasmActor.dispatch,
    });
    attachMessagePortActorHost(port, router.dispatch, {
      transferResult(value, request) {
        return hostEffects.transferResult(value, request);
      },
    });
    port.postMessage({ type: "ready" });
  } catch (error) {
    port.postMessage({ type: "init-error", error: "FE_ACTOR_WORKER_INIT" });
  }
});
