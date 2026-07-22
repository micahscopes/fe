// webgpu-runner.js - the KERNEL-BLIND WebGPU lane.
//
// This file contains NOTHING kernel-specific. Every shader-specific fact (which
// bindings exist, their group/binding index, storage access, which binding
// carries the result, where in it and how wide, the workgroup size, the entry
// point) is read from `layout.json`, which the Fe compiler emitted from the SAME
// values it used to build the naga module. The bind-group layout is declared
// EXPLICITLY from that metadata, never via wgpu/Tint reflection. A different
// kernel is just different gen/ files + this same runner.

const WORD_STAGE_ALIGN = 4; // WebGPU copy size / buffer size must be a multiple of 4.

function align4(n) {
  return Math.ceil(n / WORD_STAGE_ALIGN) * WORD_STAGE_ALIGN;
}

// Read an unsigned integer of `width` bytes (little-endian) from a DataView.
function readWord(view, offset, width) {
  if (width === 4) return view.getUint32(offset, true);
  if (width === 8) return view.getBigUint64(offset, true); // not used in the u32 browser profile
  throw new Error(`unsupported result width ${width} (layout.result.width)`);
}

// Human-readable adapter identity, so the page can show the viewer THEIR GPU
// (proving a live device, not a fallback). Shapes vary across browsers.
async function adapterName(adapter) {
  let info = adapter.info;
  if (!info && typeof adapter.requestAdapterInfo === "function") {
    try {
      info = await adapter.requestAdapterInfo();
    } catch (_e) {
      info = null;
    }
  }
  if (!info) return "unknown adapter";
  const parts = [info.vendor, info.architecture, info.device, info.description]
    .filter((s) => s && String(s).length > 0);
  return parts.length ? parts.join(" / ") : "unnamed adapter";
}

// Run the compute kernel on WebGPU, building the entire pipeline from `layout`.
// Returns { ok: true, value, adapter } on a real readback, or
// { ok: false, reason, messages? } when WebGPU is absent, the device is
// unavailable, or the shader fails to compile (messages carry Tint's text).
export async function runWebGPU(wgslText, layout) {
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

  const name = await adapterName(adapter);

  let device;
  try {
    // BROWSER PROFILE: request no features. The Fe kernel is u32-only, exactly
    // the feature set a WebGPU browser exposes (no SHADER_INT64).
    device = await adapter.requestDevice();
  } catch (e) {
    return { ok: false, reason: `requestDevice() failed: ${e.message || e}`, adapter: name };
  }

  // Surface device-lost / uncaptured errors instead of letting them vanish.
  let deviceError = null;
  device.addEventListener?.("uncapturederror", (ev) => {
    deviceError = ev.error?.message || String(ev.error);
  });

  // --- Shader module + explicit compile check (Tint messages verbatim). ---
  const module = device.createShaderModule({ code: wgslText });
  if (typeof module.getCompilationInfo === "function") {
    const compInfo = await module.getCompilationInfo();
    const errs = compInfo.messages.filter((m) => m.type === "error");
    if (errs.length) {
      return {
        ok: false,
        reason: "WGSL shader compile error (Tint)",
        messages: errs.map((m) => `${m.lineNum}:${m.linePos} ${m.message}`),
        adapter: name,
      };
    }
  }

  // --- Bind-group layouts, built EXPLICITLY from layout.bindings. ----------
  // Group bindings by their `group` index; one GPUBindGroupLayout per group.
  const groups = new Map();
  for (const b of layout.bindings) {
    if (!groups.has(b.group)) groups.set(b.group, []);
    groups.get(b.group).push(b);
  }
  const maxGroup = Math.max(...layout.bindings.map((b) => b.group));

  const bindGroupLayouts = [];
  for (let g = 0; g <= maxGroup; g++) {
    const entries = (groups.get(g) || []).map((b) => ({
      binding: b.binding,
      visibility: GPUShaderStage.COMPUTE,
      buffer: {
        // access is the compiler-declared address space; never guessed.
        type: b.access === "ReadWrite" ? "storage" : "read-only-storage",
      },
    }));
    bindGroupLayouts.push(device.createBindGroupLayout({ entries }));
  }

  // --- One storage buffer per binding, sized from the metadata. ------------
  const result = layout.result;
  const isResultBinding = (b) => b.group === result.group && b.binding === result.binding;

  const buffers = new Map(); // key: `${group}:${binding}` -> GPUBuffer
  for (const b of layout.bindings) {
    const needed = isResultBinding(b)
      ? Math.max(b.stride, result.offset + result.width)
      : b.stride;
    const size = align4(Math.max(needed, WORD_STAGE_ALIGN));
    let usage = GPUBufferUsage.STORAGE;
    if (isResultBinding(b)) usage |= GPUBufferUsage.COPY_SRC;
    buffers.set(`${b.group}:${b.binding}`, device.createBuffer({ size, usage }));
  }

  const staging = device.createBuffer({
    size: align4(result.width),
    usage: GPUBufferUsage.MAP_READ | GPUBufferUsage.COPY_DST,
  });

  // --- Bind groups (one per group), pipeline, dispatch. --------------------
  const bindGroups = [];
  for (let g = 0; g <= maxGroup; g++) {
    const entries = (groups.get(g) || []).map((b) => ({
      binding: b.binding,
      resource: { buffer: buffers.get(`${b.group}:${b.binding}`) },
    }));
    bindGroups.push(device.createBindGroup({ layout: bindGroupLayouts[g], entries }));
  }

  const pipelineLayout = device.createPipelineLayout({ bindGroupLayouts });
  const pipeline = device.createComputePipeline({
    layout: pipelineLayout,
    compute: { module, entryPoint: layout.entry_point },
  });

  const encoder = device.createCommandEncoder();
  const pass = encoder.beginComputePass();
  pass.setPipeline(pipeline);
  for (let g = 0; g <= maxGroup; g++) pass.setBindGroup(g, bindGroups[g]);
  const [wx, wy, wz] = layout.workgroup_size;
  pass.dispatchWorkgroups(wx, wy, wz);
  pass.end();

  const resultBuffer = buffers.get(`${result.group}:${result.binding}`);
  encoder.copyBufferToBuffer(resultBuffer, result.offset, staging, 0, align4(result.width));
  device.queue.submit([encoder.finish()]);

  try {
    await staging.mapAsync(GPUMapMode.READ);
  } catch (e) {
    return { ok: false, reason: `readback map failed: ${e.message || e}`, adapter: name };
  }
  const copy = staging.getMappedRange().slice(0);
  staging.unmap();

  if (deviceError) {
    return { ok: false, reason: `device error: ${deviceError}`, adapter: name };
  }

  const view = new DataView(copy);
  const raw = readWord(view, 0, result.width);
  const value = typeof raw === "bigint" ? Number(raw) : raw >>> 0;
  return { ok: true, value, adapter: name };
}

// Run a GRID-mode kernel on WebGPU: one invocation per pixel, whole-output
// readback. This is the SAME kernel-blind runner - every shader-specific fact
// (bindings, group/binding indices, access, workgroup size, entry point, word
// width, broadcast params) comes from `layout`, never from Tint reflection. It
// differs from the scalar `runWebGPU` only in Grid mode's facts: the whole
// output array is the result (no `layout.result` slot), the caller supplies the
// 2D dispatch dims, and args 2.. of the kernel are broadcast params.
//
// The ~40-line device preamble is DUPLICATED from the scalar path by design: the
// reuse contract keeps `runWebGPU` bit-for-bit, so this path may not refactor it.
//
// `runWebGPUGrid(wgslText, layout, { width, height, params = {} })` returns
// `{ ok: true, grid: Uint32Array, adapter }` on a real readback, or
// `{ ok: false, reason, messages?, adapter? }` (fail-closed, never a fudged
// dispatch).
export async function runWebGPUGrid(wgslText, layout, { width, height, params = {} }) {
  // --- Fail-closed validation FIRST (never a fudged dispatch). -------------
  if (layout.mode !== "Grid") {
    return { ok: false, reason: `runWebGPUGrid requires layout.mode === "Grid"; got "${layout.mode}"` };
  }
  if (!Number.isInteger(width) || !Number.isInteger(height) || width <= 0 || height <= 0) {
    return { ok: false, reason: `width/height must be positive integers; got ${width} x ${height}` };
  }
  const [wgx, wgy, wgz] = layout.workgroup_size;
  if (width % wgx !== 0 || height % wgy !== 0) {
    return {
      ok: false,
      reason:
        `dispatch ${width} x ${height} must tile the workgroup ${wgx} x ${wgy} exactly ` +
        `(width % ${wgx} = ${width % wgx}, height % ${wgy} = ${height % wgy}); ` +
        `ragged edges are a named later extension`,
    };
  }
  if (wgz !== 1) {
    return { ok: false, reason: `grid dispatch is 2D; workgroup z must be 1, got ${wgz}` };
  }

  // --- Device preamble (duplicated from the scalar path, by contract). -----
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

  const name = await adapterName(adapter);

  let device;
  try {
    device = await adapter.requestDevice();
  } catch (e) {
    return { ok: false, reason: `requestDevice() failed: ${e.message || e}`, adapter: name };
  }

  let deviceError = null;
  device.addEventListener?.("uncapturederror", (ev) => {
    deviceError = ev.error?.message || String(ev.error);
  });

  const module = device.createShaderModule({ code: wgslText });
  if (typeof module.getCompilationInfo === "function") {
    const compInfo = await module.getCompilationInfo();
    const errs = compInfo.messages.filter((m) => m.type === "error");
    if (errs.length) {
      return {
        ok: false,
        reason: "WGSL shader compile error (Tint)",
        messages: errs.map((m) => `${m.lineNum}:${m.linePos} ${m.message}`),
        adapter: name,
      };
    }
  }

  // --- Bind-group layouts, built EXPLICITLY from layout.bindings. ----------
  const groups = new Map();
  for (const b of layout.bindings) {
    if (!groups.has(b.group)) groups.set(b.group, []);
    groups.get(b.group).push(b);
  }
  const maxGroup = Math.max(...layout.bindings.map((b) => b.group));

  const bindGroupLayouts = [];
  for (let g = 0; g <= maxGroup; g++) {
    const entries = (groups.get(g) || []).map((b) => ({
      binding: b.binding,
      visibility: GPUShaderStage.COMPUTE,
      buffer: { type: b.access === "ReadWrite" ? "storage" : "read-only-storage" },
    }));
    bindGroupLayouts.push(device.createBindGroupLayout({ entries }));
  }

  // --- Buffers, all sized from metadata + the caller's dims. ---------------
  const wordBytes = layout.word_bytes;
  const outputBinding = layout.bindings.find((b) => b.role === "Output");
  if (!outputBinding) {
    return { ok: false, reason: "grid layout has no Output binding", adapter: name };
  }
  const inputBinding = layout.bindings.find((b) => b.role === "Input");
  const hasParams = Array.isArray(layout.params) && layout.params.length > 0;
  const outSize = align4(width * height * wordBytes);

  const buffers = new Map();
  for (const b of layout.bindings) {
    let size;
    let usage = GPUBufferUsage.STORAGE;
    if (b.role === "Output") {
      size = outSize;
      usage |= GPUBufferUsage.COPY_SRC;
    } else {
      // Input (broadcast params) binding: sized by its stride (span), min 4.
      size = align4(Math.max(WORD_STAGE_ALIGN, b.stride));
      if (hasParams) usage |= GPUBufferUsage.COPY_DST;
    }
    buffers.set(`${b.group}:${b.binding}`, device.createBuffer({ size, usage }));
  }

  // --- Params marshaling, fully metadata-driven (empty table in M1). -------
  for (const p of layout.params || []) {
    if (!(p.name in params)) {
      return { ok: false, reason: `missing param \`${p.name}\` for the grid dispatch`, adapter: name };
    }
    if (!inputBinding) {
      return {
        ok: false,
        reason: `layout.params names \`${p.name}\` but there is no Input binding`,
        adapter: name,
      };
    }
    const buf = buffers.get(`${inputBinding.group}:${inputBinding.binding}`);
    const offset = p.offset ?? 0;
    if (!isShaderParamOffset(offset)) {
      return { ok: false, reason: `param ${p.name} has invalid aligned offset ${offset}`, adapter: name };
    }
    device.queue.writeBuffer(buf, offset, encodeShaderScalar(params[p.name], p));
  }

  // --- Bind groups, pipeline, 2D dispatch. ---------------------------------
  const bindGroups = [];
  for (let g = 0; g <= maxGroup; g++) {
    const entries = (groups.get(g) || []).map((b) => ({
      binding: b.binding,
      resource: { buffer: buffers.get(`${b.group}:${b.binding}`) },
    }));
    bindGroups.push(device.createBindGroup({ layout: bindGroupLayouts[g], entries }));
  }

  const pipelineLayout = device.createPipelineLayout({ bindGroupLayouts });
  const pipeline = device.createComputePipeline({
    layout: pipelineLayout,
    compute: { module, entryPoint: layout.entry_point },
  });

  const staging = device.createBuffer({
    size: outSize,
    usage: GPUBufferUsage.MAP_READ | GPUBufferUsage.COPY_DST,
  });

  const encoder = device.createCommandEncoder();
  const pass = encoder.beginComputePass();
  pass.setPipeline(pipeline);
  for (let g = 0; g <= maxGroup; g++) pass.setBindGroup(g, bindGroups[g]);
  pass.dispatchWorkgroups(width / wgx, height / wgy, 1);
  pass.end();

  const outputBuffer = buffers.get(`${outputBinding.group}:${outputBinding.binding}`);
  encoder.copyBufferToBuffer(outputBuffer, 0, staging, 0, outSize);
  device.queue.submit([encoder.finish()]);

  try {
    await staging.mapAsync(GPUMapMode.READ);
  } catch (e) {
    return { ok: false, reason: `readback map failed: ${e.message || e}`, adapter: name };
  }
  const copy = staging.getMappedRange().slice(0);
  staging.unmap();

  if (deviceError) {
    return { ok: false, reason: `device error: ${deviceError}`, adapter: name };
  }

  return { ok: true, grid: new Uint32Array(copy), adapter: name };
}

// ===========================================================================
// RENDER path (interactive mandelbrot I3). Still KERNEL-BLIND: every fact -
// the vertex/fragment entry names, the color-target format, the Input binding
// (group/binding/access/stride) and the view `params` (name + byte offset) -
// is read from the Render-mode `layout.json` the Fe compiler emitted. This path
// draws a fullscreen triangle whose fragment IS the Fe-compiled mandelbrot; the
// three view words (center_re, center_im, scale_q) are written to the Input
// buffer before each draw. Nothing here knows it is a mandelbrot.
//
// The scalar/grid device preamble is intentionally NOT refactored here (the
// reuse contract keeps `runWebGPU`/`runWebGPUGrid` bit-for-bit); this path owns
// its own preamble.
// ===========================================================================

// Build the render-mode bind-group layout / bind group / input buffer from
// `layout.bindings` (the single Input storage buffer, FRAGMENT-visible). Shared
// by the display and verify pipelines.
function buildRenderBindings(device, layout) {
  const inputBinding = layout.bindings.find((b) => b.role === "Input");
  if (!inputBinding) throw new Error("render layout has no Input binding");
  const bgl = device.createBindGroupLayout({
    entries: [
      {
        binding: inputBinding.binding,
        visibility: GPUShaderStage.FRAGMENT,
        buffer: { type: inputBinding.access === "ReadWrite" ? "storage" : "read-only-storage" },
      },
    ],
  });
  const inputBuf = device.createBuffer({
    size: align4(Math.max(WORD_STAGE_ALIGN, inputBinding.stride)),
    usage: GPUBufferUsage.STORAGE | GPUBufferUsage.COPY_DST,
  });
  const bindGroup = device.createBindGroup({
    layout: bgl,
    entries: [{ binding: inputBinding.binding, resource: { buffer: inputBuf } }],
  });
  return { bgl, inputBuf, bindGroup, inputBinding };
}

// initWebGPURender(wgslText, layout, canvas) - stand up the render pipeline
// ONCE. Pass `null` for a readback-only offscreen handle. Returns a handle on
// success, or { ok:false, reason, messages? }
// (fail-closed) when WebGPU is absent / the device is unavailable / the shader
// fails to compile. AMBER is the caller's job (this returns ok:false, no draw).
export async function initWebGPURender(wgslText, layout, canvas) {
  if (layout.mode !== "Render") {
    return { ok: false, reason: `initWebGPURender requires layout.mode === "Render"; got "${layout.mode}"` };
  }
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
  const name = await adapterName(adapter);

  let device;
  try {
    device = await adapter.requestDevice();
  } catch (e) {
    return { ok: false, reason: `requestDevice() failed: ${e.message || e}`, adapter: name };
  }
  let deviceError = null;
  let deviceLost = null;
  device.addEventListener?.("uncapturederror", (ev) => {
    deviceError = ev.error?.message || String(ev.error);
  });
  device.lost?.then((info) => {
    deviceLost = info?.message || info?.reason || "WebGPU device lost";
  });

  const module = device.createShaderModule({ code: wgslText });
  if (typeof module.getCompilationInfo === "function") {
    const compInfo = await module.getCompilationInfo();
    const errs = compInfo.messages.filter((m) => m.type === "error");
    if (errs.length) {
      return {
        ok: false,
        reason: "WGSL shader compile error (Tint)",
        messages: errs.map((m) => `${m.lineNum}:${m.linePos} ${m.message}`),
        adapter: name,
      };
    }
  }

  const { bgl, inputBuf, bindGroup } = buildRenderBindings(device, layout);
  const pipelineLayout = device.createPipelineLayout({ bindGroupLayouts: [bgl] });

  const makePipeline = (targetFormat) =>
    device.createRenderPipeline({
      layout: pipelineLayout,
      vertex: { module, entryPoint: layout.vertex_entry, buffers: [] },
      fragment: { module, entryPoint: layout.fragment_entry, targets: [{ format: targetFormat }] },
      primitive: { topology: "triangle-list" },
    });

  // The verify pipeline targets the compiler-stated offscreen format (rgba8unorm),
  // so a readback can be byte-compared against the Fe-wasm oracle.
  const verifyFormat = layout.color_target_format || "rgba8unorm";
  const verifyPipeline = makePipeline(verifyFormat);

  let ctx = null;
  let canvasFormat = null;
  let displayPipeline = null;
  if (canvas !== null) {
    if (!canvas || typeof canvas.getContext !== "function") {
      return { ok: false, reason: "canvas must be an HTML canvas or null for offscreen mode", adapter: name };
    }
    ctx = canvas.getContext("webgpu");
    if (!ctx) return { ok: false, reason: "canvas.getContext('webgpu') returned null", adapter: name };
    canvasFormat = navigator.gpu.getPreferredCanvasFormat();
    ctx.configure({ device, format: canvasFormat, alphaMode: "opaque" });
    displayPipeline = makePipeline(canvasFormat);
  }

  return {
    ok: true,
    adapter: name,
    device,
    queue: device.queue,
    ctx,
    canvasFormat,
    verifyFormat,
    displayPipeline,
    verifyPipeline,
    inputBuf,
    bindGroup,
    layout,
    width: layout.width,
    height: layout.height,
    presentation: canvas === null ? "offscreen" : "canvas",
    deviceError: () => deviceError,
    deviceLost: () => deviceLost,
  };
}

// Write the view words into the Input buffer at the compiler-stated offsets, then
// Encode one layout parameter for queue.writeBuffer. Scalar metadata selects the
// exact 32-bit representation; older layouts without it retain the I32 default.
export function encodeShaderScalar(value, param) {
  const scalar = param.scalar ?? "I32";
  if (param.width !== undefined && param.width !== 4) {
    throw new Error(`unsupported ${scalar} parameter width ${param.width}`);
  }
  if (scalar === "F32") return new Float32Array([Number(value)]);
  if (scalar === "U32") return new Uint32Array([Number(value) >>> 0]);
  if (scalar === "I32") return new Int32Array([Number(value) | 0]);
  throw new Error(`unsupported shader parameter scalar ${scalar}`);
}

export function isShaderParamOffset(offset) {
  return Number.isInteger(offset) && offset >= 0 && offset % 4 === 0;
}

export function validateShaderParamArity(params, values) {
  if (params.length !== values.length) {
    throw new Error(`layout names ${params.length} parameters but caller supplied ${values.length}`);
  }
}

function writeTypedParams(queue, inputBuf, params, values) {
  validateShaderParamArity(params, values);
  for (let i = 0; i < params.length; i++) {
    const offset = params[i].offset ?? 0;
    if (!isShaderParamOffset(offset)) {
      throw new Error(`shader parameter offset must be a non-negative aligned integer; got ${offset}`);
    }
    queue.writeBuffer(inputBuf, offset, encodeShaderScalar(values[i], params[i]));
  }
}

export function renderFrame(handle, viewWords) {
  const { device, queue, ctx, displayPipeline, inputBuf, bindGroup, layout } = handle;
  if (!ctx || !displayPipeline) {
    throw new Error("renderFrame requires a canvas-backed render handle; this handle is offscreen-only");
  }
  const params = layout.params || [];
  writeTypedParams(queue, inputBuf, params, viewWords);
  const view = ctx.getCurrentTexture().createView();
  const encoder = device.createCommandEncoder();
  const pass = encoder.beginRenderPass({
    colorAttachments: [
      { view, loadOp: "clear", storeOp: "store", clearValue: { r: 0, g: 0, b: 0, a: 1 } },
    ],
  });
  pass.setPipeline(displayPipeline);
  pass.setBindGroup(0, bindGroup);
  pass.draw(3, 1, 0, 0);
  pass.end();
  queue.submit([encoder.finish()]);
}

// Submit one offscreen frame without allocating a staging buffer or reading it
// back. This is the presentation-equivalent path for headless actor acceptance.
export function submitOffscreenFrame(handle, viewWords) {
  const { device, queue, verifyPipeline, inputBuf, bindGroup, layout, width, height } = handle;
  const params = layout.params || [];
  writeTypedParams(queue, inputBuf, params, viewWords);
  const texture = device.createTexture({
    size: { width, height },
    format: handle.verifyFormat || "rgba8unorm",
    usage: GPUTextureUsage.RENDER_ATTACHMENT,
  });
  const encoder = device.createCommandEncoder();
  const pass = encoder.beginRenderPass({
    colorAttachments: [{
      view: texture.createView(), loadOp: "clear", storeOp: "store",
      clearValue: { r: 0, g: 0, b: 0, a: 1 },
    }],
  });
  pass.setPipeline(verifyPipeline);
  pass.setBindGroup(0, bindGroup);
  pass.draw(3, 1, 0, 0);
  pass.end();
  queue.submit([encoder.finish()]);
  texture.destroy();
}

// verifyView(handle, viewWords) - render the SAME view to an offscreen rgba8unorm
// target and read it back TIGHTLY packed (row padding stripped). The badge path
// compares these bytes against the Fe-wasm fragment's output. Returns
// { ok:true, rgba:Uint8Array } or { ok:false, reason }.
export async function verifyView(handle, viewWords) {
  const { device, queue, verifyPipeline, inputBuf, bindGroup, layout, width, height } = handle;
  const w = width, h = height;
  const params = layout.params || [];
  writeTypedParams(queue, inputBuf, params, viewWords);

  const tex = device.createTexture({
    size: { width: w, height: h },
    format: handle.verifyFormat || "rgba8unorm",
    usage: GPUTextureUsage.RENDER_ATTACHMENT | GPUTextureUsage.COPY_SRC,
  });
  const bytesPerRow = Math.ceil((w * 4) / 256) * 256;
  const staging = device.createBuffer({
    size: bytesPerRow * h,
    usage: GPUBufferUsage.MAP_READ | GPUBufferUsage.COPY_DST,
  });

  const encoder = device.createCommandEncoder();
  const pass = encoder.beginRenderPass({
    colorAttachments: [
      {
        view: tex.createView(),
        loadOp: "clear",
        storeOp: "store",
        clearValue: { r: 0, g: 0, b: 0, a: 1 },
      },
    ],
  });
  pass.setPipeline(verifyPipeline);
  pass.setBindGroup(0, bindGroup);
  pass.draw(3, 1, 0, 0);
  pass.end();
  encoder.copyTextureToBuffer(
    { texture: tex },
    { buffer: staging, bytesPerRow, rowsPerImage: h },
    { width: w, height: h }
  );
  queue.submit([encoder.finish()]);

  try {
    await staging.mapAsync(GPUMapMode.READ);
  } catch (e) {
    staging.destroy();
    tex.destroy();
    return { ok: false, reason: `verify readback map failed: ${e.message || e}` };
  }
  const data = new Uint8Array(staging.getMappedRange().slice(0));
  staging.unmap();
  staging.destroy();
  tex.destroy();
  const asynchronousError = handle.deviceError?.() || handle.deviceLost?.();
  if (asynchronousError) {
    return { ok: false, reason: `WebGPU device error after readback: ${asynchronousError}` };
  }
  const row = w * 4;
  const out = new Uint8Array(row * h);
  for (let y = 0; y < h; y++) out.set(data.subarray(y * bytesPerRow, y * bytesPerRow + row), y * row);
  return { ok: true, rgba: out };
}
