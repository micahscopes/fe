import { instantiateWasm, renderFragmentGrid } from "../webgpu-keystone/wasm-runner.js";
import { attachMessagePortActorHost } from "../shared/message-port-actor.js";
import { createGpuActorClient } from "../shared/gpu-actor.js";
self.addEventListener("message", async ({data}) => {
  if (data?.type !== "init") return;
  const {port,gpuPort,wasm,exportName,width,height,actorEpoch}=data;
  try {
    const exports=await instantiateWasm(wasm);
    const gpu=createGpuActorClient(gpuPort,{valueCount:0,rgbaBytes:width*height*4,initialEpoch:actorEpoch});
    attachMessagePortActorHost(port,({lane,generation}) => {
      if(lane==="render") return gpu.render([],generation);
      const words=renderFragmentGrid(exports,exportName,[],width,height);
      return new Uint8Array(words.buffer.slice(words.byteOffset,words.byteOffset+words.byteLength));
    });
    port.postMessage({type:"ready"});
  } catch(error){port.postMessage({type:"init-error",error:String(error)});}
});
