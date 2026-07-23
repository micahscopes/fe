import { actorEnvelope } from "../shared/actor-coordinator.js";
import { createModuleWorkerActor } from "../shared/module-worker-actor.js";
import { compileActorAdapter } from "./gen/ctl-interface.js";

export async function createMandelbrotWorkerControl({
  wasm, lane, argNames, resultOrder,
}) {
  let requestId = 0;
  const schemas = compileActorAdapter();
  const actor = await createModuleWorkerActor({
    workerUrl: new URL("./control-worker.js", import.meta.url),
    init: { wasm },
    requestSchema: schemas.requestSchema,
    resultSchema: schemas.resultSchema,
  });
  return {
    async update(args, generation = 0) {
      if (!Array.isArray(argNames) || args.length !== argNames.length) {
        throw new TypeError("control arguments do not match generated control metadata");
      }
      const payload = Object.fromEntries(argNames.map((name, index) => [name, args[index]]));
      const result = await actor.request(actorEnvelope({ type: "request", lane,
        actorEpoch: actor.epoch(), generation, requestId: ++requestId,
        payload }));
      if (!result.payload.ok) throw new Error(result.payload.error);
      return resultOrder.map((name) => result.payload.value[name]);
    },
    async restart() { requestId = 0; return actor.restart(); },
    close: actor.close,
    epoch: actor.epoch,
  };
}
