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
    device.queue.writeBuffer(buf, p.offset || 0, new Uint32Array([params[p.name] >>> 0]));
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
