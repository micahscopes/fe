const BUNDLES = Object.freeze({
  d1: "./gen",
  legacy: "./gen",
  schedule32: "./gen-schedule32",
});

export function selectArtifactBundle(query) {
  const name = query.get("bundle") || "schedule32";
  const base = BUNDLES[name];
  if (!base) {
    throw new Error(
      `unknown artifact bundle '${name}' (expected d1, legacy, or schedule32)`,
    );
  }
  return Object.freeze({
    name,
    asset: (file) => `${base}/${file}`,
  });
}
