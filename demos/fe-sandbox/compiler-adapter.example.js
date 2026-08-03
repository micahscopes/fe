// Contract example, not a bundled compiler.
//
// A future wasm32 Fe compiler worker can implement this exact interface. During
// development the body can instead POST to a local compiler service.
import {
  assertCompatibleProtocol,
  compileRequest,
  wasmArtifact,
} from "./compiler-protocol.js";

export class WorkerCrashError extends Error {
  constructor(message = "Fe compiler Worker crashed") {
    super(message);
    this.name = "WorkerCrashError";
  }
}

export class FeCompilerAdapter {
  constructor(worker) {
    this.worker = worker;
    this.nextId = 0;
    this.pending = new Map();
    this.ready = new Promise((resolve, reject) => {
      this.resolveReady = resolve;
      this.rejectReady = reject;
    });
    worker.addEventListener("message", ({ data }) => {
      if (data.type === "ready") {
        try {
          assertCompatibleProtocol(data.protocol);
          assertCompatibleProtocol(data.compilerProtocol);
          this.resolveReady();
        } catch (error) {
          this.rejectReady(error);
        }
        return;
      }
      const pending = this.pending.get(data.id);
      if (!pending) return;
      this.pending.delete(data.id);
      if (data.type === "error") pending.reject(new Error(data.error));
      else {
        const artifact = wasmArtifact(data.result);
        pending.resolve({
          wasm: new Uint8Array(artifact.bytes),
          entry: data.entry,
          diagnostics: data.result.diagnostics,
          manifest: data.result.interface,
          compilerResult: data.result,
        });
      }
    });
    const crashed = event => {
      const error = new WorkerCrashError(event?.message);
      this.rejectReady(error);
      for (const pending of this.pending.values()) pending.reject(error);
      this.pending.clear();
      worker.terminate?.();
    };
    worker.addEventListener("error", crashed);
    worker.addEventListener("messageerror", crashed);
  }

  async compile({
    source,
    sourceUrl,
    attributes = {},
    signal,
    entries,
    target = "wasm",
    options,
  }) {
    await this.ready;
    if (signal?.aborted) throw signal.reason;
    const id = ++this.nextId;
    return new Promise((resolve, reject) => {
      this.pending.set(id, { resolve, reject });
      signal?.addEventListener("abort", () => {
        this.pending.delete(id);
        this.worker.postMessage({ type: "cancel", id });
        reject(signal.reason);
      }, { once: true });
      const requestedEntries = entries || (attributes["data-fe-entry"]
        ? [attributes["data-fe-entry"]]
        : []);
      this.worker.postMessage({
        type: "compile",
        id,
        request: compileRequest({
          source,
          sourceUrl,
          entries: requestedEntries,
          target,
          ...(options ? { options } : {}),
        }),
      });
    });
  }
}
