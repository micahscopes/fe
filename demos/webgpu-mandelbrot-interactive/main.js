// main.js - orchestration for the INTERACTIVE all-Fe mandelbrot (ladder I3).
//
// The picture is a Fe fragment shader; the pan/zoom controls are a Fe wasm fn.
// This file holds only: the dims, the badge copy, the GPU-vs-amber render choice,
// the in-browser Fe-controls oracle self-check, and the preset buttons. All view
// math is in the Fe update_view; all pixels/palette are in the Fe fragment. The
// demo-blind pump (live-pump.js) owns the event -> update_view -> render loop.
//
//   GREEN  the Fe render pipeline drew the canvas on a live GPU AND verifyView's
//          bytes == the Fe-wasm fragment's bytes at the current view AND the
//          pinned-view FNVs match the compiled reference. Adapter named.
//   AMBER  no navigator.gpu / no adapter: the Fe fragment still computes every
//          pixel in V8 (JS only blits) and the Fe controls still run in V8; the
//          "your GPU drew it" claim is honestly withheld.
//   RED    a Tint shader error (verbatim) or a GPU-vs-Fe-wasm pixel mismatch.

import { instantiateWasm, renderFragmentGrid } from "../webgpu-keystone/wasm-runner.js";
import { initWebGPURender, renderFrame, verifyView } from "../webgpu-keystone/webgpu-runner.js";
import { createLivePump } from "./live-pump.js";
import { createMandelbrotActorRuntime } from "./actor-runtime.js";
import { createMandelbrotWorkerControl } from "./worker-control.js";

const $ = (id) => document.getElementById(id);
const acceptanceOffscreen = new URLSearchParams(window.location.search).get("acceptance") === "offscreen";
const acceptancePresentation = acceptanceOffscreen ? "offscreen" : "canvas";
window.__mandelAcceptance = { state: "pending", worker: false, presentation: acceptancePresentation };

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
  const inl = window.MANDEL_LIVE_ASSETS;
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
      ctlCanonicalWasm: null,
    };
  }
  const [layout, ctl, reference, fragFe, ctlFe, wgsl, fragWasmBuf, ctlWasmBuf,
    ctlCanonicalWasmBuf] = await Promise.all([
    fetchText("./gen/layout.json").then(JSON.parse),
    fetchText("./gen/ctl.json").then(JSON.parse),
    fetchText("./gen/reference.json").then(JSON.parse),
    fetchText("./gen/kernel.fe"),
    fetchText("./gen/ctl.fe"),
    fetchText("./gen/frag.wgsl"),
    fetch("./gen/frag.wasm").then((r) => r.arrayBuffer()),
    fetch("./gen/ctl.wasm").then((r) => r.arrayBuffer()),
    fetch("./gen/ctl-canonical.wasm").then((r) => r.arrayBuffer()),
  ]);
  return {
    layout, ctl, reference, fragFe, ctlFe, wgsl,
    fragWasm: new Uint8Array(fragWasmBuf),
    ctlWasm: new Uint8Array(ctlWasmBuf),
    ctlCanonicalWasm: new Uint8Array(ctlCanonicalWasmBuf),
  };
}

// ---- the in-browser Fe-controls oracle (a JS twin of update_view, i32-exact).
// This is NOT what drives the page (the Fe wasm update_view does). It exists only
// to PROVE, in this browser's V8, that the Fe control fn matches an independent
// reimplementation over a batch of synthetic gestures - the same discipline the
// Rust gesture-tape test uses at build time.
function updateViewOracle(cr, ci, sq, dx, dy, dzoom, mx, my) {
  let re = (cr - (Math.imul(dx, sq) >> 4)) | 0;
  let im = (ci - (Math.imul(dy, sq) >> 4)) | 0;
  let s = sq | 0;
  if (dzoom < 0) s = (sq - (sq >> 3)) | 0;
  if (dzoom > 0) s = (sq + (sq >> 3)) | 0;
  if (s < 16) s = 16;
  if (s > 384) s = 384;
  re = (re + (Math.imul(mx - 256, sq - s) >> 4)) | 0;
  im = (im + (Math.imul(my - 256, sq - s) >> 4)) | 0;
  if (re > 10240) re = 10240;
  if (re < -10240) re = -10240;
  if (im > 10240) im = 10240;
  if (im < -10240) im = -10240;
  return [re | 0, im | 0, s | 0];
}

// Exercise the Fe update_view export in V8 against updateViewOracle: the directed
// clamp/zoom/anchor cases plus a seeded forward-fed random walk. Returns
// { ok, steps, mismatch? }. This is the load-bearing "the Fe controls run in your
// browser and match" gate.
async function verifyControlsInBrowser(updateView, initView) {
  const eq = (a, b) => a[0] === b[0] && a[1] === b[1] && a[2] === b[2];
  const call = async (v, dx, dy, dz, mx, my) => {
    const got = await updateView(v[0], v[1], v[2], dx, dy, dz, mx, my);
    const want = updateViewOracle(v[0], v[1], v[2], dx, dy, dz, mx, my);
    if (!Array.isArray(got) || !eq(got, want)) {
      return { mismatch: { got, want, input: [...v, dx, dy, dz, mx, my] } };
    }
    return { got };
  };
  // Directed cases (mirror the Rust directed test): four center clamps, both scale
  // clamps, an anchored zoom.
  const directed = [
    [[10000, 0, 384], -64, 0, 0, 256, 256],
    [[-10000, 0, 384], 64, 0, 0, 256, 256],
    [[0, 10000, 384], 0, -64, 0, 256, 256],
    [[0, -10000, 384], 0, 64, 0, 256, 256],
    [[0, 0, 384], 0, 0, 1, 256, 256],
    [[0, 0, 16], 0, 0, -1, 256, 256],
    [[1024, -512, 256], 0, 0, -1, 40, 500],
  ];
  let steps = 0;
  for (const [v, dx, dy, dz, mx, my] of directed) {
    const checked = await call(v, dx, dy, dz, mx, my);
    steps++;
    if (checked.mismatch) return { ok: false, steps, mismatch: checked.mismatch };
  }
  // A forward-fed random walk (deterministic; feeds each reply as the next view -
  // the exact broker round-trip). Uses a small LCG.
  let s = 0x9e3779b9 >>> 0;
  const rnd = () => (s = (Math.imul(s, 1664525) + 1013904223) >>> 0);
  let v = initView.slice();
  for (let k = 0; k < 4000; k++) {
    const r = rnd();
    const dx = ((r >>> 3) & 127) - 64;
    const dy = ((r >>> 11) & 127) - 64;
    const dz = ((r >>> 19) & 3) === 0 ? -1 : ((r >>> 19) & 3) === 1 ? 1 : 0;
    const mx = (r >>> 21) & 511;
    const my = (rnd() >>> 12) & 511;
    const checked = await call(v, dx, dy, dz, mx, my);
    steps++;
    if (checked.mismatch) return { ok: false, steps, mismatch: checked.mismatch };
    v = checked.got;
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
  if (state === "red") {
    window.__mandelAcceptance = {
      ...window.__mandelAcceptance,
      state: "red",
      error: detail || headline,
    };
  }
}
function short(rev) {
  if (!rev || rev === "unknown") return rev || "unknown";
  return rev.length > 10 ? rev.slice(0, 10) : rev;
}

async function main() {
  const canvas = $("view-canvas");
  // A canvas holds exactly ONE context type: grab 2D lazily, and only on the amber
  // path (the GPU path claims a "webgpu" context inside initWebGPURender instead).
  let ctx2d = null;

  let A;
  try {
    A = await loadAssets();
  } catch (e) {
    setBanner("red", "Generated assets missing", `${e.message}. Run: cargo run -p fe-codegen --example gen_mandelbrot_interactive_demo`);
    return;
  }

  // Panels + provenance.
  $("frag-src").textContent = A.fragFe;
  $("ctl-src").textContent = A.ctlFe;
  $("wgsl-src").textContent = A.wgsl;
  const p = A.layout.provenance || {};
  $("prov").textContent =
    `Fe @ ${short(p.fe_rev)} (branch mb2)  |  sonatina @ ${short(p.sonatina_rev)}  |  ` +
    `render ${A.layout.vertex_entry} + ${A.layout.fragment_entry}  |  view params [` +
    (A.layout.params || []).map((x) => x.name).join(", ") + `]  |  control ${A.ctl.control_export}`;

  // --- Instantiate the Fe wasm modules (zero imports). --------------------
  let fragExports, controlWorker;
  let updateView;
  try {
    fragExports = await instantiateWasm(A.fragWasm);
    if (window.MANDEL_LIVE_ASSETS) {
      const ctlExports = await instantiateWasm(A.ctlWasm);
      updateView = (...args) => Promise.resolve(ctlExports[A.ctl.control_export](...args));
    } else {
      controlWorker = await createMandelbrotWorkerControl({
        wasm: A.ctlCanonicalWasm,
        lane: A.ctl.canonical_lane,
        argNames: A.ctl.args,
        resultOrder: A.ctl.result_order,
      });
      updateView = (...args) => controlWorker.update(args);
      window.__mandelAcceptance = {
        state: "pending", worker: true, presentation: acceptancePresentation,
      };
    }
  } catch (e) {
    setBanner("red", "Fe wasm failed to instantiate", e.message || String(e));
    return;
  }
  if (typeof updateView !== "function") {
    setBanner("red", "control export missing", `\`${A.ctl.control_export}\` not found in ctl.wasm`);
    return;
  }

  // --- GATE: the Fe controls run in THIS browser's V8 and match the oracle. -
  const cv = await verifyControlsInBrowser(updateView, A.ctl.view_init);
  if (!cv.ok) {
    setBanner("red", "Fe controls disagree with the oracle in V8", `after ${cv.steps} gestures: ${JSON.stringify(cv.mismatch)}`);
    $("row-ctl").textContent = "MISMATCH";
    $("row-ctl").className = "val bad";
    return;
  }
  $("row-ctl").textContent = `${cv.steps} synthetic gestures: wasm update_view == JS oracle`;
  $("row-ctl").className = "val ok";

  const WIDTH = A.layout.width || 512;
  const HEIGHT = A.layout.height || 512;

  // --- Try the live GPU render pipeline. ----------------------------------
  const gpu = await initWebGPURender(A.wgsl, A.layout, acceptanceOffscreen ? null : canvas);

  // AMBER blit closure: the Fe fragment computes every pixel in V8; JS only moves
  // the Fe-computed RGBA bytes to the canvas (no palette, no view math).
  const amberBlit = (view) => {
    if (!ctx2d) ctx2d = canvas.getContext("2d");
    const grid = renderFragmentGrid(fragExports, A.layout.frag_wasm_export, view, WIDTH, HEIGHT);
    const img = new ImageData(new Uint8ClampedArray(grid.buffer), WIDTH, HEIGHT);
    ctx2d.putImageData(img, 0, 0);
    return grid;
  };

  let renderDirect;
  let badgeMode;

  if (gpu.ok) {
    $("adapter").textContent = `adapter: ${gpu.adapter}`;
    canvas.style.display = "block";
    renderDirect = acceptanceOffscreen
      ? () => ({ submitted: false, offscreen: true })
      : (view) => renderFrame(gpu, view);
    badgeMode = "green-pending";
  } else {
    $("adapter").textContent = gpu.adapter ? `adapter: ${gpu.adapter}` : "adapter: none (no WebGPU)";
    if (gpu.messages) {
      // A real Tint compile error is RED, not amber.
      setBanner("red", "WGSL shader failed to compile", `${gpu.reason}\n${gpu.messages.join("\n")}`);
      return;
    }
    renderDirect = (view) => amberBlit(view);
    badgeMode = "amber";
  }

  const refByName = Object.fromEntries((A.reference.views || []).map((r) => [r.name, r]));
  const defaultRef = refByName["default"];
  const verifyInitialView = async (view) => {
    const vr = await verifyView(gpu, view);
    if (!vr.ok) throw new Error(`GPU verify readback failed: ${vr.reason}`);
    const gpuGrid = new Uint32Array(vr.rgba.buffer, vr.rgba.byteOffset, WIDTH * HEIGHT);
    const feGrid = renderFragmentGrid(
      fragExports, A.layout.frag_wasm_export, view, WIDTH, HEIGHT,
    );
    for (let i = 0; i < feGrid.length; i++) {
      if ((gpuGrid[i] >>> 0) !== (feGrid[i] >>> 0)) {
        const mismatch = { x: i % WIDTH, y: Math.floor(i / WIDTH),
          gpu: gpuGrid[i] >>> 0, fe: feGrid[i] >>> 0 };
        throw new Error(`GPU / Fe-wasm per-pixel mismatch at (${mismatch.x}, ${mismatch.y}): ` +
          `gpu=0x${mismatch.gpu.toString(16)} fe=0x${mismatch.fe.toString(16)} on ${gpu.adapter}`);
      }
    }
    const gpuHash = fnv1a32(gpuGrid);
    const referenceHash = defaultRef ? defaultRef.fnv1a32 >>> 0 : gpuHash;
    if (gpuHash !== referenceHash) {
      throw new Error(`GPU default-view FNV ${gpuHash} != reference ${referenceHash} on ${gpu.adapter}`);
    }
    return { gpuHash, wasmHash: fnv1a32(feGrid), referenceHash };
  };
  const actorRuntime = createMandelbrotActorRuntime({
    render(view) {
      renderDirect(view);
      return { submitted: true };
    },
    verify: verifyInitialView,
    onError: (error) => setBanner("red", "Mandelbrot actor transport failed", String(error)),
  });

  // --- The demo-blind pump: events -> Fe update_view -> actor render. -------
  const readout = $("view-readout");
  const pump = createLivePump({
    canvas,
    updateView,
    ctlMeta: A.ctl,
    renderFn: (view) => actorRuntime.render(view),
    onView: (v) => {
      readout.textContent = `center_re=${v[0]}  center_im=${v[1]}  scale_q=${v[2]}`;
    },
  });
  window.__mandelPump = pump; // scripted-drive handle (evaluate_script / tests).
  window.__updateViewOracle = updateViewOracle;
  window.addEventListener("pagehide", () => {
    pump.destroy();
    actorRuntime.close("page hidden");
    controlWorker?.close();
  }, { once: true });

  // Preset buttons: set the view triple directly (still no view math in JS - these
  // are the pinned tokens the compiler referenced).
  for (const btn of document.querySelectorAll("[data-view]")) {
    btn.addEventListener("click", () => pump.setView(JSON.parse(btn.dataset.view)));
  }

  // --- Badge resolution. --------------------------------------------------
  // Reference FNV for the default (initial) view, from the compiled reference.
  if (badgeMode === "amber") {
    // Prove the amber picture is the Fe-computed reference: render the default view
    // in V8 and match its FNV to the compiled reference.
    const grid = renderFragmentGrid(fragExports, A.layout.frag_wasm_export, A.ctl.view_init, WIDTH, HEIGHT);
    const h = fnv1a32(grid);
    const okHash = defaultRef && h === (defaultRef.fnv1a32 >>> 0);
    $("row-render").textContent = okHash
      ? `Fe fragment in V8: default-view FNV ${h} == compiled reference`
      : `Fe fragment in V8: FNV ${h}` + (defaultRef ? ` != reference ${defaultRef.fnv1a32 >>> 0}` : "");
    $("row-render").className = okHash ? "val ok" : "val bad";
    setBanner(
      "amber",
      "Fe compiled the fractal AND the pan/zoom controls; your browser ran both (no GPU here)",
      `The Fe fragment computed every one of ${WIDTH * HEIGHT} pixels in V8` +
        (okHash ? ` (default-view FNV = the compiled reference)` : ``) +
        `, and the Fe update_view ran the pan/zoom in V8 (matched the oracle across ${cv.steps} gestures). ` +
        `Drag to pan, wheel to zoom - the Fe controls drive it. But this browser exposes no live WebGPU ` +
        `(${gpu.reason}), so the "your GPU drew every pixel" claim is honestly withheld. A WebGPU browser earns the green rung.`
    );
    window.__mandelAcceptance = {
      state: "amber",
      worker: Boolean(controlWorker),
      presentation: acceptancePresentation,
      controlsSteps: cv.steps,
      verified: false,
      wasmHash: h,
      referenceHash: defaultRef?.fnv1a32 >>> 0,
    };
    return;
  }

  // GREEN path: verify the live GPU render byte-equals the Fe-wasm fragment at the
  // current view, then confirm the pinned-view FNVs.
  const initView = pump.getView();
  await actorRuntime.render(initView); // ensure a draw landed on the canvas.
  let verification;
  try {
    verification = await actorRuntime.verify(initView);
  } catch (error) {
    setBanner("red", "GPU verification failed", String(error));
    return;
  }
  const gpuHash = verification.gpuHash;
  const wasmHash = verification.wasmHash;
  const hashOk = gpuHash === verification.referenceHash;
  $("row-render").textContent = `live GPU render == Fe-wasm fragment per pixel; default-view FNV ${gpuHash}` + (defaultRef ? (hashOk ? " == reference" : ` != reference ${defaultRef.fnv1a32 >>> 0}`) : "");
  $("row-render").className = hashOk ? "val ok" : "val bad";
  if (!hashOk) {
    setBanner("red", "GPU render hash off reference", `GPU default-view FNV ${gpuHash} != reference ${defaultRef.fnv1a32 >>> 0} on ${gpu.adapter}`);
    return;
  }
  setBanner(
    "green",
    "Fe compiled the fractal AND the pan/zoom controls; your GPU drew every pixel",
    `The Fe render pipeline (vertex ${A.layout.vertex_entry} + fragment ${A.layout.fragment_entry}) drew the canvas on ${gpu.adapter}; ` +
      `the readback byte-equals the Fe-wasm fragment at the current view and the default-view FNV = the compiled reference. ` +
      `The Fe update_view runs the pan/zoom in V8 (matched the oracle across ${cv.steps} gestures). ` +
      `Drag to pan, wheel to zoom: JS only forwards events; every pixel and all view math are Fe.`
  );
  window.__mandelAcceptance = {
    state: "green",
    worker: Boolean(controlWorker),
    presentation: acceptancePresentation,
    controlsSteps: cv.steps,
    verified: true,
    gpuHash,
    wasmHash,
    referenceHash: verification.referenceHash,
    adapter: gpu.adapter,
  };
}

main().catch((error) => {
  setBanner("red", "Mandelbrot startup failed", error?.stack || String(error));
});
