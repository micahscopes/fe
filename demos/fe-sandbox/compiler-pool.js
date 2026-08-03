import {
  FeCompilerAdapter,
  WorkerCrashError,
} from "./compiler-adapter.example.js";
import {
  FE_COMPILER_PROTOCOL,
  compileRequest,
} from "./compiler-protocol.js";

async function sha256Hex(text, cryptoImpl = globalThis.crypto) {
  if (!cryptoImpl?.subtle) throw new Error("Web Crypto is required for compiler cache keys");
  const bytes = new TextEncoder().encode(text);
  const digest = new Uint8Array(await cryptoImpl.subtle.digest("SHA-256", bytes));
  return Array.from(digest, byte => byte.toString(16).padStart(2, "0")).join("");
}

function protocolRequest(args) {
  const attributes = args.attributes || {};
  return compileRequest({
    source: args.source,
    sourceUrl: args.sourceUrl,
    entries: args.entries || (attributes["data-fe-entry"]
      ? [attributes["data-fe-entry"]]
      : []),
    target: args.target || "wasm",
    options: args.options || { optimization: "none", debug_info: false },
  });
}

export async function canonicalCompileKey(args, digest = sha256Hex) {
  const request = protocolRequest(args);
  const sources = [];
  for (const source of request.sources) {
    sources.push({
      url: source.url,
      sha256: source.sha256 || await digest(source.text),
    });
  }
  return JSON.stringify({
    protocol: FE_COMPILER_PROTOCOL,
    root: request.root,
    sources,
    target: request.target,
    entries: request.entries,
    options: {
      optimization: request.options.optimization,
      debug_info: request.options.debug_info,
    },
  });
}

/**
 * Bounded protocol-only compiler pool.
 *
 * `createCompiler` returns an object implementing `compile(request)`. For real
 * browser Workers, pass `workerFactory` and the pool wraps each Worker with
 * `FeCompilerAdapter`. No DOM or Fe compiler semantics live here.
 */
export class FeCompilerPool {
  constructor({
    size = Math.max(1, Math.min(4, globalThis.navigator?.hardwareConcurrency || 1)),
    capacity = 32,
    workerFactory,
    createCompiler = workerFactory
      ? () => new FeCompilerAdapter(workerFactory())
      : undefined,
    digest = sha256Hex,
  } = {}) {
    if (!Number.isInteger(size) || size < 1) {
      throw new RangeError("compiler pool size must be a positive integer");
    }
    if (!Number.isInteger(capacity) || capacity < 0) {
      throw new RangeError("compiler cache capacity must be a non-negative integer");
    }
    if (typeof createCompiler !== "function") {
      throw new TypeError("compiler pool requires workerFactory or createCompiler");
    }
    this.capacity = capacity;
    this.createCompiler = createCompiler;
    this.digest = digest;
    this.cache = new Map();
    this.inflight = new Map();
    this.queue = [];
    this.slots = Array.from({ length: size }, () => ({
      compiler: undefined,
      busy: false,
    }));
  }

  async compile(args) {
    if (args.signal?.aborted) throw args.signal.reason;
    const key = await canonicalCompileKey(args, this.digest);
    if (args.signal?.aborted) throw args.signal.reason;
    if (this.cache.has(key)) {
      const result = this.cache.get(key);
      this.cache.delete(key);
      this.cache.set(key, result);
      return result;
    }

    let job = this.inflight.get(key);
    if (!job) {
      const { signal: _signal, ...sharedArgs } = args;
      job = { key, args: sharedArgs, subscribers: new Set(), started: false };
      this.inflight.set(key, job);
      this.queue.push(job);
    }
    const result = this.subscribe(job, args.signal);
    this.pump();
    return result;
  }

  subscribe(job, signal) {
    return new Promise((resolve, reject) => {
      const subscriber = { resolve, reject, signal, aborted: false };
      const abort = () => {
        if (subscriber.aborted) return;
        subscriber.aborted = true;
        job.subscribers.delete(subscriber);
        reject(signal.reason);
        if (!job.started && job.subscribers.size === 0) {
          this.inflight.delete(job.key);
          this.queue = this.queue.filter(value => value !== job);
        }
      };
      subscriber.abort = abort;
      job.subscribers.add(subscriber);
      signal?.addEventListener("abort", abort, { once: true });
    });
  }

  pump() {
    for (const slot of this.slots) {
      if (slot.busy) continue;
      const job = this.queue.shift();
      if (!job) return;
      if (job.subscribers.size === 0) {
        this.inflight.delete(job.key);
        continue;
      }
      slot.busy = true;
      job.started = true;
      try {
        slot.compiler ||= this.createCompiler();
      } catch (error) {
        this.inflight.delete(job.key);
        slot.busy = false;
        this.settle(job, "reject", error);
        continue;
      }
      this.execute(slot, job);
    }
  }

  async execute(slot, job) {
    try {
      const result = await slot.compiler.compile(job.args);
      if (this.capacity > 0) {
        this.cache.delete(job.key);
        this.cache.set(job.key, result);
        while (this.cache.size > this.capacity) {
          this.cache.delete(this.cache.keys().next().value);
        }
      }
      this.settle(job, "resolve", result);
    } catch (error) {
      if (error instanceof WorkerCrashError || error?.name === "WorkerCrashError") {
        slot.compiler = undefined;
      }
      this.settle(job, "reject", error);
    } finally {
      this.inflight.delete(job.key);
      slot.busy = false;
      this.pump();
    }
  }

  settle(job, method, value) {
    for (const subscriber of job.subscribers) {
      subscriber.signal?.removeEventListener("abort", subscriber.abort);
      if (!subscriber.aborted) subscriber[method](value);
    }
    job.subscribers.clear();
  }
}
