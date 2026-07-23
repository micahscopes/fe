import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import {
  canonicalInterfaceManifest,
  compiledCanonicalInterface,
  compileActorAdapter,
  createInterfaceCaller,
} from "./gen-schedule32/actor-interface.js";
import { createExactLaneRouter } from "../shared/actor-router.js";
import { selectCanonicalMainThreadGpuSchemas } from "../shared/gpu-actor.js";

assert.equal(canonicalInterfaceManifest.version, 2);
assert.deepEqual(Object.keys(compiledCanonicalInterface.lanes), [
  "render", "verify", "oracle", "oracle_pixel",
]);
assert.deepEqual(
  Object.fromEntries(Object.entries(compiledCanonicalInterface.lanes).map(
    ([name, lane]) => [name, [lane.intent.execution, lane.intent.placement]],
  )),
  {
    render: ["host_effect", "main_thread"],
    verify: ["host_effect", "main_thread"],
    oracle: ["host_effect", "worker"],
    oracle_pixel: ["wasm", "any"],
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
  worker_host: { lanes: ["oracle"], dispatch: () => null },
  wasm: { lanes: ["oracle_pixel"], dispatch: () => null },
});
assert.equal(router.ownerOf("render"), "gpu_main_thread");
assert.equal(router.ownerOf("oracle"), "worker_host");
assert.equal(router.ownerOf("oracle_pixel"), "wasm");

const actorBytes = await readFile(new URL(
  "./gen-schedule32/actor-canonical.wasm",
  import.meta.url,
));
const fragBytes = await readFile(new URL("./gen-schedule32/frag.wasm", import.meta.url));
const [{ instance: actor }, { instance: frag }] = await Promise.all([
  WebAssembly.instantiate(actorBytes),
  WebAssembly.instantiate(fragBytes),
]);
const caller = createInterfaceCaller(actor.exports);
const pixel = {
  x: 64,
  y: 64,
  cam_x: frame.cam_x,
  cam_y: frame.cam_y,
  zoom: frame.zoom,
  inv_cx: frame.inv_cx,
  inv_cy: frame.inv_cy,
};
const canonical = await caller.call("oracle_pixel", pixel);
const raw = frag.exports.cga_schedule32_vec5_de_render(
  pixel.x,
  pixel.y,
  pixel.cam_x,
  pixel.cam_y,
  pixel.zoom,
  pixel.inv_cx,
  pixel.inv_cy,
);
assert.equal(canonical.rgba >>> 0, raw >>> 0);

console.log("Schedule32 generated actor interface and Wasm oracle pixel: ok");
