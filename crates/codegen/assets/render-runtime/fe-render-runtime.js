// fe render runtime (compiler-emitted, protocol fe-web-bundle v4-v10).
//
// The ONE fixed, versioned, demo-blind WebGPU/wasm render kernel driver
// shipped by the Fe toolchain. It defines the `<fe-surface>` custom element
// (FE_WEB_V5_ORCHESTRATION_DESIGN.md section 3): a fe-web-bundle manifest in,
// a live web component out. The element reads a manifest (v4, or v5 which
// additionally carries each uniform member's source field name/doc comment
// and a `surface` section projected from the actor's `view()`) and drives the
// two lowerings of the render kernel the compiler produced from the SAME
// source. v4/v5 bundles carry one GPU shader plus a CPU fallback. v6 also
// carries ordered GPU pass graphs and shared typed resources; those graphs are
// intentionally GPU-only and report WebGPU failures without changing programs:
//   - the GPU lane reads shaders, passes, resources, and layouts from the
//     manifest;
//   - legacy bundles may fall back to module.wasm per pixel in a 2D canvas.
// v7 adds authenticated immutable resource artifacts and replays them after
// device replacement.
// v8 adds compiler-derived minimal buffer usages and pass-stage visibility;
// missing physical capability data fails closed for newly emitted bundles.
// v9 adds Fe-authored parameter presentation plans. v10 narrows every WebGPU
// binding to the exact compiler-derived shader stages that can reach it.
// Uniform controls are generated from the manifest's input binding members.
//
// One shared WebGPU adapter/device serves every surface mounted on a page,
// so a gallery of N demos (N `<fe-surface>` elements) costs one device, not
// N, and a `device.lost` event on that shared device is recovered ONCE and
// every attached element rebuilds from its own held state (section 6).
//
// This module is the ONLY copy of the render kernel's browser glue. The
// legacy `fe web build --mode render` bundle (its emitted index.html imports
// `mountRenderSurface`, a thin compatibility wrapper around the element,
// preserved below), the standards `application/fe` `data-fe-render` handoff
// (crates/html-precompile/assets/bootstrap.js, which now inserts a
// `<fe-surface>` element instead of calling `mountRenderSurface`
// imperatively), and authored `<fe-surface src=...>` pages (rewritten by the
// precompiler to `<fe-surface manifest=...>`, crates/html-precompile) all
// import this SAME module and drive the SAME element. One mount path.

const DEFAULT_SIZE = 256; // dispatch/canvas size for a v4 manifest with no declared `surface.extent`.
const SURFACE_EVENT_STRIDE = 52;
const MAX_SURFACE_EVENT_BATCH = Math.floor(0x7fffffff / SURFACE_EVENT_STRIDE);
// Legacy CPU-only artifacts receive one fixed implementation ceiling. Typed
// `SurfaceQuality<P>` artifacts own all responsive/coarse-pointer policy in Fe.
const CPU_MAX_DIMENSION = 256;
const GPU_BYTES_PER_ROW_ALIGNMENT = 256;

/** Mechanically realize compiler-derived physical buffer capabilities.
 * Missing data retains the broad v6/v7 compatibility allocation; every v8
 * compiler bundle carries an explicit minimal set. */
export function resourceBufferUsage(
  resource,
  constants = globalThis.GPUBufferUsage,
  protocolVersion = 7,
) {
  if (resource.buffer_usage === undefined && protocolVersion >= 8) {
    throw new Error("fe render runtime: v8 resource is missing compiler-derived buffer_usage");
  }
  const capabilities = resource.buffer_usage ?? ["storage", "copy_dst", "copy_src"];
  if (!Array.isArray(capabilities) || capabilities.length === 0) {
    throw new Error("fe render runtime: resource buffer_usage must be a non-empty array");
  }
  if (!constants) {
    throw new Error("fe render runtime: GPUBufferUsage constants are unavailable");
  }
  let usage = 0;
  const seen = new Set();
  for (const capability of capabilities) {
    if (seen.has(capability)) {
      throw new Error(`fe render runtime: duplicate resource buffer usage ${capability}`);
    }
    seen.add(capability);
    switch (capability) {
      case "storage":
        usage |= constants.STORAGE;
        break;
      case "copy_src":
        usage |= constants.COPY_SRC;
        break;
      case "copy_dst":
        usage |= constants.COPY_DST;
        break;
      default:
        throw new Error(`fe render runtime: unsupported resource buffer usage ${capability}`);
    }
  }
  return usage;
}

function shaderStagesVisibility(stages, constants, subject) {
  if (!Array.isArray(stages) || stages.length === 0 || !constants) {
    throw new Error(`fe render runtime: invalid ${subject} shader_stages`);
  }
  let visibility = 0;
  const seen = new Set();
  for (const stage of stages) {
    if (seen.has(stage)) {
      throw new Error(`fe render runtime: duplicate ${subject} shader stage ${stage}`);
    }
    seen.add(stage);
    switch (stage) {
      case "compute":
        visibility |= constants.COMPUTE;
        break;
      case "vertex":
        visibility |= constants.VERTEX;
        break;
      case "fragment":
        visibility |= constants.FRAGMENT;
        break;
      default:
        throw new Error(`fe render runtime: unsupported ${subject} shader stage ${stage}`);
    }
  }
  return visibility;
}

/** Mechanically realize compiler-derived pass stage demand. */
export function passShaderVisibility(
  pass,
  constants = globalThis.GPUShaderStage,
  protocolVersion = 7,
) {
  let stages = pass.shader_stages;
  if ((!Array.isArray(stages) || stages.length === 0) && protocolVersion >= 8) {
    throw new Error("fe render runtime: v8 pass is missing compiler-derived shader_stages");
  }
  stages ??= pass.layout.mode === "compute"
    ? ["compute"]
    : pass.draw_vertices
      ? ["vertex", "fragment"]
      : ["fragment"];
  return shaderStagesVisibility(stages, constants, "pass");
}

/** Realize the exact compiler-derived visibility of one physical binding. */
export function bindingShaderVisibility(
  binding,
  pass,
  constants = globalThis.GPUShaderStage,
  protocolVersion = 9,
) {
  const stages = binding.shader_stages;
  if ((!Array.isArray(stages) || stages.length === 0) && protocolVersion >= 10) {
    throw new Error("fe render runtime: v10 binding is missing compiler-derived shader_stages");
  }
  if (!Array.isArray(stages) || stages.length === 0) {
    return passShaderVisibility(pass, constants, protocolVersion);
  }
  if (protocolVersion >= 8) {
    passShaderVisibility(pass, constants, protocolVersion);
    for (const stage of stages) {
      if (!pass.shader_stages.includes(stage)) {
        throw new Error(
          `fe render runtime: binding shader stage ${stage} is outside its pass stage set`,
        );
      }
    }
  }
  return shaderStagesVisibility(stages, constants, "binding");
}

/** Preserve aspect while enforcing an implementation resource ceiling. */
export function fitBackingExtent(width, height, maxDimension = Infinity) {
  if (!Number.isFinite(maxDimension)) return { width, height };
  const scale = Math.min(1, maxDimension / Math.max(width, height));
  return {
    width: Math.max(1, Math.round(width * scale)),
    height: Math.max(1, Math.round(height * scale)),
  };
}

function hasCoarsePointer() {
  return globalThis.matchMedia?.("(pointer: coarse)")?.matches === true;
}

/** Decode one tightly specified WebGPU canvas readback into browser RGBA
 * pixels. Canvas rows are padded to WebGPU's copy alignment; the copy is
 * made while the presented texture is still owned by our submission, so it
 * does not depend on a browser retaining compositor contents for a later
 * `createImageBitmap(canvas)` call. */
export function unpackCanvasReadback(bytes, width, height, bytesPerRow, format) {
  if (format !== "rgba8unorm" && format !== "rgba8unorm-srgb" &&
      format !== "bgra8unorm" && format !== "bgra8unorm-srgb") {
    throw new Error(`fe render runtime: poster readback does not support canvas format ${format}`);
  }
  const rowBytes = width * 4;
  if (bytesPerRow < rowBytes || bytes.length < bytesPerRow * height) {
    throw new Error("fe render runtime: poster readback buffer is shorter than its declared extent");
  }
  const rgba = new Uint8ClampedArray(rowBytes * height);
  const bgra = format.startsWith("bgra");
  for (let y = 0; y < height; y++) {
    const sourceRow = y * bytesPerRow;
    const destinationRow = y * rowBytes;
    for (let x = 0; x < width; x++) {
      const source = sourceRow + x * 4;
      const destination = destinationRow + x * 4;
      rgba[destination] = bytes[source + (bgra ? 2 : 0)];
      rgba[destination + 1] = bytes[source + 1];
      rgba[destination + 2] = bytes[source + (bgra ? 0 : 2)];
      rgba[destination + 3] = bytes[source + 3];
    }
  }
  return rgba;
}

function encodeCanvasReadback(device, encoder, texture, width, height, format) {
  const bytesPerRow = Math.ceil((width * 4) / GPU_BYTES_PER_ROW_ALIGNMENT) *
    GPU_BYTES_PER_ROW_ALIGNMENT;
  const buffer = createGpuBuffer(device, {
    size: bytesPerRow * height,
    usage: GPUBufferUsage.COPY_DST | GPUBufferUsage.MAP_READ,
  });
  encoder.copyTextureToBuffer(
    { texture },
    { buffer, bytesPerRow, rowsPerImage: height },
    { width, height, depthOrArrayLayers: 1 },
  );
  return { buffer, width, height, bytesPerRow, format };
}

async function readCanvasReadback(readback) {
  const { buffer, width, height, bytesPerRow, format } = readback;
  try {
    await buffer.mapAsync(GPUMapMode.READ);
    return unpackCanvasReadback(
      new Uint8Array(buffer.getMappedRange()),
      width,
      height,
      bytesPerRow,
      format,
    );
  } finally {
    try {
      buffer.unmap();
    } catch {
      // A failed map or device loss can leave the buffer unmapped already.
    }
    buffer.destroy();
  }
}

/** Encode one standards WebGPU buffer copy without interpreting its contents.
 * The compiler-selected binding and exact physical extent are resolved before
 * this helper is called. Message meaning remains in the receiving Fe type. */
function encodeGpuBufferReadback(device, encoder, source, byteLength) {
  const buffer = createGpuBuffer(device, {
    size: byteLength,
    usage: GPUBufferUsage.COPY_DST | GPUBufferUsage.MAP_READ,
  });
  encoder.copyBufferToBuffer(source, 0, buffer, 0, byteLength);
  return { buffer, byteLength };
}

/** Materialize an owned snapshot from WebGPU's mapped-range Web IDL view.
 * The copy is required because `unmap()` invalidates the returned ArrayBuffer. */
export async function readGpuBufferSnapshot(readback) {
  const { buffer, byteLength } = readback;
  try {
    await buffer.mapAsync(GPUMapMode.READ);
    return new Uint8Array(buffer.getMappedRange(0, byteLength)).slice();
  } finally {
    try {
      buffer.unmap();
    } catch {
      // A failed map or device loss can leave the buffer unmapped already.
    }
    buffer.destroy();
  }
}

function destroyGpuBuffers(buffers) {
  for (const buffer of new Set(buffers)) {
    try {
      buffer?.destroy();
    } catch {
      // Device loss may already have invalidated the allocation.
    }
  }
}

export function rasterDrawShape(pass) {
  if (pass.draw_instances !== undefined && pass.draw_vertices === undefined) {
    throw new Error("fe render runtime: compiler-derived instances require an authored raster draw");
  }
  const vertices = pass.draw_vertices ?? 3;
  const instances = pass.draw_instances ?? 1;
  if (!Number.isSafeInteger(vertices) || vertices <= 0) {
    throw new Error(`fe render runtime: invalid compiler-derived raster vertex count ${vertices}`);
  }
  if (!Number.isSafeInteger(instances) || instances <= 0) {
    throw new Error(`fe render runtime: invalid compiler-derived raster instance count ${instances}`);
  }
  return { vertices, instances };
}

export function requiresGpuPassGraph(passes, resources = []) {
  return resources.length > 0 || passes.some(
    (pass) => pass.layout.mode === "compute" || pass.draw_vertices !== undefined,
  );
}

/** Fixed tags of the append-only Fe `SurfaceEventKind` enum. These are browser
 * facts, not application policy; the generated Wasm wrapper validates the tag
 * before authored Fe can observe it. */
export const SurfaceEventKind = Object.freeze({
  Gesture: 0,
  ParamEdit: 1,
  AnimationFrame: 2,
  GpuComplete: 3,
  Visible: 4,
  Hidden: 5,
  DeviceLost: 6,
  DeviceRecovered: 7,
  PointerDown: 8,
  PointerMove: 9,
  PointerUp: 10,
});

/** Fixed tags of Fe's bounded raw-queue effect. The selected resident policy
 * produces them; this adapter only realizes the corresponding queue operation. */
export const SurfaceQueueAction = Object.freeze({
  Retain: 0,
  KeepLatest: 1,
  Drop: 2,
});

/** Fixed tags of Fe's device-recovery effect. Every canonical surface policy
 * returns one; this host only aggregates shared retry demand and realizes the
 * selected per-surface terminal action. */
export const SurfaceRecoveryAction = Object.freeze({
  NoAction: 0,
  RetryDevice: 1,
  DegradeToWasm: 2,
  FailSurface: 3,
});

/** Fixed tags of `std::web::gpu_device_events::GpuDeviceEventKind`.
 * `Unknown` is an Fe-side placeholder and is never published by this host. */
export const GpuDeviceEventKind = Object.freeze({
  Unknown: 0,
  Available: 1,
  Lost: 2,
  Unavailable: 3,
});

/** Fixed tags of `std::web::gpu_device_events::GpuDeviceLossReason`.
 * The implementation-defined `GPUDeviceLostInfo.message` is deliberately not
 * transported or parsed as application policy. */
export const GpuDeviceLossReason = Object.freeze({
  NotLost: 0,
  Unknown: 1,
  Destroyed: 2,
});

const GPU_DEVICE_EVENT_HISTORY = 32;
const GPU_QUEUE_IDLE_HISTORY = 64;
const MAX_U32 = 0xffff_ffff;

function checkedU32(value, label) {
  if (!Number.isInteger(value) || value < 0 || value > MAX_U32) {
    throw new TypeError(`${label} must be a u32`);
  }
  return value;
}

function createBoundedGpuFactChannel(capacity, labels, project) {
  if (!Number.isSafeInteger(capacity) || capacity <= 0) {
    throw new TypeError(`${labels.history} history capacity must be positive`);
  }
  const history = [];
  const waiters = new Set();
  let sequence = 0;

  const nextAfter = (seen, previousSequence) => {
    if (history.length === 0) return null;
    if (!seen) return Object.freeze({ ...history[history.length - 1], missed: 0 });
    const earliest = history[0].sequence;
    const selected = history.find(event => event.sequence > previousSequence);
    if (!selected) return null;
    const missed = previousSequence + 1 < earliest ? earliest - previousSequence - 1 : 0;
    return Object.freeze({ ...selected, missed });
  };

  const flush = () => {
    for (const waiter of [...waiters]) {
      const event = nextAfter(waiter.seen, waiter.previousSequence);
      if (event) waiter.finish(waiter.resolve, event);
    }
  };

  const publish = (...values) => {
    const projected = project(...values);
    if (sequence === MAX_U32) {
      throw new RangeError(`${labels.history} sequence space is exhausted`);
    }
    sequence += 1;
    const event = Object.freeze({ ...projected, sequence });
    history.push(event);
    if (history.length > capacity) history.shift();
    flush();
    return event;
  };

  const observe = (seen, previousSequence, signal) => {
    if (typeof seen !== "boolean") {
      throw new TypeError(`${labels.observation} observation seen flag must be boolean`);
    }
    checkedU32(previousSequence, `${labels.previous} previous sequence`);
    if (seen && previousSequence > sequence) {
      throw new RangeError(`${labels.previous} previous sequence is newer than the host channel`);
    }
    const immediate = nextAfter(seen, previousSequence);
    if (immediate) return immediate;
    return new Promise((resolve, reject) => {
      let settled = false;
      const waiter = {
        seen,
        previousSequence,
        resolve,
        finish(complete, value) {
          if (settled) return;
          settled = true;
          waiters.delete(waiter);
          signal?.removeEventListener("abort", onAbort);
          complete(value);
        },
      };
      const onAbort = () => {
        const error = new Error(`${labels.cancellation} observation was cancelled`);
        error.name = "AbortError";
        waiter.finish(reject, error);
      };
      waiters.add(waiter);
      signal?.addEventListener("abort", onAbort, { once: true });
      if (signal?.aborted) onAbort();
      else flush(); // close the observe-to-register race
    });
  };

  return Object.freeze({ publish, observe });
}

/**
 * A bounded, replayable page-wide lifecycle channel. This is fixed host
 * transport, not a recovery scheduler: it preserves every retained device
 * fact in sequence order and reports any history gap to Fe. The consuming Fe
 * task owns retry, degradation, and failure policy.
 */
export function createGpuDeviceLifecycleChannel(capacity = GPU_DEVICE_EVENT_HISTORY) {
  return createBoundedGpuFactChannel(
    capacity,
    {
      history: "GPU device lifecycle",
      observation: "GPU device",
      previous: "GPU device",
      cancellation: "GPU device lifecycle",
    },
    (kind, reason, generation) => {
      checkedU32(kind, "GPU device event kind");
      checkedU32(reason, "GPU device loss reason");
      checkedU32(generation, "GPU device generation");
      if (kind === GpuDeviceEventKind.Unknown) {
        throw new RangeError("the host cannot publish the Fe placeholder GPU device event");
      }
      return { kind, reason, generation };
    },
  );
}

/**
 * Bounded transport for queue-idle facts already observed by the fixed render
 * host. An occurrence means all work submitted before the corresponding
 * `onSubmittedWorkDone()`/readback boundary completed. It does not expose raw
 * queue objects or invent application submission IDs; Fe owns all downstream
 * backpressure and scheduling policy.
 */
export function createGpuQueueIdleChannel(capacity = GPU_QUEUE_IDLE_HISTORY) {
  return createBoundedGpuFactChannel(
    capacity,
    {
      history: "GPU queue-idle",
      observation: "GPU queue-idle",
      previous: "GPU queue-idle",
      cancellation: "GPU queue-idle",
    },
    generation => {
      checkedU32(generation, "GPU queue-idle generation");
      return { generation };
    },
  );
}

/** Write the fixed DFS layout of untouched `std::web::SurfaceEvent` records.
 * This is transport only: the compiler-lowered Fe scheduling wrapper owns
 * coalescing and calls the authored transition inside Wasm. */
export function writeSurfaceEventBatch(memory, pointer, events) {
  const view = new DataView(memory.buffer);
  events.forEach((event, index) => {
    const base = pointer + index * SURFACE_EVENT_STRIDE;
    view.setFloat32(base, event.mx, true);
    view.setFloat32(base + 4, event.my, true);
    view.setFloat32(base + 8, event.dx, true);
    view.setFloat32(base + 12, event.dy, true);
    view.setFloat32(base + 16, event.wheelDelta, true);
    view.setUint32(base + 20, event.wheelMode, true);
    view.setUint32(base + 24, event.buttons, true);
    view.setFloat32(base + 28, event.timestamp, true);
    view.setFloat32(base + 32, event.width, true);
    view.setFloat32(base + 36, event.height, true);
    view.setUint32(base + 40, event.eventKind, true);
    view.setUint32(base + 44, event.paramIndex, true);
    view.setFloat32(base + 48, event.paramValue, true);
  });
}

// ---------------------------------------------------------------------------
// Shared WebGPU device: one adapter/device for the whole page, requested at
// most once, with `device.lost` recovery broadcast to every attached surface.
// ---------------------------------------------------------------------------

let sharedGpuPromise;
let sharedGpuFailure;
let sharedGpuRecoveryPromise;
let pendingDeviceLoss;
let sharedGpuGeneration = 0;
const sharedGpuDeviceLifecycle = createGpuDeviceLifecycleChannel();
const sharedGpuQueueIdle = createGpuQueueIdleChannel();
const DEVICE_STABILITY_MS = 50;
const DEVICE_LOSS_CONFIRMATION_MS = 250;
/** Every currently connected `<fe-surface>`, live or not (module-level so a
 * `device.lost` event, which is a page-wide fact, can reach every element). */
const attachedSurfaces = new Set();

/** Observe the actual shared render-device lifecycle. The bootstrap passes
 * this fixed capability to Fe's `GpuDeviceEvent` source; it does not subscribe
 * to a second WebGPU device and cannot choose recovery policy. */
export function observeSharedGpuDevice(seen, previousSequence, signal) {
  return sharedGpuDeviceLifecycle.observe(seen, previousSequence, signal);
}

/** Observe queue-idle facts from the actual shared render device. */
export function observeSharedGpuQueueIdle(seen, previousSequence, signal) {
  return sharedGpuQueueIdle.observe(seen, previousSequence, signal);
}

function publishSharedGpuQueueIdle(gpu) {
  return sharedGpuQueueIdle.publish(gpu?.generation ?? sharedGpuGeneration);
}

let generatedWebGpuOperations;

/** Install the compiler-assembled official-WebIDL transport used by this fixed
 * host. Fe owns resource and scheduling semantics; these generated operations
 * perform only browser standards calls. */
export function installGeneratedWebGpuOperations(operations) {
  if (!operations || typeof operations !== "object" ||
      typeof operations.queueIdle !== "function" ||
      typeof operations.bufferCreate !== "function" ||
      typeof operations.bufferWrite !== "function" ||
      typeof operations.renderDraw !== "function" ||
      typeof operations.renderDrawIndirect !== "function") {
    throw new TypeError("fe render runtime: generated WebGPU operations are incomplete");
  }
  if (generatedWebGpuOperations !== undefined) {
    throw new Error("fe render runtime: generated WebGPU operations are already installed");
  }
  generatedWebGpuOperations = Object.freeze(operations);
}

async function awaitSharedGpuQueueIdle(gpu) {
  if (generatedWebGpuOperations === undefined) {
    throw new Error("fe render runtime: generated WebGPU operations are unavailable");
  }
  await generatedWebGpuOperations.queueIdle(gpu.device.queue);
  return publishSharedGpuQueueIdle(gpu);
}

function createGpuBuffer(device, descriptor) {
  if (generatedWebGpuOperations === undefined) {
    throw new Error("fe render runtime: generated WebGPU operations are unavailable");
  }
  return generatedWebGpuOperations.bufferCreate(device, descriptor);
}

function writeGpuBuffer(queue, buffer, offset, bytes) {
  if (generatedWebGpuOperations === undefined) {
    throw new Error("fe render runtime: generated WebGPU operations are unavailable");
  }
  if (!(bytes instanceof Uint8Array)) {
    throw new TypeError("fe render runtime: GPU buffer writes require a byte view");
  }
  generatedWebGpuOperations.bufferWrite(queue, buffer, offset, bytes);
}

function drawGpu(renderPass, vertexCount, instanceCount) {
  if (generatedWebGpuOperations === undefined) {
    throw new Error("fe render runtime: generated WebGPU operations are unavailable");
  }
  generatedWebGpuOperations.renderDraw(renderPass, vertexCount, instanceCount);
}

function drawGpuIndirect(renderPass, buffer, offset) {
  if (generatedWebGpuOperations === undefined) {
    throw new Error("fe render runtime: generated WebGPU operations are unavailable");
  }
  generatedWebGpuOperations.renderDrawIndirect(renderPass, buffer, offset);
}

function publishGpuUnavailable(reason = GpuDeviceLossReason.NotLost) {
  return sharedGpuDeviceLifecycle.publish(
    GpuDeviceEventKind.Unavailable,
    reason,
    sharedGpuGeneration,
  );
}

/** One WebGPU adapter/device for the whole page, requested at most once. */
function acquireSharedGpu() {
  if (sharedGpuPromise === undefined) {
    sharedGpuPromise = requestGpu();
  }
  return sharedGpuPromise;
}

async function requestGpu() {
  sharedGpuFailure = null;
  if (!window.isSecureContext) {
    sharedGpuFailure = new Error(
      "fe render runtime: WebGPU requires a secure context; serve this page over HTTPS or localhost",
    );
    publishGpuUnavailable();
    return null;
  }
  if (!navigator.gpu) {
    sharedGpuFailure = new Error(
      "fe render runtime: this browser does not expose WebGPU (navigator.gpu is unavailable)",
    );
    publishGpuUnavailable();
    return null;
  }
  try {
    const adapter = await navigator.gpu.requestAdapter();
    if (!adapter) {
      sharedGpuFailure = new Error(
        "fe render runtime: no WebGPU adapter is available (requestAdapter returned null). " +
        "Check browser WebGPU support, hardware acceleration, the GPU blocklist, and chrome://gpu on Chromium.",
      );
      publishGpuUnavailable();
      return null;
    }
    const device = await adapter.requestDevice();
    // Some backends resolve `requestDevice()` and then immediately report
    // that their native instance disappeared. Do not hand that already-dead
    // device to every surface or recursively reacquire it forever.
    const initialState = await Promise.race([
      device.lost.then((info) => ({ lost: true, info })),
      new Promise((resolve) => setTimeout(() => resolve({ lost: false }), DEVICE_STABILITY_MS)),
    ]);
    if (initialState.lost) {
      const detail = initialState.info?.message || "the device was lost during initialization";
      sharedGpuFailure = new Error(
        `fe render runtime: WebGPU device initialization failed: ${detail}`,
      );
      publishGpuUnavailable(
        initialState.info?.reason === "destroyed"
          ? GpuDeviceLossReason.Destroyed
          : GpuDeviceLossReason.Unknown,
      );
      return null;
    }
    device.addEventListener("uncapturederror", (event) => {
      console.error("[fe web] uncaptured WebGPU error:", event.error);
    });
    device.lost.then((info) => handleSharedDeviceLoss(device, info));
    if (sharedGpuGeneration === MAX_U32) {
      throw new RangeError("WebGPU shared-device generation space is exhausted");
    }
    sharedGpuGeneration += 1;
    sharedGpuDeviceLifecycle.publish(
      GpuDeviceEventKind.Available,
      GpuDeviceLossReason.NotLost,
      sharedGpuGeneration,
    );
    return { adapter, device, generation: sharedGpuGeneration };
  } catch (error) {
    sharedGpuFailure = new Error(
      `fe render runtime: WebGPU initialization failed: ${error?.message ?? String(error)}`,
      { cause: error },
    );
    console.warn("[fe web] WebGPU initialization failed:", error);
    publishGpuUnavailable();
    return null;
  }
}

/**
 * `device.lost` recovery (FE_WEB_V5_ORCHESTRATION_DESIGN.md section 6): drop
 * the stale shared promise, re-request an adapter/device ONCE, and let every
 * attached element rebuild its own pipeline/bind group and re-render from its
 * own held params. A poster-only element holds no device-scoped resources (the
 * whole point of releasing the GPU context at "ready"), so recovery is a
 * cheap no-op for it; only elements that are actually `live` do real work.
 */
function handleSharedDeviceLoss(deadDevice, info) {
  pendingDeviceLoss = { deadDevice, info };
  if (sharedGpuRecoveryPromise === undefined) {
    sharedGpuRecoveryPromise = drainDeviceLosses().finally(() => {
      sharedGpuRecoveryPromise = undefined;
      if (pendingDeviceLoss) {
        const pending = pendingDeviceLoss;
        pendingDeviceLoss = undefined;
        handleSharedDeviceLoss(pending.deadDevice, pending.info);
      }
    });
  }
  return sharedGpuRecoveryPromise;
}

async function drainDeviceLosses() {
  while (pendingDeviceLoss) {
    const { deadDevice, info } = pendingDeviceLoss;
    pendingDeviceLoss = undefined;
    console.warn(`[fe web] WebGPU device lost (${info?.reason ?? "unknown"}): ${info?.message ?? ""}`);
    const current = sharedGpuPromise ? await sharedGpuPromise.catch(() => null) : null;
    if (current && current.device !== deadDevice) continue;
    const reason = info?.reason === "destroyed"
      ? GpuDeviceLossReason.Destroyed
      : GpuDeviceLossReason.Unknown;
    sharedGpuDeviceLifecycle.publish(
      GpuDeviceEventKind.Lost,
      reason,
      current?.generation ?? sharedGpuGeneration,
    );
    // Never leave the dead device reachable while Fe decides whether there is
    // page-wide replacement demand. Retry requests are aggregated below; the
    // host does not retain an independent attempt budget.
    sharedGpuPromise = Promise.resolve(null);
    await coordinateSurfaceRecovery(
      [...attachedSurfaces],
      reason,
      current?.generation ?? sharedGpuGeneration,
      async () => {
        sharedGpuPromise = undefined;
        return acquireSharedGpu();
      },
    );
  }
}

/**
 * Ask every affected surface's resident Fe policy what to do, aggregate all
 * `RetryDevice` decisions into exactly one shared acquisition per round, and
 * realize terminal decisions independently. Repeated rounds exist only while
 * at least one Fe policy requests them; this host owns no retry count.
 *
 * Exported for a deterministic fixed-host oracle. Production passes the
 * actual connected surfaces and `acquireSharedGpu` realization.
 */
export async function coordinateSurfaceRecovery(
  surfaces,
  reason,
  generation,
  acquire,
) {
  let retrying = [];
  const passive = [];
  for (const surface of surfaces) {
    try {
      const action = surface._beginDeviceLoss(reason, generation);
      if (action === SurfaceRecoveryAction.RetryDevice) retrying.push(surface);
      else {
        await surface._realizeDeviceRecovery(action, reason, generation);
        // A poster-only/cold surface may not need the replacement device now,
        // but its Fe supervision state still observes `Available` if another
        // surface causes the shared device to be reacquired.
        if (action === SurfaceRecoveryAction.NoAction) passive.push(surface);
      }
    } catch (error) {
      surface._fail(error);
    }
  }

  while (retrying.length > 0) {
    const fresh = await acquire();
    if (fresh) {
      for (const surface of [...retrying, ...passive]) {
        try {
          await surface._completeDeviceRecovery(fresh, generation);
        } catch (error) {
          surface._fail(error);
        }
      }
      return fresh;
    }

    const next = [];
    for (const surface of retrying) {
      try {
        const action = surface._continueDeviceRecovery(reason, generation);
        if (action === SurfaceRecoveryAction.RetryDevice) next.push(surface);
        else await surface._realizeDeviceRecovery(action, reason, generation);
      } catch (error) {
        surface._fail(error);
      }
    }
    retrying = next;
  }
  return null;
}

/** A failed WebGPU operation is not evidence of device loss by itself. Wait a
 * short, bounded interval for the platform's authoritative `device.lost`
 * signal before attempting a replacement device. Validation and shader errors
 * therefore remain ordinary visible failures instead of being misclassified
 * as recoverable device loss. */
async function confirmedDeviceLoss(device) {
  return Promise.race([
    device.lost.then((info) => ({ lost: true, info })),
    new Promise((resolve) => {
      setTimeout(
        () => resolve({ lost: false, info: null }),
        DEVICE_LOSS_CONFIRMATION_MS,
      );
    }),
  ]);
}

// ---------------------------------------------------------------------------
// Manifest-derived helpers shared by the element and by `mountRenderSurface`.
// ---------------------------------------------------------------------------

async function fetchOrThrow(url, label) {
  const response = await fetch(url);
  if (!response.ok) {
    throw new Error(`fe render runtime: could not fetch ${label} (${url}): ${response.status}`);
  }
  return response;
}

function byteHex(bytes) {
  return Array.from(bytes, byte => byte.toString(16).padStart(2, "0")).join("");
}

/** Fetch and authenticate one compiler-declared immutable resource artifact.
 * Zeroed and GPU-derived resources return null without touching the network.
 * The same helper runs on every pass-graph rebuild, so device replacement
 * replays the exact logical bytes rather than retaining stale GPU handles. */
export async function fetchVerifiedResourceArtifact(
  resource,
  manifestUrl,
  { fetchImpl = globalThis.fetch, cryptoImpl = globalThis.crypto } = {},
) {
  const initialization = resource.policy?.initialization ?? { kind: "zeroed" };
  if (initialization.kind !== "content_addressed") {
    if (resource.artifact) {
      throw new Error(
        `fe render runtime: resource \`${resource.name}\` declares an artifact without content-addressed initialization`,
      );
    }
    return null;
  }
  const artifact = resource.artifact;
  if (!artifact) {
    throw new Error(
      `fe render runtime: resource \`${resource.name}\` has no content-addressed artifact`,
    );
  }
  const expectedBytes = resource.stride * resource.length;
  if (!Number.isSafeInteger(expectedBytes) || expectedBytes <= 0 || artifact.bytes !== expectedBytes) {
    throw new Error(
      `fe render runtime: resource \`${resource.name}\` artifact length disagrees with its layout`,
    );
  }
  if (initialization.sha256 !== artifact.sha256 || !/^[0-9a-f]{64}$/.test(artifact.sha256)) {
    throw new Error(
      `fe render runtime: resource \`${resource.name}\` has inconsistent SHA-256 identity`,
    );
  }
  if (typeof fetchImpl !== "function" || !cryptoImpl?.subtle) {
    throw new Error("fe render runtime: resource verification capabilities are unavailable");
  }
  const url = new URL(artifact.path, manifestUrl);
  const response = await fetchImpl(url);
  if (!response.ok) {
    throw new Error(
      `fe render runtime: could not fetch resource \`${resource.name}\` (${url}): ${response.status}`,
    );
  }
  const bytes = new Uint8Array(await response.arrayBuffer());
  if (bytes.byteLength !== artifact.bytes) {
    throw new Error(
      `fe render runtime: resource \`${resource.name}\` fetched ${bytes.byteLength} bytes, expected ${artifact.bytes}`,
    );
  }
  const digest = byteHex(new Uint8Array(await cryptoImpl.subtle.digest("SHA-256", bytes)));
  if (digest !== artifact.sha256) {
    throw new Error(
      `fe render runtime: resource \`${resource.name}\` failed SHA-256 verification`,
    );
  }
  return bytes;
}

function resolveCanvas(canvasOption) {
  if (!canvasOption) return null;
  if (typeof canvasOption === "string") return document.querySelector(canvasOption);
  return canvasOption;
}

/** Resolve one Fe-authored parameter plan. Only legacy v4-v8 manifests infer
 * the plan from `kind`; protocol v9 requires the explicit CTFE projection. */
export function surfaceParamPlan(param, protocolVersion) {
  if (protocolVersion < 9) {
    return {
      source: param.kind === "extent_x"
        ? "surface_width"
        : param.kind === "extent_y" ? "surface_height" : "initial",
      widget: param.visible === false
        ? "hidden"
        : param.kind === "toggle" ? "checkbox" : "range",
      scale: param.kind === "log" ? "logarithmic" : "linear",
      readout: param.kind === "int"
        ? "integer"
        : param.kind === "toggle" ? "toggle" : "scalar",
    };
  }
  const source = param.source;
  const presentation = param.presentation;
  if (!presentation ||
      !["initial", "surface_width", "surface_height"].includes(source) ||
      !["hidden", "range", "checkbox", "select"].includes(presentation.widget) ||
      !["linear", "logarithmic"].includes(presentation.scale) ||
      !["scalar", "integer", "toggle"].includes(presentation.readout)) {
    throw new Error("fe render runtime: protocol v9 param is missing a supported Fe presentation plan");
  }
  if ((presentation.widget === "hidden") !== (param.visible === false)) {
    throw new Error("fe render runtime: Fe param visibility disagrees with its presentation widget");
  }
  if (presentation.widget === "checkbox" &&
      (presentation.scale !== "linear" || presentation.readout !== "toggle")) {
    throw new Error("fe render runtime: checkbox presentation requires linear toggle semantics");
  }
  if (presentation.widget === "range" && presentation.readout === "toggle") {
    throw new Error("fe render runtime: range presentation cannot use a toggle readout");
  }
  if (presentation.widget === "select" &&
      (presentation.scale !== "linear" || presentation.readout !== "integer")) {
    throw new Error("fe render runtime: select presentation requires linear integer semantics");
  }
  const options = presentation.options ?? [];
  if (!Array.isArray(options) || options.some(label => typeof label !== "string" || label.length === 0)) {
    throw new Error("fe render runtime: Fe param options must be non-empty strings");
  }
  if (presentation.widget === "select") {
    const min = Math.ceil(param.min);
    const max = Math.floor(param.max);
    if (min !== 0 || options.length !== max + 1) {
      throw new Error("fe render runtime: select options disagree with Fe ordinal bounds");
    }
  } else if (options.length !== 0) {
    throw new Error("fe render runtime: only select presentations may carry options");
  }
  return { source, ...presentation };
}

/** Aggregate the unique shader payload behind a pass graph. The compatibility
 * `artifacts.wgsl_bytes` field names only the primary pass, so it is not a
 * bundle total once a surface contains multiple GPU stages. */
export function wgslPayloadSummary(manifest) {
  const passes = Array.isArray(manifest?.passes) ? manifest.passes : [];
  if (passes.length === 0) {
    const bytes = manifest?.artifacts?.wgsl_bytes;
    return {
      bytes: Number.isSafeInteger(bytes) && bytes >= 0 ? bytes : 0,
      shaders: manifest?.artifacts?.wgsl ? 1 : 0,
    };
  }
  const shaders = new Map();
  for (const pass of passes) {
    if (typeof pass?.shader !== "string" ||
        !Number.isSafeInteger(pass.shader_bytes) || pass.shader_bytes < 0) {
      throw new Error("fe render runtime: pass graph has invalid WGSL payload metadata");
    }
    const previous = shaders.get(pass.shader);
    if (previous !== undefined && previous !== pass.shader_bytes) {
      throw new Error(
        `fe render runtime: WGSL payload \`${pass.shader}\` has conflicting byte lengths`,
      );
    }
    shaders.set(pass.shader, pass.shader_bytes);
  }
  return {
    bytes: [...shaders.values()].reduce((sum, bytes) => sum + bytes, 0),
    shaders: shaders.size,
  };
}

/** The initial uniform vector from the declared surface. Protocol v9 consumes
 * the explicit Fe value source; earlier versions retain their isolated
 * compatibility interpretation. */
function surfaceInitialUniforms(members, surface, width, height, protocolVersion) {
  const byName = new Map(surface.params.map((param) => [param.name, param]));
  return members.map((member) => {
    const param = byName.get(member.name);
    if (!param) return 0;
    const plan = surfaceParamPlan(param, protocolVersion);
    if (plan.source === "surface_width") return width;
    if (plan.source === "surface_height") return height;
    return typeof param.init === "number" ? param.init : 0;
  });
}

/**
 * A bundle with no `surface` section (no declared `view()`): every uniform
 * member is held at a fixed, honest default (1.0, not a guess and not
 * searched). This is the visible pressure the v5 migration posture calls for
 * (FE_WEB_V5_ORCHESTRATION_DESIGN.md 4.2): an undeclared view stays visibly
 * undeclared rather than silently guessing one.
 */
function undeclaredViewInitialUniforms(members) {
  return members.map(() => 1);
}

/** Overwrite only the extent-bound members of `uniforms` (leaving every other
 * live/user-adjusted value untouched); used on mount AND on every resize. */
function withExtentUniforms(members, surface, uniforms, width, height, protocolVersion) {
  if (!surface) return uniforms;
  const byName = new Map(surface.params.map((param) => [param.name, param]));
  const next = uniforms.slice();
  members.forEach((member, index) => {
    const param = byName.get(member.name);
    if (!param) return;
    const plan = surfaceParamPlan(param, protocolVersion);
    if (plan.source === "surface_width") next[index] = width;
    else if (plan.source === "surface_height") next[index] = height;
  });
  return next;
}

function writeUniformBuffer(device, uniformBuffer, span, members, uniforms) {
  const buffer = new ArrayBuffer(Math.max(16, span));
  const view = new DataView(buffer);
  members.forEach((member, index) => {
    const value = uniforms[index] ?? 0;
    if (member.scalar === "f32") view.setFloat32(member.offset, value, true);
    else if (member.scalar === "u32") view.setUint32(member.offset, value >>> 0, true);
    else view.setInt32(member.offset, value | 0, true);
  });
  writeGpuBuffer(device.queue, uniformBuffer, 0, new Uint8Array(buffer));
}

/** Validate and translate the compiler-derived Fe raster policy into the
 * corresponding WebGPU descriptor vocabulary. No application state or
 * rendering choice is made here. */
function attachmentOps(policy, label) {
  if (!policy || !["clear", "load"].includes(policy.first_load) ||
      !["clear", "load"].includes(policy.following_load) ||
      !["store", "discard"].includes(policy.store)) {
    throw new Error(`fe render runtime: invalid derived ${label} attachment operations`);
  }
  return {
    firstLoad: policy.first_load,
    followingLoad: policy.following_load,
    store: policy.store,
  };
}

function rasterPlan(surface) {
  const policy = surface?.pipeline?.raster;
  if (!policy) {
    return {
      sampleCount: 1,
      cullMode: "none",
      color: {
        clearValue: { r: 0, g: 0, b: 0, a: 1 },
        firstLoad: "clear",
        followingLoad: "load",
        store: "store",
      },
      depth: null,
    };
  }
  if (policy.sample_count !== 1 && policy.sample_count !== 4) {
    throw new Error("fe render runtime: invalid derived raster sample count");
  }
  if (!["none", "front", "back"].includes(policy.cull_mode)) {
    throw new Error("fe render runtime: invalid derived raster cull mode");
  }
  const clear = policy.color?.clear;
  if (!clear || ![clear.r, clear.g, clear.b, clear.a].every(Number.isFinite)) {
    throw new Error("fe render runtime: invalid derived color clear value");
  }
  const color = {
    clearValue: { r: clear.r, g: clear.g, b: clear.b, a: clear.a },
    ...attachmentOps(policy.color.ops, "color"),
  };
  const format = {
    depth24_plus: "depth24plus",
    depth32_float: "depth32float",
  }[policy.depth?.format];
  const compare = {
    less: "less",
    less_equal: "less-equal",
    greater: "greater",
    greater_equal: "greater-equal",
    always: "always",
  }[policy.depth?.compare];
  const depth = policy.depth
    ? {
        format,
        compare,
        writeEnabled: policy.depth.write_enabled === true,
        clearValue: policy.depth.clear,
        ...attachmentOps(policy.depth.ops, "depth"),
      }
    : null;
  if (depth && (
    !depth.format || !depth.compare || !Number.isFinite(depth.clearValue) ||
    depth.clearValue < 0 || depth.clearValue > 1
  )) {
    throw new Error("fe render runtime: invalid derived depth policy");
  }
  return { sampleCount: policy.sample_count, cullMode: policy.cull_mode, color, depth };
}

function releaseRasterAttachments(gpu) {
  for (const texture of [gpu?.multisampleTexture, gpu?.depthTexture]) {
    try { texture?.destroy(); } catch { /* device loss already released it */ }
  }
  if (!gpu) return;
  gpu.multisampleTexture = null;
  gpu.depthTexture = null;
  gpu.attachmentWidth = 0;
  gpu.attachmentHeight = 0;
}

function ensureRasterAttachments(gpu, width, height) {
  const { raster } = gpu;
  if (
    gpu.attachmentWidth === width && gpu.attachmentHeight === height &&
    (raster.sampleCount === 1 || gpu.multisampleTexture) &&
    (!raster.depth || gpu.depthTexture)
  ) return;
  releaseRasterAttachments(gpu);
  const common = {
    size: { width, height, depthOrArrayLayers: 1 },
    sampleCount: raster.sampleCount,
    usage: GPUTextureUsage.RENDER_ATTACHMENT,
  };
  if (raster.sampleCount === 4) {
    gpu.multisampleTexture = gpu.device.createTexture({ ...common, format: gpu.format });
  }
  if (raster.depth) {
    gpu.depthTexture = gpu.device.createTexture({ ...common, format: raster.depth.format });
  }
  gpu.attachmentWidth = width;
  gpu.attachmentHeight = height;
}

function presentFrame(device, context, pipeline, bindGroup, capture) {
  const encoder = device.createCommandEncoder();
  const texture = context.getCurrentTexture();
  const pass = encoder.beginRenderPass({
    colorAttachments: [
      {
        view: texture.createView(),
        clearValue: { r: 0, g: 0, b: 0, a: 1 },
        loadOp: "clear",
        storeOp: "store",
      },
    ],
  });
  pass.setPipeline(pipeline);
  if (bindGroup) pass.setBindGroup(0, bindGroup);
  drawGpu(pass, 3, 1);
  pass.end();
  const readback = capture
    ? encodeCanvasReadback(device, encoder, texture, capture.width, capture.height, capture.format)
    : null;
  device.queue.submit([encoder.finish()]);
  return readback;
}

function deepFreeze(value) {
  if (value && typeof value === "object" && !Object.isFrozen(value)) {
    Object.freeze(value);
    for (const key of Object.keys(value)) deepFreeze(value[key]);
  }
  return value;
}

// ---------------------------------------------------------------------------
// Shadow DOM: one fixed stylesheet, `part=` on every restylable node so pages
// restyle with `fe-surface::part(canvas)` etc. instead of piercing `!important`
// (FE_WEB_V5_ORCHESTRATION_DESIGN.md 3.2).
// ---------------------------------------------------------------------------

const SHADOW_CSS = `
:host { display: block;
        width: min(var(--fe-surface-max-size, 768px),
                   calc(100vw - var(--fe-surface-window-inline-margin, 32px)),
                   calc(100vh - var(--fe-surface-window-block-margin, 112px)));
        max-width: 100%; margin-inline: auto;
        font: 14px/1.5 ui-monospace, SFMono-Regular, Menlo, monospace;
        color: #cfd6e4; }
.root { position: relative; display: flex; flex-direction: column; gap: 10px; }
.stage { position: relative; width: 100%; background: #000; border-radius: 10px; overflow: hidden;
         box-shadow: 0 8px 40px #0008; }
.surface-canvas { display: block; width: 100%; height: auto; }
.surface-canvas[hidden] { display: none; }
.side { display: grid; gap: 10px; }
.side:not([open]) > :not(summary) { display: none; }
.side summary { list-style: none; }
.side summary::-webkit-details-marker { display: none; }
.side summary::before { content: "▸ "; color: #6f7889; }
.side[open] summary::before { content: "▾ "; }
.badge { justify-self: start; display: inline-block; padding: 2px 7px; border-radius: 6px;
         font-size: 11px; font-weight: 600; }
.badge.webgpu { background: #10281a; color: #5bffa0; }
.badge.wasm-2d { background: #1a2030; color: #8fb0ff; }
.badge.error { background: #35151a; color: #ff9da8; }
.panel { display: grid; gap: 12px; }
.control { display: grid; gap: 4px; }
.control label { display: flex; justify-content: space-between; color: #96a0b5; }
.control b { color: #cfd6e4; font-weight: 600; }
.control input[type=range] { width: 100%; accent-color: #5b8cff; }
.control select { width: 100%; color: #cfd6e4; background: #151923; border: 1px solid #333a4b;
  border-radius: 5px; padding: 5px 7px; font: inherit; }
.control.notice { color: #d9a441; font-size: 12px; padding: 6px 8px; border: 1px dashed #4a3a1a;
                   border-radius: 6px; background: #221a0c; }
.meta { font-size: 12px; color: #6b7688; }
.meta a { color: inherit; text-decoration: underline dotted; }
.caption ::slotted(*) { font-size: 12.5px; color: #aeb8cc; }
`;

/**
 * `<fe-surface manifest="..." state="auto|preview|live|frozen" controls="auto|none">`
 *
 * The one custom element that turns any fe-web-bundle manifest into a live
 * surface. Identity is DATA (the manifest URL); this class is machinery, not
 * per-demo glue (FE_WEB_V5_ORCHESTRATION_DESIGN.md 3.1).
 *
 * Lifecycle (section 6): cold -> ready (fetch manifest, instantiate, render
 * ONE frame at the declared initial state, capture an ImageBitmap poster,
 * release the GPU context) -> live on declared intent (`surface.activate`) ->
 * suspended off-viewport -> device.lost recovery (re-acquire the shared
 * device, rebuild the pipeline, re-render from held params).
 *
 * `.state` additionally reports `"error"` when boot fails (fetch/compile/
 * instantiate); this is not one of the FSM states section 6 names, but is a
 * natural, harmless fifth value for observability (an `fe-error` event also
 * fires, so nothing depends on polling `.state` to notice a failure).
 */
export class FeSurfaceElement extends HTMLElement {
  static get observedAttributes() {
    return ["manifest", "data-fe-scoped-tasks", "state", "controls", "width", "height"];
  }

  constructor() {
    super();
    this._shadow = this.attachShadow({ mode: "open" });
    this._fsm = "cold";
    this._mode = null;
    this._booted = false;
    this._adoptedCanvas = null;
    this._uniforms = [];
    this._members = [];
    this._memberIndexByName = new Map();
    this._controlRows = [];
    this._manifest = null;
    this._actor = null;
    this._scopedTaskBroker = null;
    this._scopedTaskMachines = null;
    this._scopedTaskLifetime = null;
    this._scopedTasksNeedReboot = false;
    this._surface = null;
    this._control = null; // R3 param gestures: the projected `control` manifest section.
    this._controlKernel = null; // the resolved wasm control export, or null (no gestures).
    this._surfaceTransitionKernel = null; // typed Fe SurfaceEvent ABI, discovered from Wasm.
    this._surfaceTransitionSchedule = "immediate";
    this._surfaceTransitionStateResident = false;
    this._surfaceStateReplaceKernel = null;
    this._surfaceScheduleKernel = null;
    this._surfaceRecoveryKernel = null;
    this._surfaceQualityKernel = null;
    this._surfaceTransitionMemory = null;
    this._surfaceTransitionAlloc = null;
    this._wasmArenaReset = null;
    this._gpuReadbackKernel = null;
    this._gpuReadbackBinding = null;
    this._gpuReadbackResource = null;
    this._pendingSurfaceEvents = [];
    this._passes = [];
    this._resources = [];
    this._graph = false;
    this._posterAttemptedDevice = null;
    this._posterRecoveryActive = false;
    this._recoveryObservedLoss = false;
    this._recoveryWasLive = false;
    this._gestureListeners = null; // { canvas, onPointerDown, onPointerMove, onPointerUp, onWheel }
    this._gestureFrame = null;
    this._surfaceFrameRequested = false;
    this._gesturePresenting = false;
    this._gestureDirty = false;
    this._presentationTail = Promise.resolve();
    this._gpu = null; // one legacy pipeline, or { passRecords, resourceBuffers } for a graph.
    this._surfaceInitializerKernel = null;
    this._liveContext = null; // GPUCanvasContext on `_liveCanvas`
    this._adoptedContext = null; // GPUCanvasContext on `_adoptedCanvas`
    this._resizeObserver = null;
    this._resizePending = false;
    this._resolveReady = null;
    this._rejectReady = null;
    this._resolveLive = null;
    this._readyPromise = new Promise((resolve, reject) => {
      this._resolveReady = resolve;
      this._rejectReady = reject;
    });
    // Declarative surfaces have no imperative consumer awaiting this private
    // promise. Observe rejection without changing the original promise that
    // `mountRenderSurface` callers await.
    this._readyPromise.catch(() => {});
    this._livePromise = new Promise((resolve) => {
      this._resolveLive = resolve;
    });
    // `mountRenderSurface` legacy-compatibility overrides (section 3.3),
    // never part of the element's own attribute contract.
    this._initialOverride = undefined;
    this._gpuOverride = undefined;
    this._buildChrome();
  }

  // -- public contract (FE_WEB_V5_ORCHESTRATION_DESIGN.md 3.2) --------------

  /** A live object keyed by param NAME; `el.params.lambda = 0.3` re-renders. */
  get params() {
    return this._paramsProxy ?? (this._paramsProxy = this._buildParamsProxy());
  }

  get state() {
    return this._fsm;
  }

  /** `"webgpu" | "wasm-2d"` (`"wasm-mesh"` joins with the mesh pipeline rungs). */
  get mode() {
    return this._mode;
  }

  /** The parsed, frozen manifest (`null` before `fe-ready`). */
  get manifest() {
    return this._manifest;
  }

  /** Start a manually booted surface and wait until its poster is ready. */
  async load() {
    if (!this._booted) {
      this._booted = true;
      this._bootSurface();
    }
    await this._readyPromise;
  }

  /** Force a transition to `live`, booting first when declared manual. */
  async live() {
    if (this._fsm === "cold") await this.load();
    await this._goLive();
  }

  /** Capture the current frame as the poster, release GPU presentation, and
   * stay there until `.live()` is called again (unlike `suspended`, which
   * re-activates automatically when the surface re-enters the viewport). */
  async freeze() {
    if (this._fsm === "cold") await this._readyPromise.catch(() => {});
    if (this._fsm !== "live") return;
    await this._capturePosterFromLive();
    this._unwireResizeObserver();
    this._fsm = "frozen";
    this._dispatch("fe-statechange", { state: "frozen" });
  }

  /** Send one compiler-derived typed message to this surface's resident actor.
   * Lane admission, request/result validation, placement, Worker ownership,
   * cancellation, and transfer policy all come from the generated interface;
   * this fixed host neither assigns numeric IDs nor knows demo message shapes. */
  async post(lane, payload, { generation = 0, signal } = {}) {
    if (typeof lane !== "string" || lane.length === 0) {
      throw new TypeError("fe-surface.post requires a canonical lane name");
    }
    if (!Number.isSafeInteger(generation) || generation < 0) {
      throw new TypeError("fe-surface.post generation must be a non-negative safe integer");
    }
    if (!this._actor) await this._readyPromise;
    if (this._manifest && !this._manifest.canonical_interface) {
      throw new Error("fe-surface: this Fe program declares no browser message lanes");
    }
    if (!this._actor) {
      throw new Error("fe-surface: canonical browser actor did not initialize");
    }
    return this._actor.request(lane, payload, generation, { signal });
  }

  /** Adopt an existing canvas element instead of generating one (the
   * `data-fe-canvas` compatibility path). Must be called before the element
   * connects; not part of the base attribute contract, kept for the legacy
   * script-tag handoff and `mountRenderSurface`. */
  adoptCanvas(canvas) {
    if (this._booted) {
      throw new Error("fe-surface: adoptCanvas must be called before the element connects");
    }
    this._adoptedCanvas = canvas ?? null;
  }

  // -- custom element lifecycle ----------------------------------------------

  connectedCallback() {
    attachedSurfaces.add(this);
    if (this._booted) {
      if (this._scopedTasksNeedReboot) {
        this._scopedTasksNeedReboot = false;
        this._bootSurface();
        return;
      }
      this._startScopedTasks();
      if (this._fsm === "live") {
        this._wireSuspendObserver();
        this._wireResizeObserver();
      }
      return;
    }
    if (this.getAttribute("boot") !== "manual") this.load();
  }

  disconnectedCallback() {
    const hadScopedTasks = this._scopedTaskMachines !== null;
    this._stopScopedTasks();
    this._scopedTasksNeedReboot = hadScopedTasks;
    this._suspendObserver?.disconnect();
    this._activationObserver?.disconnect();
    this._resizeObserver?.disconnect();
    this._resizeObserver = null;
    attachedSurfaces.delete(this);
  }

  attributeChangedCallback(name, oldValue, newValue) {
    if (oldValue === newValue) return;
    if (name === "width" || name === "height") {
      if (newValue == null) this.style.removeProperty(name);
      else this.style[name] = /^\d+$/.test(newValue) ? `${newValue}px` : newValue;
      return;
    }
    if (name === "manifest" || name === "data-fe-scoped-tasks") {
      if (this.isConnected && (this._booted || this.getAttribute("boot") !== "manual")) {
        this._booted = true;
        this._bootSurface();
      }
      return;
    }
    if (name === "controls") {
      if (this._fsm !== "cold") this._renderControls();
      return;
    }
    if (name === "state" && this._fsm !== "cold") {
      this._applyStatePolicy();
    }
  }

  // -- boot: cold -> ready ----------------------------------------------------

  async _bootSurface() {
    this._teardown();
    this._fsm = "cold";
    this._panel.replaceChildren();
    this._controlRows = [];
    const manifestAttr = this.getAttribute("manifest");
    if (!manifestAttr) {
      this._fail(new Error("fe-surface: `manifest` attribute is required"));
      return;
    }
    try {
      const manifestUrl = new URL(manifestAttr, this.baseURI);
      const manifest = await (await fetchOrThrow(manifestUrl, "manifest")).json();
      if (manifest.protocol !== "fe-web-bundle" || ![4, 5, 6, 7, 8, 9, 10].includes(manifest.protocol_version)) {
        throw new Error(
          `fe render runtime: unsupported manifest protocol ${manifest.protocol}@${manifest.protocol_version}`,
        );
      }
      this._manifestUrl = manifestUrl;
      this._manifest = deepFreeze(manifest);
      this._passes = manifest.passes?.length
        ? manifest.passes
        : [{ source_entry: manifest.source_entry, shader: manifest.artifacts.wgsl, layout: manifest.layout }];
      this._resources = manifest.resources || [];
      this._graph = requiresGpuPassGraph(this._passes, this._resources);
      const fragmentPass = [...this._passes].reverse().find((pass) => pass.layout.mode === "render");
      this._renderPass = fragmentPass ?? null;
      this._layout = fragmentPass?.layout ?? manifest.layout;
      this._surface = manifest.surface || null;
      this._surfaceParamIndexByName = new Map(
        (this._surface?.params || []).map((param, index) => [param.name, index]),
      );
      this._control = manifest.control || null;
      const inputBinding = this._layout.bindings.find((binding) => binding.role === "input");
      this._inputBinding = inputBinding ?? null;
      this._members = inputBinding ? inputBinding.members : [];
      this._memberIndexByName = new Map(this._members.map((member, index) => [member.name, index]));
      this._memberIndexByArg = new Map(this._members.map((member, index) => [member.arg_index, index]));
      this._builtins = this._layout.builtin_inputs || [];
      this._argumentCount =
        1 +
        Math.max(-1, ...this._builtins.map((b) => b.arg_index), ...this._members.map((m) => m.arg_index));

      this._wasmUrl = manifest.artifacts.wasm
        ? new URL(manifest.artifacts.wasm, manifestUrl)
        : null;
      this._wgslUrl = new URL(manifest.artifacts.wgsl, manifestUrl);
      this._passShaderUrls = this._passes.map((pass) => new URL(pass.shader, manifestUrl));
      this._kernel = null;
      let instance = null;
      if (this._wasmUrl) {
        const wasmBytes = await (await fetchOrThrow(this._wasmUrl, "wasm module")).arrayBuffer();
        const wasmModule = await WebAssembly.compile(wasmBytes);
        const scopedTasks = await this._prepareScopedTasks(wasmModule);
        instance = await WebAssembly.instantiate(wasmModule, scopedTasks?.imports ?? {});
        this._attachScopedTasks(scopedTasks, instance);
        this._kernel = instance.exports[manifest.source_entry];
        if (!this._graph && typeof this._kernel !== "function") {
          throw new Error(`fe render runtime: wasm export \`${manifest.source_entry}\` not found`);
        }
        if (this._graph && typeof this._kernel !== "function") this._kernel = null;
        await this._bootCanonicalActor(wasmBytes);
      } else if (!this._graph) {
        throw new Error("fe render runtime: bundle has neither a Wasm fallback nor a GPU pass graph");
      } else if (this.hasAttribute("data-fe-scoped-tasks")) {
        throw new Error("fe render runtime: scoped Fe tasks require a Wasm parent artifact");
      } else if (manifest.canonical_interface) {
        throw new Error("fe render runtime: canonical browser messages require a Wasm artifact");
      }
      // Optional GPU actor state comes from one Fe-authored `InitialState`
      // behavior behind a fixed compiler-owned export. The host neither knows
      // its source name nor reconstructs its computation from manifest data.
      this._surfaceInitializerKernel = instance?.exports.fe_surface_initialize_v1 ?? null;
      if (this._surfaceInitializerKernel !== null &&
          typeof this._surfaceInitializerKernel !== "function") {
        throw new Error("fe render runtime: surface initializer export is not callable");
      }
      // R3 param gestures: the SAME wasm instance carries the control export
      // (already part of the root set the compiler emitted `module.wasm`
      // with). No control block, or an export it doesn't actually find,
      // means gestures stay off -- never a JS reimplementation fallback.
      this._controlKernel = null;
      this._surfaceTransitionMemory = null;
      this._surfaceTransitionAlloc = null;
      this._wasmArenaReset = null;
      this._surfaceTransitionStateResident = false;
      this._surfaceStateReplaceKernel = null;
      this._surfaceScheduleKernel = null;
      this._surfaceRecoveryKernel = null;
      this._surfaceQualityKernel = null;
      this._gpuReadbackKernel = null;
      this._gpuReadbackBinding = null;
      this._gpuReadbackResource = null;
      const arenaReset = instance?.exports.fe_cabi_reset;
      if (arenaReset !== undefined && typeof arenaReset !== "function") {
        throw new Error("fe render runtime: canonical arena reset export is not callable");
      }
      this._wasmArenaReset = arenaReset ?? null;
      const residentScheduled =
        instance?.exports.fe_surface_transition_scheduled_v1 ??
        instance?.exports.fe_surface_transition_latest_per_frame_v4 ?? null;
      const scheduled = residentScheduled ??
        instance?.exports.fe_surface_transition_latest_per_frame_v2 ?? null;
      this._surfaceTransitionKernel = typeof scheduled === "function"
        ? scheduled
        : instance?.exports.fe_surface_transition_v2 ?? null;
      this._surfaceTransitionSchedule = typeof scheduled === "function"
        ? "resident"
        : "immediate";
      if (typeof this._surfaceTransitionKernel !== "function") {
        this._surfaceTransitionKernel = null;
        this._surfaceTransitionSchedule = "immediate";
      }
      const gpuReadback = instance?.exports.fe_gpu_readback_transition_v1;
      const gpuReadbackBinding = instance?.exports.fe_gpu_readback_binding_v1;
      if ((gpuReadback === undefined) !== (gpuReadbackBinding === undefined)) {
        throw new Error(
          "fe render runtime: typed GPU readback requires paired transition and binding exports",
        );
      }
      if (gpuReadback !== undefined) {
        if (typeof gpuReadback !== "function" || typeof gpuReadbackBinding !== "function") {
          throw new Error("fe render runtime: typed GPU readback exports are not callable");
        }
        const binding = gpuReadbackBinding();
        if (!Number.isSafeInteger(binding) || binding < 0 || binding > 0xffffffff) {
          throw new Error("fe render runtime: typed GPU readback returned an invalid binding");
        }
        const resource = this._resources.find(
          candidate => candidate.group === 0 && candidate.binding === binding,
        );
        if (!resource) {
          throw new Error(
            `fe render runtime: typed GPU readback binding ${binding} has no physical resource`,
          );
        }
        const byteLength = resource.stride * resource.length;
        if (
          !Number.isSafeInteger(byteLength) || byteLength < 4 ||
          byteLength > 0x7fffffff || byteLength % 4 !== 0
        ) {
          throw new Error("fe render runtime: typed GPU readback resource has an invalid extent");
        }
        this._gpuReadbackKernel = gpuReadback;
        this._gpuReadbackBinding = binding;
        this._gpuReadbackResource = { ...resource, byteLength };
      }
      this._surfaceTransitionStateResident =
        typeof residentScheduled === "function" || this._gpuReadbackKernel !== null;
      if (this._surfaceTransitionStateResident) {
        const replaceState = instance?.exports.fe_surface_state_replace_v1;
        if (typeof replaceState !== "function") {
          throw new Error(
            "fe render runtime: resident actor is missing its fixed state replacement export",
          );
        }
        this._surfaceStateReplaceKernel = replaceState;
      }
      if (this._surfaceTransitionSchedule === "resident" || this._gpuReadbackKernel) {
        const memory = instance?.exports.memory;
        const alloc = instance?.exports.fe_cabi_alloc;
        if (
          !(memory instanceof WebAssembly.Memory) ||
          typeof alloc !== "function" ||
          typeof this._wasmArenaReset !== "function"
        ) {
          throw new Error(
            "fe render runtime: resident byte transport is missing fixed memory/allocator/reset exports",
          );
        }
        this._surfaceTransitionMemory = memory;
        this._surfaceTransitionAlloc = alloc;
      }
      const surfaceScheduleV2 = instance?.exports.fe_surface_schedule_v2;
      const surfaceSchedule = surfaceScheduleV2 ?? instance?.exports.fe_surface_schedule_v1;
      this._surfaceScheduleKernel = typeof surfaceSchedule === "function"
        ? surfaceSchedule
        : null;
      this._surfaceScheduleHasQueueAction = typeof surfaceScheduleV2 === "function";
      const surfaceRecovery = instance?.exports.fe_surface_recovery_v1;
      if (surfaceRecovery !== undefined && typeof surfaceRecovery !== "function") {
        throw new Error("fe render runtime: surface recovery export is not callable");
      }
      this._surfaceRecoveryKernel = surfaceRecovery ?? null;
      if (
        this._surfaceTransitionSchedule === "resident" &&
        !this._surfaceScheduleKernel
      ) {
        throw new Error(
          "fe render runtime: scheduled surface transition is missing its resident Fe scheduling policy export",
        );
      }
      const surfaceQuality = instance?.exports.fe_surface_quality_v1;
      if (surfaceQuality !== undefined && typeof surfaceQuality !== "function") {
        throw new Error("fe render runtime: surface quality export is not callable");
      }
      this._surfaceQualityKernel = surfaceQuality ?? null;
      if (this._control) {
        const controlFn = instance?.exports[this._control.export];
        if (typeof controlFn === "function") {
          this._controlKernel = controlFn;
        } else {
          console.warn(
            `[fe web] fe-surface: control export \`${this._control.export}\` not found; gestures disabled`,
          );
        }
      }

      const authoredInitial = this._surfaceInitializerKernel
        ? this._runWasmArenaEpoch(() => this._surfaceInitializerKernel())
        : undefined;
      this._uniforms = this._initialOverride ??
        (authoredInitial === undefined
          ? (this._surface
            ? surfaceInitialUniforms(
              this._members,
              this._surface,
              DEFAULT_SIZE,
              DEFAULT_SIZE,
              manifest.protocol_version,
            )
            : undeclaredViewInitialUniforms(this._members))
          : this._surfaceReplyValues(authoredInitial, "surface initializer"));
      this._startScopedTasks();

      if (!this._adoptedCanvas) this._ensureStage();
      // Publish inspectable artifact links as soon as the manifest has been
      // resolved. They remain useful diagnostics even when poster rendering
      // subsequently fails because this browser has no usable WebGPU device.
      this._updateMeta();
      // Controls are an independently useful projection of the Fe-authored
      // view. Materialize them before GPU acquisition so an unavailable
      // adapter cannot erase that authored interface (or its diagnostics).
      this._renderControls();
      await this._renderPosterWithRecovery();
      this._renderControls();
      this._updateMeta();

      this._fsm = "ready";
      this._resolveReady();
      this._dispatch("fe-ready", { mode: this._mode });
      this._applyStatePolicy();
    } catch (error) {
      this._fail(error);
    }
  }

  _fail(error) {
    this._fsm = "error";
    this._badge.textContent = "error";
    this._badge.className = "badge error";
    const notice = document.createElement("div");
    notice.className = "control notice";
    notice.textContent = error?.message ?? String(error);
    // Keep any Fe-projected controls that were materialized before GPU
    // acquisition. The failure notice reports the unavailable host facility;
    // it must not erase the successfully derived application interface.
    this._panel.append(notice);
    console.error("[fe web] fe-surface failed to mount:", error);
    this._rejectReady?.(error);
    this._dispatch("fe-error", error);
  }

  _applyStatePolicy() {
    const policy = this.getAttribute("state") || "auto";
    if (policy === "live") {
      this._goLive();
      return;
    }
    if (policy === "frozen") {
      this._fsm = "frozen";
      return;
    }
    if (policy === "preview") {
      return; // poster only; `.live()` remains available programmatically.
    }
    this._wireActivation(); // "auto": poster first, live on declared intent.
  }

  // -- DOM construction ---------------------------------------------------

  _buildChrome() {
    const style = document.createElement("style");
    style.textContent = SHADOW_CSS;
    this._root = document.createElement("div");
    this._root.className = "root";

    this._side = document.createElement("details");
    this._side.className = "side";
    this._side.setAttribute("part", "side");
    this._side.open = true;
    const controlsToggle = document.createElement("summary");
    controlsToggle.setAttribute("part", "controls-toggle");
    controlsToggle.textContent = "parameters";
    this._badge = document.createElement("span");
    this._badge.className = "badge";
    this._badge.setAttribute("part", "badge");
    this._panel = document.createElement("div");
    this._panel.className = "panel";
    this._panel.setAttribute("part", "panel");
    this._meta = document.createElement("div");
    this._meta.className = "meta";
    this._meta.setAttribute("part", "meta");
    this._side.append(controlsToggle, this._badge, this._panel, this._meta);

    const captionWrap = document.createElement("div");
    captionWrap.className = "caption";
    const slot = document.createElement("slot");
    slot.name = "caption";
    captionWrap.append(slot);

    this._root.append(this._side, captionWrap);
    this._shadow.append(style, this._root);
  }

  /** Generated (non-adopted) canvases: a 2D poster canvas plus a lazily
   * created WebGPU live canvas, stacked and toggled by `hidden` (a canvas's
   * context type is permanent once created, so poster/live must be two
   * elements, not one canvas swapping context type). */
  _ensureStage() {
    if (this._stage) return;
    this._stage = document.createElement("div");
    this._stage.className = "stage";
    this._stage.setAttribute("part", "stage");
    this._posterCanvas = document.createElement("canvas");
    this._posterCanvas.className = "surface-canvas poster";
    this._posterCanvas.setAttribute("part", "canvas");
    this._stage.append(this._posterCanvas);
    this._root.prepend(this._stage);
  }

  _createLiveCanvas() {
    if (this._liveCanvas) return;
    this._liveCanvas = document.createElement("canvas");
    this._liveCanvas.className = "surface-canvas live";
    this._liveCanvas.setAttribute("part", "canvas");
    this._liveCanvas.hidden = true;
    this._stage.append(this._liveCanvas);
  }

  // -- extent -----------------------------------------------------------

  /** Supply untouched standards/device facts to an optional Fe policy and
   * validate its complete backing-store decision before realizing it. Legacy
   * artifacts retain the historical declared/CSS calculation, with only the
   * CPU implementation ceiling kept in this host. */
  _computeBackingExtent(gpu = null) {
    const declaredWidth = this._surface?.extent?.width ?? DEFAULT_SIZE;
    const declaredHeight = this._surface?.extent?.height ?? declaredWidth;
    const dpr = Number(globalThis.devicePixelRatio ?? 0);
    const probe = this._adoptedCanvas || this._stage || this;
    // A fresh canvas has a 300:150 intrinsic aspect ratio. Establish the
    // Fe-declared presentation ratio before measuring CSS bounds, otherwise
    // the first backing decision can accidentally preserve that browser
    // default (for example, a square 768 surface becoming 420x210).
    if (!this._adoptedCanvas && this._stage) {
      this._stage.style.aspectRatio = `${declaredWidth} / ${declaredHeight}`;
    }
    const rect = probe.getBoundingClientRect();
    const cssWidth = Number(rect.width);
    const cssHeight = Number(rect.height);
    const maxTextureDimension = Number(
      gpu?.device?.limits?.maxTextureDimension2D ?? 0,
    );
    const facts = [
      cssWidth,
      cssHeight,
      dpr,
      declaredWidth,
      declaredHeight,
      maxTextureDimension,
      hasCoarsePointer() ? 1 : 0,
      gpu ? 1 : 0,
    ];
    if (!facts.slice(0, 6).every(Number.isFinite)) {
      throw new Error("fe render runtime: surface quality facts must be finite");
    }
    if (this._surfaceQualityKernel) {
      const reply = this._runWasmArenaEpoch(() => this._surfaceQualityKernel(...facts));
      const extent = Array.isArray(reply) ? reply : [reply];
      if (
        extent.length !== 2 ||
        !extent.every((value) => Number.isFinite(value) && Number.isInteger(value) && value >= 1) ||
        extent[0] > declaredWidth ||
        extent[1] > declaredHeight ||
        (gpu && maxTextureDimension > 0 &&
          (extent[0] > maxTextureDimension || extent[1] > maxTextureDimension))
      ) {
        throw new Error(
          "fe render runtime: Fe surface quality policy returned an invalid backing extent",
        );
      }
      return { width: extent[0], height: extent[1] };
    }

    const effectiveDpr = dpr > 0 ? dpr : 1;
    const effectiveCssWidth = cssWidth > 0 ? cssWidth : declaredWidth;
    const effectiveCssHeight = cssHeight > 0 ? cssHeight : declaredHeight;
    const extent = {
      width: Math.max(1, Math.min(declaredWidth, Math.round(effectiveCssWidth * effectiveDpr))),
      height: Math.max(1, Math.min(declaredHeight, Math.round(effectiveCssHeight * effectiveDpr))),
    };
    return gpu ? extent : fitBackingExtent(extent.width, extent.height, CPU_MAX_DIMENSION);
  }

  /** Ask Fe for the complete current backing decision, then commit exactly
   * that decision to resident state. This boundary is used at cold boot,
   * viewport resize, and device-capability transitions; the fixed host never
   * carries a quality tier or substitutes a different typed-policy result. */
  _applyBackingExtent(gpu = null) {
    const { width, height } = this._computeBackingExtent(gpu);
    const changed = width !== this._backingWidth || height !== this._backingHeight;
    if (!changed) return false;
    this._backingWidth = width;
    this._backingHeight = height;
    this._replaceSurfaceState(
      withExtentUniforms(
        this._members,
        this._surface,
        this._uniforms,
        width,
        height,
        this._manifest?.protocol_version ?? 8,
      ),
    );
    this._applyExtentAndFilter(width, height);
    return changed;
  }

  _resizePresentationCanvases() {
    const canvas = this._adoptedCanvas ||
      (this._mode === "webgpu" ? this._liveCanvas : this._posterCanvas);
    if (!canvas) return;
    canvas.width = this._backingWidth;
    canvas.height = this._backingHeight;
  }

  /** A ResizeObserver supplies only the changed standards geometry. The
   * selected Fe policy is re-run against the current GPU/CPU capability and
   * its exact decision becomes the next resident extent and presentation. */
  async _refreshLiveBackingExtent() {
    if (this._fsm !== "live" || this._resizePending) return;
    this._resizePending = true;
    try {
      const gpu = this._mode === "webgpu"
        ? (this._gpu ?? await this._ensurePipeline())
        : null;
      if (!this._applyBackingExtent(gpu)) return;
      this._resizePresentationCanvases();
      await this._render();
    } finally {
      this._resizePending = false;
    }
  }

  _wireResizeObserver() {
    if (this._resizeObserver || typeof ResizeObserver !== "function") return;
    this._resizeObserver = new ResizeObserver(() => {
      this._refreshLiveBackingExtent().catch(error => this._fail(error));
    });
    this._resizeObserver.observe(this);
  }

  _unwireResizeObserver() {
    this._resizeObserver?.disconnect();
    this._resizeObserver = null;
    this._resizePending = false;
  }

  async _resolveGpu() {
    if (this._mode === "wasm-2d") return null;
    if (this._gpuOverride !== undefined) return this._gpuOverride;

    let gpu = await acquireSharedGpu();
    if (gpu || !this._surfaceRecoveryKernel) return gpu;

    // Initial capability failure and lazy post-loss demand both enter the same
    // Fe policy. The policy's resident `device_lost`/attempt state distinguishes
    // those episodes; this loop exists only while Fe returns `RetryDevice`.
    this._recoveryWasLive = this._fsm === "live";
    for (;;) {
      const action = this._runSurfaceRecovery(
        GpuDeviceEventKind.Unavailable,
        GpuDeviceLossReason.NotLost,
        true,
        Boolean(this._kernel),
        sharedGpuGeneration,
      );
      if (action === SurfaceRecoveryAction.RetryDevice) {
        sharedGpuPromise = undefined;
        gpu = await acquireSharedGpu();
        if (!gpu) continue;
        this._runSurfaceRecovery(
          GpuDeviceEventKind.Available,
          GpuDeviceLossReason.NotLost,
          true,
          Boolean(this._kernel),
          gpu.generation ?? sharedGpuGeneration,
        );
        this._deliverSurfaceLifecycle(SurfaceEventKind.DeviceRecovered);
        return gpu;
      }
      await this._realizeDeviceRecovery(
        action,
        GpuDeviceLossReason.NotLost,
        sharedGpuGeneration,
        true,
      );
      if (action === SurfaceRecoveryAction.FailSurface) {
        throw sharedGpuFailure ?? new Error(
          "fe render runtime: Fe recovery policy rejected unavailable WebGPU",
        );
      }
      return null;
    }
  }

  async _buildPassGraph(device, generation) {
    const format = this._layout.color_target_format || navigator.gpu.getPreferredCanvasFormat();
    const [shaderSources, resourceInitialBytes] = await Promise.all([
      Promise.all(
        this._passShaderUrls.map(async (url) => (await fetchOrThrow(url, "WGSL pass shader")).text()),
      ),
      Promise.all(
        this._resources.map(async resource => [
          resource.name,
          await fetchVerifiedResourceArtifact(resource, this._manifestUrl),
        ]),
      ).then(entries => new Map(entries)),
    ]);
    const resourceBuffers = new Map();
    const ownedBuffers = new Set();
    try {
      for (const resource of this._resources) {
        if (resource.group !== 0) {
          throw new Error("fe render runtime: pass graphs currently require resource group 0");
        }
        const buffer = createGpuBuffer(device, {
          size: Math.max(4, resource.stride * resource.length),
          usage: resourceBufferUsage(resource, undefined, this._manifest.protocol_version),
        });
        ownedBuffers.add(buffer);
        resourceBuffers.set(resource.name, buffer);
        const initialBytes = resourceInitialBytes.get(resource.name);
        if (initialBytes) writeGpuBuffer(device.queue, buffer, 0, initialBytes);
      }

      const raster = rasterPlan(this._surface);
      const passRecords = [];
      for (let index = 0; index < this._passes.length; index++) {
        const pass = this._passes[index];
        const module = device.createShaderModule({ code: shaderSources[index] });
        const layoutEntries = [];
        const groupEntries = [];
        const inputs = [];
        const outputs = [];
        for (const binding of pass.layout.bindings) {
          const visibility = bindingShaderVisibility(
            binding,
            pass,
            undefined,
            this._manifest.protocol_version,
          );
          if (binding.group !== 0) {
            throw new Error("fe render runtime: pass graphs currently require binding group 0");
          }
          if (binding.role === "resource") {
            const buffer = resourceBuffers.get(binding.name);
            if (!buffer) {
              throw new Error(`fe render runtime: resource \`${binding.name}\` is undeclared`);
            }
            layoutEntries.push({
              binding: binding.binding,
              visibility,
              buffer: { type: binding.access === "read" ? "read-only-storage" : "storage" },
            });
            groupEntries.push({ binding: binding.binding, resource: { buffer } });
          } else if (binding.role === "input") {
            const buffer = createGpuBuffer(device, {
              size: Math.max(16, binding.span),
              usage: GPUBufferUsage.STORAGE | GPUBufferUsage.COPY_DST,
            });
            ownedBuffers.add(buffer);
            layoutEntries.push({
              binding: binding.binding,
              visibility,
              buffer: { type: "read-only-storage" },
            });
            groupEntries.push({ binding: binding.binding, resource: { buffer } });
            inputs.push({ binding, buffer });
          } else if (binding.role === "output") {
            // Compiler-internal channels, including the checked-arithmetic trap
            // word, are pass-local. They are deliberately not graph resources:
            // external actor storage remains shared by resource identity while
            // these buffers are rebuilt with the pass on device recovery.
            const buffer = createGpuBuffer(device, {
              size: Math.max(4, binding.span),
              usage: GPUBufferUsage.STORAGE,
            });
            ownedBuffers.add(buffer);
            layoutEntries.push({
              binding: binding.binding,
              visibility,
              buffer: { type: binding.access === "read" ? "read-only-storage" : "storage" },
            });
            groupEntries.push({ binding: binding.binding, resource: { buffer } });
            outputs.push({ binding, buffer });
          }
        }
        const bindGroupLayout = layoutEntries.length
          ? device.createBindGroupLayout({ entries: layoutEntries })
          : null;
        const bindGroup = bindGroupLayout
          ? device.createBindGroup({ layout: bindGroupLayout, entries: groupEntries })
          : null;
        const pipelineLayout = bindGroupLayout
          ? device.createPipelineLayout({ bindGroupLayouts: [bindGroupLayout] })
          : "auto";
        const pipeline = pass.layout.mode === "compute"
          ? device.createComputePipeline({
              layout: pipelineLayout,
              compute: { module, entryPoint: pass.layout.entry_point },
            })
          : device.createRenderPipeline({
              layout: pipelineLayout,
              vertex: { module, entryPoint: pass.layout.vertex_entry },
              fragment: {
                module,
                entryPoint: pass.layout.fragment_entry,
                targets: [{ format }],
              },
              primitive: { topology: "triangle-list", cullMode: raster.cullMode },
              multisample: { count: raster.sampleCount },
              ...(pass.draw_vertices && raster.depth
                ? {
                    depthStencil: {
                      format: raster.depth.format,
                      depthWriteEnabled: raster.depth.writeEnabled,
                      depthCompare: raster.depth.compare,
                    },
                  }
                : {}),
            });
        passRecords.push({ pass, pipeline, bindGroup, inputs, outputs });
      }
      return {
        device,
        generation,
        format,
        passRecords,
        resourceBuffers,
        ownedBuffers,
        raster,
      };
    } catch (error) {
      destroyGpuBuffers(ownedBuffers);
      throw error;
    }
  }

  async _ensurePipeline() {
    this._pipelineError = null;
    const gpu = await this._resolveGpu();
    if (!gpu) return null;
    const { device } = gpu;
    if (this._gpu && this._gpu.device === device) return this._gpu;
    try {
      if (this._graph) {
        this._gpu = await this._buildPassGraph(device, gpu.generation);
        return this._gpu;
      }
      const wgsl = await (await fetchOrThrow(this._wgslUrl, "WGSL shader")).text();
      const shaderModule = device.createShaderModule({ code: wgsl });
      const format = this._layout.color_target_format || navigator.gpu.getPreferredCanvasFormat();
      let bindGroupLayout = null;
      let bindGroup = null;
      let uniformBuffer = null;
      let pipelineLayout = "auto";
      if (this._inputBinding) {
        uniformBuffer = createGpuBuffer(device, {
          size: Math.max(16, this._inputBinding.span),
          usage: GPUBufferUsage.STORAGE | GPUBufferUsage.COPY_DST,
        });
        bindGroupLayout = device.createBindGroupLayout({
          entries: [
            {
              binding: this._inputBinding.binding,
              visibility: bindingShaderVisibility(
                this._inputBinding,
                this._renderPass,
                undefined,
                this._manifest.protocol_version,
              ),
              buffer: { type: "read-only-storage" },
            },
          ],
        });
        bindGroup = device.createBindGroup({
          layout: bindGroupLayout,
          entries: [{ binding: this._inputBinding.binding, resource: { buffer: uniformBuffer } }],
        });
        pipelineLayout = device.createPipelineLayout({ bindGroupLayouts: [bindGroupLayout] });
      }
      const pipeline = device.createRenderPipeline({
        layout: pipelineLayout,
        vertex: { module: shaderModule, entryPoint: this._layout.vertex_entry },
        fragment: { module: shaderModule, entryPoint: this._layout.fragment_entry, targets: [{ format }] },
        primitive: { topology: "triangle-list" },
      });
      this._gpu = {
        device,
        generation: gpu.generation,
        format,
        pipeline,
        bindGroup,
        uniformBuffer,
      };
      return this._gpu;
    } catch (error) {
      this._pipelineError = error;
      console.warn(
        this._graph
          ? "[fe web] WebGPU pass graph init failed:"
          : "[fe web] WebGPU pipeline init failed, using wasm fallback:",
        error,
      );
      return null;
    }
  }

  _presentOn(context, uniforms, capture = null) {
    const previous = this._presentationTail ?? Promise.resolve();
    const presentation = previous.then(() => this._presentNow(context, uniforms, capture));
    this._presentationTail = presentation.catch(() => {});
    return presentation;
  }

  async _presentNow(context, uniforms, capture = null) {
    if (this._graph) {
      const gpu = this._gpu;
      const { device, passRecords } = gpu;
      if (passRecords.some(record => record.pass.layout.mode !== "compute")) {
        ensureRasterAttachments(gpu, this._backingWidth, this._backingHeight);
      }
      for (const record of passRecords) {
        for (const input of record.inputs) {
          const values = input.binding.members.map((member) => {
            const index = this._memberIndexByName.get(member.name);
            return index === undefined ? 0 : uniforms[index];
          });
          writeUniformBuffer(
            device,
            input.buffer,
            input.binding.span,
            input.binding.members,
            values,
          );
        }
      }
      let encoder = device.createCommandEncoder();
      let encoded = false;
      let encoderMode = null;
      let texture = null;
      let rendered = false;
      let depthRendered = false;
      const submitEncoder = () => {
        if (!encoded) return;
        device.queue.submit([encoder.finish()]);
        encoder = device.createCommandEncoder();
        encoded = false;
        encoderMode = null;
      };
      const executeRecord = async (record, cycleIteration = null) => {
        if (record.pass.layout.mode === "compute") {
          // Queue ordering preserves pass dependencies across submissions.
          // Keep compute and presentation work in separate command buffers so
          // browser GPU processes do not need to compile and execute one mixed
          // protocol-sized encoder before a canvas texture can be captured.
          if (encoderMode === "render") submitEncoder();
          let dispatch = record.pass.dispatch;
          if (!dispatch) throw new Error("fe render runtime: compute pass has no fixed dispatch");
          let repeat = record.pass.repeat ?? 1;
          const taper = record.pass.taper;
          if (taper !== undefined && taper !== null) {
            if (!Number.isSafeInteger(cycleIteration) || cycleIteration < 0) {
              throw new Error("fe render runtime: tapered dispatch escaped its actor cycle");
            }
            if (
              !Array.isArray(taper.shifts) || taper.shifts.length !== 3 ||
              taper.shifts.some((shift) => !Number.isSafeInteger(shift) || shift < 0 || shift > 31) ||
              !Number.isSafeInteger(taper.repeat_decrement) || taper.repeat_decrement < 0
            ) {
              throw new Error("fe render runtime: invalid compiler-derived dispatch taper");
            }
            dispatch = dispatch.map((dimension, axis) => {
              const exponent = cycleIteration * taper.shifts[axis];
              if (exponent >= 31) return 1;
              return Math.max(1, Math.ceil(dimension / (2 ** exponent)));
            });
            repeat -= cycleIteration * taper.repeat_decrement;
            if (repeat < 0) {
              throw new Error("fe render runtime: dispatch taper exhausted before cycle completion");
            }
            if (repeat === 0) return;
          } else if (cycleIteration !== null && record.pass.taper !== undefined) {
            throw new Error("fe render runtime: malformed compiler-derived dispatch taper");
          }
          if (!Number.isSafeInteger(repeat) || repeat < 1 || repeat > 65535) {
            throw new Error("fe render runtime: invalid compiler-derived compute repeat count");
          }
          const cooperation = record.pass.cooperation;
          let repeatBatch = repeat;
          if (cooperation !== undefined && cooperation !== null) {
            repeatBatch = cooperation.repeat_batch;
            if (
              !Number.isSafeInteger(repeatBatch) || repeatBatch < 1 || repeatBatch > 65535
            ) {
              throw new Error("fe render runtime: invalid compiler-derived cooperative dispatch batch");
            }
          }
          let remaining = repeat;
          while (remaining > 0) {
            const batch = Math.min(remaining, repeatBatch);
            const compute = encoder.beginComputePass();
            compute.setPipeline(record.pipeline);
            if (record.bindGroup) compute.setBindGroup(0, record.bindGroup);
            for (let iteration = 0; iteration < batch; iteration += 1) {
              compute.dispatchWorkgroups(dispatch[0], dispatch[1], dispatch[2]);
            }
            compute.end();
            encoded = true;
            encoderMode = "compute";
            remaining -= batch;
            if (cooperation !== undefined && cooperation !== null) {
              submitEncoder();
              await awaitSharedGpuQueueIdle(gpu);
            }
          }
        } else {
          if (encoderMode === "compute") submitEncoder();
          texture ??= context.getCurrentTexture();
          const targetView = texture.createView();
          const multisampleView = gpu.multisampleTexture?.createView() ?? null;
          const colorAttachment = {
            view: multisampleView ?? targetView,
            clearValue: gpu.raster.color.clearValue,
            loadOp: rendered
              ? gpu.raster.color.followingLoad
              : gpu.raster.color.firstLoad,
            storeOp: gpu.raster.color.store,
          };
          if (multisampleView) colorAttachment.resolveTarget = targetView;
          const usesDepth = Boolean(record.pass.draw_vertices && gpu.depthTexture);
          const depthStencilAttachment = usesDepth
            ? {
                view: gpu.depthTexture.createView(),
                depthClearValue: gpu.raster.depth.clearValue,
                depthLoadOp: depthRendered
                  ? gpu.raster.depth.followingLoad
                  : gpu.raster.depth.firstLoad,
                depthStoreOp: gpu.raster.depth.store,
              }
            : undefined;
          const render = encoder.beginRenderPass({
            colorAttachments: [colorAttachment],
            depthStencilAttachment,
          });
          render.setPipeline(record.pipeline);
          if (record.bindGroup) render.setBindGroup(0, record.bindGroup);
          const draw = rasterDrawShape(record.pass);
          drawGpu(render, draw.vertices, draw.instances);
          render.end();
          rendered = true;
          depthRendered ||= usesDepth;
          encoded = true;
          encoderMode = "render";
        }
      };
      let passIndex = 0;
      while (passIndex < passRecords.length) {
        const record = passRecords[passIndex];
        const cycle = record.pass.cycle;
        if (cycle === undefined || cycle === null) {
          await executeRecord(record);
          passIndex += 1;
          continue;
        }
        if (
          !Number.isSafeInteger(cycle.group) || cycle.group < 0 || cycle.group > 0xffffffff ||
          !Number.isSafeInteger(cycle.repeat) || cycle.repeat < 1 || cycle.repeat > 65535
        ) {
          throw new Error("fe render runtime: invalid compiler-derived actor pass cycle");
        }
        let cycleEnd = passIndex;
        while (cycleEnd < passRecords.length) {
          const member = passRecords[cycleEnd];
          const memberCycle = member.pass.cycle;
          if (memberCycle === undefined || memberCycle === null || memberCycle.group !== cycle.group) {
            break;
          }
          if (memberCycle.repeat !== cycle.repeat || member.pass.layout.mode !== "compute") {
            throw new Error("fe render runtime: inconsistent compiler-derived actor pass cycle");
          }
          cycleEnd += 1;
        }
        // A cycle is a compiler-derived dependency body. Queue submissions are
        // ordered, so bounding each iteration in its own command buffer keeps
        // the semantic phase order while avoiding protocol-sized encoders that
        // destabilize browser WebGPU implementations.
        submitEncoder();
        for (let iteration = 0; iteration < cycle.repeat; iteration += 1) {
          for (let memberIndex = passIndex; memberIndex < cycleEnd; memberIndex += 1) {
            await executeRecord(passRecords[memberIndex], iteration);
          }
          submitEncoder();
        }
        passIndex = cycleEnd;
      }
      if (capture && !texture) {
        throw new Error("fe render runtime: cannot capture a pass graph with no render pass");
      }
      const readback = capture
        ? encodeCanvasReadback(
            device,
            encoder,
            texture,
            capture.width,
            capture.height,
            capture.format,
          )
        : null;
      if (readback) encoded = true;
      const actorReadbackSource = this._gpuReadbackResource
        ? gpu.resourceBuffers.get(this._gpuReadbackResource.name)
        : null;
      if (this._gpuReadbackResource && !actorReadbackSource) {
        throw new Error(
          "fe render runtime: typed GPU readback resource has no live device buffer",
        );
      }
      const actorReadback = actorReadbackSource
        ? encodeGpuBufferReadback(
            device,
            encoder,
            actorReadbackSource,
            this._gpuReadbackResource.byteLength,
          )
        : null;
      if (actorReadback) encoded = true;
      submitEncoder();
      if (actorReadback) await this._deliverGpuReadback(actorReadback);
      return readback;
    }
    const { device, pipeline, bindGroup, uniformBuffer } = this._gpu;
    if (uniformBuffer) {
      writeUniformBuffer(device, uniformBuffer, this._inputBinding.span, this._members, uniforms);
    }
    return presentFrame(device, context, pipeline, bindGroup, capture);
  }

  _callKernel(px, py, uniforms) {
    if (!this._kernel) {
      throw new Error("fe render runtime: this GPU pass graph has no CPU fallback");
    }
    const args = new Array(this._argumentCount).fill(0);
    for (const builtin of this._builtins) {
      args[builtin.arg_index] = builtin.source.endsWith("_y") ? py : px;
    }
    this._members.forEach((member, index) => {
      args[member.arg_index] = uniforms[index];
    });
    // This legacy fallback invokes one scalar pixel entry repeatedly. Reset
    // before (rather than before and after) each pixel; `_renderWasmInto`
    // closes the final epoch in `finally`.
    this._wasmArenaReset?.();
    return this._kernel(...args) >>> 0; // 0xAARRGGBB
  }

  _renderWasmInto(canvas, width, height, uniforms) {
    canvas.width = width;
    canvas.height = height;
    const ctx = canvas.getContext("2d");
    const image = ctx.createImageData(width, height);
    const data = image.data;
    try {
      for (let py = 0; py < height; py++) {
        for (let px = 0; px < width; px++) {
          const rgba = this._callKernel(px, py, uniforms);
          const i = (py * width + px) * 4;
          data[i] = (rgba >>> 16) & 255;
          data[i + 1] = (rgba >>> 8) & 255;
          data[i + 2] = rgba & 255;
          data[i + 3] = (rgba >>> 24) & 255;
        }
      }
    } finally {
      this._wasmArenaReset?.();
    }
    ctx.putImageData(image, 0, 0);
  }

  _applyExtentAndFilter(width, height) {
    const filter = this._surface?.extent?.filter === "pixelated" ? "pixelated" : "auto";
    if (this._stage) this._stage.style.aspectRatio = `${width} / ${height}`;
    for (const canvas of [this._posterCanvas, this._liveCanvas, this._adoptedCanvas]) {
      if (canvas) canvas.style.imageRendering = filter;
    }
  }

  // -- ready: render one frame, capture a poster, release the GPU context --

  /** Retry a cold poster only when the exact device used for the failed frame
   * has reported `device.lost` and shared recovery produced a different
   * device. This covers first-submit backend loss without masking ordinary
   * pass-graph errors or allowing an unbounded request/retry loop. */
  async _renderPosterWithRecovery() {
    for (;;) {
      this._posterRecoveryActive = true;
      this._posterAttemptedDevice = null;
      try {
        await this._renderPoster();
        this._posterAttemptedDevice = null;
        return;
      } catch (error) {
        const attemptedDevice = this._posterAttemptedDevice;
        this._posterAttemptedDevice = null;
        if (
          this._gpuOverride ||
          !attemptedDevice
        ) {
          throw error;
        }
        const loss = await confirmedDeviceLoss(attemptedDevice);
        if (!loss.lost) throw error;

        await (sharedGpuRecoveryPromise ?? handleSharedDeviceLoss(attemptedDevice, loss.info));
        if (this._fsm === "error") throw error;
        if (this._mode === "wasm-2d") continue;
        const freshGpu = await acquireSharedGpu();
        if (!freshGpu || freshGpu.device === attemptedDevice) throw error;
        this._releaseGpuResources();
        this._pipelineError = null;
        console.warn("[fe web] retrying initial poster after Fe-selected device recovery");
      } finally {
        this._posterRecoveryActive = false;
      }
    }
  }

  /** Render ONE frame at the current (initial) uniforms, capture it as a
   * static poster, and release GPU presentation: the durable fix for a
   * gallery of N tiles costing zero configured swap chains until a tile goes
   * live (FE_WEB_V5_ORCHESTRATION_DESIGN.md section 6). */
  async _renderPoster() {
    const gpu = await this._ensurePipeline();
    this._applyBackingExtent(gpu);
    const width = this._backingWidth;
    const height = this._backingHeight;

    if (!gpu) {
      if (!this._kernel) {
        if (this._pipelineError) {
          throw new Error(
            `fe render runtime: WebGPU pass graph initialization failed: ${this._pipelineError.message}`,
            { cause: this._pipelineError },
          );
        }
        throw sharedGpuFailure ?? new Error(
          "fe render runtime: WebGPU is required for this resource pass graph",
        );
      }
      this._mode = "wasm-2d";
      this._renderWasmInto(
        this._adoptedCanvas || this._posterCanvas,
        width,
        height,
        this._uniforms,
      );
      return;
    }
    this._posterAttemptedDevice = gpu.device;
    this._mode = "webgpu";
    if (this._adoptedCanvas) {
      // An adopted canvas opts OUT of the poster/live swap (its context type
      // is the caller's to pick, and the caller owns exactly one canvas
      // element): render straight onto it and leave it configured. This
      // trades the gallery-scale "zero configured swap chains while off
      // screen" property for the ability to hand callers a specific element.
      const context = this._adoptedCanvas.getContext("webgpu");
      context.configure({ device: gpu.device, format: gpu.format, alphaMode: "opaque" });
      this._adoptedCanvas.width = width;
      this._adoptedCanvas.height = height;
      this._adoptedContext = context;
      await this._presentOn(context, this._uniforms);
      await awaitSharedGpuQueueIdle(gpu);
      return;
    }
    // Use the ordinary HTML live canvas, briefly attached and visible, for the
    // one-frame poster. OffscreenCanvas is not supported by every WebGPU
    // configuration, while a detached HTML canvas may never acquire a
    // compositor mailbox. The context is still unconfigured immediately after
    // capture, so ready galleries retain zero configured swap chains.
    this._createLiveCanvas();
    const posterSource = this._liveCanvas;
    posterSource.width = width;
    posterSource.height = height;
    posterSource.hidden = false;
    this._posterCanvas.hidden = true;
    const context = posterSource.getContext("webgpu");
    if (!context) {
      throw new Error("fe render runtime: the browser could not create a WebGPU canvas context");
    }
    try {
      try {
        context.configure({
          device: gpu.device,
          format: gpu.format,
          alphaMode: "opaque",
          usage: GPUTextureUsage.RENDER_ATTACHMENT | GPUTextureUsage.COPY_SRC,
        });
      } catch (error) {
        throw new Error(`fe render runtime: poster context configuration failed: ${error?.message ?? String(error)}`, { cause: error });
      }
      try {
        const readback = await this._presentOn(context, this._uniforms, { width, height, format: gpu.format });
        const pixels = await readCanvasReadback(readback);
        publishSharedGpuQueueIdle(gpu);
        this._paintPosterPixels(pixels, width, height);
      } catch (error) {
        throw new Error(`fe render runtime: poster command submission failed: ${error?.message ?? String(error)}`, { cause: error });
      }
      // Read the exact texture in the same GPU submission. Waiting for two
      // compositor frames and then snapshotting the canvas is observably
      // unreliable on mobile WebGPU: the swap texture may already have been
      // discarded, producing a black poster after a valid live frame.
    } finally {
      try {
        context.unconfigure();
      } catch {
        // Device loss may already have invalidated the context.
      }
      posterSource.hidden = true;
      this._posterCanvas.hidden = false;
      this._releaseGpuResources();
    }
  }

  _paintPosterPixels(pixels, width, height) {
    this._posterCanvas.width = width;
    this._posterCanvas.height = height;
    const ctx = this._posterCanvas.getContext("2d");
    const image = ctx.createImageData(width, height);
    image.data.set(pixels);
    ctx.putImageData(image, 0, 0);
  }

  async _capturePosterFromLive() {
    if (this._adoptedCanvas || this._mode !== "webgpu" || !this._liveContext) return;
    const context = this._liveContext;
    const gpu = this._gpu;
    try {
      await awaitSharedGpuQueueIdle(gpu);
      const readback = await this._presentOn(context, this._uniforms, {
        width: this._backingWidth,
        height: this._backingHeight,
        format: gpu.format,
      });
      const pixels = await readCanvasReadback(readback);
      publishSharedGpuQueueIdle(gpu);
      this._paintPosterPixels(pixels, this._backingWidth, this._backingHeight);
    } finally {
      try {
        context.unconfigure();
      } catch {
        // Device loss may already have invalidated the context.
      }
      if (this._liveContext === context) this._liveContext = null;
      this._liveCanvas.hidden = true;
      this._posterCanvas.hidden = false;
      this._releaseGpuResources();
    }
  }

  // -- ready -> live on declared intent -------------------------------------

  _wireActivation() {
    const activate = this._surface?.activate || "pointer";
    if (activate === "manual") return; // caller drives `.live()` explicitly.
    if (activate === "visible") {
      this._activationObserver = new IntersectionObserver((entries) => {
        for (const entry of entries) if (entry.isIntersecting) this._goLive();
      });
      this._activationObserver.observe(this);
      return;
    }
    // "pointer" (default): pointerenter / focus / tap.
    const onIntent = () => this._goLive();
    this.addEventListener("pointerenter", onIntent, { once: true });
    this.addEventListener("focusin", onIntent, { once: true });
    this.addEventListener("click", onIntent, { once: true });
    if (!this.hasAttribute("tabindex")) this.setAttribute("tabindex", "0");
  }

  async _goLive() {
    if (this._fsm === "live") return;
    if (this._fsm === "cold") await this._readyPromise.catch(() => {});
    if (this._fsm === "error") return;

    // Lifecycle policy may alter resident Fe state before the first live
    // presentation. The host contributes only the standards-derived fact.
    this._deliverSurfaceLifecycle(SurfaceEventKind.Visible);

    if (this._adoptedCanvas) {
      // Already presenting (webgpu, kept configured) or cheap to re-run
      // (wasm-2d); "live" is a state/event transition here, not new work.
      let gpu = null;
      if (this._mode === "webgpu") {
        gpu = await this._ensurePipeline();
        if (!gpu) {
          if (!this._kernel) {
            this._fail(sharedGpuFailure ?? new Error(
              "fe render runtime: WebGPU is required for this resource pass graph",
            ));
            return;
          }
          this._mode = "wasm-2d";
        } else if (!this._adoptedContext) {
          const context = this._adoptedCanvas.getContext("webgpu");
          context.configure({ device: gpu.device, format: gpu.format, alphaMode: "opaque" });
          this._adoptedContext = context;
        }
      }
      this._applyBackingExtent(gpu);
      this._resizePresentationCanvases();
      if (this._mode === "wasm-2d") {
        this._renderWasmInto(this._adoptedCanvas, this._backingWidth, this._backingHeight, this._uniforms);
      } else {
        await this._presentOn(this._adoptedContext, this._uniforms);
      }
      this._enterLive();
      return;
    }

    if (this._mode === "wasm-2d") {
      // No swap chain, so no cost distinction between "ready" and "live":
      // the poster canvas IS the live canvas in the CPU fallback.
      this._applyBackingExtent(null);
      this._resizePresentationCanvases();
      this._renderWasmInto(this._posterCanvas, this._backingWidth, this._backingHeight, this._uniforms);
      this._enterLive();
      return;
    }

    const gpu = await this._ensurePipeline();
    if (!gpu) {
      // WebGPU became unavailable between poster and live (e.g. a
      // pre-recovery device loss): fail over honestly, badge included.
      if (!this._kernel) {
        this._fail(sharedGpuFailure ?? new Error(
          "fe render runtime: WebGPU is required for this resource pass graph",
        ));
        return;
      }
      this._mode = "wasm-2d";
      this._applyBackingExtent(null);
      this._resizePresentationCanvases();
      this._renderWasmInto(this._posterCanvas, this._backingWidth, this._backingHeight, this._uniforms);
      this._enterLive();
      return;
    }
    this._applyBackingExtent(gpu);
    this._createLiveCanvas();
    this._liveCanvas.width = this._backingWidth;
    this._liveCanvas.height = this._backingHeight;
    const context = this._liveCanvas.getContext("webgpu");
    context.configure({
      device: gpu.device,
      format: gpu.format,
      alphaMode: "opaque",
      usage: GPUTextureUsage.RENDER_ATTACHMENT | GPUTextureUsage.COPY_SRC,
    });
    this._liveContext = context;
    await this._presentOn(context, this._uniforms);
    this._posterCanvas.hidden = true;
    this._liveCanvas.hidden = false;
    this._enterLive();
  }

  _enterLive() {
    this._fsm = "live";
    this._wireSuspendObserver();
    this._wireResizeObserver();
    this._wireGestures();
    this._updateBadge();
    this._resolveLive();
    this._dispatch("fe-live", { mode: this._mode });
    this._dispatch("fe-frame", { params: this._paramsSnapshot() });
    // A visibility/device boundary may have retained raw input while the
    // standards surface was not live. Fe has already decided whether that work
    // deserves a frame; this only realizes the browser request when possible.
    this._scheduleGestureFrame();
  }

  // -- live <-> suspended off-viewport --------------------------------------

  _wireSuspendObserver() {
    if (this._suspendObserver) return;
    this._suspendObserver = new IntersectionObserver((entries) => {
      for (const entry of entries) {
        if (!entry.isIntersecting && this._fsm === "live") this._suspend();
        else if (entry.isIntersecting && this._fsm === "suspended") this._goLive();
      }
    });
    this._suspendObserver.observe(this);
  }

  async _suspend() {
    await this._capturePosterFromLive();
    this._unwireGestures();
    this._unwireResizeObserver();
    this._fsm = "suspended";
    this._deliverSurfaceLifecycle(SurfaceEventKind.Hidden);
    this._dispatch("fe-statechange", { state: "suspended" });
  }

  // -- render --------------------------------------------------------------

  async _render(next) {
    if (next) this._replaceSurfaceState(next);
    const presentationUniforms = this._uniforms.slice();
    if (this._fsm !== "live") {
      // The resident actor plus its presentation mirror still update while
      // hidden; no GPU/CPU presentation work is spent until it becomes live.
      this._refreshControlValues();
      return;
    }
    if (this._adoptedCanvas) {
      if (this._mode === "webgpu") await this._presentOn(this._adoptedContext, presentationUniforms);
      else this._renderWasmInto(this._adoptedCanvas, this._backingWidth, this._backingHeight, presentationUniforms);
    } else if (this._mode === "webgpu") {
      await this._presentOn(this._liveContext, presentationUniforms);
    } else {
      this._renderWasmInto(this._posterCanvas, this._backingWidth, this._backingHeight, presentationUniforms);
    }
    this._refreshControlValues();
    this._dispatch("fe-frame", { params: this._paramsSnapshot(presentationUniforms) });
  }

  _paramsSnapshot(uniforms = this._uniforms) {
    const snapshot = {};
    this._members.forEach((member, index) => {
      snapshot[member.name] = uniforms[index];
    });
    return snapshot;
  }

  _buildParamsProxy() {
    const element = this;
    return new Proxy(
      {},
      {
        get(_target, prop) {
          if (typeof prop !== "string") return undefined;
          const index = element._memberIndexByName.get(prop);
          return index === undefined ? undefined : element._uniforms[index];
        },
        set(_target, prop, value) {
          if (typeof prop !== "string") return false;
          if (element._fsm === "cold" || element._fsm === "error") {
            throw new Error('fe-surface: params are not ready; await "fe-ready" first');
          }
          const index = element._memberIndexByName.get(prop);
          if (index === undefined) return false;
          const paramIndex = element._surfaceParamIndexByName.get(prop);
          if (element._surfaceTransitionKernel && paramIndex === undefined) return false;
          element._applyParamEdit(index, Number(value), paramIndex ?? index);
          return true;
        },
        has(_target, prop) {
          return typeof prop === "string" && element._memberIndexByName.has(prop);
        },
        ownKeys() {
          return [...element._memberIndexByName.keys()];
        },
        getOwnPropertyDescriptor(_target, prop) {
          if (!element._memberIndexByName.has(prop)) return undefined;
          return { enumerable: true, configurable: true, value: element.params[prop] };
        },
      },
    );
  }

  // -- device loss -----------------------------------------------------------

  /** Destroy buffer allocations and drop pipeline references whenever a tile
   * returns to poster-only state. Pipelines are rebuilt lazily on the next
   * live transition; this keeps an off-screen gallery from pinning every
   * demo's device resources at once. */
  _releaseGpuResources() {
    const gpu = this._gpu;
    this._gpu = null;
    if (!gpu) return;
    releaseRasterAttachments(gpu);
    const buffers = gpu.ownedBuffers
      ? [...gpu.ownedBuffers]
      : gpu.resourceBuffers
        ? [...new Set(gpu.resourceBuffers.values())]
        : [gpu.uniformBuffer].filter(Boolean);
    destroyGpuBuffers(buffers);
  }

  _deviceRecoveryRequired() {
    return this._mode === "webgpu" &&
      (this._fsm === "live" || this._posterRecoveryActive === true);
  }

  _beginDeviceLoss(reason, generation) {
    if (this._mode !== "webgpu") {
      this._recoveryObservedLoss = false;
      return SurfaceRecoveryAction.NoAction;
    }
    this._recoveryObservedLoss = true;
    this._recoveryWasLive = this._fsm === "live";
    this._gpu = null;
    if (this._liveContext) {
      try {
        this._liveContext.unconfigure();
      } catch {
        // already invalid; nothing to release.
      }
      this._liveContext = null;
    }
    if (this._adoptedContext) {
      try {
        this._adoptedContext.unconfigure();
      } catch {
        // already invalid.
      }
      this._adoptedContext = null;
    }
    const required = this._deviceRecoveryRequired();
    this._deliverSurfaceLifecycle(SurfaceEventKind.DeviceLost);
    const action = this._runSurfaceRecovery(
      GpuDeviceEventKind.Lost,
      reason,
      required,
      Boolean(this._kernel),
      generation,
    );
    if (this._recoveryWasLive) {
      this._dispatch("fe-statechange", { state: this._fsm, reason: "device-lost" });
    }
    // Pre-v3 bundles have no Fe recovery decision. They remain readable as
    // compatibility artifacts, but never regain the deleted host retry budget.
    if (!this._surfaceRecoveryKernel) {
      return required
        ? (this._kernel
          ? SurfaceRecoveryAction.DegradeToWasm
          : SurfaceRecoveryAction.FailSurface)
        : SurfaceRecoveryAction.NoAction;
    }
    return action ?? SurfaceRecoveryAction.FailSurface;
  }

  _continueDeviceRecovery(reason, generation) {
    const action = this._runSurfaceRecovery(
      GpuDeviceEventKind.Unavailable,
      reason,
      true,
      Boolean(this._kernel),
      generation,
    );
    if (!this._surfaceRecoveryKernel) {
      return this._kernel
        ? SurfaceRecoveryAction.DegradeToWasm
        : SurfaceRecoveryAction.FailSurface;
    }
    return action ?? SurfaceRecoveryAction.FailSurface;
  }

  async _realizeDeviceRecovery(action, _reason, _generation, deferPresentation = false) {
    if (action === SurfaceRecoveryAction.NoAction) {
      if (this._deviceRecoveryRequired()) {
        throw new Error(
          "fe render runtime: Fe recovery policy returned no action for a surface requiring a device",
        );
      }
      return;
    }
    if (action === SurfaceRecoveryAction.RetryDevice) {
      throw new Error(
        "fe render runtime: shared retry must be realized by the page-wide recovery coordinator",
      );
    }
    if (action === SurfaceRecoveryAction.FailSurface) {
      this._recoveryObservedLoss = false;
      this._fail(sharedGpuFailure ?? new Error(
        "fe render runtime: Fe recovery policy failed a GPU-only surface",
      ));
      return;
    }
    if (action !== SurfaceRecoveryAction.DegradeToWasm || !this._kernel) {
      throw new Error(
        "fe render runtime: Fe recovery policy selected an unavailable Wasm fallback",
      );
    }

    this._mode = "wasm-2d";
    this._recoveryObservedLoss = false;
    this._applyBackingExtent(null);
    this._resizePresentationCanvases();
    if (this._recoveryWasLive && !deferPresentation) {
      this._renderWasmInto(
        this._adoptedCanvas || this._posterCanvas,
        this._backingWidth,
        this._backingHeight,
        this._uniforms,
      );
      if (!this._adoptedCanvas) {
        this._liveCanvas.hidden = true;
        this._posterCanvas.hidden = false;
      }
    }
    this._updateBadge();
    this._dispatch("fe-statechange", { state: this._fsm, reason: "device-unavailable" });
  }

  async _completeDeviceRecovery(freshGpu, _lostGeneration) {
    if (!this._recoveryObservedLoss) return;
    this._recoveryObservedLoss = false;
    this._runSurfaceRecovery(
      GpuDeviceEventKind.Available,
      GpuDeviceLossReason.NotLost,
      this._deviceRecoveryRequired(),
      Boolean(this._kernel),
      freshGpu.generation ?? sharedGpuGeneration,
    );
    this._deliverSurfaceLifecycle(SurfaceEventKind.DeviceRecovered);
    if (!this._recoveryWasLive) return;
    this._fsm = "ready"; // force `_goLive` through the fresh pipeline-build path.
    await this._goLive();
    this._dispatch("fe-statechange", { state: this._fsm, reason: "device-recovered" });
  }

  _teardown() {
    this._stopScopedTasks();
    this._scopedTaskBroker = null;
    this._scopedTaskMachines = null;
    this._scopedTasksNeedReboot = false;
    this._actor?.close();
    this._actor = null;
    if (this._gestureFrame !== null) cancelAnimationFrame(this._gestureFrame);
    this._gestureFrame = null;
    this._surfaceFrameRequested = false;
    this._gestureDirty = false;
    this._pendingSurfaceEvents = [];
    this._surfaceTransitionMemory = null;
    this._surfaceTransitionAlloc = null;
    this._wasmArenaReset = null;
    this._gpuReadbackKernel = null;
    this._gpuReadbackBinding = null;
    this._gpuReadbackResource = null;
    this._surfaceStateReplaceKernel = null;
    this._surfaceTransitionStateResident = false;
    this._surfaceScheduleKernel = null;
    this._surfaceRecoveryKernel = null;
    this._recoveryObservedLoss = false;
    this._recoveryWasLive = false;
    this._surfaceQualityKernel = null;
    this._unwireResizeObserver();
    if (this._liveContext) {
      try {
        this._liveContext.unconfigure();
      } catch {
        // already invalid.
      }
    }
    if (this._adoptedContext) {
      try {
        this._adoptedContext.unconfigure();
      } catch {
        // already invalid.
      }
    }
    this._liveContext = null;
    this._adoptedContext = null;
    this._releaseGpuResources();
    this._posterAttemptedDevice = null;
    this._suspendObserver?.disconnect();
    this._suspendObserver = null;
    this._activationObserver?.disconnect();
    this._activationObserver = null;
    this._unwireGestures();
  }

  async _prepareScopedTasks(wasmModule) {
    const reference = this.getAttribute("data-fe-scoped-tasks");
    if (!reference) return null;
    const taskModule = await import(new URL(reference, this.baseURI).href);
    if (typeof taskModule.createMaterializedTaskRegistry !== "function" ||
        typeof taskModule.createHostCompletionBroker !== "function") {
      throw new Error("compiler-published scoped-task package has an invalid fixed interface");
    }
    const required = WebAssembly.Module.imports(wasmModule);
    const needsWorkerScope = required.some(value => value.module === "fe:worker-scope");
    const needsWorkerMailbox = required.some(value => value.module === "fe:worker-mailbox");
    const brokerOptions = {};
    let structuredWorkerScopes = [];
    if (needsWorkerScope || needsWorkerMailbox) {
      if (typeof taskModule.createStructuredWorkerScopes !== "function") {
        throw new Error("Worker effects require compiler-derived structured child packages");
      }
      structuredWorkerScopes = await taskModule.createStructuredWorkerScopes();
      brokerOptions.workerScopes = structuredWorkerScopes;
    }
    const broker = taskModule.createHostCompletionBroker(brokerOptions);
    let mailboxImports;
    if (needsWorkerMailbox) {
      if (typeof taskModule.createStructuredWorkerMailboxes !== "function") {
        throw new Error("fe:worker-mailbox requires a compiler-derived mailbox adapter");
      }
      mailboxImports = taskModule.createStructuredWorkerMailboxes(
        structuredWorkerScopes,
        broker.completions,
      );
    }
    const imports = Object.create(null);
    const merge = additions => {
      if (!additions) return;
      for (const [moduleName, values] of Object.entries(additions)) {
        const target = imports[moduleName] ?? (imports[moduleName] = Object.create(null));
        for (const [name, value] of Object.entries(values)) {
          if (Object.hasOwn(target, name) && target[name] !== value) {
            throw new Error(`conflicting fixed Wasm import: ${moduleName}.${name}`);
          }
          target[name] = value;
        }
      }
    };
    merge(broker.imports);
    merge(mailboxImports);
    for (const value of required) {
      if (!Object.hasOwn(imports, value.module) ||
          !Object.hasOwn(imports[value.module], value.name)) {
        throw new Error(`missing Wasm import: ${value.module}.${value.name}`);
      }
    }
    return { taskModule, broker, imports, mailboxes: mailboxImports };
  }

  _attachScopedTasks(scopedTasks, instance) {
    if (!scopedTasks) return;
    scopedTasks.mailboxes?.attach(instance.exports);
    const registry = scopedTasks.taskModule.createMaterializedTaskRegistry(instance.exports);
    const machines = Object.values(registry);
    if (machines.length === 0) {
      throw new Error("compiler-published scoped-task package contains no task machines");
    }
    this._scopedTaskBroker = scopedTasks.broker;
    this._scopedTaskMachines = machines;
  }

  _startScopedTasks() {
    if (!this.isConnected || !this._scopedTaskMachines || this._scopedTaskLifetime) return;
    const lifetime = new AbortController();
    this._scopedTaskLifetime = lifetime;
    try {
      for (const machine of this._scopedTaskMachines) {
        const inputWidth = machine.inputWidth ?? 0;
        if (!Number.isSafeInteger(inputWidth) || inputWidth < 0) {
          throw new TypeError("scoped Fe task has an invalid compiler-derived input width");
        }
        let input = [];
        if (inputWidth !== 0) {
          if (inputWidth !== this._uniforms.length) {
            throw new Error(
              `scoped Fe task input has ${inputWidth} lanes; surface state has ${this._uniforms.length}`,
            );
          }
          if (typeof machine.liftInput !== "function") {
            throw new TypeError("scoped Fe task has no compiler-derived input lifter");
          }
          input = machine.liftInput(this._uniforms);
        }
        this._scopedTaskBroker.run(machine, input, { signal: lifetime.signal }).catch(error => {
          if (!lifetime.signal.aborted && error?.name !== "AbortError") this._fail(error);
        });
      }
    } catch (error) {
      lifetime.abort();
      this._scopedTaskLifetime = null;
      this._scopedTaskBroker.cancelAll();
      throw error;
    }
  }

  _stopScopedTasks() {
    this._scopedTaskLifetime?.abort();
    this._scopedTaskLifetime = null;
    this._scopedTaskBroker?.cancelAll();
  }

  async _bootCanonicalActor(wasm) {
    const canonical = this._manifest?.canonical_interface;
    if (!canonical) return;
    const runtime = this._manifest?.browser_runtime;
    const clientArtifact = runtime?.artifacts?.find((artifact) =>
      typeof artifact?.path === "string" && artifact.path.endsWith("/actor-client.js"));
    if (!clientArtifact) {
      throw new Error("fe render runtime: canonical interface has no generated actor client");
    }
    const clientUrl = new URL(clientArtifact.path, this._manifestUrl);
    const { createCanonicalBrowserActor } = await import(clientUrl.href);
    if (typeof createCanonicalBrowserActor !== "function") {
      throw new Error("fe render runtime: generated actor client has no constructor");
    }
    // Main-thread effects are admitted by the generated intent router. Until
    // the corresponding standards capability provider is connected, invoking
    // one rejects explicitly; Worker/Wasm lanes remain fully usable. This is
    // an honest unavailable capability, not a fabricated application result.
    const handlers = Object.fromEntries(canonical.lanes
      .filter((lane) => lane.intent.execution === "host_effect"
        && lane.intent.placement === "main_thread")
      .map((lane) => [lane.name, async () => {
        throw new Error(
          `fe render runtime: host capability for canonical lane ${lane.name} is not connected`,
        );
      }]));
    this._actor = await createCanonicalBrowserActor({ wasm, handlers });
    this._dispatch("fe-actor-ready", {
      lanes: canonical.lanes.map((lane) => lane.name),
    });
  }

  // -- typed surface facts + legacy gestures --------------------------------
  //
  // Fe owns ALL gesture semantics (pan sensitivity, the zoom curve, the
  // cursor anchor, the clamps): this element delivers only raw pointer/wheel
  // facts to Fe. A typed `SurfaceTransition` is discovered directly from its
  // fixed Wasm ABI export and replaces the complete actor state; it has no
  // manifest control block, argument-name switch, or result-name mapping. The
  // older manifest lane remains below only for explicitly legacy bundles.

  /** Attach pointer/wheel listeners on the current live/adopted canvas, once
   * per canvas identity (idempotent across suspend/resume within one boot).
   * Captured drag is the compatibility default. A compiler-projected Fe actor
   * capability may additionally request raw uncaptured primary hover motion. */
  _wireGestures() {
    if (!this._surfaceTransitionKernel && (!this._control || !this._controlKernel)) return;
    const canvas = this._adoptedCanvas || (this._mode === "webgpu" ? this._liveCanvas : this._posterCanvas);
    if (!canvas || this._gestureListeners?.canvas === canvas) return;
    this._unwireGestures();
    const touchAction = canvas.style.touchAction;
    canvas.style.touchAction = "none";

    let dragging = false;
    let dragPointerId = null;
    let lastDragPoint = null;
    const hoverMotion =
      this._surface?.pointer_motion === "hover_and_captured_drag";

    const backingPoint = (event) => {
      const rect = canvas.getBoundingClientRect();
      const scaleX = this._backingWidth / (rect.width || 1);
      const scaleY = this._backingHeight / (rect.height || 1);
      return { mx: (event.clientX - rect.left) * scaleX, my: (event.clientY - rect.top) * scaleY, scaleX, scaleY };
    };

    const onPointerDown = (event) => {
      if (event.button !== 0 || dragging) return;
      dragging = true;
      dragPointerId = event.pointerId;
      const { mx, my } = backingPoint(event);
      lastDragPoint = { mx, my };
      canvas.setPointerCapture(event.pointerId);
      this._applyGesture({
        dx: 0,
        dy: 0,
        wheelDelta: 0,
        wheelMode: 0,
        mx,
        my,
        buttons: event.buttons,
        timestamp: event.timeStamp,
        eventKind: SurfaceEventKind.PointerDown,
      });
      event.preventDefault();
    };
    const onPointerMove = (event) => {
      const { mx, my } = backingPoint(event);
      if (!dragging) {
        if (!hoverMotion || event.isPrimary === false) return;
        this._applyGesture({
          dx: 0,
          dy: 0,
          wheelDelta: 0,
          wheelMode: 0,
          mx,
          my,
          buttons: event.buttons,
          timestamp: event.timeStamp,
          eventKind: SurfaceEventKind.PointerMove,
        });
        return;
      }
      if (event.pointerId !== dragPointerId) return;
      const previous = lastDragPoint;
      lastDragPoint = { mx, my };
      if (!previous) return;
      event.preventDefault();
      this._applyGesture({
        dx: mx - previous.mx,
        dy: my - previous.my,
        wheelDelta: 0,
        wheelMode: 0,
        mx,
        my,
        buttons: event.buttons,
        timestamp: event.timeStamp,
        eventKind: SurfaceEventKind.PointerMove,
      });
    };
    const onPointerUp = (event) => {
      if (event.pointerId !== dragPointerId) return;
      const { mx, my } = backingPoint(event);
      this._applyGesture({
        dx: 0,
        dy: 0,
        wheelDelta: 0,
        wheelMode: 0,
        mx,
        my,
        buttons: event.buttons,
        timestamp: event.timeStamp,
        eventKind: SurfaceEventKind.PointerUp,
      });
      dragging = false;
      dragPointerId = null;
      lastDragPoint = null;
      try {
        canvas.releasePointerCapture(event.pointerId);
      } catch {
        // capture already released (e.g. pointercancel).
      }
    };
    const onWheel = (event) => {
      event.preventDefault();
      const { mx, my } = backingPoint(event);
      this._applyGesture({
        dx: 0,
        dy: 0,
        wheelDelta: event.deltaY,
        wheelMode: event.deltaMode,
        mx,
        my,
        buttons: event.buttons,
        timestamp: event.timeStamp,
      });
    };

    canvas.addEventListener("pointerdown", onPointerDown);
    canvas.addEventListener("pointermove", onPointerMove);
    canvas.addEventListener("pointerup", onPointerUp);
    canvas.addEventListener("pointercancel", onPointerUp);
    canvas.addEventListener("lostpointercapture", onPointerUp);
    canvas.addEventListener("wheel", onWheel, { passive: false });
    this._gestureListeners = {
      canvas, onPointerDown, onPointerMove, onPointerUp, onWheel, touchAction,
    };
  }

  _unwireGestures() {
    const listeners = this._gestureListeners;
    if (!listeners) return;
    const { canvas, onPointerDown, onPointerMove, onPointerUp, onWheel, touchAction } = listeners;
    canvas.removeEventListener("pointerdown", onPointerDown);
    canvas.removeEventListener("pointermove", onPointerMove);
    canvas.removeEventListener("pointerup", onPointerUp);
    canvas.removeEventListener("pointercancel", onPointerUp);
    canvas.removeEventListener("lostpointercapture", onPointerUp);
    canvas.removeEventListener("wheel", onWheel);
    canvas.style.touchAction = touchAction;
    this._gestureListeners = null;
  }

  /** One raw gesture in: build `control.export`'s positional args from
   * `control.args` (live state by name, or the raw delta), call it, and blit
   * the reply back into `_uniforms` by `control.result`'s names. No writes
   * while cold/error/not presenting. */
  _applyGesture(raw) {
    if (this._fsm !== "live") return;
    const surfaceEvent = {
      ...raw,
      width: this._backingWidth,
      height: this._backingHeight,
      eventKind: raw.eventKind ?? SurfaceEventKind.Gesture,
      paramIndex: 0,
      paramValue: 0,
    };
    if (this._surfaceTransitionKernel) {
      if (this._surfaceTransitionSchedule === "resident") {
        if (!this._surfaceScheduleKernel) {
          throw new Error(
            "fe render runtime: scheduled surface input cannot fall back to JavaScript scheduling",
          );
        }
        this._pendingSurfaceEvents.push(surfaceEvent);
        this._notifyScheduledInput(surfaceEvent.eventKind, surfaceEvent.timestamp);
        return;
      }
      const next = this._runSurfaceTransition(surfaceEvent);
      if (next) this._queueGestureRender(next);
      return;
    }
    if (!this._controlKernel) return;
    const control = this._control;
    const args = control.args.map((arg) => {
      switch (arg.source) {
        case "state": {
          const index = this._memberIndexByName.get(arg.name);
          return index === undefined ? 0 : this._uniforms[index];
        }
        case "resource":
          return arg.wasm_type === "i64" ? 0n : 0;
        case "drag":
          return arg.axis === "x" ? surfaceEvent.dx : surfaceEvent.dy;
        case "wheel":
          return Math.sign(surfaceEvent.wheelDelta);
        case "pointer":
          return arg.axis === "x" ? surfaceEvent.mx : surfaceEvent.my;
        default:
          return 0;
      }
    });
    const reply = this._runWasmArenaEpoch(() => this._controlKernel(...args));
    const results = Array.isArray(reply) ? reply : [reply];
    const next = this._uniforms.slice();
    control.result.forEach((name, index) => {
      const memberIndex = this._memberIndexByName.get(name);
      if (memberIndex !== undefined) next[memberIndex] = results[index];
    });
    this._queueGestureRender(next);
  }

  _runSurfaceTransition(raw) {
    // Fixed DFS layout of std::web::SurfaceEvent, followed by the actor's
    // fields in declaration order. Fragment context args precede actor state
    // in the GPU signature; missing positions in that suffix are external
    // resource handles and cross into control-only Wasm as inert i64 zeroes.
    const eventArgs = [
      raw.mx,
      raw.my,
      raw.dx,
      raw.dy,
      raw.wheelDelta,
      raw.wheelMode,
      raw.buttons,
      raw.timestamp,
      raw.width,
      raw.height,
      raw.eventKind,
      raw.paramIndex,
      raw.paramValue,
    ];
    const reply = this._runWasmArenaEpoch(() =>
      this._surfaceTransitionKernel(...eventArgs, ...this._surfaceActorArgs()));
    return this._surfaceReply(reply);
  }

  _surfaceActorArgs() {
    // A paired raster fragment may have authored varying parameters before
    // actor state in its GPU signature. Those varying slots are not part of
    // the Fe transition ABI. With no resources, declaration-order state is
    // exactly the compiler-projected member sequence.
    if (this._resources.length === 0) {
      return [...this._members]
        .sort((left, right) => left.arg_index - right.arg_index)
        .map((member) => this._uniforms[this._memberIndexByName.get(member.name)]);
    }
    const actorArgStart = 1 + Math.max(-1, ...this._builtins.map((builtin) => builtin.arg_index));
    const actorArgEnd = actorArgStart + this._members.length + this._resources.length;
    const actorArgs = [];
    for (let argIndex = actorArgStart; argIndex < actorArgEnd; argIndex += 1) {
      const memberIndex = this._memberIndexByArg.get(argIndex);
      actorArgs.push(memberIndex === undefined ? 0n : this._uniforms[memberIndex]);
    }
    return actorArgs;
  }

  _surfaceResourceArgs() {
    // Resident transitions already own their scalar state. Their fixed Wasm
    // ABI therefore receives one inert host handle per declared resource,
    // independent of vertex/fragment-only parameters in the GPU signature.
    return this._resources.map(() => 0n);
  }

  /** Route a DOM slider or scripted `.params` write through the same authored
   * Fe transition as pointer/wheel input. The fixed host contributes only the
   * declaration-order index and untouched proposed value. Older, untyped
   * bundles retain their compatibility replacement path. */
  _applyParamEdit(index, value, paramIndex = index) {
    if (this._surfaceTransitionKernel) {
      if (
        this._surfaceTransitionSchedule === "resident" &&
        !this._surfaceScheduleKernel
      ) {
        throw new Error(
          "fe render runtime: scheduled parameter input cannot fall back to JavaScript scheduling",
        );
      }
      const event = {
        mx: 0,
        my: 0,
        dx: 0,
        dy: 0,
        wheelDelta: 0,
        wheelMode: 0,
        buttons: 0,
        timestamp: globalThis.performance?.now?.() ?? 0,
        width: this._backingWidth,
        height: this._backingHeight,
        eventKind: SurfaceEventKind.ParamEdit,
        paramIndex,
        paramValue: value,
      };
      if (
        this._surfaceTransitionSchedule === "resident" &&
        this._surfaceScheduleKernel
      ) {
        // The generated Wasm batch boundary preserves heterogeneous event
        // order. The host therefore keeps an older gesture and this untouched
        // edit in one raw queue and performs no eager application transition.
        this._pendingSurfaceEvents.push(event);
        this._notifyScheduledInput(event.eventKind, event.timestamp);
        return;
      }
      const next = this._surfaceTransitionSchedule === "resident"
        ? this._runSurfaceFrame([event])
        : this._runSurfaceTransition(event);
      if (!next) return;
      this._uniforms = next;
      void this._render().catch((error) => this._fail(error));
      return;
    }

    const next = this._uniforms.slice();
    next[index] = value;
    void this._render(next).catch((error) => this._fail(error));
  }

  /** Seed or explicitly replace the complete state of a resident Fe actor.
   * This is an external-boundary operation (initialization, extent change, or
   * explicit restoration), never a user-input transition path. */
  _replaceSurfaceState(next) {
    this._uniforms = next;
    if (this._surfaceStateReplaceKernel) {
      this._runWasmArenaEpoch(() => this._surfaceStateReplaceKernel(...next));
    }
  }

  /** Run one externally initiated Fe call in a fresh canonical arena epoch.
   * Aggregate storage is call-local: scalar resident actor globals survive a
   * reset, while matrices, records, and event transport from a completed (or
   * trapped) call do not accumulate across browser frames. */
  _runWasmArenaEpoch(call) {
    if (!this._wasmArenaReset) return call();
    this._wasmArenaReset();
    try {
      return call();
    } finally {
      this._wasmArenaReset();
    }
  }

  /** Deliver one opaque GPU buffer snapshot into its compiler-selected Fe
   * message transition. The host owns only WebGPU mapping and Wasm memory
   * transfer. Fe owns decoding, validation, state change, and any reply. */
  async _deliverGpuReadback(readback) {
    const bytes = await readGpuBufferSnapshot(readback);
    const next = this._runWasmArenaEpoch(() => {
      const pointer = this._surfaceTransitionAlloc(bytes.byteLength, 4);
      if (!Number.isInteger(pointer) || pointer < 0) {
        throw new Error("fe render runtime: GPU readback message allocation failed");
      }
      const memory = this._surfaceTransitionMemory;
      if (pointer + bytes.byteLength > memory.buffer.byteLength) {
        throw new Error("fe render runtime: GPU readback message exceeds Wasm memory");
      }
      new Uint8Array(memory.buffer, pointer, bytes.byteLength).set(bytes);
      const reply = this._gpuReadbackKernel(
        pointer,
        bytes.byteLength,
        ...this._surfaceResourceArgs(),
      );
      return this._surfaceReplyValues(reply, "typed GPU readback");
    });
    this._uniforms = next;
    this._refreshControlValues();
    return next;
  }

  _runSurfaceFrame(events) {
    if (events.length === 0 || events.length > MAX_SURFACE_EVENT_BATCH) {
      throw new Error(`fe render runtime: invalid surface event batch length ${events.length}`);
    }
    return this._runWasmArenaEpoch(() => {
      // The event bytes belong to this call's epoch. Caching an arena pointer
      // across resets would let later Fe aggregate allocations overwrite it.
      const pointer = this._surfaceTransitionAlloc(events.length * SURFACE_EVENT_STRIDE, 4);
      if (!Number.isInteger(pointer) || pointer < 0) {
        throw new Error("fe render runtime: surface event batch allocation failed");
      }
      writeSurfaceEventBatch(this._surfaceTransitionMemory, pointer, events);
      const reply = this._surfaceTransitionKernel(
        pointer,
        events.length,
        ...(this._surfaceTransitionStateResident
          ? this._surfaceResourceArgs()
          : this._surfaceActorArgs()),
      );
      return this._surfaceReply(reply);
    });
  }

  /** Compatibility delivery of one typed clock/lifecycle fact to an older
   * application transition. Canonical scheduled bundles deliver these facts to
   * their distinct resident Fe policy instead, leaving queued application
   * input untouched for the permitted presentation frame. */
  _deliverSurfaceBoundary(
    kind,
    timestamp = globalThis.performance?.now?.() ?? 0,
    includePending = false,
  ) {
    if (!this._surfaceTransitionKernel) return null;
    const event = this._surfaceBoundaryEvent(kind, timestamp);
    const next = this._surfaceTransitionSchedule === "resident"
      ? this._runSurfaceFrame([
          ...(includePending ? this._pendingSurfaceEvents.splice(0) : []),
          event,
        ])
      : this._runSurfaceTransition(event);
    if (!next) return null;
    this._uniforms = next;
    this._refreshControlValues();
    return next;
  }

  /** Construct one fixed standards-boundary surface fact. The same typed
   * record feeds compatibility transitions and resident continuous frames. */
  _surfaceBoundaryEvent(kind, timestamp = globalThis.performance?.now?.() ?? 0) {
    return {
      mx: 0,
      my: 0,
      dx: 0,
      dy: 0,
      wheelDelta: 0,
      wheelMode: 0,
      buttons: 0,
      timestamp,
      width: this._backingWidth,
      height: this._backingHeight,
      eventKind: kind,
      paramIndex: 0,
      paramValue: 0,
    };
  }

  /** Invoke the generated resident Fe presentation policy. Private policy
   * state remains in Wasm; the fixed host sees two booleans plus one bounded
   * raw-queue effect and performs no dirty/presenting inference of its own. */
  _runSurfaceSchedule(
    kind,
    timestamp = globalThis.performance?.now?.() ?? 0,
    pendingEvents = this._pendingSurfaceEvents.length,
  ) {
    if (!this._surfaceScheduleKernel) return null;
    const reply = this._runWasmArenaEpoch(() =>
      this._surfaceScheduleKernel(kind, timestamp, pendingEvents));
    const decisions = Array.isArray(reply) ? reply : [reply];
    const expected = this._surfaceScheduleHasQueueAction ? 3 : 2;
    if (
      decisions.length !== expected ||
      ![decisions[0], decisions[1]].every((value) => value === 0 || value === 1) ||
      (expected === 3 && !Number.isInteger(decisions[2])) ||
      (expected === 3 &&
        (decisions[2] < SurfaceQueueAction.Retain || decisions[2] > SurfaceQueueAction.Drop))
    ) {
      throw new Error(
        "fe render runtime: resident surface policy must return present/request_frame and a valid queue action",
      );
    }
    return {
      present: decisions[0] === 1,
      requestFrame: decisions[1] === 1,
      queueAction: expected === 3 ? decisions[2] : SurfaceQueueAction.Retain,
    };
  }

  /** Invoke the actor-level Fe device policy. Its two supervision fields stay
   * resident in Wasm; only the selected recovery action crosses this boundary.
   * This exists independently of application transitions and presentation
   * scheduling, so a pure compute/render pass graph owns the same policy. */
  _runSurfaceRecovery(
    kind,
    reason,
    deviceRequired,
    softwareFallback,
    generation,
  ) {
    if (!this._surfaceRecoveryKernel) return null;
    const reply = this._runWasmArenaEpoch(() => this._surfaceRecoveryKernel(
      kind,
      reason,
      deviceRequired ? 1 : 0,
      softwareFallback ? 1 : 0,
      generation,
    ));
    const decisions = Array.isArray(reply) ? reply : [reply];
    if (
      decisions.length !== 1 ||
      !Number.isInteger(decisions[0]) ||
      decisions[0] < SurfaceRecoveryAction.NoAction ||
      decisions[0] > SurfaceRecoveryAction.FailSurface
    ) {
      throw new Error(
        "fe render runtime: resident surface recovery policy returned an invalid action",
      );
    }
    return decisions[0];
  }

  /** Realize the Fe policy's bounded raw-queue effect. Numeric identities are
   * declaration-order tags of the fixed `SurfaceQueueAction` enum, not a
   * demo-authored protocol or manifest table. */
  _applySurfaceQueueAction(decision) {
    if (!decision || decision.queueAction === SurfaceQueueAction.Retain) return;
    if (decision.queueAction === SurfaceQueueAction.KeepLatest) {
      const latest = this._pendingSurfaceEvents.at(-1);
      this._pendingSurfaceEvents = latest ? [latest] : [];
      return;
    }
    if (decision.queueAction === SurfaceQueueAction.Drop) {
      this._pendingSurfaceEvents = [];
    }
  }

  /** Realize the selected Fe policy's bounded queue and browser-frame effects.
   * The remembered request lets a Visible fact arrive before `_enterLive`
   * without losing the first continuous frame. */
  _realizeSurfaceSchedule(decision) {
    if (!decision) return;
    this._applySurfaceQueueAction(decision);
    this._surfaceFrameRequested = decision.requestFrame;
    if (this._surfaceFrameRequested) this._scheduleGestureFrame();
  }

  /** Notify Fe that one untouched application input entered the raw queue.
   * Fe alone chooses retention and whether the browser should request a frame. */
  _notifyScheduledInput(kind, timestamp) {
    const decision = this._runSurfaceSchedule(kind, timestamp);
    if (!decision) {
      throw new Error("fe render runtime: resident input has no Fe scheduling decision");
    }
    if (decision.present) {
      throw new Error("fe render runtime: Fe requested presentation outside an animation frame");
    }
    this._realizeSurfaceSchedule(decision);
  }

  /** Deliver a standards fact to the resident Fe scheduling policy without
   * consuming queued input. Older bundles fall back to the application
   * transition. The host then realizes only Fe's explicit request-frame
   * decision. */
  _deliverSurfaceLifecycle(kind, timestamp = globalThis.performance?.now?.() ?? 0) {
    // A resident scheduling actor is the Fe owner of clock/lifecycle policy.
    // Older modules without it retain the prior application-transition path.
    if (!this._surfaceScheduleKernel) {
      this._deliverSurfaceBoundary(kind, timestamp, false);
    }
    const decision = this._runSurfaceSchedule(kind, timestamp);
    this._realizeSurfaceSchedule(decision);
    return decision;
  }

  _surfaceReply(reply) {
    return this._surfaceReplyValues(reply, "typed surface transition", true);
  }

  _surfaceReplyValues(reply, source, disableTransitionOnMismatch = false) {
    const next = Array.isArray(reply) ? reply : [reply];
    if (next.length !== this._members.length) {
      const error = new Error(
        `fe web: ${source} returned ${next.length} fields; expected ${this._members.length}`,
      );
      if (!disableTransitionOnMismatch) throw error;
      console.error(error.message);
      this._surfaceTransitionKernel = null;
      this._surfaceStateReplaceKernel = null;
      this._surfaceScheduleKernel = null;
      this._surfaceTransitionStateResident = false;
      this._surfaceTransitionSchedule = "immediate";
      this._pendingSurfaceEvents = [];
      return null;
    }
    return next;
  }

  /** Queue the newest Fe-computed state for presentation. For a resident
   * scheduled transition the raw event reaches this queue first and Fe runs
   * inside `_flushGestureFrame`; immediate and legacy lanes have already
   * computed `next`. A graph presentation still records its complete ordered
   * pass list in one command buffer. */
  _queueGestureRender(next) {
    this._uniforms = next;
    this._refreshControlValues();
    if (this._fsm !== "live") return;
    this._gestureDirty = true;
    this._scheduleGestureFrame();
  }

  _scheduleGestureFrame() {
    if (this._gestureFrame !== null || this._fsm !== "live") return;
    if (this._surfaceScheduleKernel) {
      if (!this._surfaceFrameRequested) return;
      this._surfaceFrameRequested = false;
    } else if (this._gesturePresenting || !this._gestureDirty) {
      return;
    }
    this._gestureFrame = requestAnimationFrame((timestamp) => {
      this._gestureFrame = null;
      void this._flushGestureFrame(timestamp).catch((error) => this._fail(error));
    });
  }

  async _flushGestureFrame(timestamp = globalThis.performance?.now?.() ?? 0) {
    if (this._surfaceScheduleKernel) {
      if (this._fsm !== "live") return;
      const decision = this._runSurfaceSchedule(
        SurfaceEventKind.AnimationFrame,
        timestamp,
      );
      this._realizeSurfaceSchedule(decision);
      if (!decision?.present) return;

      const events = [
        ...this._pendingSurfaceEvents.splice(0),
        this._surfaceBoundaryEvent(SurfaceEventKind.AnimationFrame, timestamp),
      ];
      const next = this._surfaceTransitionSchedule === "resident"
        ? this._runSurfaceFrame(events)
        : null;
      if (this._surfaceTransitionSchedule === "resident" && !next) return;
      if (next) {
        this._uniforms = next;
        this._refreshControlValues();
      }
      try {
        await this._render();
        const queue = this._mode === "webgpu" ? this._gpu?.device?.queue : null;
        if (queue?.onSubmittedWorkDone) await awaitSharedGpuQueueIdle(this._gpu);
      } finally {
        const complete = this._runSurfaceSchedule(SurfaceEventKind.GpuComplete);
        this._realizeSurfaceSchedule(complete);
      }
      return;
    }

    // Compatibility state machine for old bundles that predate the resident
    // Fe policy export. A scheduled typed transition is rejected during boot
    // and again here, so canonical gallery artifacts cannot take this branch.
    if (this._surfaceTransitionSchedule === "resident") {
      throw new Error(
        "fe render runtime: scheduled surface transition reached the legacy JavaScript state machine",
      );
    }
    if (this._fsm !== "live" || !this._gestureDirty || this._gesturePresenting) return;
    this._gestureDirty = false;
    this._gesturePresenting = true;
    try {
      if (this._surfaceTransitionKernel) {
        if (!this._deliverSurfaceBoundary(SurfaceEventKind.AnimationFrame, timestamp, true)) return;
      }
      await this._render();
      const queue = this._mode === "webgpu" ? this._gpu?.device?.queue : null;
      if (queue?.onSubmittedWorkDone) {
        await awaitSharedGpuQueueIdle(this._gpu);
        this._deliverSurfaceBoundary(SurfaceEventKind.GpuComplete, undefined, false);
      }
    } finally {
      this._gesturePresenting = false;
      if (this._fsm === "live" && this._gestureDirty) this._scheduleGestureFrame();
    }
  }

  // -- chrome: badge / controls / meta --------------------------------------

  _updateBadge() {
    if (!this._badge) return;
    const provenance = this._manifest?.provenance || {};
    const feResponsibilities = provenance.fe_responsibilities || [];
    const feControl = feResponsibilities.includes("control_transition");
    const feSchedule = feResponsibilities.includes("scheduling_policy");
    const feState = feResponsibilities.includes("resident_actor_state");
    const wasmRoles = feControl
      ? ` + control${feSchedule ? "/schedule" : ""}${feState ? "/state" : ""} Wasm`
      : "";
    this._badge.textContent = this._mode === "webgpu"
      ? `Fe WGSL${wasmRoles} · fixed JS host`
      : "Fe Wasm renderer · fixed JS host";
    const hostArtifact = provenance.fixed_host?.artifact;
    const runtimeIdentity = hostArtifact?.sha256
      ? `${hostArtifact.path} · sha256:${hostArtifact.sha256}`
      : "fe-render-runtime · unpinned build artifact";
    const hostResponsibilities = provenance.fixed_host?.responsibilities || [];
    this._badge.title = `Fe owns: ${feResponsibilities.join(", ") || "GPU program"}. Host owns: ${hostResponsibilities.join(", ") || "browser API realization"}. ${runtimeIdentity}`;
    this._badge.className = `badge ${this._mode === "webgpu" ? "webgpu" : "wasm-2d"}`;
  }

  /**
   * Controls generated from the declared v5 `surface.params`: real label (the
   * field name), doc hover, range, scale, readout, and widget from the explicit
   * Fe presentation plan. Each param maps to its uniform member by NAME (the
   * reconciled binding key).
   */
  _renderControls() {
    this._updateBadge();
    const controlsAttr = this.getAttribute("controls") || "auto";
    this._panel.innerHTML = "";
    this._controlRows = [];
    if (controlsAttr === "none") return;
    if (!this._surface) {
      const notice = document.createElement("div");
      notice.className = "control notice";
      notice.setAttribute("part", "control");
      notice.textContent = this._members.length
        ? `no view() declared: ${this._members.length} uniform member(s) held at 1.0`
        : "no view() declared";
      this._panel.append(notice);
      return;
    }
    this._surface.params.forEach((param, paramIndex) => {
      const presentation = surfaceParamPlan(
        param,
        this._manifest?.protocol_version ?? 8,
      );
      if (presentation.widget === "hidden") return;
      const index = this._memberIndexByName.get(param.name);
      if (index === undefined) return;
      const member = this._members[index];
      const row = document.createElement("div");
      row.className = "control";
      row.setAttribute("part", "control");
      const doc = member.doc || param.doc;
      if (doc) row.title = doc;
      const label = document.createElement("label");
      const value = document.createElement("b");
      const isInt = presentation.readout === "integer";
      const isToggle = presentation.readout === "toggle";
      const optionLabels = presentation.options ?? [];
      const min = typeof param.min === "number" ? param.min : 0;
      const max = typeof param.max === "number" ? param.max : 1;
      const isLog = presentation.scale === "logarithmic";
      if (isLog && !(min > 0 && max > min)) {
        throw new Error(
          `fe render runtime: logarithmic param \`${param.name}\` requires positive ordered bounds`,
        );
      }
      const encode = isLog
        ? (v) => Math.log10(Math.max(min, Math.min(max, +v)))
        : (v) => +v;
      const decode = isLog ? (v) => 10 ** (+v) : (v) => +v;
      const format = (v) => {
        const number = +v;
        if (isToggle) return number >= 0.5 ? "on" : "off";
        if (presentation.widget === "select") {
          return optionLabels[Math.round(number) - Math.ceil(min)] ?? number.toFixed(0);
        }
        if (isInt) return number.toFixed(0);
        if (isLog && (number < 0.01 || number >= 1000)) return number.toExponential(2);
        return Number(number.toPrecision(8)).toString();
      };
      value.textContent = format(this._uniforms[index]);
      const name = document.createElement("span");
      name.textContent = param.name;
      label.append(name, value);
      const input = document.createElement(
        presentation.widget === "select" ? "select" : "input",
      );
      if (presentation.widget === "select") {
        for (let optionValue = Math.ceil(min); optionValue <= Math.floor(max); optionValue += 1) {
          const option = document.createElement("option");
          option.value = String(optionValue);
          option.textContent = optionLabels[optionValue - Math.ceil(min)] ?? String(optionValue);
          input.append(option);
        }
        input.value = String(Math.round(this._uniforms[index]));
        input.oninput = () => {
          this._applyParamEdit(index, +input.value, paramIndex);
        };
      } else if (presentation.widget === "checkbox") {
        input.type = "checkbox";
        input.checked = this._uniforms[index] >= 0.5;
        input.oninput = () => {
          this._applyParamEdit(index, input.checked ? 1 : 0, paramIndex);
        };
      } else {
        input.type = "range";
        const inputMin = isLog ? Math.log10(min) : min;
        const inputMax = isLog ? Math.log10(max) : max;
        input.min = String(inputMin);
        input.max = String(inputMax);
        input.step = isInt ? "1" : String((inputMax - inputMin) / 200 || 0.01);
        input.value = String(encode(this._uniforms[index]));
        input.oninput = () => {
          this._applyParamEdit(index, decode(input.value), paramIndex);
        };
      }
      row.append(label, input);
      this._panel.append(row);
      this._controlRows.push({ index, input, value, format, encode, isToggle });
    });
  }

  _refreshControlValues() {
    for (const row of this._controlRows) {
      row.value.textContent = row.format(this._uniforms[row.index]);
      if (row.isToggle) {
        row.input.checked = this._uniforms[row.index] >= 0.5;
      } else {
        row.input.value = String(row.encode(this._uniforms[row.index]));
      }
    }
  }

  _updateMeta() {
    if (!this._meta) return;
    const link = (href, text, kind) => {
      const rawAction = this.getAttribute(`data-fe-${kind}-action`);
      const action = Number(rawAction);
      const actionAttribute = Number.isInteger(action) && action >= 0
        ? ` data-fe-action="${action >>> 0}"`
        : "";
      return `<a href="${href}" target="_blank" rel="noopener"${actionAttribute}>${text}</a>`;
    };
    const wasm = this._wasmUrl
      ? link(this._wasmUrl.href, `wasm ${this._manifest.artifacts.wasm_bytes} B`, "wasm") + ` · `
      : "";
    const wgsl = wgslPayloadSummary(this._manifest);
    const wgslLabel = wgsl.shaders > 1
      ? `wgsl ${wgsl.bytes} B / ${wgsl.shaders} shaders`
      : `wgsl ${wgsl.bytes} B`;
    this._meta.innerHTML =
      `entry ${this._manifest.source_entry} · ` + wasm +
      link(this._wgslUrl.href, wgslLabel, "wgsl") +
      ` · path ${this._mode} · fe ${this._manifest.provenance.compiler_version} · ` +
      link(this._manifestUrl.href, `manifest`, "manifest");
  }

  _dispatch(type, detail) {
    this.dispatchEvent(new CustomEvent(type, { detail, bubbles: true, composed: true }));
  }
}

customElements.define("fe-surface", FeSurfaceElement);

// ---------------------------------------------------------------------------
// `mountRenderSurface`: preserved for programmatic embedding (the legacy
// `fe web build --mode render` bundle's emitted index.html imports it). Its
// body is now a thin wrapper around the element: ONE mount path, not a fork
// (FE_WEB_V5_ORCHESTRATION_DESIGN.md 3.3).
// ---------------------------------------------------------------------------

/**
 * Mount one render surface for a fe-web-bundle manifest via a `<fe-surface>`
 * element, and wait for it to reach `live` (this function's historical
 * contract: the returned surface is already rendering).
 *
 * @param {object} options
 * @param {string|URL} options.manifestUrl - fe-web-bundle manifest URL.
 * @param {HTMLCanvasElement|string} [options.canvas] - an existing canvas
 *   element (or CSS selector) to adopt; a new one is created otherwise.
 * @param {Element} [options.container] - parent to append the element into.
 * @param {Node} [options.mountAfter] - insert the element directly after this
 *   node when neither `canvas` nor `container` is given.
 * @param {number} [options.width] - CSS presentation width (NOT dispatch size).
 * @param {number} [options.height=width]
 * @param {number[]} [options.initial] - explicit initial uniform vector,
 *   overriding the manifest's declared `surface.params[].init`.
 * @param {string|URL} [options.scopedTasksUrl] - compiler-published,
 *   manifest-free scoped-task package entry.
 * @param {{adapter: GPUAdapter, device: GPUDevice}} [options.gpu] - reuse an
 *   already-acquired adapter/device instead of the page-shared singleton.
 * @param {boolean} [options.controls=true] - generate uniform sliders and
 *   the mode badge/meta line.
 * @returns {Promise<{mode: string, canvas: HTMLCanvasElement, element: Element,
 *   manifest: object, manifestUrl: URL, uniforms: number[],
 *   render: (next?: number[]) => number[]}>}
 */
export async function mountRenderSurface(options) {
  const {
    manifestUrl,
    canvas: canvasOption,
    container,
    mountAfter,
    width: widthOption,
    height: heightOption,
    initial,
    scopedTasksUrl,
    gpu: gpuOption,
    controls = true,
  } = options;

  const resolvedManifestUrl = new URL(manifestUrl, document.baseURI);
  const surface = document.createElement("fe-surface");
  surface.setAttribute("manifest", resolvedManifestUrl.href);
  if (scopedTasksUrl) {
    surface.setAttribute(
      "data-fe-scoped-tasks",
      new URL(scopedTasksUrl, document.baseURI).href,
    );
  }
  surface.setAttribute("state", "live"); // historical contract: render immediately.
  surface.setAttribute("controls", controls ? "auto" : "none");
  if (widthOption) surface.setAttribute("width", String(widthOption));
  if (heightOption ?? widthOption) surface.setAttribute("height", String(heightOption ?? widthOption));
  if (initial) surface._initialOverride = initial;
  if (gpuOption) surface._gpuOverride = gpuOption;
  const adopted = resolveCanvas(canvasOption);
  if (adopted) surface.adoptCanvas(adopted);

  if (container) {
    container.appendChild(surface);
  } else if (mountAfter && mountAfter.parentNode) {
    mountAfter.parentNode.insertBefore(surface, mountAfter.nextSibling);
  } else {
    document.body.appendChild(surface);
  }

  await surface._readyPromise;
  await surface._livePromise;

  return {
    mode: surface.mode,
    canvas: surface._adoptedCanvas || surface._liveCanvas || surface._posterCanvas,
    element: surface,
    manifest: surface.manifest,
    manifestUrl: resolvedManifestUrl,
    get uniforms() {
      return surface._uniforms;
    },
    render(next) {
      void surface._render(next).catch((error) => surface._fail(error));
      return surface._uniforms;
    },
  };
}
