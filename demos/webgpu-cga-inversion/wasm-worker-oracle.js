import {
  createCanonicalBrowserActor,
} from "./gen-schedule32/actor/runtime/actor-client.js";

const requestPayload = (values, generation) => ({
  generation,
  cam_x: values[0],
  cam_y: values[1],
  zoom: values[2],
  inv_cx: values[3],
  inv_cy: values[4],
});

export async function createCgaWasmWorkerOracle({
  wasm,
  gpuRender,
  gpuVerify,
}) {
  const actor = await createCanonicalBrowserActor({
    wasm,
    handlers: {
      render: (payload, request) => gpuRender(payload, request),
      verify: (payload, request) => gpuVerify(payload, request),
    },
    maxPending: 8,
  });
  const request = async (lane, values, generation = 0, options) => {
    return actor.request(
      lane,
      requestPayload(values, generation),
      generation,
      options,
    );
  };
  return {
    render: (values, generation = 0, options) =>
      request("oracle", values, generation, options),
    renderGpu: (values, generation = 0, options) =>
      request("render", values, generation, options),
    restart: actor.restart,
    close: actor.close,
    epoch: actor.epoch,
    pendingCount: actor.pendingCount,
  };
}
