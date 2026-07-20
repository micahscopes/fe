// main.js - orchestration for the Fe -> GPU IMAGE page (mandelbrot ladder M2).
//
// This is the ONLY kernel-aware JS on the page. It holds exactly three things:
//   1. the dispatch dims { width: 512, height: 512 };
//   2. the M2 escape-time palette (color the Fe-computed escape COUNTS; the
//      coloring is JS, Fe coloring is the M4 rung);
//   3. the verdict wiring (GPU grid vs wasm grid, per pixel, plus the hash).
//
// Everything shader-specific stays in the kernel-blind runners in
// ../webgpu-keystone/ (imported relatively). The runners never learn what this
// kernel computes; this file never learns how the pipeline is built.
//
//   GREEN  "your GPU computed every escape count": the live WebGPU grid equals
//          the in-browser wasm grid PER PIXEL, and the GPU grid's FNV-1a-32
//          equals the compiled reference. Adapter named.
//   AMBER  no navigator.gpu / no adapter: paint the wasm grid, GPU claim NOT made.
//   RED    a Tint shader error (verbatim) or ANY per-pixel mismatch (first shown
//          as "(x, y): gpu=<v> wasm=<v>").
//
// There is no code path that paints GREEN without a live GPU readback matching.

import { runWasmGrid } from "../webgpu-keystone/wasm-runner.js";
import { runWebGPUGrid } from "../webgpu-keystone/webgpu-runner.js";

// The one dispatch-dims choice (a runtime fact the page supplies; layout.json is
// dispatch-free). Both are multiples of the workgroup dims (exact tiling).
const WIDTH = 512;
const HEIGHT = 512;

const $ = (id) => document.getElementById(id);

async function fetchText(url) {
  const r = await fetch(url);
  if (!r.ok) throw new Error(`fetch ${url} -> HTTP ${r.status}`);
  return r.text();
}
async function fetchJson(url) {
  return JSON.parse(await fetchText(url));
}

function setRow(id, value, cls) {
  const el = $(id);
  el.textContent = value;
  el.className = `val ${cls || ""}`;
}

function setBanner(state, headline, detail) {
  const b = $("banner");
  b.className = `banner ${state}`;
  $("banner-state").textContent = state.toUpperCase();
  $("banner-headline").textContent = headline;
  $("banner-detail").textContent = detail || "";
}

// FNV-1a 32-bit over the grid's LE byte stream, folded over each u32's 4 LE
// bytes in pixel order. Bit-for-bit identical to the Rust generator (offset
// 0x811c9dc5, prime 0x01000193); Math.imul does the 32-bit multiply, `>>> 0`
// the final normalize.
function fnv1a32(grid) {
  let h = 0x811c9dc5;
  for (let i = 0; i < grid.length; i++) {
    const v = grid[i] >>> 0;
    for (let s = 0; s < 4; s++) {
      const byte = (v >>> (s * 8)) & 0xff;
      h ^= byte;
      h = Math.imul(h, 0x01000193);
    }
  }
  return h >>> 0;
}

// M2 escape-time palette (JS coloring; Fe computed every escape COUNT, and Fe
// coloring is the M4 rung). The grid holds Fe-computed escape counts 0..100:
//   iter == 100  -> interior (never escaped |z| < 2): paint black;
//   else         -> a monotone blue->yellow ramp, v = round(255 * sqrt(iter/100)):
//                   fast-escaping outer pixels read blue, the high-iteration
//                   boundary filaments read yellow.
// The cardioid + period-2 bulb read as the black interior. Any transpose/flip/
// stride bug is visible here AND caught by the per-pixel compare below.
function paintGrid(ctx, grid, width, height) {
  const img = ctx.createImageData(width, height);
  for (let i = 0; i < width * height; i++) {
    const iter = grid[i];
    const o = i * 4;
    if (iter >= 100) {
      img.data[o] = 0;
      img.data[o + 1] = 0;
      img.data[o + 2] = 0;
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

// The three no-GPU reasons from the runner map to AMBER (no live device); every
// other ok:false is a real error (RED).
function isNoGpu(gpu) {
  if (gpu.ok || gpu.messages) return false;
  return /navigator\.gpu is undefined|requestAdapter\(\) (returned null|threw)|no WebGPU adapter/.test(
    gpu.reason || ""
  );
}

// First per-pixel disagreement between two equal-length grids, or null.
function firstMismatch(a, b, width) {
  for (let i = 0; i < a.length; i++) {
    if (a[i] !== b[i]) {
      return { x: i % width, y: Math.floor(i / width), gpu: a[i] >>> 0, wasm: b[i] >>> 0 };
    }
  }
  return null;
}

async function main() {
  const ctx = $("grid-canvas").getContext("2d");

  // --- Load the compiler-produced assets. --------------------------------
  let layout, reference, feSrc, wgslSrc;
  try {
    [layout, reference, feSrc, wgslSrc] = await Promise.all([
      fetchJson("./gen/layout.json"),
      fetchJson("./gen/reference.json"),
      fetchText("./gen/kernel.fe"),
      fetchText("./gen/kernel.wgsl"),
    ]);
  } catch (e) {
    setBanner("red", "Generated assets missing", `${e.message}. Run: cargo run -p fe-codegen --example gen_mandelbrot_demo`);
    return;
  }

  $("kernel-name").textContent = layout.kernel;
  $("fe-src").textContent = feSrc;
  $("wgsl-src").textContent = wgslSrc;
  const p = layout.provenance || {};
  $("prov").textContent =
    `Fe @ ${short(p.fe_rev)} (branch mb2)  |  sonatina @ ${short(p.sonatina_rev)}  |  ` +
    `word ${layout.word}  |  workgroup [${layout.workgroup_size.join(", ")}]  |  ` +
    `mode ${layout.mode}  |  entry ${layout.entry_point}`;

  const refHash = reference.fnv1a32 >>> 0;
  setRow("row-ref", `${refHash} (0x${refHash.toString(16).padStart(8, "0")})`, "ok");
  $("ref-note").textContent =
    `reference = FNV-1a-32 of the ${reference.width}x${reference.height} grid, ` +
    `Fe -> wasm executed under wasmtime at generation time`;

  setBanner("amber", "Running...", "executing the Fe grid on wasm (V8) and dispatching it on WebGPU");

  // --- wasm-in-your-browser leg (the cross-backend oracle). --------------
  let wasmGrid = null;
  let wasmErr = null;
  try {
    wasmGrid = await runWasmGrid(layout.wasm_export, WIDTH, HEIGHT);
    const wasmHash = fnv1a32(wasmGrid);
    setRow("row-wasm", `${WIDTH}x${HEIGHT} grid, FNV-1a-32 ${wasmHash}`, wasmHash === refHash ? "ok" : "bad");
  } catch (e) {
    wasmErr = e.message || String(e);
    setRow("row-wasm", `error: ${wasmErr}`, "bad");
  }

  if (wasmErr !== null || wasmGrid === null) {
    setBanner("red", "wasm leg failed", `wasm: ${wasmErr}`);
    return;
  }

  const wasmHash = fnv1a32(wasmGrid);
  if (wasmHash !== refHash) {
    // The wasm oracle disagrees with the compiled reference: stale/wrong gen/.
    // Still paint the wasm grid so the shape is visible, but the rung is RED.
    paintGrid(ctx, wasmGrid, WIDTH, HEIGHT);
    setBanner("red", "Reference is off-hash", `wasm-in-browser FNV ${wasmHash} != reference ${refHash}. The gen/ artifacts are stale or wrong.`);
    return;
  }

  // --- WebGPU grid leg (kernel-blind, built from layout.json). -----------
  const gpu = await runWebGPUGrid(wgslSrc, layout, { width: WIDTH, height: HEIGHT });

  if (gpu.ok) {
    $("adapter").textContent = `adapter: ${gpu.adapter}`;
  } else {
    const msg = gpu.messages ? `${gpu.reason}\n${gpu.messages.join("\n")}` : gpu.reason;
    setRow("row-gpu", msg, "bad");
    $("adapter").textContent = gpu.adapter ? `adapter: ${gpu.adapter}` : "adapter: none";
  }

  // --- AMBER: no live GPU. Paint the wasm grid; make NO GPU claim. --------
  if (isNoGpu(gpu)) {
    paintGrid(ctx, wasmGrid, WIDTH, HEIGHT);
    setRow("row-gpu", gpu.reason, "bad");
    setBanner(
      "amber",
      "The Fe compiler compiled this mandelbrot; wasm computed every escape count; JS colored it",
      `Fe -> wasm computed every escape count in this browser (FNV = reference), and JS colored them into the fractal above (Fe coloring is the M4 rung). But this browser has no live WebGPU device (${gpu.reason}), so the "your GPU computed every escape count" claim is honestly withheld. A WebGPU browser earns the green rung.`
    );
    return;
  }

  // --- RED: a real GPU failure (Tint error / device / tiling). -----------
  if (!gpu.ok) {
    paintGrid(ctx, wasmGrid, WIDTH, HEIGHT);
    const detail = gpu.messages ? `${gpu.reason}\n${gpu.messages.join("\n")}` : gpu.reason;
    setBanner("red", "GPU leg failed", detail);
    return;
  }

  const gpuGrid = gpu.grid;
  const gpuHash = fnv1a32(gpuGrid);
  setRow("row-gpu", `${WIDTH}x${HEIGHT} grid, FNV-1a-32 ${gpuHash}`, gpuHash === refHash ? "ok" : "bad");

  // --- The honest per-pixel compare. -------------------------------------
  const mm = firstMismatch(gpuGrid, wasmGrid, WIDTH);
  if (mm) {
    paintGrid(ctx, gpuGrid, WIDTH, HEIGHT);
    setBanner(
      "red",
      "GPU / wasm per-pixel mismatch",
      `(${mm.x}, ${mm.y}): gpu=${mm.gpu} wasm=${mm.wasm}. A real cross-backend disagreement on ${gpu.adapter}.`
    );
    return;
  }

  if (gpuHash !== refHash) {
    paintGrid(ctx, gpuGrid, WIDTH, HEIGHT);
    setBanner("red", "GPU grid hash off reference", `GPU grid FNV ${gpuHash} != reference ${refHash} on ${gpu.adapter}, though it matched wasm per pixel.`);
    return;
  }

  // --- GREEN: live GPU grid == wasm grid (per pixel) == reference hash. ---
  paintGrid(ctx, gpuGrid, WIDTH, HEIGHT);
  setBanner(
    "green",
    "The Fe compiler compiled this mandelbrot; your GPU computed every escape count",
    `One Fe kernel -> wasm (V8) and SPIR-V-IR -> WGSL (WebGPU on ${gpu.adapter}). All ${WIDTH * HEIGHT} escape counts agree per pixel, and the GPU grid's FNV-1a-32 = ${gpuHash} = the compiled reference. JS colored the counts (Fe coloring is the M4 rung).`
  );
}

function short(rev) {
  if (!rev || rev === "unknown") return rev || "unknown";
  return rev.length > 10 ? rev.slice(0, 10) : rev;
}

main();
