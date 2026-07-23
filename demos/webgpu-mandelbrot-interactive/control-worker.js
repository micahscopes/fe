import { instantiateWasm } from "../webgpu-keystone/wasm-runner.js";
import { attachMessagePortActorHost } from "../shared/message-port-actor.js";
import { createActorAdapter } from "./gen/ctl-interface.js";

self.addEventListener("message", async (event) => {
  if (event.data?.type !== "init") return;
  const { port, wasm } = event.data;
  try {
    const exports = await instantiateWasm(wasm);
    const adapter = createActorAdapter(exports);
    attachMessagePortActorHost(port, adapter.dispatch, {
      transferResult: adapter.transferResult,
    });
    port.postMessage({ type: "ready" });
  } catch (error) {
    port.postMessage({ type: "init-error", error: String(error) });
  }
});
