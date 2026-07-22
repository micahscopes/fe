import { actorEnvelope } from "../shared/actor-coordinator.js";
import { actorField, actorResultSchema, exactObject } from "../shared/actor-endpoint.js";
import { createModuleWorkerActor } from "../shared/module-worker-actor.js";
import { createMainThreadGpuBroker } from "../shared/gpu-actor.js";
export async function createQcgaActor({wasm,exportName,width,height,gpuRender,gpuVerify}){
  let requestId=0;
  const actor=await createModuleWorkerActor({workerUrl:new URL("./wasm-worker.js",import.meta.url),init:{wasm,exportName,width,height},
    requestSchema:{render:p=>exactObject(p,{}),verify:p=>exactObject(p,{})},
    resultSchema:{render:actorResultSchema(v=>exactObject(v,{submitted:actorField.boolean})),verify:actorResultSchema(actorField.uint8Array(width*height*4))},
    createAuxiliaryPorts(epoch){const c=new MessageChannel();const broker=createMainThreadGpuBroker(c.port1,{render:gpuRender,verify:gpuVerify,valueCount:0,rgbaBytes:width*height*4,initialEpoch:epoch});return{message:{gpuPort:c.port2},transfer:[c.port2],close:()=>broker.close()};}});
  const request=async(lane,generation=0)=>{const r=await actor.request(actorEnvelope({type:"request",lane,actorEpoch:actor.epoch(),generation,requestId:++requestId,payload:{}}));if(!r.payload.ok)throw new Error(r.payload.error);return r.payload.value;};
  return{render:g=>request("render",g),wasm:g=>request("verify",g),close:actor.close};
}
