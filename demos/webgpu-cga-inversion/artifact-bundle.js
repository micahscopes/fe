const BUNDLES = Object.freeze({
  legacy: "./gen",
  schedule32: "./gen-schedule32",
});

export function selectArtifactBundle(query) {
  const name = query.get("bundle") || "legacy";
  const base = BUNDLES[name];
  if (!base) {
    throw new Error(
      `unknown artifact bundle '${name}' (expected legacy or schedule32)`,
    );
  }
  return Object.freeze({
    name,
    asset: (file) => `${base}/${file}`,
  });
}
