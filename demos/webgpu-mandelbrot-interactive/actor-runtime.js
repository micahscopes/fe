import { createActorCoordinator } from "./gen/actor/runtime/actor-coordinator.js";
import {
  createCanonicalMainThreadGpuChannel,
  selectCanonicalMainThreadGpuSchemas,
} from "./gen/actor/runtime/gpu-actor.js";
import { compileActorAdapter } from "./gen/actor/interface.js";

const compiledSchemas = compileActorAdapter();
const gpuSchemas = selectCanonicalMainThreadGpuSchemas(compiledSchemas);

// Compatibility-shaped export for tests and consumers that inspected the old
// demo-owned schemas. The validators themselves are now compiler-generated.
export const MANDELBROT_ACTOR_SCHEMAS = Object.freeze({
  request: gpuSchemas.requestSchema,
  result: gpuSchemas.resultSchema,
});

const viewRecord = (view) => {
  if (!Array.isArray(view) && !(view instanceof Int32Array)) {
    throw new TypeError("Mandelbrot view must be a three-word vector");
  }
  if (view.length !== 3) {
    throw new TypeError("Mandelbrot view must be a three-word vector");
  }
  return {
    center_re: view[0],
    center_im: view[1],
    scale_q: view[2],
  };
};

const viewArray = ({ center_re, center_im, scale_q }) =>
  [center_re, center_im, scale_q];

export function createMandelbrotActorRuntime({ render, verify, onError = () => {} }) {
  if (typeof render !== "function" || typeof verify !== "function") {
    throw new TypeError("Mandelbrot render and verify handlers are required");
  }
  let generation = 0;
  const waiters = new Map();
  const { broker, client: gpu } = createCanonicalMainThreadGpuChannel({
    adapter: compiledSchemas,
    handlers: {
      render: (request) => render(viewArray(request)),
      verify: async (request) => {
        const result = await verify(viewArray(request));
        return {
          gpu_hash: result.gpuHash,
          wasm_hash: result.wasmHash,
          reference_hash: result.referenceHash,
        };
      },
    },
  });

  const execute = async (request) => {
    const value = await gpu.request(
      request.lane,
      request.payload,
      request.generation,
    );
    if (request.lane === "verify") {
      return {
        gpuHash: value.gpu_hash,
        wasmHash: value.wasm_hash,
        referenceHash: value.reference_hash,
      };
    }
    return value;
  };
  const settle = (result) => {
    const waiter = waiters.get(result.requestId);
    if (!waiter) return;
    waiters.delete(result.requestId);
    if (result.payload.dropped) waiter.resolve({ dropped: true });
    else if (result.payload.ok) waiter.resolve(result.payload.value);
    else waiter.reject(new Error(result.payload.error));
  };
  const coordinator = createActorCoordinator({
    render: execute,
    verify: execute,
    onRenderSettled: settle,
    onVerificationSettled: settle,
    onCallbackError: onError,
  });

  const send = (lane, view, nextGeneration) => {
    const payload = viewRecord(view);
    if (nextGeneration) generation = coordinator.nextGeneration();
    const request = lane === "render"
      ? coordinator.enqueueRender(payload, generation)
      : coordinator.enqueueVerification(payload, generation);
    return new Promise((resolve, reject) => {
      waiters.set(request.requestId, { resolve, reject });
    });
  };

  return Object.freeze({
    render(view) {
      const work = send("render", view, true);
      work.catch(onError);
      return work;
    },
    verify: (view) => send("verify", view, false),
    close() {
      gpu.close();
      broker.close();
    },
    reset(reason = "Mandelbrot actor restarted") {
      const epoch = gpu.restart(reason);
      broker.restart(epoch);
      return epoch;
    },
    epoch: gpu.epoch,
    state: coordinator.state,
    gpuState: broker.state,
  });
}
