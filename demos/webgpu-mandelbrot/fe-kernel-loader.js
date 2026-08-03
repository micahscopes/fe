import {
  FE_ARTIFACT_SCRIPT_TYPE,
  FE_SCRIPT_TYPE,
  createFeScriptLoader,
} from "../fe-sandbox/fe-script-loader.js";
import { FeCompilerAdapter } from "../fe-sandbox/compiler-adapter.example.js";
import { bootFeArtifacts } from "./gen/fe-bootstrap.js";

export const MANDELBROT_SOURCE_PATH = "../capstones/mandelbrot/kernel.fe";

export function rewriteFeArtifactForDevelopment(element) {
  if (element.type !== FE_ARTIFACT_SCRIPT_TYPE) {
    throw new TypeError("expected an application/fe+wasm artifact block");
  }
  const source = element.getAttribute("data-fe-source");
  if (!source) throw new Error("development Fe compilation requires data-fe-source");
  element.setAttribute("type", FE_SCRIPT_TYPE);
  element.setAttribute("data-fe-src", source);
  element.type = FE_SCRIPT_TYPE;
  return element;
}

export async function loadMandelbrotFeKernel({
  document: documentImpl = globalThis.document,
  location: locationImpl = globalThis.location,
  Worker: WorkerImpl = globalThis.Worker,
  loaderOptions = {},
} = {}) {
  const element = documentImpl.querySelector(
    `script[data-fe-entry="mandel_pixel_q12"]`,
  );
  if (!element) throw new Error("canonical Mandelbrot Fe artifact block was not found");
  const sourcePath = element.getAttribute("data-fe-source");
  if (sourcePath !== MANDELBROT_SOURCE_PATH) {
    throw new Error(`Mandelbrot data block must reference ${MANDELBROT_SOURCE_PATH}`);
  }
  const sourceUrl = new URL(sourcePath, documentImpl.baseURI).href;
  const developmentWorker =
    new URL(locationImpl.href).searchParams.get("fe-compile") === "worker";

  let compiler;
  let mode;
  let result;
  if (developmentWorker) {
    rewriteFeArtifactForDevelopment(element);
    const workerPath = element.getAttribute("data-fe-dev-worker");
    if (!workerPath) throw new Error("development Worker compilation is not configured");
    if (typeof WorkerImpl !== "function") {
      throw new Error("development Worker compilation requires Worker");
    }
    compiler = new FeCompilerAdapter(
      new WorkerImpl(new URL(workerPath, documentImpl.baseURI), { type: "module" }),
    );
    mode = "development-worker";
    const loader = createFeScriptLoader({ compiler, ...loaderOptions });
    result = await loader.run(element);
  } else {
    mode = "production-precompiled";
    if (element.type !== FE_ARTIFACT_SCRIPT_TYPE) {
      throw new Error("production requires the generic application/fe+wasm artifact block");
    }
    // The generic bootstrap owns fetch, digest verification, import preflight,
    // instantiation, state, and fe:load/fe:error dispatch. Its run is
    // idempotent, including when the bootstrap's document autostart raced us.
    if (element.dataset.feState === "complete") {
      result = element.feResult;
    } else {
      const loaded = new Promise((resolve, reject) => {
        element.addEventListener("fe:load", event => resolve(event.detail), { once: true });
        element.addEventListener("fe:error", event => reject(event.detail), { once: true });
      });
      await bootFeArtifacts(documentImpl);
      result = await loaded;
    }
    if (!result) throw new Error("generic Fe bootstrap produced no artifact result");
  }

  return Object.freeze({ ...result, mode, sourcePath, sourceUrl });
}
