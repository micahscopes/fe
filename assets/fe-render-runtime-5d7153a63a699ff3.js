// fe render runtime (compiler-emitted, protocol fe-web-bundle v4/v5/v6).
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

// ---------------------------------------------------------------------------
// Shared WebGPU device: one adapter/device for the whole page, requested at
// most once, with `device.lost` recovery broadcast to every attached surface.
// ---------------------------------------------------------------------------

let sharedGpuPromise;
let sharedGpuFailure;
let sharedGpuRecoveryPromise;
let pendingDeviceLoss;
const DEVICE_STABILITY_MS = 50;
const DEVICE_LOSS_CONFIRMATION_MS = 250;
const MAX_RECOVERY_ATTEMPTS = 2;
/** Every currently connected `<fe-surface>`, live or not (module-level so a
 * `device.lost` event, which is a page-wide fact, can reach every element). */
const attachedSurfaces = new Set();

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
    return null;
  }
  if (!navigator.gpu) {
    sharedGpuFailure = new Error(
      "fe render runtime: this browser does not expose WebGPU (navigator.gpu is unavailable)",
    );
    return null;
  }
  try {
    const adapter = await navigator.gpu.requestAdapter();
    if (!adapter) {
      sharedGpuFailure = new Error(
        "fe render runtime: no WebGPU adapter is available (requestAdapter returned null). " +
        "Check browser WebGPU support, hardware acceleration, the GPU blocklist, and chrome://gpu on Chromium.",
      );
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
      return null;
    }
    device.addEventListener("uncapturederror", (event) => {
      console.error("[fe web] uncaptured WebGPU error:", event.error);
    });
    device.lost.then((info) => handleSharedDeviceLoss(device, info));
    return { adapter, device };
  } catch (error) {
    sharedGpuFailure = new Error(
      `fe render runtime: WebGPU initialization failed: ${error?.message ?? String(error)}`,
      { cause: error },
    );
    console.warn("[fe web] WebGPU initialization failed:", error);
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
  let attempts = 0;
  while (pendingDeviceLoss) {
    const { deadDevice, info } = pendingDeviceLoss;
    pendingDeviceLoss = undefined;
    console.warn(`[fe web] WebGPU device lost (${info?.reason ?? "unknown"}): ${info?.message ?? ""}`);
    const current = sharedGpuPromise ? await sharedGpuPromise.catch(() => null) : null;
    if (current && current.device !== deadDevice) continue;

    let fresh = null;
    if (attempts < MAX_RECOVERY_ATTEMPTS) {
      attempts += 1;
      sharedGpuPromise = undefined;
      fresh = await acquireSharedGpu();
    } else {
      sharedGpuFailure = new Error(
        `fe render runtime: WebGPU device recovery stopped after ${MAX_RECOVERY_ATTEMPTS} failed attempts`,
      );
      sharedGpuPromise = Promise.resolve(null);
    }
    for (const surface of attachedSurfaces) {
      try {
        await surface._onDeviceLoss(fresh);
      } catch (error) {
        surface._fail(error);
      }
    }
  }
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

function resolveCanvas(canvasOption) {
  if (!canvasOption) return null;
  if (typeof canvasOption === "string") return document.querySelector(canvasOption);
  return canvasOption;
}

/**
 * The initial uniform vector from the declared v5 `surface`: each member takes
 * its param's `init`, and an extent-bound member (`extent_x`/`extent_y`) takes
 * the live canvas size. No search, no guessing.
 */
function surfaceInitialUniforms(members, surface, width, height) {
  const byName = new Map(surface.params.map((param) => [param.name, param]));
  return members.map((member) => {
    const param = byName.get(member.name);
    if (!param) return 0;
    if (param.kind === "extent_x") return width;
    if (param.kind === "extent_y") return height;
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
function withExtentUniforms(members, surface, uniforms, width, height) {
  if (!surface) return uniforms;
  const byName = new Map(surface.params.map((param) => [param.name, param]));
  const next = uniforms.slice();
  members.forEach((member, index) => {
    const param = byName.get(member.name);
    if (!param) return;
    if (param.kind === "extent_x") next[index] = width;
    else if (param.kind === "extent_y") next[index] = height;
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
  device.queue.writeBuffer(uniformBuffer, 0, buffer);
}

function presentFrame(device, context, pipeline, bindGroup) {
  const encoder = device.createCommandEncoder();
  const pass = encoder.beginRenderPass({
    colorAttachments: [
      {
        view: context.getCurrentTexture().createView(),
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
  device.queue.submit([encoder.finish()]);
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
:host { display: block; max-width: 420px; font: 14px/1.5 ui-monospace, SFMono-Regular, Menlo, monospace;
        color: #cfd6e4; }
.root { display: flex; flex-direction: column; gap: 10px; }
.stage { position: relative; width: 100%; background: #000; border-radius: 10px; overflow: hidden;
         box-shadow: 0 8px 40px #0008; }
.surface-canvas { display: block; width: 100%; height: auto; }
.surface-canvas[hidden] { display: none; }
.side { display: grid; gap: 10px; }
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
class FeSurfaceElement extends HTMLElement {
  static get observedAttributes() {
    return ["manifest", "state", "controls", "width", "height"];
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
    this._surface = null;
    this._control = null; // R3 param gestures: the projected `control` manifest section.
    this._controlKernel = null; // the resolved wasm control export, or null (no gestures).
    this._passes = [];
    this._resources = [];
    this._graph = false;
    this._posterAttemptedDevice = null;
    this._gestureListeners = null; // { canvas, onPointerDown, onPointerMove, onPointerUp, onWheel }
    this._gestureFrame = null;
    this._gesturePresenting = false;
    this._gestureDirty = false;
    this._gpu = null; // one legacy pipeline, or { passRecords, resourceBuffers } for a graph.
    this._liveContext = null; // GPUCanvasContext on `_liveCanvas`
    this._adoptedContext = null; // GPUCanvasContext on `_adoptedCanvas`
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

  /** Force a transition to `live`, waiting for `ready` first if still cold. */
  async live() {
    if (this._fsm === "cold") await this._readyPromise.catch(() => {});
    await this._goLive();
  }

  /** Capture the current frame as the poster, release GPU presentation, and
   * stay there until `.live()` is called again (unlike `suspended`, which
   * re-activates automatically when the surface re-enters the viewport). */
  async freeze() {
    if (this._fsm === "cold") await this._readyPromise.catch(() => {});
    if (this._fsm !== "live") return;
    await this._capturePosterFromLive();
    this._fsm = "frozen";
    this._dispatch("fe-statechange", { state: "frozen" });
  }

  // `.post()` (message lanes) is R3/R4 (FE_WEB_V5_BUILD_ORDER.md); deliberately
  // not defined here rather than shipped as a stub with no lane to call.

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
      if (this._fsm === "live") this._wireSuspendObserver();
      return;
    }
    this._booted = true;
    this._bootSurface();
  }

  disconnectedCallback() {
    this._suspendObserver?.disconnect();
    this._activationObserver?.disconnect();
    attachedSurfaces.delete(this);
  }

  attributeChangedCallback(name, oldValue, newValue) {
    if (oldValue === newValue) return;
    if (name === "width" || name === "height") {
      if (newValue == null) this.style.removeProperty(name);
      else this.style[name] = /^\d+$/.test(newValue) ? `${newValue}px` : newValue;
      return;
    }
    if (name === "manifest") {
      if (this.isConnected) this._bootSurface();
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
    const manifestAttr = this.getAttribute("manifest");
    if (!manifestAttr) {
      this._fail(new Error("fe-surface: `manifest` attribute is required"));
      return;
    }
    try {
      const manifestUrl = new URL(manifestAttr, this.baseURI);
      const manifest = await (await fetchOrThrow(manifestUrl, "manifest")).json();
      if (manifest.protocol !== "fe-web-bundle" || ![4, 5, 6].includes(manifest.protocol_version)) {
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
      this._graph = this._resources.length > 0 || this._passes.some((pass) => pass.layout.mode === "compute");
      const fragmentPass = [...this._passes].reverse().find((pass) => pass.layout.mode === "render");
      this._layout = fragmentPass?.layout ?? manifest.layout;
      this._surface = manifest.surface || null;
      this._control = manifest.control || null;
      const inputBinding = this._layout.bindings.find((binding) => binding.role === "input");
      this._inputBinding = inputBinding ?? null;
      this._members = inputBinding ? inputBinding.members : [];
      this._memberIndexByName = new Map(this._members.map((member, index) => [member.name, index]));
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
        ({ instance } = await WebAssembly.instantiate(wasmBytes, {}));
        this._kernel = instance.exports[manifest.source_entry];
        if (!this._graph && typeof this._kernel !== "function") {
          throw new Error(`fe render runtime: wasm export \`${manifest.source_entry}\` not found`);
        }
        if (this._graph && typeof this._kernel !== "function") this._kernel = null;
      } else if (!this._graph) {
        throw new Error("fe render runtime: bundle has neither a Wasm fallback nor a GPU pass graph");
      }
      // R3 param gestures: the SAME wasm instance carries the control export
      // (already part of the root set the compiler emitted `module.wasm`
      // with). No control block, or an export it doesn't actually find,
      // means gestures stay off -- never a JS reimplementation fallback.
      this._controlKernel = null;
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

      this._uniforms =
        this._initialOverride ??
        (this._surface
          ? surfaceInitialUniforms(this._members, this._surface, DEFAULT_SIZE, DEFAULT_SIZE)
          : undeclaredViewInitialUniforms(this._members));

      if (!this._adoptedCanvas) this._ensureStage();
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
    this._panel.replaceChildren(notice);
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

    this._side = document.createElement("div");
    this._side.className = "side";
    this._badge = document.createElement("span");
    this._badge.className = "badge";
    this._badge.setAttribute("part", "badge");
    this._panel = document.createElement("div");
    this._panel.className = "panel";
    this._panel.setAttribute("part", "panel");
    this._meta = document.createElement("div");
    this._meta.className = "meta";
    this._meta.setAttribute("part", "meta");
    this._side.append(this._badge, this._panel, this._meta);

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

  /** Backing store = min(declared surface.extent, css-px * devicePixelRatio):
   * never upsampled past the kernel's declared resolution, never rendered
   * larger than the box actually needs at the current DPR. */
  _computeBackingExtent() {
    const declaredWidth = this._surface?.extent?.width ?? DEFAULT_SIZE;
    const declaredHeight = this._surface?.extent?.height ?? declaredWidth;
    const dpr = window.devicePixelRatio || 1;
    const probe = this._adoptedCanvas || this._stage || this;
    const rect = probe.getBoundingClientRect();
    const cssWidth = rect.width || declaredWidth;
    const cssHeight = rect.height || declaredHeight;
    return {
      width: Math.max(1, Math.min(declaredWidth, Math.round(cssWidth * dpr))),
      height: Math.max(1, Math.min(declaredHeight, Math.round(cssHeight * dpr))),
    };
  }

  async _resolveGpu() {
    return this._gpuOverride ?? (await acquireSharedGpu());
  }

  async _buildPassGraph(device) {
    const format = this._layout.color_target_format || navigator.gpu.getPreferredCanvasFormat();
    const shaderSources = await Promise.all(
      this._passShaderUrls.map(async (url) => (await fetchOrThrow(url, "WGSL pass shader")).text()),
    );
    const resourceBuffers = new Map();
    for (const resource of this._resources) {
      if (resource.group !== 0) {
        throw new Error("fe render runtime: v6 pass graphs currently require resource group 0");
      }
      resourceBuffers.set(
        resource.name,
        device.createBuffer({
          size: Math.max(4, resource.stride * resource.length),
          usage: GPUBufferUsage.STORAGE | GPUBufferUsage.COPY_DST | GPUBufferUsage.COPY_SRC,
        }),
      );
    }

    const passRecords = [];
    for (let index = 0; index < this._passes.length; index++) {
      const pass = this._passes[index];
      const module = device.createShaderModule({ code: shaderSources[index] });
      const visibility = pass.layout.mode === "compute" ? GPUShaderStage.COMPUTE : GPUShaderStage.FRAGMENT;
      const layoutEntries = [];
      const groupEntries = [];
      const inputs = [];
      const outputs = [];
      for (const binding of pass.layout.bindings) {
        if (binding.group !== 0) {
          throw new Error("fe render runtime: v6 pass graphs currently require binding group 0");
        }
        if (binding.role === "resource") {
          const buffer = resourceBuffers.get(binding.name);
          if (!buffer) throw new Error(`fe render runtime: resource \`${binding.name}\` is undeclared`);
          layoutEntries.push({
            binding: binding.binding,
            visibility,
            buffer: { type: binding.access === "read" ? "read-only-storage" : "storage" },
          });
          groupEntries.push({ binding: binding.binding, resource: { buffer } });
        } else if (binding.role === "input") {
          const buffer = device.createBuffer({
            size: Math.max(16, binding.span),
            usage: GPUBufferUsage.STORAGE | GPUBufferUsage.COPY_DST,
          });
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
          const buffer = device.createBuffer({
            size: Math.max(4, binding.span),
            usage: GPUBufferUsage.STORAGE | GPUBufferUsage.COPY_DST,
          });
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
            primitive: { topology: "triangle-list" },
          });
      passRecords.push({ pass, pipeline, bindGroup, inputs, outputs });
    }
    return { device, format, passRecords, resourceBuffers };
  }

  async _ensurePipeline() {
    this._pipelineError = null;
    const gpu = await this._resolveGpu();
    if (!gpu) return null;
    const { device } = gpu;
    if (this._gpu && this._gpu.device === device) return this._gpu;
    try {
      if (this._graph) {
        this._gpu = await this._buildPassGraph(device);
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
        uniformBuffer = device.createBuffer({
          size: Math.max(16, this._inputBinding.span),
          usage: GPUBufferUsage.STORAGE | GPUBufferUsage.COPY_DST,
        });
        bindGroupLayout = device.createBindGroupLayout({
          entries: [
            {
              binding: this._inputBinding.binding,
              visibility: GPUShaderStage.FRAGMENT,
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
      this._gpu = { device, format, pipeline, bindGroup, uniformBuffer };
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

  _presentOn(context, uniforms) {
    if (this._graph) {
      const { device, passRecords } = this._gpu;
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
      const encoder = device.createCommandEncoder();
      for (const record of passRecords) {
        if (record.pass.layout.mode === "compute") {
          const compute = encoder.beginComputePass();
          compute.setPipeline(record.pipeline);
          if (record.bindGroup) compute.setBindGroup(0, record.bindGroup);
          const dispatch = record.pass.dispatch;
          if (!dispatch) throw new Error("fe render runtime: compute pass has no fixed dispatch");
          compute.dispatchWorkgroups(dispatch[0], dispatch[1], dispatch[2]);
          compute.end();
        } else {
          const render = encoder.beginRenderPass({
            colorAttachments: [{
              view: context.getCurrentTexture().createView(),
              clearValue: { r: 0, g: 0, b: 0, a: 1 },
              loadOp: "clear",
              storeOp: "store",
            }],
          });
          render.setPipeline(record.pipeline);
          if (record.bindGroup) render.setBindGroup(0, record.bindGroup);
          render.draw(3);
          render.end();
        }
      }
      device.queue.submit([encoder.finish()]);
      return;
    }
    const { device, pipeline, bindGroup, uniformBuffer } = this._gpu;
    if (uniformBuffer) {
      writeUniformBuffer(device, uniformBuffer, this._inputBinding.span, this._members, uniforms);
    }
    presentFrame(device, context, pipeline, bindGroup);
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
    return this._kernel(...args) >>> 0; // 0xAARRGGBB
  }

  _renderWasmInto(canvas, width, height, uniforms) {
    canvas.width = width;
    canvas.height = height;
    const ctx = canvas.getContext("2d");
    const image = ctx.createImageData(width, height);
    const data = image.data;
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
    let lastError;
    for (let attempt = 0; attempt <= MAX_RECOVERY_ATTEMPTS; attempt++) {
      this._posterAttemptedDevice = null;
      try {
        await this._renderPoster();
        this._posterAttemptedDevice = null;
        return;
      } catch (error) {
        lastError = error;
        const attemptedDevice = this._posterAttemptedDevice;
        this._posterAttemptedDevice = null;
        if (
          this._gpuOverride ||
          !attemptedDevice ||
          attempt === MAX_RECOVERY_ATTEMPTS
        ) {
          throw error;
        }
        const loss = await confirmedDeviceLoss(attemptedDevice);
        if (!loss.lost) throw error;

        await (sharedGpuRecoveryPromise ?? handleSharedDeviceLoss(attemptedDevice, loss.info));
        const freshGpu = await acquireSharedGpu();
        if (!freshGpu || freshGpu.device === attemptedDevice) throw error;
        this._gpu = null;
        this._pipelineError = null;
        console.warn(
          `[fe web] retrying initial poster after confirmed device loss (${attempt + 1}/${MAX_RECOVERY_ATTEMPTS})`,
        );
      }
    }
    throw lastError;
  }

  /** Render ONE frame at the current (initial) uniforms, capture it as a
   * static poster, and release GPU presentation: the durable fix for a
   * gallery of N tiles costing zero configured swap chains until a tile goes
   * live (FE_WEB_V5_ORCHESTRATION_DESIGN.md section 6). */
  async _renderPoster() {
    const { width, height } = this._computeBackingExtent();
    this._backingWidth = width;
    this._backingHeight = height;
    this._uniforms = withExtentUniforms(this._members, this._surface, this._uniforms, width, height);
    this._applyExtentAndFilter(width, height);

    const gpu = await this._ensurePipeline();
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
      this._renderWasmInto(this._adoptedCanvas || this._posterCanvas, width, height, this._uniforms);
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
      this._presentOn(context, this._uniforms);
      await gpu.device.queue.onSubmittedWorkDone();
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
        context.configure({ device: gpu.device, format: gpu.format, alphaMode: "opaque" });
      } catch (error) {
        throw new Error(`fe render runtime: poster context configuration failed: ${error?.message ?? String(error)}`, { cause: error });
      }
      try {
        this._presentOn(context, this._uniforms);
      } catch (error) {
        throw new Error(`fe render runtime: poster command submission failed: ${error?.message ?? String(error)}`, { cause: error });
      }
      try {
        await gpu.device.queue.onSubmittedWorkDone();
      } catch (error) {
        throw new Error(`fe render runtime: poster GPU completion failed: ${error?.message ?? String(error)}`, { cause: error });
      }
      await new Promise((resolve) => {
        requestAnimationFrame(() => requestAnimationFrame(resolve));
      });
      let bitmap;
      try {
        bitmap = await createImageBitmap(posterSource);
      } catch (error) {
        throw new Error(`fe render runtime: poster bitmap capture failed: ${error?.message ?? String(error)}`, { cause: error });
      }
      context.unconfigure();
      this._paintPoster(bitmap, width, height);
    } finally {
      try {
        context.unconfigure();
      } catch {
        // Device loss may already have invalidated the context.
      }
      posterSource.hidden = true;
      this._posterCanvas.hidden = false;
    }
  }

  _paintPoster(bitmap, width, height) {
    this._posterCanvas.width = width;
    this._posterCanvas.height = height;
    const ctx = this._posterCanvas.getContext("2d");
    ctx.clearRect(0, 0, width, height);
    ctx.drawImage(bitmap, 0, 0, width, height);
    bitmap.close?.();
  }

  async _capturePosterFromLive() {
    if (this._adoptedCanvas || this._mode !== "webgpu" || !this._liveContext) return;
    await this._gpu.device.queue.onSubmittedWorkDone();
    await new Promise((resolve) => {
      requestAnimationFrame(() => requestAnimationFrame(resolve));
    });
    const bitmap = await createImageBitmap(this._liveCanvas);
    this._paintPoster(bitmap, this._backingWidth, this._backingHeight);
    this._liveContext.unconfigure();
    this._liveCanvas.hidden = true;
    this._posterCanvas.hidden = false;
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

    if (this._adoptedCanvas) {
      // Already presenting (webgpu, kept configured) or cheap to re-run
      // (wasm-2d); "live" is a state/event transition here, not new work.
      if (this._mode === "wasm-2d") {
        this._renderWasmInto(this._adoptedCanvas, this._backingWidth, this._backingHeight, this._uniforms);
      }
      this._enterLive();
      return;
    }

    if (this._mode === "wasm-2d") {
      // No swap chain, so no cost distinction between "ready" and "live":
      // the poster canvas IS the live canvas in the CPU fallback.
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
      this._renderWasmInto(this._posterCanvas, this._backingWidth, this._backingHeight, this._uniforms);
      this._enterLive();
      return;
    }
    this._createLiveCanvas();
    this._liveCanvas.width = this._backingWidth;
    this._liveCanvas.height = this._backingHeight;
    const context = this._liveCanvas.getContext("webgpu");
    context.configure({ device: gpu.device, format: gpu.format, alphaMode: "opaque" });
    this._liveContext = context;
    this._presentOn(context, this._uniforms);
    this._posterCanvas.hidden = true;
    this._liveCanvas.hidden = false;
    this._enterLive();
  }

  _enterLive() {
    this._fsm = "live";
    this._wireSuspendObserver();
    this._wireGestures();
    this._updateBadge();
    this._resolveLive();
    this._dispatch("fe-live", { mode: this._mode });
    this._dispatch("fe-frame", { params: this._paramsSnapshot() });
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
    this._fsm = "suspended";
    this._dispatch("fe-statechange", { state: "suspended" });
  }

  // -- render (param write path: sliders, `.params`, gestures in R3) -------

  _render(next) {
    if (next) this._uniforms = next;
    if (this._fsm !== "live") {
      // Held (courier) state still updates while not presenting (section
      // 5.2/11.4); no GPU/CPU work is spent while hidden or not yet live.
      this._refreshControlValues();
      return;
    }
    if (this._adoptedCanvas) {
      if (this._mode === "webgpu") this._presentOn(this._adoptedContext, this._uniforms);
      else this._renderWasmInto(this._adoptedCanvas, this._backingWidth, this._backingHeight, this._uniforms);
    } else if (this._mode === "webgpu") {
      this._presentOn(this._liveContext, this._uniforms);
    } else {
      this._renderWasmInto(this._posterCanvas, this._backingWidth, this._backingHeight, this._uniforms);
    }
    this._refreshControlValues();
    this._dispatch("fe-frame", { params: this._paramsSnapshot() });
  }

  _paramsSnapshot() {
    const snapshot = {};
    this._members.forEach((member, index) => {
      snapshot[member.name] = this._uniforms[index];
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
          const next = element._uniforms.slice();
          next[index] = Number(value);
          element._render(next);
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

  async _onDeviceLoss(freshGpu) {
    if (this._mode !== "webgpu") return; // wasm-2d surfaces hold no device resources.
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
    if (this._fsm !== "live") return; // posters rebuild lazily on the next `.live()`.
    this._dispatch("fe-statechange", { state: this._fsm, reason: "device-lost" });
    if (!freshGpu) {
      if (!this._kernel) {
        this._fail(sharedGpuFailure ?? new Error(
          "fe render runtime: WebGPU device recovery failed for a GPU-only pass graph",
        ));
        return;
      }
      this._mode = "wasm-2d";
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
      this._updateBadge();
      this._dispatch("fe-statechange", { state: this._fsm, reason: "device-unavailable" });
      return;
    }
    this._fsm = "ready"; // force `_goLive` back through the real pipeline-build path.
    await this._goLive();
    this._dispatch("fe-statechange", { state: this._fsm, reason: "device-recovered" });
  }

  _teardown() {
    if (this._gestureFrame !== null) cancelAnimationFrame(this._gestureFrame);
    this._gestureFrame = null;
    this._gestureDirty = false;
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
    this._gpu = null;
    this._posterAttemptedDevice = null;
    this._suspendObserver?.disconnect();
    this._suspendObserver = null;
    this._activationObserver?.disconnect();
    this._activationObserver = null;
    this._unwireGestures();
  }

  // -- gestures: drag pans, wheel zooms (R3 param gestures) ------------------
  //
  // Fe owns ALL gesture semantics (pan sensitivity, the zoom curve, the
  // cursor anchor, the clamps): this element delivers only raw pointer/wheel
  // deltas to `manifest.control.export` and blits the returned state back by
  // NAME. No pan/zoom arithmetic lives here.

  /** Attach drag/wheel listeners on the current live/adopted canvas, once per
   * canvas identity (idempotent across suspend/resume within one boot). */
  _wireGestures() {
    if (!this._control || !this._controlKernel) return;
    const canvas = this._adoptedCanvas || (this._mode === "webgpu" ? this._liveCanvas : this._posterCanvas);
    if (!canvas || this._gestureListeners?.canvas === canvas) return;
    this._unwireGestures();

    let dragging = false;
    let dragPointerId = null;
    let lastDragPoint = null;

    const backingPoint = (event) => {
      const rect = canvas.getBoundingClientRect();
      const scaleX = this._backingWidth / (rect.width || 1);
      const scaleY = this._backingHeight / (rect.height || 1);
      return { mx: (event.clientX - rect.left) * scaleX, my: (event.clientY - rect.top) * scaleY, scaleX, scaleY };
    };

    const onPointerDown = (event) => {
      if (event.button !== 0) return;
      dragging = true;
      dragPointerId = event.pointerId;
      const { mx, my } = backingPoint(event);
      lastDragPoint = { mx, my };
      canvas.setPointerCapture(event.pointerId);
      event.preventDefault();
    };
    const onPointerMove = (event) => {
      if (!dragging || event.pointerId !== dragPointerId) return;
      const { mx, my } = backingPoint(event);
      const previous = lastDragPoint;
      lastDragPoint = { mx, my };
      if (!previous) return;
      this._applyGesture({ dx: mx - previous.mx, dy: my - previous.my, dzoom: 0, mx, my });
    };
    const onPointerUp = (event) => {
      if (event.pointerId !== dragPointerId) return;
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
      this._applyGesture({ dx: 0, dy: 0, dzoom: Math.sign(event.deltaY), mx, my });
    };

    canvas.addEventListener("pointerdown", onPointerDown);
    canvas.addEventListener("pointermove", onPointerMove);
    canvas.addEventListener("pointerup", onPointerUp);
    canvas.addEventListener("pointercancel", onPointerUp);
    canvas.addEventListener("wheel", onWheel, { passive: false });
    this._gestureListeners = { canvas, onPointerDown, onPointerMove, onPointerUp, onWheel };
  }

  _unwireGestures() {
    const listeners = this._gestureListeners;
    if (!listeners) return;
    const { canvas, onPointerDown, onPointerMove, onPointerUp, onWheel } = listeners;
    canvas.removeEventListener("pointerdown", onPointerDown);
    canvas.removeEventListener("pointermove", onPointerMove);
    canvas.removeEventListener("pointerup", onPointerUp);
    canvas.removeEventListener("pointercancel", onPointerUp);
    canvas.removeEventListener("wheel", onWheel);
    this._gestureListeners = null;
  }

  /** One raw gesture in: build `control.export`'s positional args from
   * `control.args` (live state by name, or the raw delta), call it, and blit
   * the reply back into `_uniforms` by `control.result`'s names. No writes
   * while cold/error/not presenting. */
  _applyGesture(raw) {
    if (this._fsm !== "live" || !this._controlKernel) return;
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
          return arg.axis === "x" ? raw.dx : raw.dy;
        case "wheel":
          return raw.dzoom;
        case "pointer":
          return arg.axis === "x" ? raw.mx : raw.my;
        default:
          return 0;
      }
    });
    const reply = this._controlKernel(...args);
    const results = Array.isArray(reply) ? reply : [reply];
    const next = this._uniforms.slice();
    control.result.forEach((name, index) => {
      const memberIndex = this._memberIndexByName.get(name);
      if (memberIndex !== undefined) next[memberIndex] = results[index];
    });
    this._queueGestureRender(next);
  }

  /** Keep every Fe-computed state transition, but present only the newest one
   * after the next animation-frame boundary and after the prior GPU submission
   * has completed. This is a generic latest-state throttle, not demo-specific
   * gesture math. A graph presentation still records its complete ordered pass
   * list in one command buffer. */
  _queueGestureRender(next) {
    this._uniforms = next;
    this._refreshControlValues();
    if (this._fsm !== "live") return;
    this._gestureDirty = true;
    this._scheduleGestureFrame();
  }

  _scheduleGestureFrame() {
    if (this._gestureFrame !== null || this._gesturePresenting || !this._gestureDirty) return;
    this._gestureFrame = requestAnimationFrame(() => {
      this._gestureFrame = null;
      void this._flushGestureFrame();
    });
  }

  async _flushGestureFrame() {
    if (this._fsm !== "live" || !this._gestureDirty || this._gesturePresenting) return;
    this._gestureDirty = false;
    this._gesturePresenting = true;
    try {
      this._render();
      const queue = this._mode === "webgpu" ? this._gpu?.device?.queue : null;
      if (queue?.onSubmittedWorkDone) await queue.onSubmittedWorkDone();
    } finally {
      this._gesturePresenting = false;
      if (this._fsm === "live" && this._gestureDirty) this._scheduleGestureFrame();
    }
  }

  // -- chrome: badge / controls / meta --------------------------------------

  _updateBadge() {
    if (!this._badge) return;
    this._badge.textContent = this._mode === "webgpu"
      ? `WebGPU · ${this._passes.length} pass${this._passes.length === 1 ? "" : "es"}`
      : "wasm · module.wasm";
    this._badge.className = `badge ${this._mode === "webgpu" ? "webgpu" : "wasm-2d"}`;
  }

  /**
   * Controls generated from the declared v5 `surface.params`: real label (the
   * field name), doc hover, range/step/init by kind. Extent-bound and fixed
   * params are not user-visible. Each param maps to its uniform member by
   * NAME (the reconciled binding key).
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
        ? `no view() declared — ${this._members.length} uniform member(s) held at 1.0`
        : "no view() declared";
      this._panel.append(notice);
      return;
    }
    this._surface.params.forEach((param) => {
      if (param.visible === false) return;
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
      const isInt = param.kind === "int";
      const min = typeof param.min === "number" ? param.min : 0;
      const max = typeof param.max === "number" ? param.max : 1;
      const isLog = param.kind === "log" && min > 0 && max > min;
      const encode = isLog
        ? (v) => Math.log10(Math.max(min, Math.min(max, +v)))
        : (v) => +v;
      const decode = isLog ? (v) => 10 ** (+v) : (v) => +v;
      const format = (v) => {
        const number = +v;
        if (isInt) return number.toFixed(0);
        if (isLog && (number < 0.01 || number >= 1000)) return number.toExponential(2);
        return Number(number.toPrecision(8)).toString();
      };
      value.textContent = format(this._uniforms[index]);
      const name = document.createElement("span");
      name.textContent = param.name;
      label.append(name, value);
      const input = document.createElement("input");
      input.type = "range";
      const inputMin = isLog ? Math.log10(min) : min;
      const inputMax = isLog ? Math.log10(max) : max;
      input.min = String(inputMin);
      input.max = String(inputMax);
      input.step = isInt ? "1" : String((inputMax - inputMin) / 200 || 0.01);
      input.value = String(encode(this._uniforms[index]));
      input.oninput = () => {
        const next = this._uniforms.slice();
        next[index] = decode(input.value);
        this._render(next);
      };
      row.append(label, input);
      this._panel.append(row);
      this._controlRows.push({ index, input, value, format, encode });
    });
  }

  _refreshControlValues() {
    for (const row of this._controlRows) {
      row.value.textContent = row.format(this._uniforms[row.index]);
      row.input.value = String(row.encode(this._uniforms[row.index]));
    }
  }

  _updateMeta() {
    if (!this._meta) return;
    const link = (href, text) => `<a href="${href}" target="_blank" rel="noopener">${text}</a>`;
    const wasm = this._wasmUrl
      ? link(this._wasmUrl.href, `wasm ${this._manifest.artifacts.wasm_bytes} B`) + ` · `
      : "";
    this._meta.innerHTML =
      `entry ${this._manifest.source_entry} · ` + wasm +
      link(this._wgslUrl.href, `wgsl ${this._manifest.artifacts.wgsl_bytes} B`) +
      ` · path ${this._mode} · fe ${this._manifest.provenance.compiler_version} · ` +
      link(this._manifestUrl.href, `manifest`);
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
    gpu: gpuOption,
    controls = true,
  } = options;

  const resolvedManifestUrl = new URL(manifestUrl, document.baseURI);
  const surface = document.createElement("fe-surface");
  surface.setAttribute("manifest", resolvedManifestUrl.href);
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
      surface._render(next);
      return surface._uniforms;
    },
  };
}
