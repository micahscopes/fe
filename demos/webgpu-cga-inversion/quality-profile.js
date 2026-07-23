export const TEASER_RESOLUTION = 256;

export function resolveQualityProfile(value, explicitResolution = null) {
  const profile = value === null || value === "" ? "full" : value;
  if (profile !== "teaser" && profile !== "full") {
    throw new TypeError("quality must be teaser or full");
  }
  if (profile === "teaser") {
    if (explicitResolution !== null && explicitResolution !== TEASER_RESOLUTION) {
      throw new TypeError(
        `quality=teaser requires resolution=${TEASER_RESOLUTION}`,
      );
    }
    return Object.freeze({
      profile,
      fixedResolution: TEASER_RESOLUTION,
      kernelStepsChanged: false,
    });
  }
  return Object.freeze({
    profile,
    fixedResolution: explicitResolution,
    kernelStepsChanged: false,
  });
}

export function qualityStatus(profile, width, height) {
  return Object.freeze({
    profile: profile.profile,
    fixedResolution: profile.fixedResolution,
    actualResolution: Object.freeze({ width, height }),
    kernelStepsChanged: false,
  });
}
