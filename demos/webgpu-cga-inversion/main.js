import { initWebGPURender, renderFrame, verifyView } from "../webgpu-keystone/webgpu-runner.js";
import { DEFAULT_CAMERA, createTrailingCoalescer, normalizeCamera, panCamera, zoomCamera } from "./camera-controls.js";
import { createPerformanceMeter } from "./performance-meter.js";
import { createCgaActorLifecycle } from "./actor-lifecycle.js";
import { createCgaWasmWorkerOracle } from "./wasm-worker-oracle.js";

const $ = (id) => document.getElementById(id);
const query = new URLSearchParams(window.location.search);
const acceptanceMode = query.get("acceptance");
const presentation = acceptanceMode === null || acceptanceMode === ""
  ? "canvas"
  : acceptanceMode;
const verificationOff = query.get("verify") === "off";
const continuousVerification = query.get("verify") === "continuous";
const DEFAULT_INVERSION = Object.freeze({ x: 0.5, y: 0, radius: 1 });
const LOGICAL_SIZE = 128;
const performanceMeter = createPerformanceMeter();
window.__cgaPerformance = performanceMeter.state;

function banner(kind, detail) {
  $("banner").className = `banner ${kind}`;
  document.documentElement.dataset.status = kind;
  $("banner").dataset.status = kind;
  $("state").textContent = kind.toUpperCase();
  $("detail").textContent = detail;
}

async function fetchOk(url, kind) {
  const response = await fetch(url);
  if (!response.ok) throw new Error(`fetch ${url} -> HTTP ${response.status}`);
  if (kind === "json") return response.json();
  if (kind === "text") return response.text();
  return response.arrayBuffer();
}

function fnv1a32(bytes) {
  let hash = 0x811c9dc5;
  for (const byte of bytes) hash = Math.imul((hash ^ byte) >>> 0, 0x01000193) >>> 0;
  return hash >>> 0;
}

function viewValues(camera, inversion, width = LOGICAL_SIZE, height = LOGICAL_SIZE) {
  const scale = LOGICAL_SIZE / Math.max(1, width);
  const zoom = camera.zoom * scale;
  return Object.freeze([
    Math.fround(camera.x + (LOGICAL_SIZE / 2 - width / 2) * zoom),
    Math.fround(camera.y + (LOGICAL_SIZE / 2 - height / 2) * zoom),
    Math.fround(zoom),
    Math.fround(inversion.x),
    Math.fround(inversion.y),
  ]);
}

function showCamera(camera) {
  $("camera-values").textContent =
    `cam_x=${camera.x.toFixed(5)}  cam_y=${camera.y.toFixed(5)}  zoom=${camera.zoom.toFixed(6)}`;
}

function pointerWorld(camera, canvas, event) {
  const rect = canvas.getBoundingClientRect();
  const px = (event.clientX - rect.left) * canvas.width / rect.width;
  const py = (event.clientY - rect.top) * canvas.height / rect.height;
  const worldPerPixelX = camera.zoom * LOGICAL_SIZE / canvas.width;
  const worldPerPixelY = camera.zoom * LOGICAL_SIZE / canvas.height;
  return Object.freeze({
    x: camera.x + (px - canvas.width / 2) * worldPerPixelX,
    y: camera.y + (py - canvas.height / 2) * worldPerPixelY,
  });
}

function drawInversionOverlay(camera, inversion) {
  const overlay = $("inversion-overlay");
  if (!overlay) return;
  const context = overlay.getContext("2d");
  context.clearRect(0, 0, overlay.width, overlay.height);
  const worldPerPixelX = camera.zoom * LOGICAL_SIZE / overlay.width;
  const worldPerPixelY = camera.zoom * LOGICAL_SIZE / overlay.height;
  const cx = overlay.width / 2 + (inversion.x - camera.x) / worldPerPixelX;
  const cy = overlay.height / 2 + (inversion.y - camera.y) / worldPerPixelY;
  const radius = inversion.radius / worldPerPixelX;
  context.save();
  context.strokeStyle = "rgba(255, 214, 102, 0.9)";
  context.fillStyle = "rgba(255, 214, 102, 0.95)";
  context.lineWidth = 1.25;
  context.setLineDash([3, 2]);
  context.beginPath();
  context.arc(cx, cy, radius, 0, Math.PI * 2);
  context.stroke();
  context.setLineDash([]);
  context.beginPath();
  context.moveTo(cx - 4, cy);
  context.lineTo(cx + 4, cy);
  context.moveTo(cx, cy - 4);
  context.lineTo(cx, cy + 4);
  context.stroke();
  context.beginPath();
  context.arc(cx, cy, 1.75, 0, Math.PI * 2);
  context.fill();
  context.restore();
}

function validateTypedLayout(layout) {
  if (layout.mode !== "Render") throw new Error(`expected Render layout, got ${layout.mode}`);
  if (layout.width !== 128 || layout.height !== 128) throw new Error("D1 layout must be exactly 128x128");
  if (layout.entry_point !== "fs_main"
      || layout.vertex_entry !== "vs_fullscreen" || layout.fragment_entry !== "fs_main") {
    throw new Error("unexpected Render entry points");
  }
  if (layout.color_target_format !== "rgba8unorm") throw new Error("D1 requires rgba8unorm");
  const inputs = layout.bindings.filter((binding) => binding.role === "Input");
  const input = inputs[0];
  if (!input) throw new Error("Render layout has no Input binding");
  if (inputs.length !== 1) throw new Error("D1 requires exactly one Input binding");
  if (input.group !== 0 || input.binding !== 1 || input.access !== "Read") {
    throw new Error("D1 Input must be read-only group 0 binding 1");
  }
  if (input.span !== 20 || input.stride !== 20) throw new Error("D1 Input span/stride must both be 20");
  const paramTuples = (layout.params || []).map((p) => [p.arg_index, p.offset, p.width, p.scalar]);
  const expectedParams = [
    [2, 0, 4, "F32"], [3, 4, 4, "F32"], [4, 8, 4, "F32"],
    [5, 12, 4, "F32"], [6, 16, 4, "F32"],
  ];
  if (JSON.stringify(paramTuples) !== JSON.stringify(expectedParams)) throw new Error("unexpected ordered D1 params");
  if (JSON.stringify(layout.params.map((p) => p.name))
      !== JSON.stringify(["cam_x", "cam_y", "zoom", "inv_cx", "inv_cy"])) {
    throw new Error("unexpected ordered D1 param names");
  }
  const builtinTuples = (layout.builtin_inputs || []).map((b) => [b.arg_index, b.scalar, b.source]);
  const expectedBuiltins = [[0, "I32", "FragmentPositionX"], [1, "I32", "FragmentPositionY"]];
  if (JSON.stringify(builtinTuples) !== JSON.stringify(expectedBuiltins)) throw new Error("unexpected ordered D1 builtin inputs");
  return input;
}

function validateReference(reference) {
  if (reference.width !== 128 || reference.height !== 128) {
    throw new Error("D1 reference must be exactly 128x128");
  }
  const counts = ["sky_pixels", "hit_pixels", "upper_pixels", "lower_pixels"];
  for (const field of counts) {
    if (!Number.isInteger(reference[field]) || reference[field] < 0) {
      throw new Error(`D1 reference ${field} must be a non-negative integer`);
    }
  }
  if (reference.upper_pixels === 0 || reference.lower_pixels === 0) {
    throw new Error("D1 reference must contain positive pixel counts for both palette halves");
  }
  if (reference.upper_pixels + reference.lower_pixels !== reference.hit_pixels) {
    throw new Error("D1 reference palette pixel counts must sum to hit_pixels");
  }
  if (reference.sky_pixels + reference.hit_pixels !== reference.width * reference.height) {
    throw new Error("D1 reference sky_pixels + hit_pixels must cover the full frame");
  }
}

async function main() {
  if (presentation !== "canvas" && presentation !== "offscreen") {
    banner("red", `invalid acceptance presentation: ${presentation}`);
    return { state: "red", presentation, reason: "acceptance must be canvas or offscreen" };
  }
  let layout, reference, source, wgsl, wasm;
  const artifactFetchStart = performanceMeter.start();
  try {
    [layout, source, wgsl] = await Promise.all([
      fetchOk("./gen/layout.json", "json"),
      fetchOk("./gen/kernel.fe", "text"),
      fetchOk("./gen/frag.wgsl", "text"),
    ]);
    if (!verificationOff) {
      [reference, wasm] = await Promise.all([
        fetchOk("./gen/reference.json", "json"),
        fetchOk("./gen/frag.wasm", "bytes"),
      ]);
    }
    validateTypedLayout(layout);
    if (!verificationOff) validateReference(reference);
    performanceMeter.finish("artifactFetchMs", artifactFetchStart);
  } catch (error) {
    banner("red", `artifact contract failed: ${error.message || error}`);
    return { state: "red", presentation, reason: String(error) };
  }

  $("source").textContent = source;
  $("wgsl").textContent = wgsl;
  $("meta").textContent = `Fe ${layout.provenance?.fe_rev || "unknown"} | Sonatina ${layout.provenance?.sonatina_rev || "unknown"}`;

  validateTypedLayout(layout);
  const renderLayout = { ...layout, params: layout.params };

  let wasmOracle;
  if (!verificationOff) {
    try {
      wasmOracle = await createCgaWasmWorkerOracle({
        wasm, exportName: layout.frag_wasm_export,
        width: reference.width, height: reference.height,
      });
    } catch (error) {
      banner("red", `browser Wasm worker oracle failed: ${error.message || error}`);
      return { state: "red", presentation, reason: String(error) };
    }
  }
  const defaultValues = viewValues(normalizeCamera(DEFAULT_CAMERA), DEFAULT_INVERSION);
  let defaultWasmHash;
  if (!verificationOff) try {
    const defaultWasmRgba = await wasmOracle.render(defaultValues, 0);
    defaultWasmHash = fnv1a32(defaultWasmRgba);
    if (defaultWasmHash !== (reference.fnv1a32 >>> 0)) {
      banner("red", `browser Wasm/reference mismatch: ${defaultWasmHash} != ${reference.fnv1a32 >>> 0}`);
      return { state: "red", presentation, wasmHash: defaultWasmHash,
        reason: "default browser-Wasm frame differs from packaged reference" };
    }
  } catch (error) {
    banner("red", `browser Wasm oracle failed: ${error.message || error}`);
    return { state: "red", presentation, reason: String(error) };
  }

  const canvas = $("view");
  const resizeDisplayCanvas = () => {
    if (presentation !== "canvas") return false;
    const cssWidth = canvas.getBoundingClientRect().width || LOGICAL_SIZE;
    const dpr = Math.min(window.devicePixelRatio || 1, 2);
    const pixels = Math.max(LOGICAL_SIZE, Math.min(768, Math.round(cssWidth * dpr)));
    if (canvas.width === pixels && canvas.height === pixels) return false;
    canvas.width = pixels;
    canvas.height = pixels;
    $("inversion-overlay").width = pixels;
    $("inversion-overlay").height = pixels;
    return true;
  };
  resizeDisplayCanvas();
  const gpuInitStart = performanceMeter.start();
  const gpu = await initWebGPURender(
    wgsl,
    renderLayout,
    presentation === "offscreen" ? null : $("view"),
  );
  performanceMeter.finish("gpuInitMs", gpuInitStart);
  if (!gpu.ok) {
    const detail = verificationOff
      ? `fast showcase unavailable: ${gpu.reason}`
      : `browser Wasm matches the packaged frame; no live WebGPU render: ${gpu.reason}`;
    banner("amber", detail);
    return { state: "amber", presentation, verified: false,
      wasmHash: defaultWasmHash, reason: gpu.reason };
  }

  const verifyCamera = async (
    camera, inversion, generation, requireReference = false, initial = false,
  ) => {
    await new Promise((resolve) => setTimeout(resolve, 0));
    const values = viewValues(camera, inversion);
    const wasmRgba = await wasmOracle.render(values, generation);
    const wasmHash = fnv1a32(wasmRgba);
    const readback = await verifyView(gpu, values);
    if (!readback.ok) {
      return { state: "red", presentation, generation, wasmHash, reason: readback.reason };
    }
    const gpuHash = fnv1a32(readback.rgba);
    const mismatches = [];
    let mismatchCount = 0;
    if (readback.rgba.length === wasmRgba.length) {
      for (let offset = 0; offset < wasmRgba.length; offset += 4) {
        const wasmPixel = wasmRgba.slice(offset, offset + 4);
        const gpuPixel = readback.rgba.slice(offset, offset + 4);
        if (wasmPixel.some((byte, channel) => byte !== gpuPixel[channel])) {
          mismatchCount += 1;
          if (mismatches.length < 16) {
            const pixel = offset / 4;
            mismatches.push({
              x: pixel % reference.width,
              y: Math.floor(pixel / reference.width),
              wasm: Array.from(wasmPixel),
              gpu: Array.from(gpuPixel),
            });
          }
        }
      }
    } else {
      mismatchCount = Math.max(readback.rgba.length, wasmRgba.length) / 4;
    }
    const equal = mismatchCount === 0 && readback.rgba.length === wasmRgba.length;
    if (!equal || gpuHash !== wasmHash) {
      return { state: "red", presentation, generation, wasmHash, gpuHash,
        mismatchCount, mismatches,
        reason: `GPU/browser-Wasm byte mismatch at ${mismatchCount} pixels` };
    }
    if (requireReference && wasmHash !== (reference.fnv1a32 >>> 0)) {
      return { state: "red", presentation, generation, wasmHash, gpuHash,
        reason: `default view differs from reference ${reference.fnv1a32 >>> 0}` };
    }
    return { state: "green", presentation, generation, wasmHash, gpuHash, adapter: gpu.adapter, initial,
      camera: values.slice(0, 3), inversion: values.slice(3) };
  };

  const finishVerification = (result) => {
    if (!result) return;
    if (result.state === "green") {
      banner("green", `current camera: WebGPU readback exactly matches browser Wasm (FNV-1a ${result.gpuHash}) on ${gpu.adapter}`);
    } else {
      banner("red", result.reason);
    }
    publishAcceptance(result);
  };

  let camera = normalizeCamera(DEFAULT_CAMERA);
  let inversion = { ...DEFAULT_INVERSION };
  let inversionFrozen = false;
  showCamera(camera);
  drawInversionOverlay(camera, inversion);

  let initialAccepted = false;
  const acceptanceStart = verificationOff ? null : performanceMeter.start();
  let resolveInitial;
  const initialPromise = verificationOff
    ? Promise.resolve(null)
    : new Promise((resolve) => { resolveInitial = resolve; });
  let firstFrame = true;
  const lifecycle = createCgaActorLifecycle({
    mode: verificationOff ? "off" : (continuousVerification ? "continuous" : "manual"),
    render: ({ payload }) => new Promise((resolve) => {
      if (presentation !== "canvas") {
        resolve(null);
        return;
      }
      requestAnimationFrame((rafTime) => {
        const submitStart = performanceMeter.start();
        renderFrame(gpu, viewValues(
          payload.camera, payload.inversion, payload.width, payload.height,
        ));
        if (firstFrame) {
          performanceMeter.finish("firstFrameSubmitMs", submitStart);
          firstFrame = false;
        } else {
          const submitCpuMs = performanceMeter.elapsed(submitStart);
          performanceMeter.recordFrame(rafTime, submitCpuMs);
          updatePerformanceUi(rafTime);
        }
        drawInversionOverlay(payload.camera, payload.inversion);
        resolve(null);
      });
    }),
    verify: async ({ payload, generation }) => ({
      ...await verifyCamera(
        payload.camera, payload.inversion, generation, payload.requireReference, payload.initial,
      ),
      initial: payload.initial,
    }),
    onVerificationResult: (envelope) => {
      if (envelope.payload.value?.initial) return;
      finishVerification(envelope.payload.ok
        ? envelope.payload.value
        : { state: "red", presentation, generation: envelope.generation,
          reason: envelope.payload.error });
    },
    onVerificationSettled: (envelope, settlement) => {
      const isInitial = settlement.request.payload.initial === true;
      const result = envelope.payload.ok
        ? envelope.payload.value
        : { state: "red", presentation, generation: envelope.generation,
          initial: isInitial, reason: envelope.payload.error };
      if (result?.initial) resolveInitial(result);
    },
  });
  const renderPayload = () => ({
    camera, inversion: { ...inversion }, width: canvas.width, height: canvas.height,
  });
  const verificationPayload = (initial = false) => ({
    camera, inversion: { ...inversion }, requireReference: initial, initial,
  });
  const coalescer = createTrailingCoalescer((job) => {
    lifecycle.enqueueVerification(job.payload, job.generation);
  });
  const requestVerification = (useCurrentGeneration = false) => {
    const generation = useCurrentGeneration ? lifecycle.generation() : lifecycle.advance();
    coalescer.submit({ payload: verificationPayload(), generation });
    banner("amber", "verifying current view with browser Wasm and WebGPU readback...");
    publishAcceptance({ state: "pending", presentation, generation,
      camera: viewValues(camera, inversion).slice(0, 3), inversion: [inversion.x, inversion.y] });
  };
  let lastPerformanceUiUpdate = -Infinity;
  const updatePerformanceUi = (rafTime) => {
    if (rafTime - lastPerformanceUiUpdate < 250) return;
    lastPerformanceUiUpdate = rafTime;
    const frames = performanceMeter.state.frames;
    const fps = frames.fps === null ? "--" : frames.fps.toFixed(1);
    const submit = frames.averageSubmitCpuMs === null
      ? "--"
      : frames.averageSubmitCpuMs.toFixed(2);
    $("performance-stat").textContent = `rAF ${fps} fps | submit CPU ${submit} ms`;
  };
  const requestDraw = () => lifecycle.enqueueRender(renderPayload());

  // The deterministic acceptance gate runs once before interaction begins.
  // Presentation then reuses the live pipeline and only updates its uniform
  // buffer unless the user explicitly requests continuous verification.
  lifecycle.begin(renderPayload(), verificationPayload(true));
  if (presentation === "canvas") {
    new ResizeObserver(() => {
      if (resizeDisplayCanvas()) requestDraw();
    }).observe(canvas.parentElement);
  }
  const settle = (nextCamera) => {
    camera = normalizeCamera(nextCamera);
    showCamera(camera);
    lifecycle.interact(renderPayload());
    if (verificationOff) {
      banner("presentation", `fast WebGPU showcase on ${gpu.adapter}; verification is off`);
    } else if (continuousVerification) {
      requestVerification(true);
    } else {
      const acceptance = initialAccepted
        ? "initial acceptance passed; current view not reverified"
        : "initial acceptance still running";
      banner("amber", `live WebGPU presentation on ${gpu.adapter}; ${acceptance}`);
    }
  };

  if (presentation === "canvas") {
    let drag = null;
    canvas.addEventListener("pointerdown", (event) => {
      if (!event.isPrimary || event.button !== 0) return;
      drag = { x: event.clientX, y: event.clientY };
      canvas.setPointerCapture(event.pointerId);
    });
    canvas.addEventListener("pointermove", (event) => {
      const world = pointerWorld(camera, canvas, event);
      $("pointer-values").textContent =
        `pointer_world=(${world.x.toFixed(5)}, ${world.y.toFixed(5)})`;
      if (!drag && !inversionFrozen) {
        inversion = { ...inversion, x: Math.fround(world.x), y: Math.fround(world.y) };
        $("inversion-values").textContent =
          `inv_center=(${inversion.x.toFixed(5)}, ${inversion.y.toFixed(5)})`;
        lifecycle.interact(renderPayload());
        if (continuousVerification && !verificationOff) requestVerification(true);
      }
      if (!drag) return;
      const scaleX = LOGICAL_SIZE / canvas.clientWidth;
      const scaleY = LOGICAL_SIZE / canvas.clientHeight;
      const next = panCamera(camera, (event.clientX - drag.x) * scaleX,
        (event.clientY - drag.y) * scaleY);
      drag = { x: event.clientX, y: event.clientY };
      settle(next);
    });
    const endDrag = (event) => {
      if (drag) canvas.releasePointerCapture?.(event.pointerId);
      drag = null;
    };
    canvas.addEventListener("pointerup", endDrag);
    canvas.addEventListener("pointercancel", endDrag);
    canvas.addEventListener("lostpointercapture", () => { drag = null; });
    canvas.addEventListener("pointerleave", () => {
      if (!drag) $("pointer-values").textContent = "pointer_world=—";
    });
    canvas.addEventListener("wheel", (event) => {
      event.preventDefault();
      const rect = canvas.getBoundingClientRect();
      const px = (event.clientX - rect.left) * LOGICAL_SIZE / rect.width;
      const py = (event.clientY - rect.top) * LOGICAL_SIZE / rect.height;
      settle(zoomCamera(camera, event.deltaY, px, py, LOGICAL_SIZE, LOGICAL_SIZE));
    }, { passive: false });
    $("focus-inversion").addEventListener("click", () => settle({
      x: inversion.x,
      y: inversion.y,
      zoom: camera.zoom,
    }));
    $("freeze-inversion").addEventListener("click", (event) => {
      inversionFrozen = !inversionFrozen;
      event.currentTarget.textContent = inversionFrozen ? "Unfreeze inversion" : "Freeze inversion";
    });
    $("reset-camera").addEventListener("click", () => {
      inversion = { ...DEFAULT_INVERSION };
      $("inversion-values").textContent = "inv_center=(0.50000, 0.00000)";
      settle(DEFAULT_CAMERA);
    });
    if (verificationOff) {
      $("verify-view").hidden = true;
    } else {
      $("verify-view").addEventListener("click", () => requestVerification());
    }
    if (continuousVerification && !verificationOff) {
      $("verify-view").textContent = "Verify now (continuous on)";
    }
    window.__cgaCamera = { get: () => ({ ...camera }), reset: () => settle(DEFAULT_CAMERA) };
  }
  const initialResult = await initialPromise;
  if (acceptanceStart !== null) {
    performanceMeter.finish("initialAcceptanceMs", acceptanceStart);
  }
  if (verificationOff) {
    const result = { state: "presentation", presentation, verified: false,
      adapter: gpu.adapter, camera: viewValues(camera, inversion).slice(0, 3),
      inversion: [inversion.x, inversion.y] };
    banner("presentation", `fast WebGPU showcase on ${gpu.adapter}; verification is off`);
    publishAcceptance(result);
    return result;
  }
  initialAccepted = initialResult?.state === "green";
  let completionResult = initialResult;
  if (initialResult) {
    if (initialResult.generation === lifecycle.generation()) {
      finishVerification(initialResult);
    } else {
      // The default-frame acceptance is still useful structured evidence, but
      // must not overwrite status for a newer interactive generation.
      window.__cgaInitialAcceptance = { ...initialResult, initial: true };
      completionResult = null;
      if (!initialAccepted) {
        banner("red", `initial acceptance failed: ${initialResult.reason}`);
        publishAcceptance({ ...initialResult, initial: true, currentViewVerified: false });
      } else if (lifecycle.state().verify.pending === null) {
        publishAcceptance({ state: "green", presentation, generation: lifecycle.generation(),
          initialAccepted: true, currentViewVerified: false, adapter: gpu.adapter,
          initialWasmHash: initialResult.wasmHash, initialGpuHash: initialResult.gpuHash });
        banner("green", `live WebGPU presentation on ${gpu.adapter}; initial acceptance passed, current view not reverified`);
      }
    }
  }
  return completionResult;
}

window.__cgaAcceptance = { state: "pending", presentation };
document.documentElement.dataset.status = "pending";
function publishAcceptance(result) {
  Object.assign(window.__cgaAcceptance, result);
  document.documentElement.dataset.status = result.state;
  const node = document.getElementById("acceptance-json");
  if (node) node.textContent = JSON.stringify(result);
  return result;
}
window.__cgaAcceptance.promise = main().then((result) => {
  return result ? publishAcceptance(result) : window.__cgaAcceptance;
}).catch((error) => {
  const result = { state: "red", presentation, reason: String(error) };
  banner("red", result.reason);
  return publishAcceptance(result);
});
