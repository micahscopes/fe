// main.js — orchestration for the Fe web sandbox (slice S1).
//
// S1 is the highlighted editor + the shipped run stage over a COMMITTED prebuilt
// kernel bundle. It is NOT the in-browser compiler (that is S3, the 20-site wasm
// port). So: the editor highlights live via real tree-sitter, and the Run button
// executes the committed gen/ artifacts (kernel.wasm on V8, kernel.wgsl on WebGPU)
// through the SHIPPED kernel-blind runners — byte-identical to demos/webgpu-*. The
// pipeline (compile → gen/) that produced those artifacts ran natively; editing the
// source here does not recompile (S3 lights that up).
//
// The runners are reused verbatim from ../webgpu-keystone/. The only run-side code
// local to this page is a tiny kernel-blind wasm-grid oracle loop (the shipped
// runWasmGrid hardcodes a page-relative ./gen URL, which the sandbox's two-kernel +
// inline-asset design can't feed; the loop reads the export name from layout.json,
// so it stays kernel-blind).

import { runWasm } from "../webgpu-keystone/wasm-runner.js";
import { runWebGPU, runWebGPUGrid } from "../webgpu-keystone/webgpu-runner.js";

const $ = (id) => document.getElementById(id);

const KERNELS = {
  poseidon: {
    id: "poseidon",
    label: "poseidon_sigma_u32 — scalar keystone",
    dir: "../webgpu-keystone/gen",
  },
  mandelbrot: {
    id: "mandelbrot",
    label: "mandelbrot_q12 — 512×512 grid image",
    dir: "../webgpu-mandelbrot/gen",
    width: 512,
    height: 512,
  },
};

let current = null; // { kernel, feSrc, wgslSrc, layout, reference, wasmBytes }

// ---------------------------------------------------------------------------
// Asset loading: inline (window.FE_ASSETS, netns-safe) or fetch (static hosting).
// ---------------------------------------------------------------------------
function b64ToUint8(b64) {
  const bin = atob(b64);
  const arr = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) arr[i] = bin.charCodeAt(i);
  return arr;
}

async function loadBundle(k) {
  const A = window.FE_ASSETS;
  if (A && A.kernels && A.kernels[k.id]) {
    const b = A.kernels[k.id];
    return {
      kernel: k,
      feSrc: b.fe,
      wgslSrc: b.wgsl,
      layout: JSON.parse(b.layout),
      reference: JSON.parse(b.reference),
      wasmBytes: b64ToUint8(b.wasm_b64),
      assetSource: "inline (window.FE_ASSETS)",
    };
  }
  const dir = k.dir;
  const [feSrc, wgslSrc, layout, reference, wasmBuf] = await Promise.all([
    fetch(`${dir}/kernel.fe`).then((r) => r.text()),
    fetch(`${dir}/kernel.wgsl`).then((r) => r.text()),
    fetch(`${dir}/layout.json`).then((r) => r.json()),
    fetch(`${dir}/reference.json`).then((r) => r.json()),
    fetch(`${dir}/kernel.wasm`).then((r) => r.arrayBuffer()),
  ]);
  return {
    kernel: k,
    feSrc,
    wgslSrc,
    layout,
    reference,
    wasmBytes: new Uint8Array(wasmBuf),
    assetSource: `fetch (${dir})`,
  };
}

// ---------------------------------------------------------------------------
// UI helpers
// ---------------------------------------------------------------------------
function setBanner(state, headline, detail) {
  const b = $("banner");
  b.className = `banner ${state}`;
  $("banner-state").textContent = state.toUpperCase();
  $("banner-headline").textContent = headline;
  $("banner-detail").textContent = detail || "";
}
function short(rev) {
  if (!rev || rev === "unknown") return rev || "unknown";
  return rev.length > 10 ? rev.slice(0, 10) : rev;
}
function fnv1a32(grid) {
  let h = 0x811c9dc5;
  for (let i = 0; i < grid.length; i++) {
    const v = grid[i] >>> 0;
    for (let s = 0; s < 4; s++) {
      h ^= (v >>> (s * 8)) & 0xff;
      h = Math.imul(h, 0x01000193);
    }
  }
  return h >>> 0;
}

// Kernel-blind wasm-grid oracle: instantiate the bundle's wasm bytes and call the
// layout-named export per pixel (row-major). Mirrors runWasmGrid but takes bytes.
async function runWasmGridBytes(bytes, exportName, width, height) {
  const { instance } = await WebAssembly.instantiate(bytes, {});
  const fn = instance.exports[exportName];
  if (typeof fn !== "function") throw new Error(`wasm export \`${exportName}\` not found`);
  const grid = new Uint32Array(width * height);
  for (let y = 0; y < height; y++) {
    for (let x = 0; x < width; x++) grid[y * width + x] = fn(x, y) >>> 0;
  }
  return grid;
}

function paintGrid(ctx, grid, width, height) {
  const img = ctx.createImageData(width, height);
  for (let i = 0; i < width * height; i++) {
    const iter = grid[i];
    const o = i * 4;
    if (iter >= 100) {
      img.data[o] = img.data[o + 1] = img.data[o + 2] = 0;
    } else {
      const v = Math.round(255 * Math.sqrt(iter / 100));
      img.data[o] = v;
      img.data[o + 1] = v;
      img.data[o + 2] = 255 - v;
    }
    img.data[o + 3] = 255;
  }
  ctx.putImageData(img, 0, 0);
}

function isNoGpu(gpu) {
  if (gpu.ok || gpu.messages) return false;
  return /navigator\.gpu is undefined|requestAdapter\(\) (returned null|threw)|no WebGPU adapter/.test(
    gpu.reason || ""
  );
}

// ---------------------------------------------------------------------------
// Load + present a kernel bundle (no run yet).
// ---------------------------------------------------------------------------
async function selectKernel(id) {
  const k = KERNELS[id];
  setBanner("amber", "Loading prebuilt bundle…", `${k.label}`);
  $("scalar-panel").hidden = true;
  $("grid-panel").hidden = true;
  try {
    current = await loadBundle(k);
  } catch (e) {
    setBanner("red", "Bundle missing", `${e.message}. Run: cargo run -p fe-codegen --example gen_${id === "poseidon" ? "webgpu" : "mandelbrot"}_demo`);
    return;
  }

  const { feSrc, wgslSrc, layout, reference } = current;

  $("editor").value = feSrc;
  current.committedFe = feSrc;

  // Artifacts.
  $("art-wgsl").textContent = wgslSrc;
  $("art-layout").textContent = JSON.stringify(layout, null, 2);
  $("art-wasm").textContent =
    `${current.wasmBytes.length} bytes (Fe → wasm, zero imports)\n` +
    `export: ${layout.wasm_export}\n\n` +
    hexDump(current.wasmBytes, 160);

  // Provenance.
  const p = layout.provenance || {};
  $("prov").textContent =
    `Fe @ ${short(p.fe_rev)} (branch mb2)  |  sonatina @ ${short(p.sonatina_rev)}  |  ` +
    `word ${layout.word}  |  workgroup [${layout.workgroup_size.join(", ")}]  |  ` +
    `mode ${layout.mode}  |  entry ${layout.entry_point}  |  assets: ${current.assetSource}`;

  setBanner(
    "amber",
    "Bundle loaded — press Run",
    `${k.label}. This runs the COMMITTED prebuilt artifacts (compiled natively); editing the Fe here does not recompile yet — the in-browser compiler is slice S3.`
  );
  updateEditNote();
}

function updateEditNote() {
  const note = $("edit-note");
  if (!current) { note.textContent = ""; return; }
  const edited = $("editor").value !== current.committedFe;
  note.textContent = edited
    ? "source edited — Run still executes the committed prebuilt kernel (in-browser compile is S3)"
    : "showing the committed kernel.fe (the exact source the prebuilt artifacts were compiled from)";
  note.className = edited ? "edit-note edited" : "edit-note";
}

function hexDump(bytes, max) {
  const n = Math.min(bytes.length, max);
  let out = "";
  for (let i = 0; i < n; i += 16) {
    const row = [];
    for (let j = i; j < Math.min(i + 16, n); j++) row.push(bytes[j].toString(16).padStart(2, "0"));
    out += row.join(" ") + "\n";
  }
  if (bytes.length > max) out += `… (${bytes.length - max} more bytes)\n`;
  return out;
}

// ---------------------------------------------------------------------------
// Run the loaded bundle (mode-branched, honest ladder).
// ---------------------------------------------------------------------------
async function run() {
  if (!current) return;
  const { layout } = current;
  if (layout.mode === "Grid") return runGrid();
  return runScalar();
}

async function runScalar() {
  const { wgslSrc, layout, reference, wasmBytes } = current;
  $("scalar-panel").hidden = false;
  $("grid-panel").hidden = true;

  const pinned = reference.pinned;
  setRow("row-ref", `${reference.value}`, reference.value === pinned ? "ok" : "bad");
  $("pin-note").textContent = `pinned = ${pinned} (revm / wasmtime / lavapipe, cross-backend)`;
  setBanner("amber", "Running…", "executing Fe→wasm in V8 and dispatching Fe→WGSL on WebGPU");

  // wasm leg — reuse the shipped runWasm via a blob URL of the bundle's bytes.
  const wasmUrl = URL.createObjectURL(new Blob([wasmBytes], { type: "application/wasm" }));
  let wasmValue = null, wasmErr = null;
  try {
    wasmValue = await runWasm(wasmUrl, layout);
    setRow("row-wasm", `${wasmValue}`, wasmValue === pinned ? "ok" : "bad");
  } catch (e) {
    wasmErr = e.message || String(e);
    setRow("row-wasm", `error: ${wasmErr}`, "bad");
  } finally {
    URL.revokeObjectURL(wasmUrl);
  }

  const gpu = await runWebGPU(wgslSrc, layout);
  if (gpu.ok) {
    setRow("row-gpu", `${gpu.value}`, gpu.value === pinned ? "ok" : "bad");
    $("adapter").textContent = `adapter: ${gpu.adapter}`;
  } else {
    const msg = gpu.messages ? `${gpu.reason}\n${gpu.messages.join("\n")}` : gpu.reason;
    setRow("row-gpu", msg, "bad");
    $("adapter").textContent = gpu.adapter ? `adapter: ${gpu.adapter}` : "adapter: none";
  }

  const refOk = reference.value === pinned;
  const wasmOk = wasmErr === null && wasmValue === pinned;
  if (!refOk) return setBanner("red", "Reference is off-pin", `reference ${reference.value} != pin ${pinned}.`);
  if (!wasmOk) return setBanner("red", "wasm leg failed", wasmErr ? `wasm: ${wasmErr}` : `wasm ${wasmValue} != pin ${pinned}`);

  if (gpu.ok && gpu.value === wasmValue && wasmValue === pinned) {
    return setBanner("green", "R-chrome earned",
      `Fe → wasm (V8) and Fe → WGSL (WebGPU on ${gpu.adapter}) both = ${pinned} = the revm/wasmtime/lavapipe pin.`);
  }
  if (!gpu.ok) {
    return setBanner("amber", "wasm leg matches the pin (no live GPU)",
      `Fe → wasm in this browser = ${wasmValue} = the cross-backend pin. No live WebGPU device (${gpu.reason}), so the GPU claim is honestly withheld. A WebGPU browser earns GREEN.`);
  }
  return setBanner("red", "GPU mismatch", `GPU ${gpu.value} != wasm/pin ${pinned} on ${gpu.adapter}.`);
}

async function runGrid() {
  const { wgslSrc, layout, reference, wasmBytes, kernel } = current;
  const W = kernel.width, H = kernel.height;
  $("scalar-panel").hidden = true;
  $("grid-panel").hidden = false;
  const ctx = $("grid-canvas").getContext("2d");

  const refHash = reference.fnv1a32 >>> 0;
  setRow("grow-ref", `${refHash} (0x${refHash.toString(16).padStart(8, "0")})`, "ok");
  setBanner("amber", "Running…", "executing the Fe grid on wasm (V8) and dispatching it on WebGPU");

  let wasmGrid = null, wasmErr = null;
  try {
    wasmGrid = await runWasmGridBytes(wasmBytes, layout.wasm_export, W, H);
  } catch (e) {
    wasmErr = e.message || String(e);
  }
  if (wasmErr || !wasmGrid) { setRow("grow-wasm", `error: ${wasmErr}`, "bad"); return setBanner("red", "wasm leg failed", `wasm: ${wasmErr}`); }

  const wasmHash = fnv1a32(wasmGrid);
  setRow("grow-wasm", `${W}×${H} grid, FNV-1a-32 ${wasmHash}`, wasmHash === refHash ? "ok" : "bad");
  if (wasmHash !== refHash) {
    paintGrid(ctx, wasmGrid, W, H);
    return setBanner("red", "Reference is off-hash", `wasm FNV ${wasmHash} != reference ${refHash}.`);
  }

  const gpu = await runWebGPUGrid(wgslSrc, layout, { width: W, height: H });
  if (gpu.ok) { $("adapter").textContent = `adapter: ${gpu.adapter}`; }
  else {
    const msg = gpu.messages ? `${gpu.reason}\n${gpu.messages.join("\n")}` : gpu.reason;
    setRow("grow-gpu", msg, "bad");
    $("adapter").textContent = gpu.adapter ? `adapter: ${gpu.adapter}` : "adapter: none";
  }

  if (isNoGpu(gpu)) {
    paintGrid(ctx, wasmGrid, W, H);
    setRow("grow-gpu", gpu.reason, "bad");
    return setBanner("amber", "Fe computed every escape count; wasm ran it here; JS colored it",
      `Fe → wasm computed all ${W * H} escape counts in this browser (FNV = reference) and JS colored them into the fractal. No live WebGPU device (${gpu.reason}); a WebGPU browser earns GREEN.`);
  }
  if (!gpu.ok) {
    paintGrid(ctx, wasmGrid, W, H);
    return setBanner("red", "GPU leg failed", gpu.messages ? `${gpu.reason}\n${gpu.messages.join("\n")}` : gpu.reason);
  }

  const gpuGrid = gpu.grid, gpuHash = fnv1a32(gpuGrid);
  setRow("grow-gpu", `${W}×${H} grid, FNV-1a-32 ${gpuHash}`, gpuHash === refHash ? "ok" : "bad");
  for (let i = 0; i < gpuGrid.length; i++) {
    if (gpuGrid[i] !== wasmGrid[i]) {
      paintGrid(ctx, gpuGrid, W, H);
      return setBanner("red", "GPU / wasm per-pixel mismatch",
        `(${i % W}, ${Math.floor(i / W)}): gpu=${gpuGrid[i] >>> 0} wasm=${wasmGrid[i] >>> 0} on ${gpu.adapter}.`);
    }
  }
  paintGrid(ctx, gpuGrid, W, H);
  setBanner("green", "The Fe compiler compiled this mandelbrot; your GPU computed every escape count",
    `All ${W * H} escape counts agree per pixel; GPU grid FNV = ${gpuHash} = reference on ${gpu.adapter}.`);
}

function setRow(id, value, cls) {
  const el = $(id);
  el.textContent = value;
  el.className = `val ${cls || ""}`;
}

// ---------------------------------------------------------------------------
// Tabs + wiring
// ---------------------------------------------------------------------------
function wireTabs() {
  const tabs = document.querySelectorAll(".tab");
  tabs.forEach((t) => {
    t.addEventListener("click", () => {
      tabs.forEach((x) => x.classList.remove("active"));
      document.querySelectorAll(".art-pane").forEach((p) => (p.hidden = true));
      t.classList.add("active");
      $(`art-${t.dataset.art}-pane`).hidden = false;
    });
  });
}

function boot() {
  wireTabs();
  const sel = $("kernel-select");
  Object.values(KERNELS).forEach((k) => {
    const o = document.createElement("option");
    o.value = k.id;
    o.textContent = k.label;
    sel.appendChild(o);
  });
  sel.addEventListener("change", () => selectKernel(sel.value));
  $("run-btn").addEventListener("click", run);
  $("editor").addEventListener("fe-change", updateEditNote);

  selectKernel("poseidon");
}

if (document.readyState === "loading") {
  document.addEventListener("DOMContentLoaded", boot);
} else {
  boot();
}
