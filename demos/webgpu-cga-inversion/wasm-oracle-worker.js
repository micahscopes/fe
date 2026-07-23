import { instantiateWasm, renderFragmentGrid } from "../webgpu-keystone/wasm-runner.js";
import {
  attachMessagePortActorHost,
  transferOwnedTypedArray,
} from "../shared/message-port-actor.js";
import { createGpuActorClient } from "../shared/gpu-actor.js";

self.addEventListener("message", async (event) => {
  if (event.data?.type !== "init") return;
  const { port, gpuPort, wasm, exportName, width, height, actorEpoch } = event.data;
  try {
    const exports = await instantiateWasm(wasm);
    const gpu = createGpuActorClient(gpuPort, {
      valueCount: 5, rgbaBytes: width * height * 4, initialEpoch: actorEpoch,
    });
    attachMessagePortActorHost(port, ({ lane, payload, generation }) => {
      if (lane === "render") return gpu.render(payload.values, generation);
      const words = renderFragmentGrid(exports, exportName, Array.from(payload.values), width, height);
      return new Uint8Array(words.buffer.slice(words.byteOffset, words.byteOffset + words.byteLength));
    }, {
      transferResult: (value, request) =>
        request.lane === "verify" ? transferOwnedTypedArray(value) : [],
    });
    port.postMessage({ type: "ready" });
  } catch (error) {
    port.postMessage({ type: "init-error", error: String(error) });
  }
});
