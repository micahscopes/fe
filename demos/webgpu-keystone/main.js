// main.js - orchestration for the Fe -> GPU keystone page.
//
// Fetches the generated (compiler-produced) assets, runs both Fe backends in the
// browser (wasm in V8, SPIR-V-IR -> WGSL on WebGPU), compares both to the
// pinned reference, and paints the honest rung state:
//
//   GREEN  "R-chrome earned": GPU readback === wasm-in-browser === reference,
//                             with the adapter named.
//   AMBER  "wasm-only":       no WebGPU / no adapter; wasm === reference still
//                             shown, GPU row red with the reason.
//   RED    :                  any mismatch, a wasm failure, or a shader error
//                             (Tint text shown).
//
// There is no code path that paints GREEN without a live GPU readback matching.

import { runWasm } from "./wasm-runner.js";
import { runWebGPU } from "./webgpu-runner.js";

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

async function main() {
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
    setBanner("red", "Generated assets missing", `${e.message}. Run: cargo run -p fe-codegen --example gen_webgpu_demo`);
    return;
  }

  // Provenance + source panels (all from gen/, i.e. from the Fe compiler).
  $("kernel-name").textContent = layout.kernel;
  $("fe-src").textContent = feSrc;
  $("wgsl-src").textContent = wgslSrc;
  const p = layout.provenance || {};
  $("prov").textContent =
    `Fe @ ${short(p.fe_rev)} (branch mb2)  |  sonatina @ ${short(p.sonatina_rev)}  |  ` +
    `word ${layout.word}  |  workgroup [${layout.workgroup_size.join(", ")}]  |  entry ${layout.entry_point}`;

  const pinned = reference.pinned;
  setRow("row-ref", `${reference.value}`, reference.value === pinned ? "ok" : "bad");
  $("pin-note").textContent = `pinned = ${pinned} (revm / wasmtime / lavapipe, cross-backend)`;

  setBanner("amber", "Running...", "compiling wasm in V8 and dispatching WebGPU");

  // --- wasm-in-your-browser leg. -----------------------------------------
  let wasmValue = null;
  let wasmErr = null;
  try {
    wasmValue = await runWasm("./gen/kernel.wasm", layout);
    setRow("row-wasm", `${wasmValue}`, wasmValue === pinned ? "ok" : "bad");
  } catch (e) {
    wasmErr = e.message || String(e);
    setRow("row-wasm", `error: ${wasmErr}`, "bad");
  }

  // --- WebGPU leg (kernel-blind, built from layout.json). ----------------
  const gpu = await runWebGPU(wgslSrc, layout);
  if (gpu.ok) {
    setRow("row-gpu", `${gpu.value}`, gpu.value === pinned ? "ok" : "bad");
    $("adapter").textContent = `adapter: ${gpu.adapter}`;
  } else {
    const msg = gpu.messages ? `${gpu.reason}\n${gpu.messages.join("\n")}` : gpu.reason;
    setRow("row-gpu", msg, "bad");
    $("adapter").textContent = gpu.adapter ? `adapter: ${gpu.adapter}` : "adapter: none";
  }

  // --- Rung decision (honest ladder). ------------------------------------
  const refOk = reference.value === pinned;
  const wasmOk = wasmErr === null && wasmValue === pinned;

  if (!refOk) {
    setBanner("red", "Reference is off-pin", `reference.json value ${reference.value} != pin ${pinned}. The gen/ artifacts are stale or wrong.`);
    return;
  }
  if (!wasmOk) {
    setBanner("red", "wasm leg failed", wasmErr ? `wasm: ${wasmErr}` : `wasm-in-browser ${wasmValue} != pin ${pinned}`);
    return;
  }

  if (gpu.ok && gpu.value === wasmValue && wasmValue === pinned) {
    setBanner(
      "green",
      "R-chrome earned",
      `One Fe kernel -> wasm (V8) and SPIR-V-IR -> WGSL (WebGPU on ${gpu.adapter}), both = ${pinned}, equal to the revm/wasmtime/lavapipe pin.`
    );
    return;
  }

  if (!gpu.ok) {
    setBanner(
      "amber",
      "wasm leg only, R-chrome NOT earned",
      `wasm-in-browser = ${wasmValue} = pin, but no live GPU readback (${gpu.reason}). This is the honest no-WebGPU state.`
    );
    return;
  }

  // gpu ran but disagreed: red mismatch.
  setBanner(
    "red",
    "GPU mismatch",
    `GPU readback ${gpu.value} != wasm/pin ${pinned} on ${gpu.adapter}. A real cross-backend disagreement.`
  );
}

function short(rev) {
  if (!rev || rev === "unknown") return rev || "unknown";
  return rev.length > 10 ? rev.slice(0, 10) : rev;
}

main();
