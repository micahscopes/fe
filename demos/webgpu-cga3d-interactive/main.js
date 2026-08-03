// main.js - orchestration for the INTERACTIVE all-Fe cga3d Pencil demo.
//
// The picture is a Fe fragment shader (the pencil-of-spheres incidence field,
// `demos/sketches/cga3d/src/lib.fe`'s `shade`, a REAL sparse Cl(4,1)
// contraction, not a hand-derived formula); the pencil controls are a Fe wasm
// fn (`update_pencil`: a canvas drag/wheel composes the next lambda/theta/zoom
// triple - the SAME three uniforms the fragment reads). This file holds only:
// the dims, the badge copy, the GPU-vs-amber render choice, the in-browser
// Fe-controls oracle self-check, and the preset buttons. All pencil math is in
// the Fe `update_pencil`; all pixels/color are in the Fe fragment. The
// demo-blind pump (live-pump.js) owns the event -> update_pencil -> render
// loop.
//
//   WEBGPU  a live WebGPU adapter rendered the canvas: the Fe render pipeline
//           (vertex_entry/fragment_entry/binding table from manifest.json)
//           drew every pixel.
//   AMBER   no navigator.gpu / no adapter: the Fe fragment still computes
//           every pixel in V8 (JS only blits); the "your GPU drew it" claim is
//           honestly withheld.
//   RED     a Tint shader error (verbatim) or the Fe controls disagree with
//           the independent oracle.
//
// RESOLUTION NOTE: `demos/sketches/cga3d/src/lib.fe` bakes
// `const RESOLUTION: f32 = 128.0` into the pixel -> world-coordinate map
// (`u = (px + 0.5) / RESOLUTION * 2 - 1`). The compiler-emitted generic
// `fe web build` render-runtime page dispatches at a hardcoded 256x256,
// silently mismatched against that constant (a 2x stretch/zoom-out nobody
// asked for). This page dispatches at 128x128 (`WIDTH`/`HEIGHT` below) to
// match RESOLUTION exactly, without touching the Fe algebra; the CSS scales
// the canvas up for display.

import { createLivePump } from "./live-pump.js";

const $ = (id) => document.getElementById(id);

const WIDTH = 128;
const HEIGHT = 128;

// ---- asset loading, all from ./gen/ (produced by ./generate.sh). ----------
async function fetchText(url) {
  const r = await fetch(url);
  if (!r.ok) throw new Error(`fetch ${url} -> HTTP ${r.status}`);
  return r.text();
}
async function fetchJson(url) {
  return JSON.parse(await fetchText(url));
}
async function loadAssets() {
  const [manifest, ctl, kernelFe, ctlFe, wgsl, fragWasmBuf, ctlWasmBuf] = await Promise.all([
    fetchJson("./gen/manifest.json"),
    fetchJson("./gen/ctl.json"),
    fetchText("./gen/kernel.fe"),
    fetchText("./gen/ctl.fe"),
    fetchText("./gen/frag.wgsl"),
    fetch("./gen/frag.wasm").then((r) => r.arrayBuffer()),
    fetch("./gen/ctl.wasm").then((r) => r.arrayBuffer()),
  ]);
  return {
    manifest, ctl, kernelFe, ctlFe, wgsl,
    fragWasm: new Uint8Array(fragWasmBuf),
    ctlWasm: new Uint8Array(ctlWasmBuf),
  };
}

// ---- the in-browser Fe-controls oracle (a JS twin of update_pencil). ------
// This is NOT what drives the page (the Fe wasm update_pencil does). It
// exists only to PROVE, in this browser's V8, that the Fe control fn matches
// an independent reimplementation over directed cases + a random walk - the
// same discipline the generator's Rust oracle uses at build time. Unlike the
// clifford rotor's fixed-point i32 arithmetic, this is plain f32, so the
// comparison is epsilon-tolerant (JS does f64 arithmetic; wasm does f32).
const LAMBDA_SENSITIVITY = 0.0025;
const THETA_SENSITIVITY = 0.01;
const ZOOM_MIN = 0.3;
const ZOOM_MAX = 4.0;
const ZOOM_STEP_IN = 0.875;
const ZOOM_STEP_OUT = 1.125;
const ORACLE_EPS = 1e-3;

function updatePencilOracle(lambda, theta, zoom, dx, dy, dzoom) {
  const clamp = (v, lo, hi) => Math.min(hi, Math.max(lo, v));
  const lambda1 = clamp(lambda + dx * LAMBDA_SENSITIVITY, 0.0, 1.0);
  const theta1 = theta + dy * THETA_SENSITIVITY;
  let zoom1 = zoom;
  if (dzoom < 0) zoom1 = zoom1 * ZOOM_STEP_IN;
  if (dzoom > 0) zoom1 = zoom1 * ZOOM_STEP_OUT;
  zoom1 = clamp(zoom1, ZOOM_MIN, ZOOM_MAX);
  return [lambda1, theta1, zoom1];
}

// Exercise the Fe update_pencil export in V8 against updatePencilOracle:
// directed no-op/sweep/clamp cases plus a seeded forward-fed random walk.
// Returns { ok, steps, mismatch? }.
function verifyControlsInBrowser(updatePencil, initView) {
  const close = (a, b) => Math.abs(a - b) <= ORACLE_EPS;
  const eq = (a, b) => close(a[0], b[0]) && close(a[1], b[1]) && close(a[2], b[2]);
  const call = (v, dx, dy, dzoom) => {
    const got = updatePencil(v[0], v[1], v[2], dx, dy, dzoom);
    const want = updatePencilOracle(v[0], v[1], v[2], dx, dy, dzoom);
    if (!Array.isArray(got) || !eq(got, want)) {
      return { got, want, input: [...v, dx, dy, dzoom] };
    }
    return null;
  };
  const directed = [
    [[0.5, 0.0, 1.0], 0, 0, 0],
    [[0.0, 0.0, 1.0], 200, 0, 0],
    [[0.9, 0.0, 1.0], 200, 0, 0],
    [[0.5, 0.0, 1.0], -300, 0, 0],
    [[0.5, 0.5, 1.0], 0, 100, 0],
    [[0.5, 0.5, 1.0], 0, 0, -1],
    [[0.5, 0.5, 1.0], 0, 0, 1],
    [[0.5, 0.5, 0.32], 0, 0, -1],
    [[0.5, 0.5, 3.9], 0, 0, 1],
  ];
  let steps = 0;
  for (const [v, dx, dy, dzoom] of directed) {
    const mm = call(v, dx, dy, dzoom);
    steps++;
    if (mm) return { ok: false, steps, mismatch: mm };
  }
  let s = 0x9e3779b9 >>> 0;
  const rnd = () => (s = (Math.imul(s, 1664525) + 1013904223) >>> 0);
  let v = initView.slice();
  for (let k = 0; k < 1000; k++) {
    const q = rnd();
    const dx = ((q >>> 3) & 63) - 32;
    const dy = ((q >>> 12) & 63) - 32;
    const dzoom = ((q >>> 20) & 3) - 1;
    const mm = call(v, dx, dy, dzoom);
    steps++;
    if (mm) return { ok: false, steps, mismatch: mm };
    v = updatePencil(v[0], v[1], v[2], dx, dy, dzoom);
  }
  return { ok: true, steps };
}

// ---- WebGPU render lane, driven entirely by manifest.json's real layout. --
// Kernel-blind beyond the fixed WIDTH/HEIGHT above: every binding fact
// (group/binding/access/stride/member offsets/scalar kinds) and every entry
// name comes from `manifest.layout`, the same protocol `fe web build` emits
// for every render bundle.
async function initWebGPURender(wgsl, manifest, canvas) {
  if (!("gpu" in navigator) || !navigator.gpu) {
    return { ok: false, reason: "navigator.gpu is undefined (this browser exposes no WebGPU)" };
  }
  let adapter;
  try {
    adapter = await navigator.gpu.requestAdapter();
  } catch (e) {
    return { ok: false, reason: `requestAdapter() threw: ${e.message || e}` };
  }
  if (!adapter) {
    return { ok: false, reason: "requestAdapter() returned null (no WebGPU adapter available)" };
  }
  let info = adapter.info;
  if (!info && typeof adapter.requestAdapterInfo === "function") {
    try {
      info = await adapter.requestAdapterInfo();
    } catch (_e) {
      info = null;
    }
  }
  const adapterName = info
    ? [info.vendor, info.architecture, info.device, info.description].filter(Boolean).join(" / ") ||
      "unnamed adapter"
    : "unknown adapter";

  let device;
  try {
    device = await adapter.requestDevice();
  } catch (e) {
    return { ok: false, reason: `requestDevice() failed: ${e.message || e}`, adapter: adapterName };
  }

  const layout = manifest.layout;
  if (layout.mode !== "render") {
    return { ok: false, reason: `expected layout.mode "render", got "${layout.mode}"`, adapter: adapterName };
  }

  const module = device.createShaderModule({ code: wgsl });
  if (typeof module.getCompilationInfo === "function") {
    const compInfo = await module.getCompilationInfo();
    const errs = compInfo.messages.filter((m) => m.type === "error");
    if (errs.length) {
      return {
        ok: false,
        reason: "WGSL shader compile error (Tint)",
        messages: errs.map((m) => `${m.lineNum}:${m.linePos} ${m.message}`),
        adapter: adapterName,
      };
    }
  }

  const inputBinding = (layout.bindings || []).find((b) => b.role === "input");
  const members = inputBinding ? [...inputBinding.members].sort((a, b) => a.arg_index - b.arg_index) : [];
  const format = layout.color_target_format || navigator.gpu.getPreferredCanvasFormat();
  const ctx = canvas.getContext("webgpu");
  if (!ctx) return { ok: false, reason: "canvas.getContext('webgpu') returned null", adapter: adapterName };
  ctx.configure({ device, format, alphaMode: "opaque" });

  let bindGroupLayout = null;
  let inputBuf = null;
  let bindGroup = null;
  let pipelineLayoutDesc = "auto";
  if (inputBinding) {
    inputBuf = device.createBuffer({
      size: Math.max(16, inputBinding.span),
      usage: GPUBufferUsage.STORAGE | GPUBufferUsage.COPY_DST,
    });
    bindGroupLayout = device.createBindGroupLayout({
      entries: [
        { binding: inputBinding.binding, visibility: GPUShaderStage.FRAGMENT, buffer: { type: "read-only-storage" } },
      ],
    });
    bindGroup = device.createBindGroup({
      layout: bindGroupLayout,
      entries: [{ binding: inputBinding.binding, resource: { buffer: inputBuf } }],
    });
    pipelineLayoutDesc = device.createPipelineLayout({ bindGroupLayouts: [bindGroupLayout] });
  }

  const pipeline = device.createRenderPipeline({
    layout: pipelineLayoutDesc,
    vertex: { module, entryPoint: layout.vertex_entry },
    fragment: { module, entryPoint: layout.fragment_entry, targets: [{ format }] },
    primitive: { topology: "triangle-list" },
  });

  return {
    ok: true,
    adapter: adapterName,
    render(view) {
      if (inputBuf) {
        const buf = new ArrayBuffer(Math.max(16, inputBinding.span));
        const dv = new DataView(buf);
        members.forEach((m, i) => {
          const v = view[i];
          if (m.scalar === "f32") dv.setFloat32(m.offset, v, true);
          else if (m.scalar === "u32") dv.setUint32(m.offset, v >>> 0, true);
          else dv.setInt32(m.offset, v | 0, true);
        });
        device.queue.writeBuffer(inputBuf, 0, buf);
      }
      const enc = device.createCommandEncoder();
      const pass = enc.beginRenderPass({
        colorAttachments: [
          {
            view: ctx.getCurrentTexture().createView(),
            clearValue: { r: 0, g: 0, b: 0, a: 1 },
            loadOp: "clear",
            storeOp: "store",
          },
        ],
      });
      pass.setPipeline(pipeline);
      if (bindGroup) pass.setBindGroup(0, bindGroup);
      pass.draw(3);
      pass.end();
      device.queue.submit([enc.finish()]);
    },
  };
}

// ---- CPU fallback (frag.wasm per pixel into a 2D canvas). -----------------
// Mirrors the compiler-emitted render-runtime page's own `callKernel`: builtin
// px/py args from manifest.layout.builtin_inputs, uniform args from
// manifest.layout's Input binding members, both by arg_index (never assumed
// positional beyond what the manifest states).
function makeAmberRenderer(fragExports, sourceEntry, manifest, canvas) {
  const layout = manifest.layout;
  const builtins = layout.builtin_inputs || [];
  const inputBinding = (layout.bindings || []).find((b) => b.role === "input");
  const members = inputBinding ? [...inputBinding.members].sort((a, b) => a.arg_index - b.arg_index) : [];
  const argc = 1 + Math.max(-1, ...builtins.map((b) => b.arg_index), ...members.map((m) => m.arg_index));
  const kernel = fragExports[sourceEntry];
  if (typeof kernel !== "function") {
    throw new Error(`wasm export \`${sourceEntry}\` not found in frag.wasm`);
  }
  const ctx2d = canvas.getContext("2d");
  const img = ctx2d.createImageData(WIDTH, HEIGHT);

  function callKernel(px, py, view) {
    const args = new Array(argc).fill(0);
    for (const b of builtins) args[b.arg_index] = String(b.source).endsWith("_y") ? py : px;
    members.forEach((m, i) => {
      args[m.arg_index] = view[i];
    });
    return kernel(...args) >>> 0;
  }

  return {
    render(view) {
      const d = img.data;
      for (let py = 0; py < HEIGHT; py++) {
        for (let px = 0; px < WIDTH; px++) {
          const rgba = callKernel(px, py, view);
          const i = (py * WIDTH + px) * 4;
          d[i] = (rgba >>> 16) & 255;
          d[i + 1] = (rgba >>> 8) & 255;
          d[i + 2] = rgba & 255;
          d[i + 3] = (rgba >>> 24) & 255;
        }
      }
      ctx2d.putImageData(img, 0, 0);
    },
  };
}

// ---- badge / readout wiring. ----------------------------------------------
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
function fmtView(v) {
  return `lambda=${v[0].toFixed(3)}  theta=${v[1].toFixed(3)}  zoom=${v[2].toFixed(3)}`;
}

async function main() {
  const canvas = $("view-canvas");
  canvas.width = WIDTH;
  canvas.height = HEIGHT;

  let A;
  try {
    A = await loadAssets();
  } catch (e) {
    setBanner("red", "Generated assets missing", `${e.message}. Run: demos/webgpu-cga3d-interactive/generate.sh`);
    return;
  }

  $("kernel-src").textContent = A.kernelFe;
  $("ctl-src").textContent = A.ctlFe;
  $("wgsl-src").textContent = A.wgsl;
  const p = A.ctl.provenance || {};
  $("prov").textContent =
    `Fe @ ${short(p.fe_rev)} (branch mb2)  |  render ${A.manifest.layout.vertex_entry} + ` +
    `${A.manifest.layout.fragment_entry}  |  pencil uniforms [lambda, theta, zoom]  |  ` +
    `control ${A.ctl.control_export}`;

  // --- Instantiate the Fe wasm modules (zero imports). --------------------
  let ctlExports, fragExports;
  try {
    ctlExports = (await WebAssembly.instantiate(A.ctlWasm, {})).instance.exports;
    fragExports = (await WebAssembly.instantiate(A.fragWasm, {})).instance.exports;
  } catch (e) {
    setBanner("red", "Fe wasm failed to instantiate", e.message || String(e));
    return;
  }
  const updatePencil = ctlExports[A.ctl.control_export];
  if (typeof updatePencil !== "function") {
    setBanner("red", "control export missing", `\`${A.ctl.control_export}\` not found in ctl.wasm`);
    return;
  }

  // Sanity gate: the fragment's uniform-member count must match the control's
  // view arity, since the two are built by SEPARATE tools (fe web build for
  // the fragment, gen_cga3d_interactive_ctl for the controls) rather than one
  // unified generator. A drift here would silently scramble the binding.
  const inputBinding = (A.manifest.layout.bindings || []).find((b) => b.role === "input");
  const memberCount = inputBinding ? inputBinding.members.length : 0;
  if (memberCount !== A.ctl.view_arg_count) {
    setBanner(
      "red",
      "fragment/control arity mismatch",
      `fragment has ${memberCount} uniform members but ctl.json declares view_arg_count ${A.ctl.view_arg_count}`
    );
    return;
  }

  // --- GATE: the Fe controls run in THIS browser's V8 and match the oracle. -
  const cv = verifyControlsInBrowser(updatePencil, A.ctl.view_init);
  if (!cv.ok) {
    setBanner("red", "Fe controls disagree with the oracle in V8", `after ${cv.steps} gestures: ${JSON.stringify(cv.mismatch)}`);
    $("row-ctl").textContent = "MISMATCH";
    $("row-ctl").className = "val bad";
    return;
  }
  $("row-ctl").textContent = `${cv.steps} synthetic gestures: wasm update_pencil == JS oracle (within ${ORACLE_EPS})`;
  $("row-ctl").className = "val ok";

  // --- Try the live GPU render pipeline; else the amber V8 fallback. -------
  const gpu = await initWebGPURender(A.wgsl, A.manifest, canvas);
  let renderFn;
  if (gpu.ok) {
    $("adapter").textContent = `adapter: ${gpu.adapter}`;
    canvas.style.display = "block";
    renderFn = (view) => gpu.render(view);
    $("row-render").textContent = `live WebGPU render on ${gpu.adapter}`;
    $("row-render").className = "val ok";
  } else {
    $("adapter").textContent = gpu.adapter ? `adapter: ${gpu.adapter}` : "adapter: none (no WebGPU)";
    if (gpu.messages) {
      setBanner("red", "WGSL shader failed to compile", `${gpu.reason}\n${gpu.messages.join("\n")}`);
      return;
    }
    const amber = makeAmberRenderer(fragExports, A.manifest.source_entry, A.manifest, canvas);
    renderFn = (view) => amber.render(view);
    $("row-render").textContent = `no WebGPU (${gpu.reason}); the Fe fragment computes every pixel in V8`;
    $("row-render").className = "val";
  }

  // --- The demo-blind pump: events -> Fe update_pencil -> renderFn. --------
  const readout = $("view-readout");
  const pump = createLivePump({
    canvas,
    updateView: updatePencil,
    ctlMeta: A.ctl,
    renderFn,
    onView: (v) => {
      readout.textContent = fmtView(v);
    },
  });
  window.__cga3dPump = pump; // scripted-drive handle (evaluate_script / tests).
  window.__updatePencilOracle = updatePencilOracle;

  // Preset buttons: set the view directly (still no pencil math in JS - these
  // are the pinned views sweeping the pencil's own story: pure generator ->
  // blend -> radical plane -> rotated blend).
  for (const btn of document.querySelectorAll("[data-view]")) {
    btn.addEventListener("click", () => pump.setView(JSON.parse(btn.dataset.view)));
  }

  if (gpu.ok) {
    setBanner(
      "green",
      "Fe compiled the pencil-of-spheres fragment AND the drag controls; your GPU drew every pixel",
      `The Fe render pipeline (vertex ${A.manifest.layout.vertex_entry} + fragment ` +
        `${A.manifest.layout.fragment_entry}) drew the canvas on ${gpu.adapter}. The Fe update_pencil ` +
        `runs the drag/wheel -> pencil composition in V8 (matched the oracle across ${cv.steps} gestures). ` +
        `Drag horizontally to sweep lambda (sphere -> sphere -> radical plane), vertically to turn theta, ` +
        `wheel to zoom.`
    );
  } else {
    setBanner(
      "amber",
      "Fe compiled the pencil-of-spheres fragment AND the drag controls; your browser ran both (no GPU here)",
      `The Fe fragment computes every one of ${WIDTH * HEIGHT} pixels of the sparse Cl(4,1) incidence ` +
        `field in V8, and the Fe update_pencil ran the drag/wheel -> pencil composition in V8 (matched the ` +
        `oracle across ${cv.steps} gestures). Drag to sweep the pencil - the Fe controls drive it. But this ` +
        `browser exposes no live WebGPU (${gpu.reason}), so the "your GPU drew every pixel" claim is ` +
        `honestly withheld.`
    );
  }
}

main();
