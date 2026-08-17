import { compileActorAdapter } from "../interface.js";
import {
  createCanonicalBrowserActor as createCanonicalBrowserActorCore,
  createCanonicalBrowserWorkerScope as createCanonicalBrowserWorkerScopeCore,
  createCanonicalWorkerMailboxImports,
} from "./actor-client-core.js";

export { createCanonicalWorkerMailboxImports };

// Default wrapper for a canonical actor package whose generated interface is
// published at `../interface.js`. Structured-child packages use the same fixed
// core with their own compiler-derived adapter and Worker wrapper.
export function createCanonicalBrowserActor(options = {}) {
  if (!options || typeof options !== "object" || Array.isArray(options)) {
    throw new TypeError("canonical browser actor options must be an object");
  }
  if (Object.hasOwn(options, "adapter")) {
    throw new TypeError("generated actor composition owns its canonical adapter");
  }
  return createCanonicalBrowserActorCore({
    ...options,
    adapter: compileActorAdapter(),
    workerUrl: options.workerUrl ?? new URL("./worker-host.js", import.meta.url),
  });
}

export function createCanonicalBrowserWorkerScope(options = {}) {
  if (!options || typeof options !== "object" || Array.isArray(options)) {
    throw new TypeError("canonical browser Worker scope options must be an object");
  }
  if (Object.hasOwn(options, "adapter")) {
    throw new TypeError("generated actor composition owns its canonical adapter");
  }
  return createCanonicalBrowserWorkerScopeCore({
    ...options,
    adapter: compileActorAdapter(),
    workerUrl: options.workerUrl ?? new URL("./worker-host.js", import.meta.url),
  });
}
