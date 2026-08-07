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

/**
 * `data-fe-render` handoff: a render bundle's manifest/wasm/wgsl fetching,
 * pipeline setup, lifecycle, and per-pixel wasm fallback all live in the ONE
 * shipped `fe-render-runtime.js` module (the same module the legacy `fe web
 * build --mode render` bundle's emitted index.html imports), not here. This
 * function locates that published module (importing it defines the
 * `<fe-surface>` custom element as a side effect) and inserts a
 * `<fe-surface manifest=...>` element after the script, letting the element
 * mount and drive its own lifecycle rather than calling `mountRenderSurface`
 * imperatively (FE_WEB_V5_ORCHESTRATION_DESIGN.md 3.3). `fe:load` resolves
 * once the element reaches `ready` (manifest fetched, poster rendered); it no
 * longer waits for the surface to go live, since v5 tiles are poster-first by
 * design.
 */
async function runRenderSurface(element, manifestUrl) {
  const runtimeReference = element.dataset.feRenderRuntime;
  if (!runtimeReference) {
    throw new Error("data-fe-render requires data-fe-render-runtime");
  }
  const runtimeUrl = new URL(runtimeReference, element.baseURI);
  await import(runtimeUrl); // defines `<fe-surface>`; no other export is needed here.

  const surface = document.createElement("fe-surface");
  surface.setAttribute("manifest", manifestUrl.href);
  if (element.dataset.feWidth) surface.setAttribute("width", element.dataset.feWidth);
  if (element.dataset.feHeight) surface.setAttribute("height", element.dataset.feHeight);

  const canvasSelector = element.dataset.feCanvas;
  const adopted = canvasSelector ? document.querySelector(canvasSelector) : null;
  if (adopted) {
    // `data-fe-canvas` adoption keeps working: the element accepts an
    // adopted canvas instead of generating its own poster/live pair.
    surface.adoptCanvas(adopted);
    adopted.insertAdjacentElement("afterend", surface);
  } else {
    // With no `data-fe-canvas`, mount immediately after the script element
    // (page authors keep layout control via HTML when they DO adopt a
    // canvas by selector).
    element.insertAdjacentElement("afterend", surface);
  }

  await new Promise((resolve, reject) => {
    surface.addEventListener("fe-ready", () => resolve(), { once: true });
    surface.addEventListener("fe-error", (event) => reject(event.detail), { once: true });
  });
  return { manifest: surface.manifest, surface, instance: null, module: null, value: undefined };
}

async function run(element) {
  if (element.dataset.feState === "complete") return element.feResult;
  if (element.dataset.feState === "running") return element.fePromise;
  const promise = (async () => {
    element.dataset.feState = "running";
    try {
      const manifestUrl = new URL(element.dataset.feManifest, element.baseURI);
      if (element.dataset.feRender !== undefined) {
        const result = await runRenderSurface(element, manifestUrl);
        element.feResult = result;
        element.dataset.feState = "complete";
        element.dispatchEvent(new CustomEvent("fe:load", { detail: result }));
        return result;
      }
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
