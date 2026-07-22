import { initWebGPURender, renderFrame, verifyView } from "../webgpu-keystone/webgpu-runner.js";
import { instantiateWasm, renderFragmentGrid } from "../webgpu-keystone/wasm-runner.js";

const CAMERA = new Map([[2, 0.0], [3, 0.0], [4, 0.0125]]);
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
  const values = params.map((param) => CAMERA.get(param.arg_index));
  const renderLayout = { ...layout, params };

  let wasmRgba;
  try {
    const exports = await instantiateWasm(wasm);
    const words = renderFragmentGrid(
      exports,
      layout.frag_wasm_export,
      values,
      reference.width,
      reference.height,
    );
    wasmRgba = new Uint8Array(words.buffer, words.byteOffset, words.byteLength);
  } catch (error) {
    banner("red", `browser wasm oracle failed: ${error.message || error}`);
    return { state: "red", presentation, reason: String(error) };
  }
  const wasmHash = fnv1a32(wasmRgba);
  if (wasmHash !== (reference.fnv1a32 >>> 0)) {
    banner("red", `browser wasm/reference mismatch: ${wasmHash} != ${reference.fnv1a32 >>> 0}`);
    return { state: "red", presentation, wasmHash };
  }

  const gpu = await initWebGPURender(
    wgsl,
    renderLayout,
    presentation === "offscreen" ? null : $("view"),
  );
  if (!gpu.ok) {
    banner("amber", `browser wasm matches the compiled full frame; no live WebGPU render: ${gpu.reason}`);
    return { state: "amber", presentation, wasmHash, reason: gpu.reason };
  }

  if (presentation === "canvas") renderFrame(gpu, values);
  const readback = await verifyView(gpu, values);
  if (!readback.ok) {
    banner("red", `GPU readback failed: ${readback.reason}`);
    return { state: "red", presentation, wasmHash, reason: readback.reason };
  }
  const hash = fnv1a32(readback.rgba);
  if (hash !== wasmHash || readback.rgba.length !== wasmRgba.length
      || readback.rgba.some((byte, index) => byte !== wasmRgba[index])) {
    banner("red", `GPU/browser-wasm mismatch: GPU ${hash}, wasm ${wasmHash}`);
    return { state: "red", presentation, wasmHash, gpuHash: hash };
  }
  banner("green", `live two-sphere WebGPU ${presentation} readback matches compiled 128x128 reference (FNV-1a ${hash}) on ${gpu.adapter}`);
  return { state: "green", presentation, wasmHash, gpuHash: hash, adapter: gpu.adapter };
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
  return publishAcceptance(result);
}).catch((error) => {
  const result = { state: "red", presentation, reason: String(error) };
  banner("red", result.reason);
  return publishAcceptance(result);
});
