import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import {
  canonicalInterfaceManifest,
  compiledCanonicalInterface,
  compileActorAdapter,
  createInterfaceCaller,
} from "./gen-schedule32/actor/interface.js";
import { createExactLaneRouter } from "./gen-schedule32/actor/runtime/actor-router.js";
import { selectCanonicalMainThreadGpuSchemas } from "./gen-schedule32/actor/runtime/gpu-actor.js";

assert.equal(canonicalInterfaceManifest.version, 2);
assert.deepEqual(Object.keys(compiledCanonicalInterface.lanes), [
  "render", "verify", "oracle",
]);
assert.deepEqual(
  Object.fromEntries(Object.entries(compiledCanonicalInterface.lanes).map(
    ([name, lane]) => [name, [lane.intent.execution, lane.intent.placement]],
  )),
  {
    render: ["host_effect", "main_thread"],
    verify: ["host_effect", "main_thread"],
    oracle: ["wasm", "any"],
  },
);

const schemas = compileActorAdapter();
assert.deepEqual(
  selectCanonicalMainThreadGpuSchemas(schemas).lanes,
  ["render", "verify"],
);
const frame = {
  generation: 7,
  cam_x: 0,
  cam_y: 0,
  zoom: Math.fround(0.0125),
  inv_cx: Math.fround(0.5),
  inv_cy: 0,
};
schemas.requestSchema.render(frame);
schemas.requestSchema.verify(frame);
schemas.requestSchema.oracle(frame);
assert.throws(
  () => schemas.requestSchema.render({ values: new Float32Array(5) }),
  /FE_ACTOR_INVALID_PAYLOAD/,
  "the flagship must not accept its former handwritten Float32Array schema",
);

const router = createExactLaneRouter(compiledCanonicalInterface.lanes, {
  gpu_main_thread: { lanes: ["render", "verify"], dispatch: () => null },
  wasm: { lanes: ["oracle"], dispatch: () => null },
});
assert.equal(router.ownerOf("render"), "gpu_main_thread");
assert.equal(router.ownerOf("oracle"), "wasm");

const actorBytes = await readFile(new URL(
  "./gen-schedule32/actor/module.wasm",
  import.meta.url,
));
const reference = JSON.parse(await readFile(
  new URL("./gen-schedule32/reference.json", import.meta.url),
  "utf8",
));
const { instance: actor } = await WebAssembly.instantiate(actorBytes);
const caller = createInterfaceCaller(actor.exports);
const canonical = await caller.call("oracle", frame);
assert.equal(canonical.byteLength, 128 * 128 * 4);
let hash = 0x811c9dc5;
for (const byte of canonical) {
  hash = Math.imul((hash ^ byte) >>> 0, 0x01000193) >>> 0;
}
assert.equal(hash >>> 0, reference.fnv1a32 >>> 0);

console.log("Schedule32 generated actor interface and one-call Wasm frame: ok");
