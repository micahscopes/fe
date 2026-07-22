import assert from "node:assert/strict";
import { DEFAULT_CAMERA, createTrailingCoalescer, normalizeCamera, panCamera, zoomCamera } from "./camera-controls.js";

assert.deepEqual(panCamera(DEFAULT_CAMERA, 8, -4), normalizeCamera({ x: -0.1, y: 0.05, zoom: 0.0125 }));
const centered = zoomCamera(DEFAULT_CAMERA, -1, 64, 64, 128, 128);
assert.equal(centered.x, 0);
assert.equal(centered.y, 0);
assert.ok(centered.zoom < DEFAULT_CAMERA.zoom);
assert.deepEqual(zoomCamera(DEFAULT_CAMERA, 0, 96, 32, 128, 128), normalizeCamera(DEFAULT_CAMERA));
const anchored = zoomCamera(DEFAULT_CAMERA, -1, 96, 32, 128, 128);
assert.equal(
  Math.fround(DEFAULT_CAMERA.x + (96 - 64) * DEFAULT_CAMERA.zoom),
  Math.fround(anchored.x + (96 - 64) * anchored.zoom),
);
assert.equal(
  Math.fround(DEFAULT_CAMERA.y + (32 - 64) * DEFAULT_CAMERA.zoom),
  Math.fround(anchored.y + (32 - 64) * anchored.zoom),
);
assert.throws(() => normalizeCamera({ x: NaN, y: 0, zoom: 1 }), /finite/);
assert.equal(normalizeCamera({ x: 0, y: 0, zoom: 100 }).zoom, Math.fround(0.05));

let queued = [];
const seen = [];
const coalescer = createTrailingCoalescer(
  (value, generation) => seen.push([value, generation]),
  (fn) => { queued.push(fn); return fn; },
  (fn) => { queued = queued.filter((candidate) => candidate !== fn); },
  0,
);
coalescer.submit("old");
coalescer.submit("new");
assert.equal(queued.length, 1);
queued[0]();
assert.deepEqual(seen, [["new", 2]]);

console.log("CGA camera math and coalescing: ok");
