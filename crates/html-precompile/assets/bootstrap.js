const providers = globalThis.feImportProviders ??= [];

export function registerFeImportProvider(provider) {
  if (typeof provider !== "function" && (typeof provider !== "object" || provider === null)) {
    throw new TypeError("Fe import provider must be an object or function");
  }
  providers.push(provider);
}

globalThis.registerFeImportProvider ??= registerFeImportProvider;

function mergeImports(target, additions) {
  for (const [module, members] of Object.entries(additions || {})) {
    const namespace = target[module] ??= {};
    for (const [name, value] of Object.entries(members)) {
      if (Object.hasOwn(namespace, name) && namespace[name] !== value) {
        throw new Error(`duplicate Wasm import: ${module}.${name}`);
      }
      namespace[name] = value;
    }
  }
}

async function importsFor(context, additions) {
  const imports = {};
  mergeImports(imports, additions);
  for (const provider of providers) {
    mergeImports(
      imports,
      typeof provider === "function" ? await provider(context) : provider,
    );
  }
  return imports;
}

async function sha256Hex(bytes) {
  const digest = new Uint8Array(await crypto.subtle.digest("SHA-256", bytes));
  return Array.from(digest, byte => byte.toString(16).padStart(2, "0")).join("");
}

async function run(element) {
  if (element.dataset.feState === "complete") return element.feResult;
  if (element.dataset.feState === "running") return element.fePromise;
  const promise = (async () => {
    element.dataset.feState = "running";
    try {
      const manifestUrl = new URL(element.dataset.feManifest, element.baseURI);
      const wasmUrl = new URL(element.dataset.feSrc, element.baseURI);
      const [manifestResponse, wasmResponse] = await Promise.all([
        fetch(manifestUrl, { mode: "cors", credentials: "same-origin" }),
        fetch(wasmUrl, {
          mode: "cors",
          credentials: "same-origin",
          integrity: element.dataset.feIntegrity || "",
        }),
      ]);
      if (!manifestResponse.ok || !wasmResponse.ok) {
        throw new Error("could not load precompiled Fe manifest or Wasm artifact");
      }
      const manifest = await manifestResponse.json();
      const artifact = manifest.artifacts?.find(value => value.kind === "wasm_module");
      if (!artifact) throw new Error("Fe manifest contains no Wasm module");
      const bytes = new Uint8Array(await wasmResponse.arrayBuffer());
      if (bytes.byteLength !== artifact.byte_len ||
          await sha256Hex(bytes) !== artifact.sha256) {
        throw new Error("Fe Wasm artifact failed manifest integrity verification");
      }
      const module = await WebAssembly.compile(bytes);
      const context = { element, manifest, module };
      let selectedImports;
      if (element.dataset.feAdapter) {
        const environmentFactory = globalThis.feAdapterEnvironment;
        if (!environmentFactory) {
          throw new Error("selected Fe adapter requires globalThis.feAdapterEnvironment");
        }
        const environment = typeof environmentFactory === "function"
          ? await environmentFactory(context)
          : environmentFactory;
        const adapterModule = await import(new URL(element.dataset.feAdapter, element.baseURI));
        if (typeof adapterModule.createFeHostAdapter !== "function") {
          throw new Error("selected Fe adapter exports no createFeHostAdapter");
        }
        selectedImports =
          adapterModule.createFeHostAdapter(environment.host, environment.runtime).imports;
      }
      const imports = await importsFor(context, selectedImports);
      for (const required of WebAssembly.Module.imports(module)) {
        if (!Object.hasOwn(imports, required.module) ||
            !Object.hasOwn(imports[required.module], required.name)) {
          throw new Error(`missing Wasm import: ${required.module}.${required.name}`);
        }
      }
      const instance = await WebAssembly.instantiate(module, imports);
      const entryName = element.dataset.feEntry || manifest.entry || "main";
      const entry = instance.exports[entryName];
      if (typeof entry !== "function") {
        throw new Error(`Fe Wasm export \`${entryName}\` was not found`);
      }
      const value = element.dataset.feAutostart === "false" ? undefined : await entry();
      const result = { instance, module, manifest, value };
      element.feResult = result;
      element.dataset.feState = "complete";
      element.dispatchEvent(new CustomEvent("fe:load", { detail: result }));
      return result;
    } catch (error) {
      element.dataset.feState = "error";
      element.dispatchEvent(new CustomEvent("fe:error", { detail: error }));
      throw error;
    }
  })();
  element.fePromise = promise;
  try {
    return await promise;
  } finally {
    delete element.fePromise;
  }
}

export async function bootFeArtifacts(root = document) {
  const results = [];
  for (const element of root.querySelectorAll('script[type="application/fe+wasm"]')) {
    results.push(await run(element));
  }
  return results;
}

if (typeof document !== "undefined") {
  bootFeArtifacts().catch(error => {
    globalThis.dispatchEvent?.(new CustomEvent("fe:bootstrap-error", { detail: error }));
  });
}
