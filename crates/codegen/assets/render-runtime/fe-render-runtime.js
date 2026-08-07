// fe render runtime (compiler-emitted, protocol fe-web-bundle v4/v5).
//
// The ONE fixed, versioned, demo-blind WebGPU/wasm render kernel driver
// shipped by the Fe toolchain. It is not hand-written per demo: it reads a
// fe-web-bundle manifest (v4, or v5 which additionally carries each uniform
// member's source field name + doc comment) and drives the two lowerings of
// the render kernel the compiler produced from the SAME source:
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
.fe-render canvas, .fe-render-controls canvas { image-rendering: auto; width: 100%;
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
.fe-render-undeclared { color: #d9a441; font-size: 12px; padding: 6px 8px;
             border: 1px dashed #4a3a1a; border-radius: 6px; background: #221a0c; }
`;
  document.head.appendChild(style);
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

/**
 * A bundle with no `surface` section (no declared `view()`): every uniform
 * member is held at a fixed, honest default (1.0, not a guess and not
 * searched) and the panel shows why there is no slider, instead of a
 * fabricated [0,128] range. This is the visible pressure the v5 migration
 * posture calls for (FE_WEB_V5_ORCHESTRATION_DESIGN.md 4.2): an undeclared
 * view stays visibly undeclared rather than silently guessing one.
 */
function undeclaredViewInitialUniforms(members) {
  return members.map(() => 1);
}

function buildUndeclaredViewNotice(panel, members) {
  panel.innerHTML = "";
  const notice = document.createElement("div");
  notice.className = "fe-render-ctl fe-render-undeclared";
  notice.textContent = members.length
    ? `no view() declared — ${members.length} uniform member(s) held at 1.0`
    : "no view() declared";
  panel.append(notice);
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
 * Build controls from the declared v5 `surface.params`: real label (the field
 * name), doc hover, range/step/init by kind. Extent-bound and fixed params are
 * not user-visible and get no control. Each param maps to its uniform member by
 * NAME (the reconciled binding key).
 */
function buildSurfaceControls(panel, members, surface, current, onChange) {
  panel.innerHTML = "";
  const indexByName = new Map(members.map((member, index) => [member.name, index]));
  surface.params.forEach((param) => {
    if (param.visible === false) return;
    const index = indexByName.get(param.name);
    if (index === undefined) return;
    const member = members[index];
    const row = document.createElement("div");
    row.className = "fe-render-ctl";
    const doc = member.doc || param.doc;
    if (doc) row.title = doc;
    const label = document.createElement("label");
    const value = document.createElement("b");
    const isInt = param.kind === "int";
    const format = (v) => (+v).toFixed(isInt ? 0 : 2);
    value.textContent = format(current()[index]);
    const name = document.createElement("span");
    name.textContent = param.name;
    label.append(name, value);
    const input = document.createElement("input");
    input.type = "range";
    const min = typeof param.min === "number" ? param.min : 0;
    const max = typeof param.max === "number" ? param.max : 1;
    input.min = String(min);
    input.max = String(max);
    input.step = isInt ? "1" : String((max - min) / 200 || 0.01);
    input.value = String(current()[index]);
    input.oninput = () => {
      // Slice the LIVE uniform vector so moving one slider preserves the rest.
      const next = current().slice();
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
 * @param {number[]} [options.initial] - explicit initial uniform vector,
 *   overriding the manifest. Real initial values normally come from the
 *   bundle's declared `surface.params[].init` (protocol v5, projected from
 *   the actor's `view()`); a bundle with no declared view holds every member
 *   at a fixed 1.0 (visibly undeclared, never searched or guessed).
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
  const manifest = await (await fetchOrThrow(resolvedManifestUrl, "manifest")).json();
  const layout = manifest.layout;
  // Protocol v5 `surface` section (projected from the actor's `view()`): real
  // param ranges/init/kind and the dispatch extent. When present the runtime
  // guesses NOTHING: no [0,128] slider, no uniform search, no page-attr size.
  const surface = manifest.surface || null;
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

  // Dispatch/canvas extent: the declared `surface.extent` when present (the
  // page carries no sizes in v5), else the caller's width/height, else the
  // legacy default.
  const width = surface?.extent?.width ?? widthOption ?? DEFAULT_SIZE;
  const height = surface?.extent?.height ?? heightOption ?? widthOption ?? DEFAULT_SIZE;

  ensureStyle();
  const dom = buildDom({ canvasOption, container, mountAfter, controls });
  dom.canvas.width = width;
  dom.canvas.height = height;

  const renderer =
    (await initWebGpu({ canvas: dom.canvas, layout, inputBinding, members, gpuOption, wgslUrl })) ??
    initWasmFallback({ canvas: dom.canvas, width, height, callKernel });

  // Initial uniform vector: from the declared `surface` (init values, with
  // extent-bound members fed the live canvas size); an explicit `initial`
  // override; or, for a bundle with no declared view(), a fixed 1.0 per
  // member (visibly undeclared, never searched or guessed).
  let uniforms = surface
    ? surfaceInitialUniforms(members, surface, width, height)
    : (initial ?? undeclaredViewInitialUniforms(members));

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
    if (surface) {
      buildSurfaceControls(dom.panel, members, surface, () => uniforms, (next) => render(next));
    } else {
      buildUndeclaredViewNotice(dom.panel, members);
    }
  }
  if (dom.metaEl) {
    // Unobtrusive links to the generated artifacts the toolchain emitted from
    // the same source: the wasm kernel, the wgsl shader, and the v-manifest.
    const link = (href, text) =>
      `<a href="${href}" target="_blank" rel="noopener" style="color:inherit;text-decoration:underline dotted">${text}</a>`;
    dom.metaEl.innerHTML =
      `entry ${manifest.source_entry} · ` +
      link(wasmUrl.href, `wasm ${manifest.artifacts.wasm_bytes} B`) +
      ` · ` +
      link(wgslUrl.href, `wgsl ${manifest.artifacts.wgsl_bytes} B`) +
      ` · path ${renderer.mode} · fe ${manifest.provenance.compiler_version} · ` +
      link(resolvedManifestUrl.href, `manifest`);
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
