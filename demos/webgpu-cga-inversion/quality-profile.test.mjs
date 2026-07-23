import assert from "node:assert/strict";
import {
  TEASER_RESOLUTION,
  qualityStatus,
  resolveQualityProfile,
} from "./quality-profile.js";

assert.deepEqual(resolveQualityProfile(null), {
  profile: "full",
  fixedResolution: null,
  kernelStepsChanged: false,
});
assert.deepEqual(resolveQualityProfile(""), resolveQualityProfile("full"));
assert.deepEqual(resolveQualityProfile("teaser"), {
  profile: "teaser",
  fixedResolution: TEASER_RESOLUTION,
  kernelStepsChanged: false,
});
assert.equal(resolveQualityProfile("full", 512).fixedResolution, 512);
assert.equal(resolveQualityProfile("teaser", 256).fixedResolution, 256);
assert.throws(
  () => resolveQualityProfile("teaser", 512),
  /quality=teaser requires resolution=256/,
);
assert.throws(() => resolveQualityProfile("fast"), /quality must be teaser or full/);
assert.deepEqual(qualityStatus(resolveQualityProfile("teaser"), 256, 256), {
  profile: "teaser",
  fixedResolution: 256,
  actualResolution: { width: 256, height: 256 },
  kernelStepsChanged: false,
});

console.log("CGA quality profiles: ok");
