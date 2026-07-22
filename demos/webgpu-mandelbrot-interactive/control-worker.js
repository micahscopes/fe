import { instantiateWasm } from "../webgpu-keystone/wasm-runner.js";
import { attachMessagePortActorHost } from "../shared/message-port-actor.js";

self.addEventListener("message", async (event) => {
  if (event.data?.type !== "init") return;
  const { port, wasm, exportName } = event.data;
  try {
    const exports = await instantiateWasm(wasm);
    const control = exports[exportName];
    if (typeof control !== "function") throw new Error(`control export ${exportName} missing`);
    attachMessagePortActorHost(port, ({ payload }) => new Int32Array(control(...payload.args)));
    port.postMessage({ type: "ready" });
  } catch (error) {
    port.postMessage({ type: "init-error", error: String(error) });
  }
});
