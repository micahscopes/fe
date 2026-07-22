import { instantiateWasm, renderFragmentGrid } from "../webgpu-keystone/wasm-runner.js";
import { attachMessagePortActorHost } from "../shared/message-port-actor.js";

self.addEventListener("message", async (event) => {
  if (event.data?.type !== "init") return;
  const { port, wasm, exportName, width, height } = event.data;
  try {
    const exports = await instantiateWasm(wasm);
    attachMessagePortActorHost(port, ({ payload }) => {
      const words = renderFragmentGrid(exports, exportName, Array.from(payload.values), width, height);
      return new Uint8Array(words.buffer.slice(words.byteOffset, words.byteOffset + words.byteLength));
    });
    port.postMessage({ type: "ready" });
  } catch (error) {
    port.postMessage({ type: "init-error", error: String(error) });
  }
});
