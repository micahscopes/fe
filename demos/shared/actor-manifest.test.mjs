import assert from "node:assert/strict";
import { compileActorManifest } from "./actor-manifest.js";

const manifest = {
  protocol: "fe-demo-actor",
  version: 2,
  lanes: {
    render: {
      request: { kind: "record", fields: { args: { kind: "i32-array", length: 8 } } },
      result: { kind: "i32-array", length: 3 },
    },
  },
};
const schemas = compileActorManifest(manifest);
schemas.request.render({ args: new Int32Array(8) });
schemas.result.render({ ok: true, value: new Int32Array(3) });
assert.throws(() => schemas.request.render({ args: new Int32Array(7) }), /Int32Array\(8\)/);
assert.throws(() => schemas.result.render({ ok: true, value: new Int32Array(4) }), /Int32Array\(3\)/);
assert.throws(() => compileActorManifest({ ...manifest, version: 1 }), /actor version/);
assert.throws(() => compileActorManifest({ ...manifest, extra: true }), /unexpected/);

console.log("Fe actor manifest compiler: ok");
