import { instantiateWasm } from "../webgpu-keystone/wasm-runner.js";
import { attachMessagePortActorHost } from "../shared/message-port-actor.js";
import {
  createTypedGpuActorClient,
  selectActorSchemas,
} from "../shared/gpu-actor.js";
import { createExactLaneRouter } from "../shared/actor-router.js";
import {
  compiledCanonicalInterface,
  compileActorAdapter,
  createActorAdapter,
  createHostEffectAdapter,
  createInterfaceCaller,
} from "./gen-schedule32/actor-interface.js";

const WIDTH = 128;
const HEIGHT = 128;

const laneNames = (execution, placement = null) =>
  Object.entries(compiledCanonicalInterface.lanes)
    .filter(([, lane]) => lane.intent.execution === execution
      && (placement === null || lane.intent.placement === placement))
  .map(([name]) => name);

const gpuLaneNames = () => Object.entries(compiledCanonicalInterface.lanes)
  .filter(([, lane]) => lane.intent.capabilities
    .some(({ capability }) => capability === "webgpu_dispatch"))
  .map(([name]) => name);

self.addEventListener("message", async ({ data }) => {
  if (data?.type !== "init") return;
  const { port, gpuPort, wasm, actorEpoch } = data;
  try {
    const exports = await instantiateWasm(wasm);
    const wasmCaller = createInterfaceCaller(exports);
    const wasmActor = createActorAdapter(exports, { placement: "worker" });
    const schemas = compileActorAdapter();
    const gpuLanes = gpuLaneNames();
    const gpu = createTypedGpuActorClient(gpuPort, {
      ...selectActorSchemas(schemas, gpuLanes),
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
    const router = createExactLaneRouter(compiledCanonicalInterface.lanes, {
      gpu_main_thread: {
        lanes: laneNames("host_effect", "main_thread"),
        dispatch: (request) => gpu.request(
          request.lane,
          request.payload,
          request.generation,
        ),
      },
      worker_host: {
        lanes: laneNames("host_effect", "worker"),
        dispatch: hostEffects.dispatch,
      },
      wasm: {
        lanes: laneNames("wasm"),
        dispatch: wasmActor.dispatch,
      },
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
