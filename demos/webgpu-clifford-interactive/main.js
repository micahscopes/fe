// main.js - orchestration for the INTERACTIVE all-Fe Clifford rotor (ladder C3).
//
// The picture is a Fe fragment shader (the Cl(3) rotor sandwich, returning packed
// RGBA8); the rotor controls are a Fe wasm fn (update_rotor: a pointer drag composes
// a small rotor by geometric product). This file holds only: the dims, the badge
// copy, the GPU-vs-amber render choice, the in-browser Fe-controls oracle self-check,
// and the preset rotor buttons. All rotor math is in the Fe update_rotor; all
// pixels/color are in the Fe fragment. The demo-blind pump (live-pump.js) owns the
// event -> update_rotor -> render loop.
//
//   GREEN  the Fe render pipeline drew the canvas on a live GPU AND verifyView's
//          bytes == the Fe-wasm fragment's bytes at the current rotor AND the
//          pinned-rotor FNVs match the compiled reference. Adapter named.
//   AMBER  no navigator.gpu / no adapter: the Fe fragment still computes every
//          pixel in V8 (JS only blits) and the Fe controls still run in V8; the
//          "your GPU drew it" claim is honestly withheld.
//   RED    a Tint shader error (verbatim) or a GPU-vs-Fe-wasm pixel mismatch.

import { instantiateWasm, renderFragmentGrid } from "../webgpu-keystone/wasm-runner.js";
import { initWebGPURender, renderFrame, verifyView } from "../webgpu-keystone/webgpu-runner.js";
import { createLivePump } from "./live-pump.js";

const $ = (id) => document.getElementById(id);

// ---- asset loading: fetched from ./gen/ (served page) or inlined (standalone).
function b64ToBytes(b64) {
  const bin = atob(b64);
  const out = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
  return out;
}
async function fetchText(url) {
  const r = await fetch(url);
  if (!r.ok) throw new Error(`fetch ${url} -> HTTP ${r.status}`);
  return r.text();
}
async function loadAssets() {
  const inl = window.CLIFFORD_LIVE_ASSETS;
  if (inl) {
    return {
      layout: JSON.parse(inl.layout),
      ctl: JSON.parse(inl.ctl),
      reference: JSON.parse(inl.reference),
      fragFe: inl.frag_fe,
      ctlFe: inl.ctl_fe,
      wgsl: inl.wgsl,
      fragWasm: b64ToBytes(inl.frag_wasm_b64),
      ctlWasm: b64ToBytes(inl.ctl_wasm_b64),
    };
  }
  const [layout, ctl, reference, fragFe, ctlFe, wgsl, fragWasmBuf, ctlWasmBuf] = await Promise.all([
    fetchText("./gen/layout.json").then(JSON.parse),
    fetchText("./gen/ctl.json").then(JSON.parse),
    fetchText("./gen/reference.json").then(JSON.parse),
    fetchText("./gen/kernel.fe"),
    fetchText("./gen/ctl.fe"),
    fetchText("./gen/frag.wgsl"),
    fetch("./gen/frag.wasm").then((r) => r.arrayBuffer()),
    fetch("./gen/ctl.wasm").then((r) => r.arrayBuffer()),
  ]);
  return {
    layout, ctl, reference, fragFe, ctlFe, wgsl,
    fragWasm: new Uint8Array(fragWasmBuf),
    ctlWasm: new Uint8Array(ctlWasmBuf),
  };
}

// ---- the in-browser Fe-controls oracle (a JS twin of update_rotor, i32-exact).
// This is NOT what drives the page (the Fe wasm update_rotor does). It exists only
// to PROVE, in this browser's V8, that the Fe control fn matches an independent
// reimplementation over a batch of synthetic gestures - the same discipline the
// Rust gesture-tape test uses at build time. Math.imul is exact 32-bit multiply;
// `>> 12` is the arithmetic shift (Sar) the Fe kernel uses.
function updateRotorOracle(rc, r12, r13, r23, dx, dy) {
  const isNeg = (d) => (d < 0 ? 1 : 0);
  const isPos = (d) => (d > 0 ? 1 : 0);
  const dirSin = (d) => (isNeg(d) - isPos(d)) * 128;
  const dirCos = (d) => 4096 - (isNeg(d) + isPos(d));
  const clamp = (c) => Math.max(-8192, Math.min(8192, c));
  const c0y = dirCos(dx), sy0 = dirSin(dx);
  const rc1 = (Math.imul(c0y, rc) - Math.imul(sy0, r12)) >> 12;
  const r121 = (Math.imul(sy0, rc) + Math.imul(c0y, r12)) >> 12;
  const r131 = (Math.imul(c0y, r13) + Math.imul(sy0, r23)) >> 12;
  const r231 = (Math.imul(c0y, r23) - Math.imul(sy0, r13)) >> 12;
  const c0p = dirCos(dy), sp0 = dirSin(dy);
  const rc2 = (Math.imul(c0p, rc1) - Math.imul(sp0, r131)) >> 12;
  const r122 = (Math.imul(c0p, r121) - Math.imul(sp0, r231)) >> 12;
  const r132 = (Math.imul(sp0, rc1) + Math.imul(c0p, r131)) >> 12;
  const r232 = (Math.imul(c0p, r231) + Math.imul(sp0, r121)) >> 12;
  return [clamp(rc2) | 0, clamp(r122) | 0, clamp(r132) | 0, clamp(r232) | 0];
}

// Exercise the Fe update_rotor export in V8 against updateRotorOracle: directed
// no-op/yaw/pitch/clamp cases plus a seeded forward-fed random walk. Returns
// { ok, steps, mismatch? }. This is the load-bearing "the Fe controls run in your
// browser and match" gate.
function verifyControlsInBrowser(updateRotor, initRotor) {
  const eq = (a, b) => a[0] === b[0] && a[1] === b[1] && a[2] === b[2] && a[3] === b[3];
  const call = (r, dx, dy) => {
    const got = updateRotor(r[0], r[1], r[2], r[3], dx, dy);
    const want = updateRotorOracle(r[0], r[1], r[2], r[3], dx, dy);
    if (!Array.isArray(got) || !eq(got, want)) {
      return { got, want, input: [...r, dx, dy] };
    }
    return null;
  };
  // Directed cases (mirror the Rust directed test): no-op identity, yaw both ways,
  // pitch, and a runaway drag that exercises the component clamp.
  const directed = [
    [[4096, 0, 0, 0], 0, 0],
    [[4096, 0, 0, 0], 8, 0],
    [[4096, 0, 0, 0], -8, 0],
    [[4096, 0, 0, 0], 0, 8],
    [[3712, 577, 1154, 1154], 5, -3],
    [[8000, 8000, 8000, 8000], -20, -20],
  ];
  let steps = 0;
  for (const [r, dx, dy] of directed) {
    const mm = call(r, dx, dy);
    steps++;
    if (mm) return { ok: false, steps, mismatch: mm };
  }
  // A forward-fed random walk (deterministic; feeds each reply as the next rotor -
  // the exact broker round-trip). Uses a small LCG.
  let s = 0x9e3779b9 >>> 0;
  const rnd = () => (s = (Math.imul(s, 1664525) + 1013904223) >>> 0);
  let r = initRotor.slice();
  for (let k = 0; k < 4000; k++) {
    const q = rnd();
    const dx = ((q >>> 3) & 63) - 32;
    const dy = ((q >>> 12) & 63) - 32;
    const mm = call(r, dx, dy);
    steps++;
    if (mm) return { ok: false, steps, mismatch: mm };
    r = updateRotor(r[0], r[1], r[2], r[3], dx, dy);
  }
  return { ok: true, steps };
}

// FNV-1a-32 over a Uint32Array frame's LE byte stream (bit-for-bit the generator).
function fnv1a32(frame) {
  let h = 0x811c9dc5;
  for (let i = 0; i < frame.length; i++) {
    const val = frame[i] >>> 0;
    for (let sft = 0; sft < 4; sft++) {
      h ^= (val >>> (sft * 8)) & 0xff;
      h = Math.imul(h, 0x01000193);
    }
  }
  return h >>> 0;
}

// ---- badge / readout wiring.
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

async function main() {
  const canvas = $("view-canvas");
  let ctx2d = null;

  let A;
  try {
    A = await loadAssets();
  } catch (e) {
    setBanner("red", "Generated assets missing", `${e.message}. Run: cargo run -p fe-codegen --example gen_clifford_interactive_demo`);
    return;
  }

  // Panels + provenance.
  $("frag-src").textContent = A.fragFe;
  $("ctl-src").textContent = A.ctlFe;
  $("wgsl-src").textContent = A.wgsl;
  const p = A.layout.provenance || {};
  $("prov").textContent =
    `Fe @ ${short(p.fe_rev)} (branch mb2)  |  sonatina @ ${short(p.sonatina_rev)}  |  ` +
    `render ${A.layout.vertex_entry} + ${A.layout.fragment_entry}  |  rotor params [` +
    (A.layout.params || []).map((x) => x.name).join(", ") + `]  |  control ${A.ctl.control_export}`;

  // --- Instantiate the Fe wasm modules (zero imports). --------------------
  let ctlExports, fragExports;
  try {
    ctlExports = await instantiateWasm(A.ctlWasm);
    fragExports = await instantiateWasm(A.fragWasm);
  } catch (e) {
    setBanner("red", "Fe wasm failed to instantiate", e.message || String(e));
    return;
  }
  const updateRotor = ctlExports[A.ctl.control_export];
  if (typeof updateRotor !== "function") {
    setBanner("red", "control export missing", `\`${A.ctl.control_export}\` not found in ctl.wasm`);
    return;
  }

  // --- GATE: the Fe controls run in THIS browser's V8 and match the oracle. -
  const cv = verifyControlsInBrowser(updateRotor, A.ctl.view_init);
  if (!cv.ok) {
    setBanner("red", "Fe controls disagree with the oracle in V8", `after ${cv.steps} gestures: ${JSON.stringify(cv.mismatch)}`);
    $("row-ctl").textContent = "MISMATCH";
    $("row-ctl").className = "val bad";
    return;
  }
  $("row-ctl").textContent = `${cv.steps} synthetic gestures: wasm update_rotor == JS oracle`;
  $("row-ctl").className = "val ok";

  const WIDTH = A.layout.width || 512;
  const HEIGHT = A.layout.height || 512;

  // --- Try the live GPU render pipeline. ----------------------------------
  const gpu = await initWebGPURender(A.wgsl, A.layout, canvas);

  // AMBER blit closure: the Fe fragment computes every pixel in V8; JS only moves
  // the Fe-computed RGBA bytes to the canvas (no color, no rotor math).
  const amberBlit = (rotor) => {
    if (!ctx2d) ctx2d = canvas.getContext("2d");
    const grid = renderFragmentGrid(fragExports, A.layout.frag_wasm_export, rotor, WIDTH, HEIGHT);
    const img = new ImageData(new Uint8ClampedArray(grid.buffer), WIDTH, HEIGHT);
    ctx2d.putImageData(img, 0, 0);
    return grid;
  };

  let renderFn;
  let badgeMode;

  if (gpu.ok) {
    $("adapter").textContent = `adapter: ${gpu.adapter}`;
    canvas.style.display = "block";
    renderFn = (rotor) => renderFrame(gpu, rotor);
    badgeMode = "green-pending";
  } else {
    $("adapter").textContent = gpu.adapter ? `adapter: ${gpu.adapter}` : "adapter: none (no WebGPU)";
    if (gpu.messages) {
      setBanner("red", "WGSL shader failed to compile", `${gpu.reason}\n${gpu.messages.join("\n")}`);
      return;
    }
    renderFn = (rotor) => amberBlit(rotor);
    badgeMode = "amber";
  }

  // --- The demo-blind pump: events -> Fe update_rotor -> renderFn. ---------
  const readout = $("view-readout");
  const pump = createLivePump({
    canvas,
    updateView: updateRotor,
    ctlMeta: A.ctl,
    renderFn,
    onView: (v) => {
      readout.textContent = `rc=${v[0]}  r12=${v[1]}  r13=${v[2]}  r23=${v[3]}`;
    },
  });
  window.__cliffordPump = pump; // scripted-drive handle (evaluate_script / tests).
  window.__updateRotorOracle = updateRotorOracle;

  // Preset buttons: set the rotor directly (still no rotor math in JS - these are
  // the pinned rotors the compiler referenced).
  for (const btn of document.querySelectorAll("[data-view]")) {
    btn.addEventListener("click", () => pump.setView(JSON.parse(btn.dataset.view)));
  }

  // --- Badge resolution. --------------------------------------------------
  // The initial rotor (view_init) is the tilted_default pin, so its FNV reference
  // exists in reference.json.
  const refByName = Object.fromEntries((A.reference.views || []).map((r) => [r.name, r]));
  const initRef = refByName["tilted_default"];

  if (badgeMode === "amber") {
    // Prove the amber picture is the Fe-computed reference: render the initial rotor
    // in V8 and match its FNV to the compiled reference.
    const grid = renderFragmentGrid(fragExports, A.layout.frag_wasm_export, A.ctl.view_init, WIDTH, HEIGHT);
    const h = fnv1a32(grid);
    const okHash = initRef && h === (initRef.fnv1a32 >>> 0);
    $("row-render").textContent = okHash
      ? `Fe fragment in V8: initial-rotor FNV ${h} == compiled reference`
      : `Fe fragment in V8: FNV ${h}` + (initRef ? ` != reference ${initRef.fnv1a32 >>> 0}` : "");
    $("row-render").className = okHash ? "val ok" : "val bad";
    setBanner(
      "amber",
      "Fe compiled the rotor sandwich AND the drag controls; your browser ran both (no GPU here)",
      `The Fe fragment computed every one of ${WIDTH * HEIGHT} pixels of the Cl(3) rotor sandwich in V8` +
        (okHash ? ` (initial-rotor FNV = the compiled reference)` : ``) +
        `, and the Fe update_rotor ran the drag->rotor composition in V8 (matched the oracle across ${cv.steps} gestures). ` +
        `Drag to tumble the rotor - the Fe controls drive it. But this browser exposes no live WebGPU ` +
        `(${gpu.reason}), so the "your GPU drew every pixel" claim is honestly withheld. A WebGPU browser earns the green rung.`
    );
    return;
  }

  // GREEN path: verify the live GPU render byte-equals the Fe-wasm fragment at the
  // current rotor, then confirm the initial-rotor FNV.
  const initRotor = pump.getView();
  renderFrame(gpu, initRotor);
  const vr = await verifyView(gpu, initRotor);
  if (!vr.ok) {
    setBanner("red", "GPU verify readback failed", vr.reason);
    return;
  }
  const gpuGrid = new Uint32Array(vr.rgba.buffer, vr.rgba.byteOffset, WIDTH * HEIGHT);
  const feGrid = renderFragmentGrid(fragExports, A.layout.frag_wasm_export, initRotor, WIDTH, HEIGHT);
  let mismatch = null;
  for (let i = 0; i < feGrid.length; i++) {
    if ((gpuGrid[i] >>> 0) !== (feGrid[i] >>> 0)) {
      mismatch = { x: i % WIDTH, y: Math.floor(i / WIDTH), gpu: gpuGrid[i] >>> 0, fe: feGrid[i] >>> 0 };
      break;
    }
  }
  if (mismatch) {
    setBanner("red", "GPU / Fe-wasm per-pixel mismatch", `(${mismatch.x}, ${mismatch.y}): gpu=0x${mismatch.gpu.toString(16)} fe=0x${mismatch.fe.toString(16)} on ${gpu.adapter}`);
    return;
  }
  const gpuHash = fnv1a32(gpuGrid);
  const hashOk = initRef ? gpuHash === (initRef.fnv1a32 >>> 0) : true;
  $("row-render").textContent = `live GPU render == Fe-wasm fragment per pixel; initial-rotor FNV ${gpuHash}` + (initRef ? (hashOk ? " == reference" : ` != reference ${initRef.fnv1a32 >>> 0}`) : "");
  $("row-render").className = hashOk ? "val ok" : "val bad";
  if (!hashOk) {
    setBanner("red", "GPU render hash off reference", `GPU initial-rotor FNV ${gpuHash} != reference ${initRef.fnv1a32 >>> 0} on ${gpu.adapter}`);
    return;
  }
  setBanner(
    "green",
    "Fe compiled the Cl(3) rotor sandwich AND the drag controls; your GPU drew every pixel",
    `The Fe render pipeline (vertex ${A.layout.vertex_entry} + fragment ${A.layout.fragment_entry}) drew the canvas on ${gpu.adapter}; ` +
      `the readback byte-equals the Fe-wasm fragment at the current rotor and the initial-rotor FNV = the compiled reference. ` +
      `The Fe update_rotor runs the drag->rotor composition in V8 (matched the oracle across ${cv.steps} gestures). ` +
      `Drag to tumble the rotor: JS only forwards events; every pixel and all rotor math are Fe.`
  );
}

main();
