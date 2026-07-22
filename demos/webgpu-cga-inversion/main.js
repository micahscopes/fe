import { initWebGPURender, renderFrame, verifyView } from "../webgpu-keystone/webgpu-runner.js";
import { instantiateWasm, renderFragmentGrid } from "../webgpu-keystone/wasm-runner.js";
import { DEFAULT_CAMERA, createTrailingCoalescer, normalizeCamera, panCamera, zoomCamera } from "./camera-controls.js";

const $ = (id) => document.getElementById(id);
const acceptanceMode = new URLSearchParams(window.location.search).get("acceptance");
const presentation = acceptanceMode === null || acceptanceMode === ""
  ? "canvas"
  : acceptanceMode;

function banner(kind, detail) {
  $("banner").className = `banner ${kind}`;
  document.documentElement.dataset.status = kind;
  $("banner").dataset.status = kind;
  $("state").textContent = kind.toUpperCase();
  $("detail").textContent = detail;
}

async function fetchOk(url, kind) {
  const response = await fetch(url);
  if (!response.ok) throw new Error(`fetch ${url} -> HTTP ${response.status}`);
  if (kind === "json") return response.json();
  if (kind === "text") return response.text();
  return response.arrayBuffer();
}

function fnv1a32(bytes) {
  let hash = 0x811c9dc5;
  for (const byte of bytes) hash = Math.imul((hash ^ byte) >>> 0, 0x01000193) >>> 0;
  return hash >>> 0;
}

function cameraValues(camera) {
  return Object.freeze([Math.fround(camera.x), Math.fround(camera.y), Math.fround(camera.zoom)]);
}

function showCamera(camera) {
  $("camera-values").textContent =
    `cam_x=${camera.x.toFixed(5)}  cam_y=${camera.y.toFixed(5)}  zoom=${camera.zoom.toFixed(6)}`;
}

function validateTypedLayout(layout) {
  if (layout.mode !== "Render") throw new Error(`expected Render layout, got ${layout.mode}`);
  if (layout.width !== 128 || layout.height !== 128) throw new Error("D1 layout must be exactly 128x128");
  if (layout.entry_point !== "fs_main"
      || layout.vertex_entry !== "vs_fullscreen" || layout.fragment_entry !== "fs_main") {
    throw new Error("unexpected Render entry points");
  }
  if (layout.color_target_format !== "rgba8unorm") throw new Error("D1 requires rgba8unorm");
  const inputs = layout.bindings.filter((binding) => binding.role === "Input");
  const input = inputs[0];
  if (!input) throw new Error("Render layout has no Input binding");
  if (inputs.length !== 1) throw new Error("D1 requires exactly one Input binding");
  if (input.group !== 0 || input.binding !== 1 || input.access !== "Read") {
    throw new Error("D1 Input must be read-only group 0 binding 1");
  }
  if (input.span !== 12 || input.stride !== 12) throw new Error("D1 Input span/stride must both be 12");
  const paramTuples = (layout.params || []).map((p) => [p.arg_index, p.offset, p.width, p.scalar]);
  const expectedParams = [[2, 0, 4, "F32"], [3, 4, 4, "F32"], [4, 8, 4, "F32"]];
  if (JSON.stringify(paramTuples) !== JSON.stringify(expectedParams)) throw new Error("unexpected ordered D1 params");
  if (JSON.stringify(layout.params.map((p) => p.name)) !== JSON.stringify(["cam_x", "cam_y", "zoom"])) {
    throw new Error("unexpected ordered D1 param names");
  }
  const builtinTuples = (layout.builtin_inputs || []).map((b) => [b.arg_index, b.scalar, b.source]);
  const expectedBuiltins = [[0, "I32", "FragmentPositionX"], [1, "I32", "FragmentPositionY"]];
  if (JSON.stringify(builtinTuples) !== JSON.stringify(expectedBuiltins)) throw new Error("unexpected ordered D1 builtin inputs");
  return input;
}

function validateReference(reference) {
  if (reference.width !== 128 || reference.height !== 128) {
    throw new Error("D1 reference must be exactly 128x128");
  }
  const counts = [
    "sky_pixels",
    "hit_pixels",
    "material_a_pixels",
    "material_b_pixels",
  ];
  for (const field of counts) {
    if (!Number.isInteger(reference[field]) || reference[field] < 0) {
      throw new Error(`D1 reference ${field} must be a non-negative integer`);
    }
  }
  if (reference.material_a_pixels === 0 || reference.material_b_pixels === 0) {
    throw new Error("D1 reference must contain positive pixel counts for both materials");
  }
  if (reference.material_a_pixels + reference.material_b_pixels !== reference.hit_pixels) {
    throw new Error("D1 reference material pixel counts must sum to hit_pixels");
  }
  if (reference.sky_pixels + reference.hit_pixels !== reference.width * reference.height) {
    throw new Error("D1 reference sky_pixels + hit_pixels must cover the full frame");
  }
}

async function main() {
  if (presentation !== "canvas" && presentation !== "offscreen") {
    banner("red", `invalid acceptance presentation: ${presentation}`);
    return { state: "red", presentation, reason: "acceptance must be canvas or offscreen" };
  }
  let layout, reference, source, wgsl, wasm;
  try {
    [layout, reference, source, wgsl, wasm] = await Promise.all([
      fetchOk("./gen/layout.json", "json"),
      fetchOk("./gen/reference.json", "json"),
      fetchOk("./gen/kernel.fe", "text"),
      fetchOk("./gen/frag.wgsl", "text"),
      fetchOk("./gen/frag.wasm", "bytes"),
    ]);
    validateTypedLayout(layout);
    validateReference(reference);
  } catch (error) {
    banner("red", `artifact contract failed: ${error.message || error}`);
    return { state: "red", presentation, reason: String(error) };
  }

  $("source").textContent = source;
  $("wgsl").textContent = wgsl;
  $("meta").textContent = `Fe ${layout.provenance?.fe_rev || "unknown"} | Sonatina ${layout.provenance?.sonatina_rev || "unknown"}`;

  validateTypedLayout(layout);
  const params = layout.params;
  const renderLayout = { ...layout, params };

  let fragExports;
  try {
    fragExports = await instantiateWasm(wasm);
  } catch (error) {
    banner("red", `browser wasm oracle failed: ${error.message || error}`);
    return { state: "red", presentation, reason: String(error) };
  }

  const defaultValues = cameraValues(normalizeCamera(DEFAULT_CAMERA));
  let defaultWasmHash;
  try {
    const defaultWords = renderFragmentGrid(
      fragExports, layout.frag_wasm_export, defaultValues, reference.width, reference.height,
    );
    const defaultWasmRgba = new Uint8Array(
      defaultWords.buffer, defaultWords.byteOffset, defaultWords.byteLength,
    );
    defaultWasmHash = fnv1a32(defaultWasmRgba);
    if (defaultWasmHash !== (reference.fnv1a32 >>> 0)) {
      banner("red", `browser Wasm/reference mismatch: ${defaultWasmHash} != ${reference.fnv1a32 >>> 0}`);
      return { state: "red", presentation, wasmHash: defaultWasmHash,
        reason: "default browser-Wasm frame differs from packaged reference" };
    }
  } catch (error) {
    banner("red", `browser Wasm oracle failed: ${error.message || error}`);
    return { state: "red", presentation, reason: String(error) };
  }

  const gpu = await initWebGPURender(
    wgsl,
    renderLayout,
    presentation === "offscreen" ? null : $("view"),
  );
  if (!gpu.ok) {
    banner("amber", `browser Wasm matches the packaged frame; no live WebGPU render: ${gpu.reason}`);
    return { state: "amber", presentation, wasmHash: defaultWasmHash, reason: gpu.reason };
  }

  let latestGeneration = 0;
  const verifyCamera = async (camera, generation, requireReference = false) => {
    await new Promise((resolve) => setTimeout(resolve, 0));
    const values = cameraValues(camera);
    const words = renderFragmentGrid(
      fragExports, layout.frag_wasm_export, values, reference.width, reference.height,
    );
    const wasmRgba = new Uint8Array(words.buffer, words.byteOffset, words.byteLength);
    const wasmHash = fnv1a32(wasmRgba);
    if (generation !== latestGeneration) return null;
    const readback = await verifyView(gpu, values);
    if (generation !== latestGeneration) return null;
    if (!readback.ok) {
      return { state: "red", presentation, generation, wasmHash, reason: readback.reason };
    }
    const gpuHash = fnv1a32(readback.rgba);
    const equal = readback.rgba.length === wasmRgba.length
      && !readback.rgba.some((byte, index) => byte !== wasmRgba[index]);
    if (!equal || gpuHash !== wasmHash) {
      return { state: "red", presentation, generation, wasmHash, gpuHash,
        reason: "GPU/browser-Wasm byte mismatch" };
    }
    if (requireReference && wasmHash !== (reference.fnv1a32 >>> 0)) {
      return { state: "red", presentation, generation, wasmHash, gpuHash,
        reason: `default view differs from reference ${reference.fnv1a32 >>> 0}` };
    }
    return { state: "green", presentation, generation, wasmHash, gpuHash, adapter: gpu.adapter,
      camera: values };
  };

  const finishVerification = (result) => {
    if (!result || result.generation !== latestGeneration) return;
    if (result.state === "green") {
      banner("green", `current camera: WebGPU readback exactly matches browser Wasm (FNV-1a ${result.gpuHash}) on ${gpu.adapter}`);
    } else {
      banner("red", result.reason);
    }
    publishAcceptance(result);
  };

  let camera = normalizeCamera(DEFAULT_CAMERA);
  showCamera(camera);
  const canvas = $("view");
  let queuedVerification = null;
  let verificationRunning = false;
  const drainVerifications = async () => {
    if (verificationRunning) return;
    verificationRunning = true;
    while (queuedVerification) {
      const job = queuedVerification;
      queuedVerification = null;
      try {
        finishVerification(await verifyCamera(job.camera, job.generation));
      } catch (error) {
        finishVerification({ state: "red", presentation, generation: job.generation,
          reason: String(error) });
      }
    }
    verificationRunning = false;
  };
  const coalescer = createTrailingCoalescer((job) => {
    queuedVerification = job;
    drainVerifications();
  });
  let drawPending = false;
  let drawCamera = camera;
  const requestDraw = (nextCamera) => {
    drawCamera = nextCamera;
    if (drawPending) return;
    drawPending = true;
    requestAnimationFrame(() => {
      drawPending = false;
      renderFrame(gpu, cameraValues(drawCamera));
    });
  };
  const settle = (nextCamera) => {
    camera = normalizeCamera(nextCamera);
    showCamera(camera);
    if (presentation === "canvas") requestDraw(camera);
    latestGeneration += 1;
    coalescer.submit({ camera, generation: latestGeneration });
    banner("amber", "camera changed; checking browser-Wasm against WebGPU readback...");
    publishAcceptance({ state: "pending", presentation, generation: latestGeneration,
      camera: cameraValues(camera) });
  };

  if (presentation === "canvas") {
    let drag = null;
    canvas.addEventListener("pointerdown", (event) => {
      if (!event.isPrimary || event.button !== 0) return;
      drag = { x: event.clientX, y: event.clientY };
      canvas.setPointerCapture(event.pointerId);
    });
    canvas.addEventListener("pointermove", (event) => {
      if (!drag) return;
      const scaleX = canvas.width / canvas.clientWidth;
      const scaleY = canvas.height / canvas.clientHeight;
      const next = panCamera(camera, (event.clientX - drag.x) * scaleX,
        (event.clientY - drag.y) * scaleY);
      drag = { x: event.clientX, y: event.clientY };
      settle(next);
    });
    const endDrag = (event) => {
      if (drag) canvas.releasePointerCapture?.(event.pointerId);
      drag = null;
    };
    canvas.addEventListener("pointerup", endDrag);
    canvas.addEventListener("pointercancel", endDrag);
    canvas.addEventListener("lostpointercapture", () => { drag = null; });
    canvas.addEventListener("wheel", (event) => {
      event.preventDefault();
      const rect = canvas.getBoundingClientRect();
      const px = (event.clientX - rect.left) * canvas.width / rect.width;
      const py = (event.clientY - rect.top) * canvas.height / rect.height;
      settle(zoomCamera(camera, event.deltaY, px, py, canvas.width, canvas.height));
    }, { passive: false });
    $("reset-camera").addEventListener("click", () => settle(DEFAULT_CAMERA));
    window.__cgaCamera = { get: () => ({ ...camera }), reset: () => settle(DEFAULT_CAMERA) };
  }

  const initialGeneration = ++latestGeneration;
  if (presentation === "canvas") renderFrame(gpu, cameraValues(camera));
  verificationRunning = true;
  const initial = await verifyCamera(camera, initialGeneration, true);
  verificationRunning = false;
  if (initial) finishVerification(initial);
  drainVerifications();
  return initial;
}

window.__cgaAcceptance = { state: "pending", presentation };
document.documentElement.dataset.status = "pending";
function publishAcceptance(result) {
  Object.assign(window.__cgaAcceptance, result);
  document.documentElement.dataset.status = result.state;
  const node = document.getElementById("acceptance-json");
  if (node) node.textContent = JSON.stringify(result);
  return result;
}
window.__cgaAcceptance.promise = main().then((result) => {
  return result ? publishAcceptance(result) : window.__cgaAcceptance;
}).catch((error) => {
  const result = { state: "red", presentation, reason: String(error) };
  banner("red", result.reason);
  return publishAcceptance(result);
});
