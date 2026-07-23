import { instantiateWasm } from "../webgpu-keystone/wasm-runner.js";
import { attachMessagePortActorHost } from "../shared/message-port-actor.js";
import { createGpuActorClient } from "../shared/gpu-actor.js";
import { createExactLaneRouter } from "../shared/actor-router.js";
import {
  compiledCanonicalInterface,
  createActorAdapter,
  createHostEffectAdapter,
  createInterfaceCaller,
} from "./gen/actor-interface.js";

const WIDTH = 128;
const HEIGHT = 128;

self.addEventListener("message", async ({ data }) => {
  if (data?.type !== "init") return;
  const { port, gpuPort, wasm, actorEpoch } = data;
  try {
    const exports = await instantiateWasm(wasm);
    const wasmCaller = createInterfaceCaller(exports);
    const wasmActor = createActorAdapter(exports);
    const gpu = createGpuActorClient(gpuPort, {
      valueCount: 0,
      rgbaBytes: WIDTH * HEIGHT * 4,
      initialEpoch: actorEpoch,
    });
    const hostEffects = createHostEffectAdapter({
      render: ({ generation }) => gpu.render([], generation),
      verify: ({ generation }) => gpu.verify([], generation),
      oracle: async () => {
        const frame = new Uint8Array(WIDTH * HEIGHT * 4);
        const words = new DataView(frame.buffer);
        for (let y = 0; y < HEIGHT; y += 1) {
          for (let x = 0; x < WIDTH; x += 1) {
            const { rgba } = await wasmCaller.call("oracle_pixel", { x, y });
            words.setUint32((y * WIDTH + x) * 4, rgba, true);
          }
        }
        return frame;
      },
    });
    // Placement is explicit application policy, while the complete lane set is
    // compiler-derived. Initialization fails if a newly generated Fe lane is
    // unowned or multiply owned; runtime dispatch has no fallback actor.
    const router = createExactLaneRouter(compiledCanonicalInterface.lanes, {
      host: {
        lanes: ["render", "verify", "oracle"],
        dispatch: hostEffects.dispatch,
      },
      wasm: {
        lanes: ["oracle_pixel"],
        dispatch: wasmActor.dispatch,
      },
    });
    attachMessagePortActorHost(
      port,
      router.dispatch,
      { transferResult: hostEffects.transferResult },
    );
    port.postMessage({ type: "ready" });
  } catch (error) {
    port.postMessage({ type: "init-error", error: String(error) });
  }
});
