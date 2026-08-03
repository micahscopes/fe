import {
  FE_COMPILER_PROTOCOL,
  assertCompatibleProtocol,
} from "./compiler-protocol.js";

// Scheduling shell for the wasm-bindgen compiler module. It is dependency
// injected so its lifecycle can be tested without substituting compilation in
// the real Worker build.
export function createCompilerWorkerRuntime({
  compileJson,
  compilerProtocolMajor,
  compilerProtocolMinor,
  postMessage,
}) {
  if (typeof compileJson !== "function") {
    throw new TypeError("compiler Worker requires compileJson");
  }
  const compilerProtocol = {
    major: compilerProtocolMajor(),
    minor: compilerProtocolMinor(),
  };
  assertCompatibleProtocol(compilerProtocol);
  const cancelled = new Set();

  function sendResult(id, result) {
    const transfers = [];
    for (const artifact of result.artifacts || []) {
      const bytes = artifact.bytes instanceof Uint8Array
        ? artifact.bytes
        : new Uint8Array(artifact.bytes);
      artifact.bytes = bytes;
      transfers.push(bytes.buffer);
    }
    postMessage({ type: "result", id, result }, transfers);
  }

  async function receive({ data }) {
    if (data?.type === "cancel") {
      cancelled.add(data.id);
      return;
    }
    if (data?.type !== "compile") return;
    const { id, request } = data;
    try {
      assertCompatibleProtocol(request?.protocol);
      // Yield once so a cancellation already queued for this request can win
      // before entering the currently synchronous compiler.
      await Promise.resolve();
      if (cancelled.delete(id)) return;
      const result = JSON.parse(compileJson(JSON.stringify(request)));
      assertCompatibleProtocol(result.protocol);
      if (!cancelled.delete(id)) sendResult(id, result);
    } catch (error) {
      if (!cancelled.delete(id)) {
        postMessage({
          type: "error",
          id,
          error: error?.message || String(error),
        });
      }
    }
  }

  postMessage({
    type: "ready",
    protocol: FE_COMPILER_PROTOCOL,
    compilerProtocol,
  });
  return { receive };
}

