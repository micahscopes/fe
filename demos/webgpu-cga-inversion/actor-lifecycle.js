import { createActorCoordinator } from "./gen-schedule32/actor/runtime/actor-coordinator.js";

const MODES = new Set(["manual", "continuous", "off"]);

export function createCgaActorLifecycle({ mode, ...handlers }) {
  if (!MODES.has(mode)) throw new TypeError(`invalid CGA verification mode: ${mode}`);
  const coordinator = createActorCoordinator(handlers);
  return Object.freeze({
    mode,
    generation: coordinator.generation,
    state: coordinator.state,
    advance() { return coordinator.nextGeneration(); },
    begin(renderPayload, verificationPayload) {
      const generation = coordinator.nextGeneration();
      const render = coordinator.enqueueRender(renderPayload, generation);
      const verification = mode === "off"
        ? null
        : coordinator.enqueueVerification(verificationPayload, generation);
      return { generation, render, verification };
    },
    interact(renderPayload) {
      const generation = coordinator.nextGeneration();
      return { generation, render: coordinator.enqueueRender(renderPayload, generation) };
    },
    enqueueRender(payload) {
      return coordinator.enqueueRender(payload, coordinator.generation());
    },
    enqueueVerification(payload, atGeneration = coordinator.generation()) {
      if (mode === "off") return null;
      if (atGeneration !== coordinator.generation()) return null;
      return coordinator.enqueueVerification(payload, atGeneration);
    },
    shouldVerifyAfterInteraction: mode === "continuous",
  });
}
