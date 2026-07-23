import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { createMandelbrotWorkerControl } from "./worker-control.js";

const wasm = new Uint8Array(await readFile(
  new URL("./gen/actor/module.wasm", import.meta.url),
));
const argNames = [
  "center_re", "center_im", "scale_q", "dx", "dy", "dzoom", "mx", "my",
];
const resultOrder = ["center_re", "center_im", "scale_q"];
const control = await createMandelbrotWorkerControl({
  wasm,
  lane: "update_view_message",
  argNames,
  resultOrder,
});

try {
  assert.equal(control.epoch(), 0);
  assert.deepEqual(
    await control.update([-2048, 0, 384, 0, 0, 0, 256, 256], 1),
    [-2048, 0, 384],
  );
  assert.deepEqual(
    await control.update([-2048, 0, 384, 16, -8, -1, 256, 256], 2),
    [-2432, 192, 336],
  );

  const interrupted = control.update([-2048, 0, 384, 1, 1, 0, 256, 256], 3);
  const restarted = control.restart();
  await assert.rejects(interrupted, /restarting module worker/);
  assert.equal(await restarted, 1);
  assert.equal(control.epoch(), 1);
  assert.deepEqual(
    await control.update([10240, 10240, 384, -1000, -1000, 1, 511, 511], 4),
    [10240, 10240, 384],
    "the restarted Worker must preserve Fe clamp semantics",
  );
} finally {
  control.close();
}

console.log("generated canonical Mandelbrot Worker/MessageChannel restart: ok");
