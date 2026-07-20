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
