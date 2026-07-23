import assert from "node:assert/strict";
import { selectArtifactBundle } from "./artifact-bundle.js";

const legacy = selectArtifactBundle(new URLSearchParams());
assert.equal(legacy.name, "legacy");
assert.equal(legacy.asset("frag.wgsl"), "./gen/frag.wgsl");

const schedule32 = selectArtifactBundle(
  new URLSearchParams("bundle=schedule32&verify=off"),
);
assert.equal(schedule32.name, "schedule32");
assert.equal(schedule32.asset("frag.wasm"), "./gen-schedule32/frag.wasm");

assert.throws(
  () => selectArtifactBundle(new URLSearchParams("bundle=../../tmp")),
  /unknown artifact bundle/,
);
