// JavaScript half of fe-compiler-protocol v1.
//
// Keep this thin: Rust owns the canonical data model and golden wire fixtures.
// The Worker transport adds request correlation and transferable artifact bytes.

export const FE_COMPILER_PROTOCOL = Object.freeze({ major: 1, minor: 1 });

export function assertCompatibleProtocol(protocol) {
  if (!protocol || protocol.major !== FE_COMPILER_PROTOCOL.major) {
    throw new Error(
      `incompatible Fe compiler protocol: expected major ${FE_COMPILER_PROTOCOL.major}, ` +
      `received ${protocol?.major ?? "missing"}`,
    );
  }
}

export function compileRequest({
  source,
  sourceUrl,
  entries = [],
  target = "wasm",
  options = { optimization: "none", debug_info: false },
}) {
  return {
    protocol: FE_COMPILER_PROTOCOL,
    root: sourceUrl,
    // The compiler verifies source hashes when supplied. Hashing is asynchronous
    // in browsers, so the transport may omit it and let the compiler calculate it.
    sources: [{ url: sourceUrl, text: source }],
    target,
    entries,
    options,
  };
}

export function wasmArtifact(result) {
  assertCompatibleProtocol(result?.protocol);
  const artifact = result.artifacts?.find(({ kind }) => kind === "wasm_module");
  if (!artifact) throw new Error("Fe compiler returned no Wasm module artifact");
  return artifact;
}
