import { createCanonicalModuleWorkerActor } from "../shared/module-worker-actor.js";
import { compileActorAdapter } from "./gen/ctl-interface.js";

export async function createMandelbrotWorkerControl({
  wasm, lane, argNames, resultOrder,
}) {
  const adapter = compileActorAdapter();
  const actor = await createCanonicalModuleWorkerActor({
    workerUrl: new URL("./control-worker.js", import.meta.url),
    init: { wasm },
    adapter,
    maxPending: 4,
  });
  return {
    async update(args, generation = 0) {
      if (!Array.isArray(argNames) || args.length !== argNames.length) {
        throw new TypeError("control arguments do not match generated control metadata");
      }
      const payload = Object.fromEntries(argNames.map((name, index) => [name, args[index]]));
      const result = await actor.request(lane, payload, generation);
      return resultOrder.map((name) => result[name]);
    },
    restart: actor.restart,
    close: actor.close,
    epoch: actor.epoch,
  };
}
