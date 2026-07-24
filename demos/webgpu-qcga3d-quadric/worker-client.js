import {
  createCanonicalBrowserActor,
} from "./gen/runtime/actor-client.js";

export async function createQcgaActor({
  wasm,
  width,
  height,
  params,
  gpuRender,
  gpuVerify,
}) {
  const actor = await createCanonicalBrowserActor({
    wasm,
    handlers: {
      render: (request) =>
        gpuRender(params.map(({ name }) => request[name]), request),
      verify: (request) =>
        gpuVerify(params.map(({ name }) => request[name]), request),
    },
  });
  const request = (lane, payload, options) =>
    actor.request(lane, payload, payload.generation, options);
  return {
    render: (payload, options) => request("render", payload, options),
    gpu: (payload, options) => request("verify", payload, options),
    wasm: (payload, options) => request("oracle", payload, options),
    restart: actor.restart,
    close: actor.close,
    epoch: actor.epoch,
    pendingCount: actor.pendingCount,
    status: actor.status,
  };
}
