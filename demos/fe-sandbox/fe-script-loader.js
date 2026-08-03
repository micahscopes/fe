// Browser-side convention for inert source and precompiled Fe script elements.
//
// This module deliberately knows nothing about Fe's compiler internals or about
// particular Web APIs. A compiler and generated Web IDL import providers are
// ordinary injected modules.

export const FE_SCRIPT_TYPE = "application/fe";
export const FE_ARTIFACT_SCRIPT_TYPE = "application/fe+wasm";

import { assertCompatibleProtocol } from "./compiler-protocol.js";

function asBytes(value) {
  if (value instanceof Uint8Array) return value;
  if (value instanceof ArrayBuffer) return new Uint8Array(value);
  if (ArrayBuffer.isView(value)) {
    return new Uint8Array(value.buffer, value.byteOffset, value.byteLength);
  }
  return value;
}

function mergeImportNamespaces(target, additions) {
  for (const [namespace, members] of Object.entries(additions || {})) {
    const destination = (target[namespace] ||= {});
    for (const [name, value] of Object.entries(members)) {
      if (Object.hasOwn(destination, name) && destination[name] !== value) {
        throw new Error(`duplicate Wasm import: ${namespace}.${name}`);
      }
      destination[name] = value;
    }
  }
}

async function resolveImports(providers, context) {
  const imports = {};
  for (const provider of providers) {
    const supplied =
      typeof provider === "function" ? await provider(context) : await provider;
    mergeImportNamespaces(imports, supplied);
  }
  return imports;
}

function throwIfAborted(signal) {
  if (!signal?.aborted) return;
  if (typeof signal.throwIfAborted === "function") signal.throwIfAborted();
  throw signal.reason || new DOMException("The operation was aborted", "AbortError");
}

function elementBaseUrl(element, configuredBaseUrl) {
  const value = configuredBaseUrl || element.baseURI || element.ownerDocument?.baseURI;
  if (!value) throw new Error("Fe script loader requires an absolute document base URL");
  return new URL(value).href;
}

function fetchPolicy(element, signal, integrity) {
  const crossOrigin = element.getAttribute("crossorigin");
  const credentials = crossOrigin === "use-credentials"
    ? "include"
    : crossOrigin !== null
      ? "omit"
      : "same-origin";
  const referrerPolicy = element.getAttribute("referrerpolicy") || "";
  return {
    signal,
    mode: "cors",
    credentials,
    ...(referrerPolicy ? { referrerPolicy } : {}),
    ...(integrity ? { integrity } : {}),
  };
}

async function readSource(element, fetchImpl, baseUrl, signal) {
  throwIfAborted(signal);
  const src = element.getAttribute("data-fe-src");
  if (!src) {
    return { source: element.textContent || "", sourceUrl: baseUrl };
  }
  const sourceUrl = new URL(src, baseUrl).href;
  const integrity =
    element.getAttribute("integrity") || element.getAttribute("data-fe-integrity");
  const response = await fetchImpl(
    sourceUrl,
    fetchPolicy(element, signal, integrity),
  );
  if (!response.ok) {
    throw new Error(`could not load Fe source ${sourceUrl}: HTTP ${response.status}`);
  }
  return { source: await response.text(), sourceUrl };
}

async function fetchOk(fetchImpl, element, url, kind, signal, integrity) {
  throwIfAborted(signal);
  const response = await fetchImpl(url, fetchPolicy(element, signal, integrity));
  if (!response.ok) {
    throw new Error(`could not load Fe ${kind} ${url}: HTTP ${response.status}`);
  }
  return response;
}

export async function verifyArtifactDigest(bytes, expected, cryptoImpl = globalThis.crypto) {
  if (!expected) throw new Error("Fe artifact manifest has no SHA-256 digest");
  if (!cryptoImpl?.subtle) throw new Error("Web Crypto is required to verify Fe artifacts");
  const digest = new Uint8Array(await cryptoImpl.subtle.digest("SHA-256", bytes));
  const actual = Array.from(digest, (byte) => byte.toString(16).padStart(2, "0")).join("");
  if (actual !== expected) {
    throw new Error(`Fe artifact SHA-256 mismatch: expected ${expected}, received ${actual}`);
  }
}

async function readPrecompiled(element, fetchImpl, baseUrl, verifyDigest, signal) {
  const manifestAttribute = element.getAttribute("data-fe-manifest");
  if (!manifestAttribute) {
    throw new Error("precompiled Fe script requires data-fe-manifest");
  }
  const src = element.getAttribute("data-fe-src");
  if (!src) throw new Error("precompiled Fe script requires data-fe-src");
  const manifestUrl = new URL(manifestAttribute, baseUrl).href;
  const wasmUrl = new URL(src, baseUrl).href;
  const wasmIntegrity = element.getAttribute("data-fe-integrity");
  const manifestIntegrity = element.getAttribute("data-fe-manifest-integrity");
  const [manifestResponse, wasmResponse] = await Promise.all([
    fetchOk(
      fetchImpl,
      element,
      manifestUrl,
      "manifest",
      signal,
      manifestIntegrity,
    ),
    fetchOk(fetchImpl, element, wasmUrl, "Wasm", signal, wasmIntegrity),
  ]);
  const manifest = await manifestResponse.json();
  throwIfAborted(signal);
  assertCompatibleProtocol(manifest.protocol);
  const wasmArtifact = manifest.artifacts?.find(({ kind }) => kind === "wasm_module");
  if (!wasmArtifact) throw new Error("Fe manifest contains no Wasm module");
  const wasm = new Uint8Array(await wasmResponse.arrayBuffer());
  if (wasm.byteLength !== wasmArtifact.byte_len) {
    throw new Error(
      `Fe artifact byte length mismatch: expected ${wasmArtifact.byte_len}, received ${wasm.byteLength}`,
    );
  }
  await verifyDigest(wasm, wasmArtifact.sha256);
  if (wasmIntegrity) {
    const expected = `sha256-${hexToBase64(wasmArtifact.sha256)}`;
    if (wasmIntegrity !== expected) {
      throw new Error(
        `Fe artifact integrity does not match manifest: expected ${expected}, received ${wasmIntegrity}`,
      );
    }
  }
  throwIfAborted(signal);
  return {
    wasm,
    entry: manifest.entry,
    manifest: manifest.interface,
    publishedManifest: manifest,
  };
}

function hexToBase64(hex) {
  if (!/^[0-9a-f]{64}$/.test(hex)) {
    throw new Error("Fe artifact manifest SHA-256 is not canonical lowercase hex");
  }
  const bytes = Uint8Array.from(
    hex.match(/../g),
    (pair) => Number.parseInt(pair, 16),
  );
  if (typeof Buffer !== "undefined") return Buffer.from(bytes).toString("base64");
  let binary = "";
  for (const byte of bytes) binary += String.fromCharCode(byte);
  return btoa(binary);
}

async function waitForDom(root, signal) {
  if (root?.nodeType !== 9 || root.readyState !== "loading") return;
  throwIfAborted(signal);
  await new Promise((resolve, reject) => {
    const ready = () => {
      cleanup();
      resolve();
    };
    const aborted = () => {
      cleanup();
      reject(signal.reason || new DOMException("The operation was aborted", "AbortError"));
    };
    const cleanup = () => {
      root.removeEventListener("DOMContentLoaded", ready);
      signal?.removeEventListener("abort", aborted);
    };
    root.addEventListener("DOMContentLoaded", ready, { once: true });
    signal?.addEventListener("abort", aborted, { once: true });
  });
}

function preflightImports(module, imports, moduleImports) {
  for (const required of moduleImports(module)) {
    if (!Object.hasOwn(imports, required.module) ||
        !Object.hasOwn(imports[required.module], required.name)) {
      throw new Error(`missing Wasm import: ${required.module}.${required.name}`);
    }
  }
}

/**
 * Create an Fe script runner.
 *
 * Compiler contract:
 *   await compiler.compile({ source, sourceUrl, attributes, signal })
 *     -> Uint8Array | WebAssembly.Module | {
 *          module | wasm, imports?, entry?, diagnostics?
 *        }
 *
 * `imports` and `importProviders` use the native WebAssembly import-object
 * shape. A generated Web IDL adapter is therefore just another provider.
 */
export function createFeScriptLoader({
  compiler,
  importProviders = [],
  fetch: fetchImpl = globalThis.fetch,
  compile = WebAssembly.compile,
  instantiate = WebAssembly.instantiate,
  moduleImports = WebAssembly.Module.imports,
  verifyDigest = verifyArtifactDigest,
  baseUrl = globalThis.document?.baseURI,
  workerExecutor,
} = {}) {
  if (compiler && typeof compiler.compile !== "function") {
    throw new TypeError("Fe script compiler must implement compile(request)");
  }
  if (typeof fetchImpl !== "function") {
    throw new TypeError("Fe script loader requires fetch");
  }

  async function run(element, { signal } = {}) {
    if (![FE_SCRIPT_TYPE, FE_ARTIFACT_SCRIPT_TYPE].includes(element.type)) {
      throw new TypeError("expected an Fe source or artifact script element");
    }
    if (element.dataset.feState === "running") return element.fePromise;
    if (element.dataset.feState === "complete") return element.feResult;

    const running = runOnce(element, signal);
    element.fePromise = running;
    try {
      return await running;
    } finally {
      delete element.fePromise;
    }
  }

  async function runOnce(element, signal) {
    element.dataset.feState = "running";
    const attributes = Object.fromEntries(
      Array.from(element.attributes || [], ({ name, value }) => [name, value]),
    );

    try {
      throwIfAborted(signal);
      const resolvedBaseUrl = elementBaseUrl(element, baseUrl);
      let request;
      let artifact;
      if (element.type === FE_ARTIFACT_SCRIPT_TYPE) {
        artifact = await readPrecompiled(
          element,
          fetchImpl,
          resolvedBaseUrl,
          verifyDigest,
          signal,
        );
      } else {
        if (!compiler) {
          throw new Error("Fe source script requires a compiler");
        }
        const input = await readSource(element, fetchImpl, resolvedBaseUrl, signal);
        request = { ...input, attributes, signal };
        const output = await compiler.compile(request);
        throwIfAborted(signal);
        artifact =
          output instanceof WebAssembly.Module || output instanceof Uint8Array ||
          output instanceof ArrayBuffer || ArrayBuffer.isView(output)
            ? { wasm: output }
            : output;
      }
      if (!artifact || (!artifact.module && !artifact.wasm)) {
        throw new TypeError("Fe compiler returned no `module` or `wasm` artifact");
      }

      const entryName = element.dataset.feEntry || artifact.entry || "main";
      const autostart = element.dataset.feAutostart !== "false";
      const placement = element.dataset.feExecution || "main";
      if (placement === "worker") {
        if (!workerExecutor || typeof workerExecutor.run !== "function") {
          throw new Error(
            "Fe Worker execution requires a workerExecutor implementing run(request)",
          );
        }
        const result = await workerExecutor.run({
          element,
          request,
          artifact,
          attributes,
          entry: entryName,
          autostart,
          signal,
        });
        throwIfAborted(signal);
        element.feResult = result;
        element.dataset.feState = "complete";
        element.dispatchEvent?.(new CustomEvent("fe:load", { detail: result }));
        return result;
      }
      if (placement !== "main") {
        throw new Error(`unsupported Fe execution placement: ${placement}`);
      }

      const context = { element, request, artifact };
      const imports = await resolveImports(
        [...importProviders, ...(artifact.imports ? [artifact.imports] : [])],
        context,
      );
      throwIfAborted(signal);
      const module = artifact.module || await compile(asBytes(artifact.wasm));
      throwIfAborted(signal);
      preflightImports(module, imports, moduleImports);
      const instantiated = await instantiate(
        module,
        imports,
      );
      const instance = instantiated.instance || instantiated;
      const entry = instance.exports[entryName];
      if (typeof entry !== "function") {
        throw new Error(`Fe Wasm export \`${entryName}\` was not found`);
      }
      // Data-block consumers such as compute kernels need the instantiated
      // exports but supply arguments later. This is not script-src behavior;
      // it is an explicit loader policy on an inert Fe data block.
      const value = autostart ? await entry() : undefined;
      const result = { instance, module: instantiated.module, value, artifact };
      element.feResult = result;
      element.dataset.feState = "complete";
      element.dispatchEvent?.(new CustomEvent("fe:load", { detail: result }));
      return result;
    } catch (error) {
      const cancelled = signal?.aborted;
      element.dataset.feState = cancelled ? "cancelled" : "error";
      element.dispatchEvent?.(
        new CustomEvent(cancelled ? "fe:cancel" : "fe:error", { detail: error }),
      );
      throw error;
    }
  }

  async function boot(root = document, options) {
    await waitForDom(root, options?.signal);
    const elements = Array.from(
      root.querySelectorAll(
        `script[type="${FE_SCRIPT_TYPE}"],script[type="${FE_ARTIFACT_SCRIPT_TYPE}"]`,
      ),
    );
    // Script execution order is observable, matching classic inline scripts.
    const results = [];
    for (const element of elements) results.push(await run(element, options));
    return results;
  }

  return { boot, run };
}
