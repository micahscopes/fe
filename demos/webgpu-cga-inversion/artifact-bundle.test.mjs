import assert from "node:assert/strict";
import { selectArtifactBundle } from "./artifact-bundle.js";

const defaultBundle = selectArtifactBundle(new URLSearchParams());
assert.equal(defaultBundle.name, "schedule32");
assert.equal(defaultBundle.asset("frag.wgsl"), "./gen-schedule32/frag.wgsl");

const legacy = selectArtifactBundle(new URLSearchParams("bundle=legacy"));
assert.equal(legacy.name, "legacy");
assert.equal(legacy.asset("frag.wgsl"), "./gen/frag.wgsl");

const d1 = selectArtifactBundle(new URLSearchParams("bundle=d1"));
assert.equal(d1.name, "d1");
assert.equal(d1.asset("frag.wgsl"), "./gen/frag.wgsl");

const schedule32 = selectArtifactBundle(
  new URLSearchParams("bundle=schedule32&verify=off"),
);
assert.equal(schedule32.name, "schedule32");
assert.equal(schedule32.asset("frag.wasm"), "./gen-schedule32/frag.wasm");

assert.throws(
  () => selectArtifactBundle(new URLSearchParams("bundle=../../tmp")),
  /unknown artifact bundle/,
);
