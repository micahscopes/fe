// fe render runtime (compiler-emitted, protocol fe-web-bundle v4).
//
// The ONE fixed, versioned, demo-blind WebGPU/wasm render kernel driver
// shipped by the Fe toolchain. It is not hand-written per demo: it reads a
// fe-web-bundle v4 manifest and drives the two lowerings of the render
// kernel the compiler produced from the SAME source:
//   - the GPU lane: shader.wgsl via WebGPU (vertex/fragment entries and the
//     uniform binding table are read from manifest.layout);
//   - a CPU fallback when WebGPU is unavailable (e.g. an insecure origin):
//     the module.wasm kernel invoked per pixel into a 2D canvas.
// Uniform controls are generated from the manifest's input binding members.
//
// One shared WebGPU adapter/device serves every surface mounted on a page,
// so a gallery of N demos (N `mountRenderSurface` calls, one per canvas)
// costs one device, not N.
//
// This module is the ONLY copy of the render kernel's browser glue. Both the
// legacy `fe web build --mode render` bundle (its emitted index.html imports
// it as a sibling file) and the standards `application/fe` `data-fe-render`
// handoff (crates/html-precompile/assets/bootstrap.js, dispatched instead of
// calling a Wasm entry with zero arguments) import this SAME text.

const STYLE_ID = "fe-render-runtime-style";
const DEFAULT_SIZE = 256; // dispatch/canvas size; matches the legacy bundle's prior default.

let sharedGpuPromise;

/** One WebGPU adapter/device for the whole page, requested at most once. */
function acquireSharedGpu() {
  if (sharedGpuPromise === undefined) {
    sharedGpuPromise = requestGpu();
  }
  return sharedGpuPromise;
}

async function requestGpu() {
  if (!navigator.gpu) return null;
  try {
    const adapter = await navigator.gpu.requestAdapter();
    if (!adapter) return null;
    const device = await adapter.requestDevice();
    return { adapter, device };
  } catch (error) {
    console.warn("[fe web] WebGPU init failed, using wasm fallback:", error);
    return null;
  }
}

function ensureStyle() {
  if (document.getElementById(STYLE_ID)) return;
  const style = document.createElement("style");
  style.id = STYLE_ID;
  style.textContent = `
.fe-render { display: grid; gap: 16px; grid-template-columns: auto minmax(180px, 260px);
             align-items: start; font: 14px/1.5 ui-monospace, SFMono-Regular, Menlo, monospace;
             color: #cfd6e4; margin: 0; }
.fe-render canvas, .fe-render-controls canvas { image-rendering: pixelated; width: 100%;
             max-width: 384px; aspect-ratio: 1; border-radius: 10px; box-shadow: 0 8px 40px #0008;
             background: #000; }
.fe-render-panel { display: grid; gap: 12px; }
.fe-render-ctl { display: grid; gap: 4px; }
.fe-render-ctl label { display: flex; justify-content: space-between; color: #96a0b5; }
.fe-render-ctl b { color: #cfd6e4; font-weight: 600; }
.fe-render input[type=range], .fe-render-controls input[type=range] { width: 100%; accent-color: #5b8cff; }
.fe-render-meta { font-size: 12px; color: #6b7688; }
.fe-render-badge { display: inline-block; padding: 2px 7px; border-radius: 6px;
                    font-size: 11px; font-weight: 600; }
.fe-render-badge.webgpu { background: #10281a; color: #5bffa0; }
.fe-render-badge.wasm { background: #1a2030; color: #8fb0ff; }
`;
  document.head.appendChild(style);
}

/** Deterministic mulberry32 PRNG: same manifest -> same search -> same pixels. */
function mulberry32(seed) {
  let a = seed >>> 0;
  return function next() {
    a |= 0;
    a = (a + 0x6d2b79f5) | 0;
    let t = Math.imul(a ^ (a >>> 15), 1 | a);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

/**
 * Bounded, DETERMINISTIC search for a uniform vector that makes the kernel
 * vary spatially, so any render bundle shows something meaningful on first
 * load with no authored initial state.
 *
 * This is an interim default, not the destination: an actor's real initial
 * view belongs in Fe (a manifest `controls.initial` projection, the v5
 * ctl-lane work), not guessed by the runtime. It replaces a `Math.random()`
 * search that produced different, nondeterministic pixels on every reload
 * for the exact same bundle; a seeded search at least makes "first load"
 * reproducible in the meantime. Callers with a real initial state pass
 * `initial` to `mountRenderSurface` and this function is not consulted.
 */
function deterministicInitialUniforms(members, callKernel) {
  if (members.length === 0) return [];
  const presets = [0, 0.5, 1, 2, 4, 8, 16, 32, 64];
  const size = 48;
  const random = mulberry32(0x9e3779b9);
  let best = members.map(() => 1);
  let bestVariance = -1;
  for (let trial = 0; trial < 96; trial++) {
    const candidate = members.map(() => presets[(random() * presets.length) | 0]);
    let sum = 0;
    let sumSquares = 0;
    let count = 0;
    const seen = new Set();
    for (let py = 0; py < size; py += 2) {
      for (let px = 0; px < size; px += 2) {
        const value = callKernel(px, py, candidate);
        seen.add(value);
        const luminance = ((value >>> 16) & 255) + ((value >>> 8) & 255) + (value & 255);
        sum += luminance;
        sumSquares += luminance * luminance;
        count += 1;
      }
    }
    const variance = sumSquares / count - (sum / count) ** 2;
    if (seen.size > 3 && variance > bestVariance) {
      bestVariance = variance;
      best = candidate;
    }
  }
  return best;
}

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

function buildDom({ canvasOption, container, mountAfter, controls }) {
  const adopted = resolveCanvas(canvasOption);
  if (adopted) {
    let panel = null;
    let modeEl = null;
    let metaEl = null;
    if (controls) {
      const side = document.createElement("div");
      side.className = "fe-render-controls";
      modeEl = document.createElement("span");
      modeEl.className = "fe-render-badge";
      panel = document.createElement("div");
      panel.className = "fe-render-panel";
      metaEl = document.createElement("div");
      metaEl.className = "fe-render-meta";
      side.append(modeEl, panel, metaEl);
      adopted.insertAdjacentElement("afterend", side);
    }
    return { root: adopted, canvas: adopted, panel, modeEl, metaEl };
  }

  const figure = document.createElement("figure");
  figure.className = "fe-render";
  const canvas = document.createElement("canvas");
  const side = document.createElement("div");
  const modeEl = document.createElement("span");
  modeEl.className = "fe-render-badge";
  const panel = document.createElement("div");
  panel.className = "fe-render-panel";
  const metaEl = document.createElement("div");
  metaEl.className = "fe-render-meta";
  side.append(modeEl, panel, metaEl);
  figure.append(canvas, side);

  if (container) {
    container.appendChild(figure);
  } else if (mountAfter && mountAfter.parentNode) {
    mountAfter.parentNode.insertBefore(figure, mountAfter.nextSibling);
  } else {
    document.body.appendChild(figure);
  }
  return { root: figure, canvas, panel, modeEl, metaEl };
}

function buildControls(panel, members, uniforms, onChange) {
  panel.innerHTML = "";
  members.forEach((member, index) => {
    const row = document.createElement("div");
    row.className = "fe-render-ctl";
    const label = document.createElement("label");
    const value = document.createElement("b");
    const format = (v) => (+v).toFixed(member.scalar === "f32" ? 2 : 0);
    value.textContent = format(uniforms[index]);
    const name = document.createElement("span");
    name.textContent = `${member.scalar} @${member.arg_index}`;
    label.append(name, value);
    const input = document.createElement("input");
    input.type = "range";
    input.min = "0";
    input.max = "128";
    input.step = member.scalar === "f32" ? "0.25" : "1";
    input.value = String(uniforms[index]);
    input.oninput = () => {
      const next = uniforms.slice();
      next[index] = +input.value;
      value.textContent = format(input.value);
      onChange(next);
    };
    row.append(label, input);
    panel.append(row);
  });
}

async function initWebGpu({ canvas, layout, inputBinding, members, gpuOption, wgslUrl }) {
  const gpu = gpuOption ?? (await acquireSharedGpu());
  if (!gpu) return null;
  const { device } = gpu;
  try {
    const wgsl = await (await fetchOrThrow(wgslUrl, "WGSL shader")).text();
    const shaderModule = device.createShaderModule({ code: wgsl });
    const format = layout.color_target_format || navigator.gpu.getPreferredCanvasFormat();
    const context = canvas.getContext("webgpu");
    context.configure({ device, format, alphaMode: "opaque" });

    let bindGroup = null;
    let uniformBuffer = null;
    let pipelineLayout = "auto";
    if (inputBinding) {
      uniformBuffer = device.createBuffer({
        size: Math.max(16, inputBinding.span),
        usage: GPUBufferUsage.STORAGE | GPUBufferUsage.COPY_DST,
      });
      const bindGroupLayout = device.createBindGroupLayout({
        entries: [
          {
            binding: inputBinding.binding,
            visibility: GPUShaderStage.FRAGMENT,
            buffer: { type: "read-only-storage" },
          },
        ],
      });
      bindGroup = device.createBindGroup({
        layout: bindGroupLayout,
        entries: [{ binding: inputBinding.binding, resource: { buffer: uniformBuffer } }],
      });
      pipelineLayout = device.createPipelineLayout({ bindGroupLayouts: [bindGroupLayout] });
    }
    const pipeline = device.createRenderPipeline({
      layout: pipelineLayout,
      vertex: { module: shaderModule, entryPoint: layout.vertex_entry },
      fragment: { module: shaderModule, entryPoint: layout.fragment_entry, targets: [{ format }] },
      primitive: { topology: "triangle-list" },
    });

    return {
      mode: "webgpu",
      render(uniforms) {
        if (uniformBuffer) {
          const buffer = new ArrayBuffer(Math.max(16, inputBinding.span));
          const view = new DataView(buffer);
          members.forEach((member, index) => {
            if (member.scalar === "f32") view.setFloat32(member.offset, uniforms[index], true);
            else if (member.scalar === "u32") view.setUint32(member.offset, uniforms[index] >>> 0, true);
            else view.setInt32(member.offset, uniforms[index] | 0, true);
          });
          device.queue.writeBuffer(uniformBuffer, 0, buffer);
        }
        const encoder = device.createCommandEncoder();
        const pass = encoder.beginRenderPass({
          colorAttachments: [
            {
              view: canvas.getContext("webgpu").getCurrentTexture().createView(),
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
      },
    };
  } catch (error) {
    console.warn("[fe web] WebGPU pipeline init failed, using wasm fallback:", error);
    return null;
  }
}

function initWasmFallback({ canvas, width, height, callKernel }) {
  const context = canvas.getContext("2d");
  const image = context.createImageData(width, height);
  return {
    mode: "wasm",
    render(uniforms) {
      const data = image.data;
      for (let py = 0; py < height; py++) {
        for (let px = 0; px < width; px++) {
          const rgba = callKernel(px, py, uniforms);
          const i = (py * width + px) * 4;
          data[i] = (rgba >>> 16) & 255;
          data[i + 1] = (rgba >>> 8) & 255;
          data[i + 2] = rgba & 255;
          data[i + 3] = (rgba >>> 24) & 255;
        }
      }
      context.putImageData(image, 0, 0);
    },
  };
}

/**
 * Mount one render surface for a fe-web-bundle v4 manifest.
 *
 * @param {object} options
 * @param {string|URL} options.manifestUrl - fe-web-bundle v4 manifest URL.
 *   wasm/wgsl artifact paths named in the manifest are resolved RELATIVE TO
 *   THIS URL (not the page), so a manifest published anywhere (a co-located
 *   legacy bundle, or a content-addressed `assets/fe-render-<hash>.json`)
 *   resolves its sibling artifacts correctly.
 * @param {HTMLCanvasElement|string} [options.canvas] - an existing canvas
 *   element (or CSS selector) to adopt; a new one is created otherwise.
 * @param {Element} [options.container] - parent to append a generated
 *   `<figure class="fe-render">` into, when `canvas` is not adopted.
 * @param {Node} [options.mountAfter] - insert the generated figure directly
 *   after this node when neither `canvas` nor `container` is given.
 * @param {number} [options.width=256] - dispatch/canvas resolution.
 * @param {number} [options.height=width]
 * @param {number[]} [options.initial] - explicit initial uniform vector (the
 *   landing hook for a future Fe-declared `controls.initial` manifest
 *   projection). Falls back to the deterministic search above when absent.
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
    width = DEFAULT_SIZE,
    height = width,
    initial,
    gpu: gpuOption,
    controls = true,
  } = options;

  const resolvedManifestUrl = new URL(manifestUrl, document.baseURI);
  const manifest = await (await fetchOrThrow(resolvedManifestUrl, "manifest")).json();
  const layout = manifest.layout;
  const inputBinding = layout.bindings.find((binding) => binding.role === "input");
  const members = inputBinding ? inputBinding.members : [];
  const builtins = layout.builtin_inputs || [];
  const argumentCount =
    1 + Math.max(-1, ...builtins.map((b) => b.arg_index), ...members.map((m) => m.arg_index));

  const wasmUrl = new URL(manifest.artifacts.wasm, resolvedManifestUrl);
  const wgslUrl = new URL(manifest.artifacts.wgsl, resolvedManifestUrl);

  const wasmBytes = await (await fetchOrThrow(wasmUrl, "wasm module")).arrayBuffer();
  const { instance } = await WebAssembly.instantiate(wasmBytes, {});
  const kernel = instance.exports[manifest.source_entry];
  if (typeof kernel !== "function") {
    throw new Error(`fe render runtime: wasm export \`${manifest.source_entry}\` not found`);
  }

  function callKernel(px, py, uniforms) {
    const args = new Array(argumentCount).fill(0);
    for (const builtin of builtins) {
      args[builtin.arg_index] = builtin.source.endsWith("_y") ? py : px;
    }
    members.forEach((member, index) => {
      args[member.arg_index] = uniforms[index];
    });
    return kernel(...args) >>> 0; // 0xAARRGGBB
  }

  ensureStyle();
  const dom = buildDom({ canvasOption, container, mountAfter, controls });
  dom.canvas.width = width;
  dom.canvas.height = height;

  const renderer =
    (await initWebGpu({ canvas: dom.canvas, layout, inputBinding, members, gpuOption, wgslUrl })) ??
    initWasmFallback({ canvas: dom.canvas, width, height, callKernel });

  let uniforms = initial ?? deterministicInitialUniforms(members, callKernel);

  function render(nextUniforms) {
    if (nextUniforms) uniforms = nextUniforms;
    renderer.render(uniforms);
    if (dom.modeEl) {
      dom.modeEl.textContent = renderer.mode === "webgpu" ? "WebGPU · shader.wgsl" : "wasm · module.wasm";
      dom.modeEl.className = `fe-render-badge ${renderer.mode}`;
    }
    return uniforms;
  }

  if (dom.panel && controls) {
    buildControls(dom.panel, members, uniforms, (next) => render(next));
  }
  if (dom.metaEl) {
    dom.metaEl.textContent =
      `entry ${manifest.source_entry} · wasm ${manifest.artifacts.wasm_bytes} B` +
      ` · wgsl ${manifest.artifacts.wgsl_bytes} B · path ${renderer.mode}` +
      ` · fe ${manifest.provenance.compiler_version}`;
  }

  render(uniforms);
  window.__feReady = true;
  window.__feMode = renderer.mode;

  return {
    mode: renderer.mode,
    canvas: dom.canvas,
    element: dom.root,
    manifest,
    manifestUrl: resolvedManifestUrl,
    get uniforms() {
      return uniforms;
    },
    render,
  };
}
