import { actorEnvelope } from "../shared/actor-coordinator.js";
import { compileActorManifest } from "../shared/actor-manifest.js";
import { createModuleWorkerActor } from "../shared/module-worker-actor.js";

export async function createMandelbrotWorkerControl({ wasm, exportName, actorManifest }) {
  let requestId = 0;
  const schemas = compileActorManifest(actorManifest);
  const actor = await createModuleWorkerActor({
    workerUrl: new URL("./control-worker.js", import.meta.url),
    init: { wasm, exportName },
    requestSchema: schemas.request,
    resultSchema: schemas.result,
  });
  return {
    async update(args, generation = 0) {
      const result = await actor.request(actorEnvelope({ type: "request", lane: "render",
        actorEpoch: actor.epoch(), generation, requestId: ++requestId,
        payload: { args: new Int32Array(args) } }));
      if (!result.payload.ok) throw new Error(result.payload.error);
      return Array.from(result.payload.value);
    },
    async restart() { requestId = 0; return actor.restart(); },
    close: actor.close,
    epoch: actor.epoch,
  };
}
