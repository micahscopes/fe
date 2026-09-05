import assert from "node:assert/strict";
import { webcrypto } from "node:crypto";
import test from "node:test";

// The module defines a custom element at load time. These minimal standards
// stubs let this host-policy unit gate import the fixed runtime without
// constructing a surface or pretending to be a browser.
globalThis.HTMLElement = class HTMLElement {};
globalThis.customElements = { define() {} };
const { rasterPlan, rasterColorTarget, rasterPrimitive } = await import("./fe-render-runtime.js");

const { FeSurfaceElement, GpuDeviceEventKind, GpuDeviceLossReason, PassPreparationMode, SurfaceEventKind, SurfaceQueueAction, SurfaceRecoveryAction, bindingShaderVisibility, coordinateSurfaceRecovery, createGpuDeviceLifecycleChannel, createGpuQueueIdleChannel, fetchVerifiedResourceArtifact, fitBackingExtent, installGeneratedWebGpuOperations, passShaderVisibility, rasterDrawShape, readGpuBufferSnapshot, realizePassPipeline, requiresGpuPassGraph, resourceBufferUsage, selectActivePassRecords, selectPreparedPassRecords, surfaceParamPlan, unpackCanvasReadback, wgslPayloadSummary, writeSurfaceEventBatch } =
  await import("./fe-render-runtime.js");

test("Fe pass activation selects a memoized subgraph once per policy", () => {
  const records = [
    { pass: { source_entry: "background" } },
    { pass: { source_entry: "analytic", activation: 0 } },
    { pass: { source_entry: "pullback-a", activation: 1 } },
    { pass: { source_entry: "pullback-b", activation: 1 } },
  ];
  const calls = [0, 0];
  const kernels = [
    (mode) => { calls[0] += 1; return mode < 0.5 ? 1 : 0; },
    (mode) => { calls[1] += 1; return mode >= 0.5 ? 1 : 0; },
  ];
  assert.deepEqual(
    selectActivePassRecords(records, kernels, [0]).map(record => record.pass.source_entry),
    ["background", "analytic"],
  );
  assert.deepEqual(calls, [1, 1]);
  calls[0] = 0;
  calls[1] = 0;
  assert.deepEqual(
    selectActivePassRecords(records, kernels, [1]).map(record => record.pass.source_entry),
    ["background", "pullback-a", "pullback-b"],
  );
  assert.deepEqual(calls, [1, 1], "one policy shared by two passes is evaluated once");
});

test("selected pass pipelines are realized lazily and memoized while resident", async () => {
  const calls = { modules: 0, compute: 0, render: 0 };
  const device = {
    createShaderModule() {
      calls.modules += 1;
      return { kind: "module" };
    },
    async createComputePipelineAsync(descriptor) {
      calls.compute += 1;
      assert.equal(descriptor.compute.module.kind, "module");
      return { kind: "compute" };
    },
    async createRenderPipelineAsync() {
      calls.render += 1;
      return { kind: "render" };
    },
  };
  const record = {
    pass: { layout: { mode: "compute" } },
    pipeline: null,
    shaderModule: null,
    shaderSource: "@compute @workgroup_size(1) fn sample() {}",
    pipelineDescriptor: { compute: { entryPoint: "sample" } },
  };
  const first = realizePassPipeline(device, record);
  const second = realizePassPipeline(device, record);
  assert.equal(first, second);
  assert.deepEqual(await Promise.all([first, second]), [record.pipeline, record.pipeline]);
  assert.deepEqual(calls, { modules: 1, compute: 1, render: 0 });
});

test("Fe pass preparation groups policies without granting activation", () => {
  const records = [
    { pass: { source_entry: "always-lazy" } },
    { pass: { source_entry: "analytic", preparation: 0 } },
    { pass: { source_entry: "pullback-a", preparation: 1 } },
    { pass: { source_entry: "pullback-b", preparation: 1 } },
  ];
  const calls = [0, 0];
  const plan = selectPreparedPassRecords(records, [
    () => { calls[0] += 1; return PassPreparationMode.Eager; },
    () => { calls[1] += 1; return PassPreparationMode.VisibleIdle; },
  ], []);
  assert.deepEqual(
    plan.eager.map(record => record.pass.source_entry),
    ["analytic"],
  );
  assert.deepEqual(
    plan.visibleIdle.map(record => record.pass.source_entry),
    ["pullback-a", "pullback-b"],
  );
  assert.deepEqual(calls, [1, 1]);
  assert.throws(
    () => selectPreparedPassRecords(
      [{ pass: { source_entry: "bad", preparation: 0 } }],
      [() => 3],
      [],
    ),
    /returned an invalid mode/,
  );
});

test("asynchronous pass preparation shares one resident pipeline promise", async () => {
  const calls = { modules: 0, pipelines: 0 };
  const device = {
    createShaderModule() {
      calls.modules += 1;
      return { kind: "module" };
    },
    async createComputePipelineAsync(descriptor) {
      calls.pipelines += 1;
      assert.equal(descriptor.compute.module.kind, "module");
      await Promise.resolve();
      return { kind: "prepared-compute" };
    },
  };
  const record = {
    pass: { layout: { mode: "compute" } },
    pipeline: null,
    pipelinePromise: null,
    shaderModule: null,
    shaderSource: "@compute @workgroup_size(1) fn sample() {}",
    pipelineDescriptor: { compute: { entryPoint: "sample" } },
  };
  const first = realizePassPipeline(device, record);
  const second = realizePassPipeline(device, record);
  assert.equal(first, second);
  assert.deepEqual(await Promise.all([first, second]), [record.pipeline, record.pipeline]);
  assert.deepEqual(calls, { modules: 1, pipelines: 1 });
});

test("WGSL summary aggregates unique pass shaders rather than the primary artifact", () => {
  assert.deepEqual(
    wgslPayloadSummary({
      artifacts: { wgsl: "last.wgsl", wgsl_bytes: 790 },
      passes: [
        { shader: "patch.wgsl", shader_bytes: 17549 },
        { shader: "handle.wgsl", shader_bytes: 2400 },
        { shader: "last.wgsl", shader_bytes: 790 },
        { shader: "patch.wgsl", shader_bytes: 17549 },
      ],
    }),
    { bytes: 20739, shaders: 3 },
  );
  assert.deepEqual(
    wgslPayloadSummary({ artifacts: { wgsl: "one.wgsl", wgsl_bytes: 790 } }),
    { bytes: 790, shaders: 1 },
  );
});

installGeneratedWebGpuOperations({
  renderBlendConstant: (pass, color) => pass.setBlendConstant(color),
  queueIdle: queue => queue.onSubmittedWorkDone(),
  bufferCreate: (device, descriptor) => device.createBuffer(descriptor),
  bufferWrite: (queue, buffer, offset, bytes) =>
    queue.writeBuffer(buffer, offset, bytes),
  renderDraw: (pass, vertexCount, instanceCount, firstVertex, firstInstance) =>
    pass.draw(vertexCount, instanceCount, firstVertex, firstInstance),
  renderDrawIndirect: (pass, buffer, offset) =>
    pass.drawIndirect(buffer, offset),
});

test("Fe primitive plans preserve all native topologies and winding per pass", () => {
  const raster = { cullMode: "back" };
  for (const topology of ["point_list", "line_list", "line_strip", "triangle_list", "triangle_strip"]) {
    for (const front_face of ["ccw", "cw"]) {
      assert.deepEqual(rasterPrimitive({primitive: {topology, front_face}}, raster), {
        topology: topology.replaceAll("_", "-"), frontFace: front_face, cullMode: "back",
      });
    }
  }
  assert.deepEqual(rasterPrimitive({}, raster), { topology: "triangle-list", frontFace: "ccw", cullMode: "back" });
  assert.throws(() => rasterPrimitive({primitive: {topology: "quads", front_face: "ccw"}}, raster), /invalid Fe primitive/);
  assert.throws(() => rasterPrimitive({primitive: null}, raster), /invalid Fe primitive/);
  assert.throws(() => rasterPrimitive({primitive: {topology: "line_list", front_face: "unknown"}}, raster), /invalid Fe primitive/);
});

test("Fe color targets preserve blend components, write masks, and constants", () => {
  assert.equal(requiresGpuPassGraph([{layout: {mode: "render"}}], [], {sample_count: 4}), true,
    "a fullscreen-only shader still requires its authored raster policy");
  const component = (src, dst, operation = "add") => ({ operation, src_factor: src, dst_factor: dst });
  const policy = {
    sample_count: 4, cull_mode: "none", depth: null,
    color: {
      clear: { r: 0, g: 0, b: 0, a: 1 },
      ops: { first_load: "clear", following_load: "load", store: "store" },
      write_mask: 7, blend_constant: { r: 0.1, g: 0.2, b: 0.3, a: 0.4 },
      blend: { color: component("src_alpha", "one_minus_src_alpha"), alpha: component("one", "one_minus_src_alpha") },
    },
  };
  const plan = () => rasterPlan({ pipeline: { raster: policy } });
  assert.deepEqual(rasterPlan(null, policy), plan(), "render policy must not depend on a UI view");
  assert.deepEqual(rasterColorTarget("bgra8unorm", plan()), {
    format: "bgra8unorm", writeMask: 7,
    blend: {
      color: { operation: "add", srcFactor: "src-alpha", dstFactor: "one-minus-src-alpha" },
      alpha: { operation: "add", srcFactor: "one", dstFactor: "one-minus-src-alpha" },
    },
  });
  assert.deepEqual(plan().color.blendConstant, policy.color.blend_constant);
  // Every portable factor travels independently in either component. Do not
  // accidentally make the presets the only representable native blend states.
  const factors = ["zero", "one", "src", "one_minus_src", "src_alpha", "one_minus_src_alpha",
    "dst", "one_minus_dst", "dst_alpha", "one_minus_dst_alpha", "src_alpha_saturated",
    "constant", "one_minus_constant"];
  for (const operation of ["add", "subtract", "reverse_subtract"]) {
    for (const src of factors) for (const dst of factors) {
      policy.color.blend.color = component(src, dst, operation);
      assert.deepEqual(plan().color.blend.color, {
        operation: operation.replaceAll("_", "-"),
        srcFactor: src.replaceAll("_", "-"), dstFactor: dst.replaceAll("_", "-"),
      });
    }
  }
  for (const operation of ["min", "max"]) {
    policy.color.blend.color = component("one", "one", operation);
    assert.equal(plan().color.blend.color.operation, operation);
  }
  for (const [operation, src, dst] of [["max", "src_alpha", "one"], ["multiply", "one", "one"], ["add", "src1", "one"]]) {
    policy.color.blend.color = component(src, dst, operation);
    assert.throws(plan, /invalid derived blend component/);
  }
  policy.color.blend = null;
  assert.deepEqual(rasterColorTarget("bgra8unorm", plan()), { format: "bgra8unorm", writeMask: 7 });
  policy.color.write_mask = 16;
  assert.throws(plan, /invalid derived color write mask/);
  policy.color.write_mask = 0;
  assert.equal(plan().color.writeMask, 0);
  policy.color.blend_constant.a = NaN;
  assert.throws(plan, /invalid derived color write mask or blend constant/);
});

test("compiler-derived resource usage maps exactly and legacy manifests stay compatible", () => {
  const constants = { STORAGE: 0x80, COPY_SRC: 0x04, COPY_DST: 0x08 };
  assert.equal(resourceBufferUsage({ buffer_usage: ["storage"] }, constants), 0x80);
  assert.equal(
    resourceBufferUsage({ buffer_usage: ["storage", "copy_dst"] }, constants),
    0x88,
  );
  assert.equal(
    resourceBufferUsage({ buffer_usage: ["storage", "copy_src"] }, constants),
    0x84,
  );
  assert.equal(resourceBufferUsage({}, constants, 7), 0x8c);
  assert.throws(
    () => resourceBufferUsage({}, constants, 8),
    /v8 resource is missing compiler-derived buffer_usage/,
  );
  assert.throws(
    () => resourceBufferUsage({ buffer_usage: ["storage", "storage"] }, constants),
    /duplicate resource buffer usage/,
  );
  assert.throws(
    () => resourceBufferUsage({ buffer_usage: ["storage", "map_read"] }, constants),
    /unsupported resource buffer usage/,
  );
});

test("compiler-derived pass stages map exactly and v8 omissions fail closed", () => {
  const constants = { COMPUTE: 0x04, VERTEX: 0x01, FRAGMENT: 0x02 };
  assert.equal(
    passShaderVisibility({ shader_stages: ["compute"] }, constants, 8),
    0x04,
  );
  assert.equal(
    passShaderVisibility({ shader_stages: ["vertex", "fragment"] }, constants, 8),
    0x03,
  );
  assert.equal(
    passShaderVisibility({ layout: { mode: "render" }, draw_vertices: 3 }, constants, 7),
    0x03,
  );
  assert.throws(
    () => passShaderVisibility({ layout: { mode: "render" } }, constants, 8),
    /v8 pass is missing compiler-derived shader_stages/,
  );
});

test("compiler-derived binding stages stay narrow and v10 omissions fail closed", () => {
  const constants = { COMPUTE: 0x04, VERTEX: 0x01, FRAGMENT: 0x02 };
  const pass = { shader_stages: ["vertex", "fragment"] };
  assert.equal(
    bindingShaderVisibility({ shader_stages: ["vertex"] }, pass, constants, 10),
    0x01,
  );
  assert.equal(
    bindingShaderVisibility({ shader_stages: ["fragment"] }, pass, constants, 10),
    0x02,
  );
  assert.equal(
    bindingShaderVisibility(
      { shader_stages: ["vertex", "fragment"] },
      pass,
      constants,
      10,
    ),
    0x03,
  );
  assert.equal(
    bindingShaderVisibility({}, pass, constants, 9),
    0x03,
    "v9 manifests retain pass-wide binding visibility",
  );
  assert.throws(
    () => bindingShaderVisibility({}, pass, constants, 10),
    /v10 binding is missing compiler-derived shader_stages/,
  );
  assert.throws(
    () => bindingShaderVisibility(
      { shader_stages: ["compute"] },
      pass,
      constants,
      10,
    ),
    /outside its pass stage set/,
  );
});

test("immutable resource artifacts are authenticated on every realization", async () => {
  const expected = new TextEncoder().encode("0123456789abcde\n");
  const sha256 = "dc08b6f2c7aaeca6d88cd9c82797b328160ccb3b1a84243b8eadb296744426c4";
  const resource = {
    name: "palette",
    stride: 4,
    length: 4,
    policy: { initialization: { kind: "content_addressed", sha256 } },
    artifact: {
      path: `resources/sha256-${sha256}.bin`,
      bytes: expected.byteLength,
      sha256,
    },
  };
  const urls = [];
  const fetchImpl = async url => {
    urls.push(String(url));
    return {
      ok: true,
      status: 200,
      async arrayBuffer() {
        return expected.slice().buffer;
      },
    };
  };
  const options = { fetchImpl, cryptoImpl: webcrypto };
  assert.deepEqual(
    await fetchVerifiedResourceArtifact(resource, new URL("https://example.test/demo/manifest.json"), options),
    expected,
  );
  assert.deepEqual(
    await fetchVerifiedResourceArtifact(resource, new URL("https://example.test/demo/manifest.json"), options),
    expected,
  );
  assert.deepEqual(urls, [
    `https://example.test/demo/resources/sha256-${sha256}.bin`,
    `https://example.test/demo/resources/sha256-${sha256}.bin`,
  ], "device reconstruction must re-fetch and re-authenticate logical bytes");

  const corrupted = expected.slice();
  corrupted[0] ^= 0xff;
  await assert.rejects(
    fetchVerifiedResourceArtifact(resource, new URL("https://example.test/demo/manifest.json"), {
      cryptoImpl: webcrypto,
      fetchImpl: async () => ({
        ok: true,
        status: 200,
        async arrayBuffer() { return corrupted.buffer; },
      }),
    }),
    /failed SHA-256 verification/,
  );

  const zeroed = { name: "scratch", stride: 4, length: 4 };
  assert.equal(
    await fetchVerifiedResourceArtifact(zeroed, new URL("https://example.test/manifest.json"), {
      fetchImpl: () => assert.fail("zeroed storage must not fetch"),
      cryptoImpl: webcrypto,
    }),
    null,
  );
});

test("shared GPU lifecycle channel replays ordered typed facts and reports bounded gaps", async () => {
  const channel = createGpuDeviceLifecycleChannel(2);
  const waiting = channel.observe(false, 0);
  channel.publish(GpuDeviceEventKind.Available, GpuDeviceLossReason.NotLost, 1);
  assert.deepEqual(await waiting, {
    kind: 1, reason: 0, generation: 1, sequence: 1, missed: 0,
  });

  channel.publish(GpuDeviceEventKind.Lost, GpuDeviceLossReason.Unknown, 1);
  channel.publish(GpuDeviceEventKind.Available, GpuDeviceLossReason.NotLost, 2);
  assert.deepEqual(await channel.observe(true, 1), {
    kind: 2, reason: 1, generation: 1, sequence: 2, missed: 0,
  });

  // Sequence 2 and 3 are retained. A consumer that last saw sequence 0 gets
  // the first retained fact plus an explicit one-event history gap.
  assert.deepEqual(await channel.observe(true, 0), {
    kind: 2, reason: 1, generation: 1, sequence: 2, missed: 1,
  });
  assert.deepEqual(await channel.observe(false, 0), {
    kind: 1, reason: 0, generation: 2, sequence: 3, missed: 0,
  });
});

test("shared GPU lifecycle observation is affine and cancellable", async () => {
  const channel = createGpuDeviceLifecycleChannel();
  const controller = new AbortController();
  const pending = channel.observe(false, 0, controller.signal);
  controller.abort();
  await assert.rejects(pending, error => error.name === "AbortError");
  channel.publish(GpuDeviceEventKind.Unavailable, GpuDeviceLossReason.NotLost, 0);
  assert.deepEqual(await channel.observe(false, 0), {
    kind: 3, reason: 0, generation: 0, sequence: 1, missed: 0,
  });
  assert.throws(
    () => channel.publish(GpuDeviceEventKind.Unknown, GpuDeviceLossReason.NotLost, 0),
    /cannot publish the Fe placeholder/,
  );
});

test("shared GPU queue-idle channel reports generations, replay gaps, and cancellation", async () => {
  const channel = createGpuQueueIdleChannel(2);
  const first = channel.observe(false, 0);
  channel.publish(3);
  assert.deepEqual(await first, { generation: 3, sequence: 1, missed: 0 });

  channel.publish(3);
  channel.publish(4);
  assert.deepEqual(await channel.observe(true, 0), {
    generation: 3, sequence: 2, missed: 1,
  });
  assert.deepEqual(await channel.observe(false, 0), {
    generation: 4, sequence: 3, missed: 0,
  });

  const controller = new AbortController();
  const pending = channel.observe(true, 3, controller.signal);
  controller.abort();
  await assert.rejects(pending, error => error.name === "AbortError");
});

test("typed recovery transports raw device facts and validates Fe recovery decisions", () => {
  const surface = Object.create(FeSurfaceElement.prototype);
  surface._runWasmArenaEpoch = call => call();
  const calls = [];
  surface._surfaceRecoveryKernel = (...facts) => {
    calls.push(facts);
    return SurfaceRecoveryAction.RetryDevice;
  };
  assert.equal(
    surface._runSurfaceRecovery(
      GpuDeviceEventKind.Lost,
      GpuDeviceLossReason.Unknown,
      true,
      true,
      7,
    ),
    SurfaceRecoveryAction.RetryDevice,
  );
  assert.deepEqual(calls, [[
    GpuDeviceEventKind.Lost,
    GpuDeviceLossReason.Unknown,
    1,
    1,
    7,
  ]]);

  surface._surfaceRecoveryKernel = () => 4;
  assert.throws(
    () => surface._runSurfaceRecovery(
      GpuDeviceEventKind.Lost,
      GpuDeviceLossReason.Unknown,
      true,
      true,
      7,
    ),
    /invalid action/,
  );
});

test("shared recovery aggregates Fe retry demand and has no host retry budget", async () => {
  const trace = [];
  const fake = (name, actions) => ({
    _beginDeviceLoss(reason, generation) {
      trace.push(`${name}:lost:${reason}:${generation}`);
      return actions.shift();
    },
    _continueDeviceRecovery(reason, generation) {
      trace.push(`${name}:unavailable:${reason}:${generation}`);
      return actions.shift();
    },
    async _realizeDeviceRecovery(action) {
      trace.push(`${name}:realize:${action}`);
    },
    async _completeDeviceRecovery(gpu) {
      trace.push(`${name}:recovered:${gpu.generation}`);
    },
    _fail(error) {
      trace.push(`${name}:error:${error.message}`);
    },
  });
  const first = fake("first", [
    SurfaceRecoveryAction.RetryDevice,
    SurfaceRecoveryAction.RetryDevice,
    SurfaceRecoveryAction.DegradeToWasm,
  ]);
  const second = fake("second", [
    SurfaceRecoveryAction.RetryDevice,
    SurfaceRecoveryAction.FailSurface,
  ]);
  const idle = fake("idle", [SurfaceRecoveryAction.NoAction]);
  let acquisitions = 0;
  const result = await coordinateSurfaceRecovery(
    [first, second, idle],
    GpuDeviceLossReason.Unknown,
    4,
    async () => {
      acquisitions += 1;
      trace.push(`acquire:${acquisitions}`);
      return null;
    },
  );
  assert.equal(result, null);
  assert.equal(acquisitions, 2, "two Fe-selected rounds, not one request per surface");
  assert.deepEqual(trace, [
    "first:lost:1:4",
    "second:lost:1:4",
    "idle:lost:1:4",
    "idle:realize:0",
    "acquire:1",
    "first:unavailable:1:4",
    "second:unavailable:1:4",
    "second:realize:3",
    "acquire:2",
    "first:unavailable:1:4",
    "first:realize:2",
  ]);

  trace.length = 0;
  const left = fake("left", [SurfaceRecoveryAction.RetryDevice]);
  const right = fake("right", [SurfaceRecoveryAction.RetryDevice]);
  const waitingPoster = fake("poster", [SurfaceRecoveryAction.NoAction]);
  acquisitions = 0;
  const fresh = { generation: 5 };
  assert.equal(
    await coordinateSurfaceRecovery(
      [left, right, waitingPoster],
      GpuDeviceLossReason.Unknown,
      4,
      async () => {
        acquisitions += 1;
        return fresh;
      },
    ),
    fresh,
  );
  assert.equal(acquisitions, 1);
  assert.deepEqual(trace, [
    "left:lost:1:4",
    "right:lost:1:4",
    "poster:lost:1:4",
    "poster:realize:0",
    "left:recovered:5",
    "right:recovered:5",
    "poster:recovered:5",
  ]);
});

test("shared device loss does not fail or recover a surface already on its Fe-selected Wasm fallback", async () => {
  const surface = Object.create(FeSurfaceElement.prototype);
  surface._fsm = "live";
  surface._mode = "wasm-2d";
  surface._posterRecoveryActive = false;
  surface._recoveryObservedLoss = false;
  let lifecycle = 0;
  surface._deliverSurfaceLifecycle = () => { lifecycle += 1; };

  const action = surface._beginDeviceLoss(GpuDeviceLossReason.Unknown, 4);
  assert.equal(action, SurfaceRecoveryAction.NoAction);
  await surface._realizeDeviceRecovery(action, GpuDeviceLossReason.Unknown, 4);
  await surface._completeDeviceRecovery({ generation: 5 }, 4);

  assert.equal(surface._mode, "wasm-2d");
  assert.equal(surface._fsm, "live");
  assert.equal(lifecycle, 0);
});

test("ordinary poster failures preserve the original error outside device recovery", async () => {
  const surface = Object.create(FeSurfaceElement.prototype);
  surface._gpuOverride = undefined;
  surface._posterRecoveryActive = false;
  const original = new Error("ordinary pass-graph failure");
  surface._renderPoster = async () => { throw original; };

  await assert.rejects(surface._renderPosterWithRecovery(), error => error === original);
  assert.equal(surface._posterRecoveryActive, false);
});

test("compute-only pass graphs defer work until live and require no canvas", async () => {
  const surface = Object.create(FeSurfaceElement.prototype);
  surface._graph = true;
  surface._hasRenderPass = false;
  let posters = 0;
  surface._renderPosterWithRecovery = async () => { posters += 1; };
  await surface._prepareReadyFrame();
  assert.equal(posters, 0);

  const gpu = { device: {} };
  surface._fsm = "ready";
  surface._mode = null;
  surface._uniforms = [3, 5, 8];
  surface._deliverSurfaceLifecycle = () => {};
  surface._ensurePipeline = async () => gpu;
  const presentations = [];
  surface._presentOn = async (context, uniforms) => {
    presentations.push([context, uniforms]);
  };
  let entered = 0;
  surface._enterLive = () => { entered += 1; };

  await surface._goLive();

  assert.equal(surface._mode, "webgpu");
  assert.deepEqual(presentations, [[null, [3, 5, 8]]]);
  assert.equal(entered, 1);
});

test("a backend-pending surface does not claim to be a Wasm renderer", () => {
  const surface = Object.create(FeSurfaceElement.prototype);
  surface._mode = null;
  surface._manifest = { provenance: {} };
  surface._badge = { textContent: "", title: "", className: "" };

  surface._updateBadge();

  assert.equal(surface._badge.textContent, "Fe renderer · fixed JS host");
  assert.equal(surface._badge.className, "badge ready");
});

test("legacy CPU backing ceiling preserves aspect instead of cropping work", () => {
  assert.deepEqual(fitBackingExtent(512, 256, 128), { width: 128, height: 64 });
  assert.deepEqual(fitBackingExtent(96, 64, 128), { width: 96, height: 64 });
  assert.deepEqual(fitBackingExtent(512, 256), { width: 512, height: 256 });
});

test("protocol v9 realizes explicit Fe param plans without inferring from kind", () => {
  const param = {
    kind: "deliberately_misleading",
    visible: true,
    source: "surface_width",
    presentation: { widget: "range", scale: "logarithmic", readout: "integer", options: [] },
  };
  assert.deepEqual(surfaceParamPlan(param, 9), {
    source: "surface_width",
    widget: "range",
    scale: "logarithmic",
    readout: "integer",
    options: [],
  });
  assert.throws(
    () => surfaceParamPlan({ kind: "range", visible: true }, 9),
    /missing a supported Fe presentation plan/,
  );
  assert.deepEqual(
    surfaceParamPlan({ kind: "extent_y", visible: false }, 8),
    { source: "surface_height", widget: "hidden", scale: "linear", readout: "scalar" },
    "the kind-derived path is isolated to legacy protocols",
  );
});

test("protocol v9 preserves Fe-authored names for scalar select ordinals", () => {
  const plan = surfaceParamPlan({
    kind: "int",
    visible: true,
    source: "initial",
    min: 0,
    max: 3,
    presentation: {
      widget: "select",
      scale: "linear",
      readout: "integer",
      options: ["regular grid", "atlas charts", "eight-chart fan", "pullback blue noise"],
    },
  }, 9);
  assert.deepEqual(plan.options, [
    "regular grid", "atlas charts", "eight-chart fan", "pullback blue noise",
  ]);
  assert.throws(
    () => surfaceParamPlan({
      kind: "int",
      visible: true,
      source: "initial",
      min: 0,
      max: 2,
      presentation: { widget: "select", scale: "linear", readout: "integer", options: ["a", "b"] },
    }, 9),
    /options disagree with Fe ordinal bounds/,
  );
});

test("fixed host supplies raw capability facts and realizes Fe backing extent exactly", () => {
  const oldDpr = globalThis.devicePixelRatio;
  const oldMatchMedia = globalThis.matchMedia;
  try {
    globalThis.devicePixelRatio = 2;
    globalThis.matchMedia = query => ({ matches: query === "(pointer: coarse)" });
    const surface = Object.create(FeSurfaceElement.prototype);
    surface._surface = { extent: { width: 512, height: 256 } };
    surface._adoptedCanvas = null;
    surface._stage = {
      style: {},
      getBoundingClientRect() { return { width: 200, height: 100 }; },
    };
    surface._runWasmArenaEpoch = call => call();
    let received;
    surface._surfaceQualityKernel = (...facts) => {
      received = facts;
      // Deliberately larger than the deleted host coarse-pointer ceiling.
      return [400, 200];
    };
    const extent = surface._computeBackingExtent({
      device: { limits: { maxTextureDimension2D: 4096 } },
    });
    assert.deepEqual(received, [200, 100, 2, 512, 256, 4096, 1, 1]);
    assert.deepEqual(extent, { width: 400, height: 200 });
  } finally {
    globalThis.devicePixelRatio = oldDpr;
    globalThis.matchMedia = oldMatchMedia;
  }
});

test("fixed host rejects malformed Fe backing decisions without choosing a replacement", () => {
  const oldDpr = globalThis.devicePixelRatio;
  const oldMatchMedia = globalThis.matchMedia;
  try {
    globalThis.devicePixelRatio = 1;
    globalThis.matchMedia = () => ({ matches: false });
    const surface = Object.create(FeSurfaceElement.prototype);
    surface._surface = { extent: { width: 512, height: 256 } };
    surface._adoptedCanvas = null;
    surface._stage = {
      style: {},
      getBoundingClientRect() { return { width: 512, height: 256 }; },
    };
    surface._runWasmArenaEpoch = call => call();
    const gpu = { device: { limits: { maxTextureDimension2D: 384 } } };
    for (const invalid of [[0, 64], [128.5, 64], [385, 64], [128], [NaN, 64]]) {
      surface._surfaceQualityKernel = () => invalid;
      assert.throws(
        () => surface._computeBackingExtent(gpu),
        /Fe surface quality policy returned an invalid backing extent/,
      );
    }
  } finally {
    globalThis.devicePixelRatio = oldDpr;
    globalThis.matchMedia = oldMatchMedia;
  }
});

test("live resize re-runs the Fe policy against current GPU facts and realizes its extent", async () => {
  const gpu = { device: { limits: { maxTextureDimension2D: 2048 } } };
  const surface = Object.create(FeSurfaceElement.prototype);
  surface._fsm = "live";
  surface._mode = "webgpu";
  surface._gpu = gpu;
  surface._resizePending = false;
  surface._backingWidth = 400;
  surface._backingHeight = 200;
  surface._uniforms = [400, 200, 9];
  surface._manifest = { protocol_version: 9 };
  surface._members = [
    { name: "width" },
    { name: "height" },
    { name: "scene" },
  ];
  surface._surface = { params: [
    {
      name: "width", kind: "misleading", visible: false, source: "surface_width",
      presentation: { widget: "hidden", scale: "linear", readout: "scalar" },
    },
    {
      name: "height", kind: "misleading", visible: false, source: "surface_height",
      presentation: { widget: "hidden", scale: "linear", readout: "scalar" },
    },
    {
      name: "scene", kind: "misleading", visible: false, source: "initial",
      presentation: { widget: "hidden", scale: "linear", readout: "scalar" },
    },
  ] };
  surface._adoptedCanvas = null;
  surface._liveCanvas = { width: 400, height: 200 };
  const capabilities = [];
  surface._computeBackingExtent = currentGpu => {
    capabilities.push(currentGpu);
    return { width: 320, height: 160 };
  };
  const replacements = [];
  surface._replaceSurfaceState = next => {
    replacements.push(next);
    surface._uniforms = next;
  };
  const filters = [];
  surface._applyExtentAndFilter = (...extent) => filters.push(extent);
  let renders = 0;
  surface._render = () => { renders += 1; };

  await surface._refreshLiveBackingExtent();

  assert.deepEqual(capabilities, [gpu]);
  assert.deepEqual(replacements, [[320, 160, 9]]);
  assert.deepEqual(filters, [[320, 160]]);
  assert.deepEqual([surface._liveCanvas.width, surface._liveCanvas.height], [320, 160]);
  assert.equal(renders, 1);
  assert.equal(surface._resizePending, false);
});

test("device fallback re-runs Fe quality with CPU capability before rendering", async () => {
  const surface = Object.create(FeSurfaceElement.prototype);
  surface._fsm = "live";
  surface._mode = "webgpu";
  surface._recoveryWasLive = true;
  surface._gpu = { device: {} };
  surface._liveContext = null;
  surface._adoptedContext = null;
  surface._adoptedCanvas = null;
  surface._kernel = () => {};
  surface._uniforms = [1];
  surface._posterCanvas = { hidden: true, width: 0, height: 0 };
  surface._liveCanvas = { hidden: false, width: 400, height: 200 };
  surface._deliverSurfaceLifecycle = () => {};
  surface._dispatch = () => {};
  surface._updateBadge = () => {};
  const capabilities = [];
  surface._applyBackingExtent = gpu => {
    capabilities.push(gpu);
    surface._backingWidth = 128;
    surface._backingHeight = 64;
    return true;
  };
  const renders = [];
  surface._renderWasmInto = (_canvas, width, height) => renders.push([width, height]);

  await surface._realizeDeviceRecovery(
    SurfaceRecoveryAction.DegradeToWasm,
    GpuDeviceLossReason.Unknown,
    1,
  );

  assert.deepEqual(capabilities, [null]);
  assert.deepEqual(renders, [[128, 64]]);
  assert.deepEqual([surface._posterCanvas.width, surface._posterCanvas.height], [128, 64]);
  assert.equal(surface._mode, "wasm-2d");
  assert.equal(surface._liveCanvas.hidden, true);
  assert.equal(surface._posterCanvas.hidden, false);
});

test("adopted-canvas recovery reconfigures the fresh GPU before Fe quality and presentation", async () => {
  const freshGpu = { device: { marker: "fresh" }, format: "rgba8unorm" };
  const configurations = [];
  const context = { configure(options) { configurations.push(options); } };
  const surface = Object.create(FeSurfaceElement.prototype);
  surface._fsm = "ready";
  surface._mode = "webgpu";
  surface._adoptedCanvas = {
    width: 400,
    height: 200,
    getContext(kind) {
      assert.equal(kind, "webgpu");
      return context;
    },
  };
  surface._adoptedContext = null;
  surface._deliverSurfaceLifecycle = () => {};
  surface._ensurePipeline = async () => freshGpu;
  const capabilities = [];
  surface._applyBackingExtent = gpu => {
    capabilities.push(gpu);
    surface._backingWidth = 300;
    surface._backingHeight = 150;
    return true;
  };
  const presentations = [];
  surface._presentOn = (configuredContext, uniforms) => {
    presentations.push([configuredContext, uniforms]);
  };
  surface._uniforms = [7];
  let entered = 0;
  surface._enterLive = () => { entered += 1; };

  await surface._goLive();

  assert.deepEqual(configurations, [{
    device: freshGpu.device,
    format: freshGpu.format,
    alphaMode: "opaque",
  }]);
  assert.deepEqual(capabilities, [freshGpu]);
  assert.deepEqual(presentations, [[context, [7]]]);
  assert.deepEqual([surface._adoptedCanvas.width, surface._adoptedCanvas.height], [300, 150]);
  assert.equal(entered, 1);
});

test("poster readback removes row padding and normalizes canvas channel order", () => {
  const rgbaSource = new Uint8Array(256);
  rgbaSource.set([1, 2, 3, 255, 4, 5, 6, 254]);
  assert.deepEqual(
    [...unpackCanvasReadback(rgbaSource, 2, 1, 256, "rgba8unorm")],
    [1, 2, 3, 255, 4, 5, 6, 254],
  );

  const bgraSource = new Uint8Array(512);
  bgraSource.set([30, 20, 10, 255], 0);
  bgraSource.set([60, 50, 40, 253], 256);
  assert.deepEqual(
    [...unpackCanvasReadback(bgraSource, 1, 2, 256, "bgra8unorm")],
    [10, 20, 30, 255, 40, 50, 60, 253],
  );
});

test("mapped WebGPU bytes become an owned snapshot before unmap", async () => {
  globalThis.GPUMapMode = { READ: 1 };
  const trace = [];
  const mapped = new Uint8Array([17, 19, 23, 29]);
  const buffer = {
    async mapAsync(mode) { trace.push(["map", mode]); },
    getMappedRange(offset, length) {
      trace.push(["range", offset, length]);
      return mapped.buffer;
    },
    unmap() {
      trace.push(["unmap"]);
      mapped.fill(0);
    },
    destroy() { trace.push(["destroy"]); },
  };
  const snapshot = await readGpuBufferSnapshot({ buffer, byteLength: 4 });
  assert.deepEqual([...snapshot], [17, 19, 23, 29]);
  assert.deepEqual(trace, [
    ["map", GPUMapMode.READ],
    ["range", 0, 4],
    ["unmap"],
    ["destroy"],
  ]);
});

test("poster copy is encoded after rendering in the same GPU submission", async () => {
  globalThis.GPUBufferUsage = { COPY_DST: 1, MAP_READ: 2 };
  const trace = [];
  const buffer = {};
  const encoder = {
    beginRenderPass() {
      trace.push("render-begin");
      return {
        setPipeline() {},
        draw() {},
        end() { trace.push("render-end"); },
      };
    },
    copyTextureToBuffer() { trace.push("copy"); },
    finish() { trace.push("finish"); return {}; },
  };
  const device = {
    createCommandEncoder() { return encoder; },
    createBuffer() { return buffer; },
    queue: { submit() { trace.push("submit"); } },
  };
  const context = {
    getCurrentTexture() {
      return { createView() { return {}; } };
    },
  };
  const surface = Object.create(FeSurfaceElement.prototype);
  surface._graph = false;
  surface._gpu = { device, pipeline: {}, bindGroup: null, uniformBuffer: null };

  const readback = await surface._presentOn(
    context,
    [],
    { width: 1, height: 1, format: "rgba8unorm" },
  );
  assert.equal(readback.buffer, buffer);
  assert.deepEqual(trace, ["render-begin", "render-end", "copy", "finish", "submit"]);
});

test("poster-only surfaces destroy all retained GPU buffers exactly once", () => {
  const surface = Object.create(FeSurfaceElement.prototype);
  const trace = [];
  const shared = { destroy: () => trace.push("shared") };
  const output = { destroy: () => trace.push("output") };
  surface._gpu = {
    resourceBuffers: new Map([
      ["input", shared],
      ["alias", shared],
      ["output", output],
    ]),
  };
  surface._releaseGpuResources();
  assert.equal(surface._gpu, null);
  assert.deepEqual(trace, ["shared", "output"]);
  surface._releaseGpuResources();
  assert.deepEqual(trace, ["shared", "output"]);

  surface._gpu = { uniformBuffer: { destroy: () => trace.push("uniform") } };
  surface._releaseGpuResources();
  assert.deepEqual(trace, ["shared", "output", "uniform"]);

  const resource = { destroy: () => trace.push("owned-resource") };
  const passInput = { destroy: () => trace.push("owned-input") };
  const passOutput = { destroy: () => trace.push("owned-output") };
  surface._gpu = {
    resourceBuffers: new Map([["resource", resource]]),
    ownedBuffers: new Set([resource, passInput, passOutput]),
  };
  surface._releaseGpuResources();
  assert.deepEqual(trace, [
    "shared",
    "output",
    "uniform",
    "owned-resource",
    "owned-input",
    "owned-output",
  ]);
});

test("failed lazy pipeline realization releases resources and pass-local buffers", async () => {
  const oldBufferUsage = globalThis.GPUBufferUsage;
  const oldShaderStage = globalThis.GPUShaderStage;
  try {
    globalThis.GPUBufferUsage = { STORAGE: 0x80, COPY_SRC: 0x04, COPY_DST: 0x08 };
    globalThis.GPUShaderStage = { COMPUTE: 0x04, VERTEX: 0x01, FRAGMENT: 0x02 };
    const descriptors = [];
    const destroyed = [];
    const device = {
      createBuffer(descriptor) {
        const index = descriptors.length;
        descriptors.push(descriptor);
        return { destroy() { destroyed.push(index); } };
      },
      createShaderModule() { return {}; },
      createBindGroupLayout() { return {}; },
      createBindGroup() { return {}; },
      createPipelineLayout() { return {}; },
      async createComputePipelineAsync() { throw new Error("deliberate pipeline failure"); },
      queue: { writeBuffer() {} },
    };
    const surface = Object.create(FeSurfaceElement.prototype);
    surface._layout = { color_target_format: "rgba8unorm" };
    surface._surface = null;
    surface._manifest = { protocol_version: 8 };
    surface._manifestUrl = new URL("https://example.test/manifest.json");
    surface._passShaderUrls = [new URL("data:text/plain,@compute @workgroup_size(1) fn main() {}")];
    surface._resources = [{
      group: 0,
      binding: 0,
      name: "state",
      stride: 4,
      length: 1,
      buffer_usage: ["storage"],
    }];
    surface._passes = [{
      shader_stages: ["compute"],
      layout: {
        mode: "compute",
        entry_point: "main",
        bindings: [
          { group: 0, binding: 0, name: "state", role: "resource", access: "read_write" },
          { group: 0, binding: 1, name: "input", role: "input", access: "read", span: 4 },
          { group: 0, binding: 2, name: "output", role: "output", access: "read_write", span: 4 },
        ],
      },
    }];

    const graph = await surface._buildPassGraph(device, 1);
    await assert.rejects(
      realizePassPipeline(device, graph.passRecords[0]),
      /deliberate pipeline failure/,
    );
    surface._gpu = graph;
    surface._releaseGpuResources();
    assert.deepEqual(
      descriptors.map(descriptor => descriptor.usage),
      [0x80, 0x88, 0x80],
      "resource, host input, and GPU-only output receive only required usage bits",
    );
    assert.deepEqual(destroyed, [0, 1, 2]);
  } finally {
    globalThis.GPUBufferUsage = oldBufferUsage;
    globalThis.GPUShaderStage = oldShaderStage;
  }
});

test("mobile pointer capture is single-owner and restores native touch scrolling", () => {
  const listeners = new Map();
  const captures = [];
  const canvas = {
    style: { touchAction: "pan-y" },
    addEventListener(name, callback) { listeners.set(name, callback); },
    removeEventListener(name) { listeners.delete(name); },
    getBoundingClientRect() { return { left: 0, top: 0, width: 100, height: 100 }; },
    setPointerCapture(pointer) { captures.push(["set", pointer]); },
    releasePointerCapture(pointer) { captures.push(["release", pointer]); },
  };
  const surface = Object.create(FeSurfaceElement.prototype);
  surface._surfaceTransitionKernel = () => {};
  surface._control = null;
  surface._controlKernel = null;
  surface._adoptedCanvas = null;
  surface._mode = "webgpu";
  surface._liveCanvas = canvas;
  surface._posterCanvas = null;
  surface._gestureListeners = null;
  surface._backingWidth = 100;
  surface._backingHeight = 100;
  const delivered = [];
  surface._applyGesture = event => delivered.push(event);

  surface._wireGestures();
  assert.equal(canvas.style.touchAction, "none");
  assert.equal(listeners.has("lostpointercapture"), true);
  listeners.get("pointermove")({
    pointerId: 7, isPrimary: true, clientX: 5, clientY: 6, buttons: 0, timeStamp: 0,
    preventDefault() { throw new Error("default hover must not be consumed"); },
  });
  assert.deepEqual(delivered, [], "captured drag remains the default input policy");
  listeners.get("pointerdown")({
    button: 0, pointerId: 7, clientX: 10, clientY: 20, buttons: 1,
    timeStamp: 1, preventDefault() {},
  });
  listeners.get("pointerdown")({
    button: 0, pointerId: 8, clientX: 30, clientY: 40, buttons: 1,
    timeStamp: 2, preventDefault() {},
  });
  let movePrevented = false;
  listeners.get("pointermove")({
    pointerId: 7, clientX: 15, clientY: 27, buttons: 1, timeStamp: 3,
    preventDefault() { movePrevented = true; },
  });
  listeners.get("pointercancel")({
    pointerId: 7, clientX: 15, clientY: 27, buttons: 0, timeStamp: 4,
  });
  assert.equal(movePrevented, true);
  assert.deepEqual(captures, [["set", 7], ["release", 7]]);
  assert.deepEqual(
    delivered.map(event => event.eventKind),
    [SurfaceEventKind.PointerDown, SurfaceEventKind.PointerMove, SurfaceEventKind.PointerUp],
  );

  surface._unwireGestures();
  assert.equal(canvas.style.touchAction, "pan-y");
  assert.equal(listeners.size, 0);
});

test("Fe-selected surface pointer motion delivers hover without weakening captured drag", () => {
  const listeners = new Map();
  const captures = [];
  const canvas = {
    style: { touchAction: "" },
    addEventListener(name, callback) { listeners.set(name, callback); },
    removeEventListener(name) { listeners.delete(name); },
    getBoundingClientRect() { return { left: 10, top: 20, width: 100, height: 50 }; },
    setPointerCapture(pointer) { captures.push(["set", pointer]); },
    releasePointerCapture(pointer) { captures.push(["release", pointer]); },
  };
  const surface = Object.create(FeSurfaceElement.prototype);
  surface._surfaceTransitionKernel = () => {};
  surface._control = null;
  surface._controlKernel = null;
  surface._surface = { pointer_motion: "hover_and_captured_drag" };
  surface._adoptedCanvas = null;
  surface._mode = "webgpu";
  surface._liveCanvas = canvas;
  surface._posterCanvas = null;
  surface._gestureListeners = null;
  surface._backingWidth = 200;
  surface._backingHeight = 200;
  const delivered = [];
  surface._applyGesture = event => delivered.push(event);

  surface._wireGestures();
  let hoverPrevented = false;
  listeners.get("pointermove")({
    pointerId: 4, isPrimary: true, clientX: 35, clientY: 30, buttons: 0, timeStamp: 1,
    preventDefault() { hoverPrevented = true; },
  });
  assert.equal(hoverPrevented, false, "raw hover must preserve native pointer behavior");
  assert.deepEqual(delivered[0], {
    dx: 0, dy: 0, wheelDelta: 0, wheelMode: 0,
    mx: 50, my: 40, buttons: 0, timestamp: 1,
    eventKind: SurfaceEventKind.PointerMove,
  });
  assert.deepEqual(captures, [], "hover must not manufacture pointer capture");

  listeners.get("pointerdown")({
    button: 0, pointerId: 4, isPrimary: true, clientX: 35, clientY: 30,
    buttons: 1, timeStamp: 2, preventDefault() {},
  });
  listeners.get("pointermove")({
    pointerId: 4, isPrimary: true, clientX: 45, clientY: 35,
    buttons: 1, timeStamp: 3, preventDefault() {},
  });
  listeners.get("pointerup")({
    pointerId: 4, isPrimary: true, clientX: 45, clientY: 35,
    buttons: 0, timeStamp: 4,
  });
  assert.deepEqual(captures, [["set", 4], ["release", 4]]);
  assert.deepEqual(
    delivered.slice(1).map(event => [event.eventKind, event.dx, event.dy]),
    [
      [SurfaceEventKind.PointerDown, 0, 0],
      [SurfaceEventKind.PointerMove, 20, 20],
      [SurfaceEventKind.PointerUp, 0, 0],
    ],
  );
});

test("fixed host consumes the Fe-derived authored-raster draw shape", () => {
  assert.deepEqual(rasterDrawShape({ draw_vertices: 7, draw_instances: 11 }), {
    vertices: 7,
    instances: 11,
  });
  assert.deepEqual(
    rasterDrawShape({}),
    { vertices: 3, instances: 1 },
    "legacy fullscreen render remains one three-vertex instance",
  );
  assert.throws(
    () => rasterDrawShape({ draw_vertices: -1 }),
    /invalid compiler-derived raster vertex count/,
  );
  assert.deepEqual(rasterDrawShape({draw_vertices: 0, draw_instances: 0}), {vertices: 0, instances: 0});
  assert.throws(() => rasterDrawShape({draw_vertices: 0x100000000}), /invalid compiler-derived raster vertex count/);
  assert.throws(
    () => rasterDrawShape({ draw_vertices: 3, draw_instances: -1 }),
    /invalid compiler-derived raster instance count/,
  );
  assert.throws(
    () => rasterDrawShape({ draw_instances: 2 }),
    /instances require an authored raster draw/,
  );
});

test("one authored raster pass takes the GPU pass-graph path", () => {
  assert.equal(
    requiresGpuPassGraph([{ draw_vertices: 13824, layout: { mode: "render" } }]),
    true,
  );
  assert.equal(
    requiresGpuPassGraph([{ layout: { mode: "render" } }]),
    false,
    "legacy fullscreen rendering keeps its established path",
  );
});

test("selected pass graphs fetch and realize pipelines sequentially through async WebGPU", async () => {
  globalThis.GPUShaderStage = { COMPUTE: 1, VERTEX: 2, FRAGMENT: 4 };
  const previousFetch = globalThis.fetch;
  const trace = [];
  let activeFetches = 0;
  let activePipelines = 0;
  let maximumFetches = 0;
  let maximumPipelines = 0;
  globalThis.fetch = async url => {
    activeFetches += 1;
    maximumFetches = Math.max(maximumFetches, activeFetches);
    trace.push(["fetch", String(url)]);
    await Promise.resolve();
    activeFetches -= 1;
    return {
      ok: true,
      status: 200,
      async text() { return `// ${String(url)}`; },
    };
  };
  const realize = async (kind, descriptor) => {
    activePipelines += 1;
    maximumPipelines = Math.max(maximumPipelines, activePipelines);
    trace.push(["pipeline", kind, descriptor.compute?.entryPoint
      ?? descriptor.fragment?.entryPoint]);
    await Promise.resolve();
    activePipelines -= 1;
    return { kind };
  };
  const device = {
    createShaderModule({ code }) {
      trace.push(["module", code]);
      return { code };
    },
    createComputePipeline() {
      assert.fail("pass graphs must not synchronously create compute pipelines");
    },
    createRenderPipeline() {
      assert.fail("pass graphs must not synchronously create render pipelines");
    },
    createComputePipelineAsync(descriptor) {
      return realize("compute", descriptor);
    },
    createRenderPipelineAsync(descriptor) {
      return realize("render", descriptor);
    },
  };
  const surface = Object.create(FeSurfaceElement.prototype);
  surface._layout = { color_target_format: "rgba8unorm" };
  surface._manifestUrl = new URL("https://example.test/proof/manifest.json");
  surface._manifest = { protocol_version: 7 };
  surface._passShaderUrls = [
    new URL("https://example.test/proof/first.wgsl"),
    new URL("https://example.test/proof/second.wgsl"),
  ];
  surface._resources = [];
  surface._passes = [
    {
      layout: {
        mode: "compute",
        bindings: [],
        entry_point: "first",
      },
    },
    {
      draw_vertices: 3,
      layout: {
        mode: "render",
        bindings: [],
        vertex_entry: "vertex",
        fragment_entry: "second",
      },
    },
  ];

  try {
    const graph = await surface._buildPassGraph(device, 7);
    assert.equal(graph.generation, 7);
    assert.deepEqual(trace, [], "layout construction must not fetch inactive pass shaders");
    for (const record of graph.passRecords) {
      await realizePassPipeline(device, record);
    }
    assert.deepEqual(graph.passRecords.map(record => record.pipeline.kind), [
      "compute",
      "render",
    ]);
  } finally {
    globalThis.fetch = previousFetch;
  }

  assert.equal(maximumFetches, 1, "shader source fetches must remain bounded");
  assert.equal(maximumPipelines, 1, "pipeline creation must remain bounded");
  assert.deepEqual(trace.map(event => event.slice(0, 2)), [
    ["fetch", "https://example.test/proof/first.wgsl"],
    ["module", "// https://example.test/proof/first.wgsl"],
    ["pipeline", "compute"],
    ["fetch", "https://example.test/proof/second.wgsl"],
    ["module", "// https://example.test/proof/second.wgsl"],
    ["pipeline", "render"],
  ]);
});

test("ordered render passes clear once and preserve earlier Fe-authored color", async () => {
  const loadOps = [];
  const draws = [];
  const encoder = {
    beginRenderPass(descriptor) {
      loadOps.push(descriptor.colorAttachments[0].loadOp);
      return {
        setPipeline() {},
        setBindGroup() {},
        draw(vertices, instances) { draws.push([vertices, instances]); },
        end() {},
      };
    },
    finish() { return {}; },
  };
  const device = {
    createCommandEncoder() { return encoder; },
    queue: { submit() {} },
  };
  const context = {
    getCurrentTexture() {
      return { createView() { return {}; } };
    },
  };
  const surface = Object.create(FeSurfaceElement.prototype);
  surface._graph = true;
  surface._gpu = {
    device,
    raster: {
      sampleCount: 1,
      cullMode: "none",
      color: {
        clearValue: { r: 0, g: 0, b: 0, a: 1 },
        firstLoad: "clear",
        followingLoad: "load",
        store: "store",
      },
      depth: null,
    },
    passRecords: [
      { pass: { layout: { mode: "render" } }, pipeline: {}, bindGroup: null, inputs: [] },
      { pass: { layout: { mode: "render" }, draw_vertices: 54 }, pipeline: {}, bindGroup: null, inputs: [] },
    ],
  };

  await surface._presentOn(context, []);
  assert.deepEqual(loadOps, ["clear", "load"]);
  assert.deepEqual(draws, [[3, 1], [54, 1]]);
});

test("typed actor readback follows all Fe GPU passes in the same submission", async () => {
  globalThis.GPUBufferUsage = { COPY_DST: 1, MAP_READ: 2 };
  const trace = [];
  const source = { name: "source" };
  const staging = { name: "staging" };
  const device = {
    createBuffer(descriptor) {
      trace.push(["create-staging", descriptor.size, descriptor.usage]);
      return staging;
    },
    createCommandEncoder() {
      return {
        beginComputePass() {
          trace.push(["compute"]);
          return {
            setPipeline() {},
            setBindGroup() {},
            dispatchWorkgroups() { trace.push(["dispatch"]); },
            end() { trace.push(["compute-end"]); },
          };
        },
        copyBufferToBuffer(from, fromOffset, to, toOffset, length) {
          trace.push(["copy", from, fromOffset, to, toOffset, length]);
        },
        finish() { trace.push(["finish"]); return {}; },
      };
    },
    queue: { submit() { trace.push(["submit"]); } },
  };
  const surface = Object.create(FeSurfaceElement.prototype);
  surface._graph = true;
  surface._memberIndexByName = new Map();
  surface._gpuReadbackResource = { name: "output", byteLength: 4 };
  surface._deliverGpuReadback = async readback => {
    trace.push(["deliver", readback.buffer, readback.byteLength]);
  };
  surface._gpu = {
    device,
    passRecords: [{
      pass: { layout: { mode: "compute" }, dispatch: [1, 1, 1], repeat: 1 },
      pipeline: {},
      bindGroup: null,
      inputs: [],
    }],
    resourceBuffers: new Map([["output", source]]),
  };

  await surface._presentOn({}, []);

  assert.deepEqual(trace, [
    ["compute"],
    ["dispatch"],
    ["compute-end"],
    ["create-staging", 4, GPUBufferUsage.COPY_DST | GPUBufferUsage.MAP_READ],
    ["copy", source, 0, staging, 0, 4],
    ["finish"],
    ["submit"],
    ["deliver", staging, 4],
  ]);
});

test("typed actor readback transfers exact opaque bytes into resident Fe state", async () => {
  globalThis.GPUMapMode = { READ: 1 };
  const memory = new WebAssembly.Memory({ initial: 1 });
  const mapped = new Uint8Array([17, 19, 23, 29]);
  const resets = [];
  const calls = [];
  const surface = Object.create(FeSurfaceElement.prototype);
  surface._members = [{ name: "accepted" }];
  surface._resources = [{ name: "receipt" }];
  surface._uniforms = [7];
  surface._surfaceTransitionMemory = memory;
  surface._surfaceTransitionAlloc = (length, alignment) => {
    calls.push(["alloc", length, alignment]);
    return 32;
  };
  surface._wasmArenaReset = () => resets.push("reset");
  surface._gpuReadbackKernel = (pointer, length, resource) => {
    calls.push([
      "transition",
      pointer,
      length,
      resource,
      [...new Uint8Array(memory.buffer, pointer, length)],
    ]);
    return 23;
  };
  surface._refreshControlValues = () => calls.push(["refresh"]);

  const buffer = {
    async mapAsync() {},
    getMappedRange() { return mapped.buffer; },
    unmap() { mapped.fill(0); },
    destroy() {},
  };
  const next = await surface._deliverGpuReadback({ buffer, byteLength: 4 });

  assert.deepEqual(next, [23]);
  assert.deepEqual(surface._uniforms, [23]);
  assert.deepEqual(resets, ["reset", "reset"]);
  assert.deepEqual(calls, [
    ["alloc", 4, 4],
    ["transition", 32, 4, 0n, [17, 19, 23, 29]],
    ["refresh"],
  ]);
});

test("Fe-derived cooperative dispatch batches preserve stage order and await queue idle", async () => {
  const submissions = [];
  let queueIdle = 0;
  const device = {
    createCommandEncoder() {
      const dispatches = [];
      let pipeline = null;
      return {
        beginComputePass() {
          return {
            setPipeline(next) { pipeline = next; },
            setBindGroup() {},
            dispatchWorkgroups(x, y, z) { dispatches.push([pipeline.name, x, y, z]); },
            end() {},
          };
        },
        finish() { return { dispatches }; },
      };
    },
    queue: {
      submit(commands) { submissions.push(commands[0].dispatches); },
      async onSubmittedWorkDone() { queueIdle += 1; },
    },
  };
  const compute = (name, repeat, cooperation = undefined) => ({
    pass: {
      layout: { mode: "compute" },
      dispatch: [3, 1, 1],
      repeat,
      cooperation,
    },
    pipeline: { name },
    bindGroup: null,
    inputs: [],
  });
  const surface = Object.create(FeSurfaceElement.prototype);
  surface._graph = true;
  surface._memberIndexByName = new Map();
  surface._gpu = {
    device,
    generation: 7,
    passRecords: [
      compute("cooperative", 5, { repeat_batch: 2 }),
      compute("successor", 1),
    ],
  };

  await surface._presentOn({}, []);

  assert.deepEqual(
    submissions.map((submission) => submission.map(([name]) => name)),
    [
      ["cooperative", "cooperative"],
      ["cooperative", "cooperative"],
      ["cooperative"],
      ["successor"],
    ],
  );
  assert.equal(queueIdle, 3);
});

test("cooperative presentations serialize complete frame snapshots", async () => {
  let releaseFirst;
  const firstBoundary = new Promise((resolve) => { releaseFirst = resolve; });
  const presentations = [];
  const frames = [];
  const surface = Object.create(FeSurfaceElement.prototype);
  surface._presentationTail = Promise.resolve();
  surface._presentNow = async (_context, uniforms) => {
    presentations.push([...uniforms]);
    if (uniforms[0] === 1) await firstBoundary;
  };
  surface._fsm = "live";
  surface._mode = "webgpu";
  surface._adoptedCanvas = {};
  surface._adoptedContext = {};
  surface._uniforms = [0];
  surface._members = [{ name: "value" }];
  surface._refreshControlValues = () => {};
  surface._dispatch = (type, detail) => {
    if (type === "fe-frame") frames.push(detail.params);
  };

  const first = surface._render([1]);
  const second = surface._render([2]);
  await Promise.resolve();
  assert.deepEqual(presentations, [[1]]);
  releaseFirst();
  await Promise.all([first, second]);

  assert.deepEqual(presentations, [[1], [2]]);
  assert.deepEqual(frames, [{ value: 1 }, { value: 2 }]);
});

test("authored raster varyings never become Fe actor or resource arguments", () => {
  const surface = Object.create(FeSurfaceElement.prototype);
  surface._builtins = [{ arg_index: 0 }];
  surface._members = [
    { name: "yaw", arg_index: 7 },
    { name: "lambda", arg_index: 6 },
  ];
  surface._memberIndexByName = new Map([
    ["lambda", 0],
    ["yaw", 1],
  ]);
  surface._memberIndexByArg = new Map([
    [6, 0],
    [7, 1],
  ]);
  surface._uniforms = [0.15, 0.6];
  surface._resources = [];

  assert.deepEqual(surface._surfaceActorArgs(), [0.15, 0.6]);
  assert.deepEqual(surface._surfaceResourceArgs(), []);

  surface._resources = [{ name: "reference_orbit" }];
  assert.deepEqual(
    surface._surfaceResourceArgs(),
    [0n],
    "resident transitions receive only declared resources, not varying gaps",
  );
});

test("fixed host writes untouched SurfaceEvent records in the versioned memory layout", () => {
  const memory = new WebAssembly.Memory({ initial: 1 });
  const events = [
    {
      mx: 10, my: 20, dx: 4, dy: -3, wheelDelta: -120,
      wheelMode: 2, buttons: 3, timestamp: 1.25, width: 512, height: 256,
      eventKind: SurfaceEventKind.Gesture, paramIndex: 0, paramValue: 0,
    },
    {
      mx: 14, my: 17, dx: -1, dy: 9, wheelDelta: 40,
      wheelMode: 1, buttons: 0, timestamp: 2.5, width: 640, height: 480,
      eventKind: SurfaceEventKind.Gesture, paramIndex: 0, paramValue: 0,
    },
  ];
  const before = structuredClone(events);
  writeSurfaceEventBatch(memory, 64, events);
  const view = new DataView(memory.buffer);
  const decode = base => ({
    mx: view.getFloat32(base, true),
    my: view.getFloat32(base + 4, true),
    dx: view.getFloat32(base + 8, true),
    dy: view.getFloat32(base + 12, true),
    wheelDelta: view.getFloat32(base + 16, true),
    wheelMode: view.getUint32(base + 20, true),
    buttons: view.getUint32(base + 24, true),
    timestamp: view.getFloat32(base + 28, true),
    width: view.getFloat32(base + 32, true),
    height: view.getFloat32(base + 36, true),
    eventKind: view.getUint32(base + 40, true),
    paramIndex: view.getUint32(base + 44, true),
    paramValue: view.getFloat32(base + 48, true),
  });
  assert.deepEqual([decode(64), decode(116)], events);
  assert.deepEqual(events, before);
});

test("each Fe surface call gets a bounded arena epoch, including traps", () => {
  const surface = Object.create(FeSurfaceElement.prototype);
  surface._surfaceTransitionMemory = new WebAssembly.Memory({ initial: 1 });
  surface._surfaceTransitionStateResident = true;
  surface._resources = [];
  surface._members = [{ name: "generation" }];
  const trace = [];
  let cursor = 64;
  surface._wasmArenaReset = () => {
    trace.push("reset");
    cursor = 64;
  };
  surface._surfaceTransitionAlloc = (bytes, align) => {
    trace.push(`alloc:${cursor}:${bytes}:${align}`);
    const pointer = cursor;
    cursor += bytes;
    return pointer;
  };
  surface._surfaceTransitionKernel = () => {
    trace.push("transition");
    return [7];
  };
  const event = {
    mx: 1, my: 2, dx: 3, dy: 4, wheelDelta: 0,
    wheelMode: 0, buttons: 1, timestamp: 5, width: 64, height: 64,
    eventKind: SurfaceEventKind.PointerMove, paramIndex: 0, paramValue: 0,
  };

  assert.deepEqual(surface._runSurfaceFrame([event]), [7]);
  assert.deepEqual(trace, ["reset", "alloc:64:52:4", "transition", "reset"]);

  trace.length = 0;
  surface._surfaceTransitionKernel = () => {
    trace.push("trap");
    throw new WebAssembly.RuntimeError("unreachable");
  };
  assert.throws(() => surface._runSurfaceFrame([event]), /unreachable/);
  assert.deepEqual(trace, ["reset", "alloc:64:52:4", "trap", "reset"]);

  trace.length = 0;
  surface._surfaceTransitionKernel = () => {
    trace.push("recovered");
    return [8];
  };
  assert.deepEqual(surface._runSurfaceFrame([event]), [8]);
  assert.deepEqual(trace, ["reset", "alloc:64:52:4", "recovered", "reset"]);
});

test("legacy per-pixel Wasm fallback also bounds aggregate epochs", () => {
  const surface = Object.create(FeSurfaceElement.prototype);
  surface._argumentCount = 2;
  surface._builtins = [
    { arg_index: 0, source: "position_x" },
    { arg_index: 1, source: "position_y" },
  ];
  surface._members = [];
  const trace = [];
  surface._wasmArenaReset = () => trace.push("reset");
  surface._kernel = (px, py) => {
    trace.push(`pixel:${px}:${py}`);
    if (px === 1) throw new WebAssembly.RuntimeError("pixel trap");
    return 0xff000000;
  };
  const canvas = {
    set width(value) {},
    set height(value) {},
    getContext() {
      return {
        createImageData(width, height) {
          return { data: new Uint8Array(width * height * 4) };
        },
        putImageData() {
          throw new Error("trapped render must not publish a partial image");
        },
      };
    },
  };

  assert.throws(() => surface._renderWasmInto(canvas, 2, 1, []), /pixel trap/);
  assert.deepEqual(trace, ["reset", "pixel:0:0", "reset", "pixel:1:0", "reset"]);
});

test("a burst crosses into the Fe transition once at the presentation boundary", async () => {
  const frameCallbacks = [];
  globalThis.requestAnimationFrame = callback => {
    frameCallbacks.push(callback);
    return frameCallbacks.length;
  };

  // Bypass the DOM-heavy constructor and supply only the fixed transition
  // state touched by this scheduling path.
  const surface = Object.create(FeSurfaceElement.prototype);
  surface._fsm = "live";
  surface._surfaceTransitionSchedule = "resident";
  surface._surfaceScheduleHasQueueAction = true;
  surface._surfaceTransitionStateResident = true;
  surface._pendingSurfaceEvents = [];
  surface._gestureFrame = null;
  surface._gesturePresenting = false;
  surface._gestureDirty = false;
  surface._backingWidth = 512;
  surface._backingHeight = 256;
  surface._builtins = [];
  surface._members = [{ arg_index: 0 }];
  surface._memberIndexByArg = new Map([[0, 0]]);
  surface._resources = [];
  surface._uniforms = [41];
  let residentState = null;
  const stateReplacementCalls = [];
  surface._surfaceStateReplaceKernel = (...state) => {
    stateReplacementCalls.push(state);
    [residentState] = state;
  };
  surface._replaceSurfaceState([41]);
  surface._surfaceTransitionMemory = new WebAssembly.Memory({ initial: 1 });
  surface._wasmArenaReset = () => {};
  const allocations = [];
  surface._surfaceTransitionAlloc = (bytes, align) => {
    allocations.push([bytes, align]);
    return 64;
  };
  surface._mode = "wasm";
  surface._refreshControlValues = () => {};
  let renders = 0;
  surface._render = () => { renders += 1; };
  let transitionCalls = 0;
  const transitionArgCounts = [];
  let transportedEvents;
  surface._surfaceTransitionKernel = (...args) => {
    const [pointer, count] = args;
    transitionCalls += 1;
    transitionArgCounts.push(args.length);
    const view = new DataView(surface._surfaceTransitionMemory.buffer);
    transportedEvents = Array.from({ length: count }, (_, index) => {
      const base = pointer + index * 52;
      return {
        mx: view.getFloat32(base, true),
        my: view.getFloat32(base + 4, true),
        dx: view.getFloat32(base + 8, true),
        dy: view.getFloat32(base + 12, true),
        wheelDelta: view.getFloat32(base + 16, true),
        wheelMode: view.getUint32(base + 20, true),
        buttons: view.getUint32(base + 24, true),
        timestamp: view.getFloat32(base + 28, true),
        width: view.getFloat32(base + 32, true),
        height: view.getFloat32(base + 36, true),
        eventKind: view.getUint32(base + 40, true),
        paramIndex: view.getUint32(base + 44, true),
        paramValue: view.getFloat32(base + 48, true),
      };
    });
    residentState += 1;
    return [residentState];
  };
  const scheduleCalls = [];
  surface._surfaceScheduleKernel = (kind, timestamp, pendingEvents) => {
    scheduleCalls.push([kind, timestamp, pendingEvents]);
    if (kind === SurfaceEventKind.Gesture) return [0, 1, 0];
    if (kind === SurfaceEventKind.AnimationFrame) return [1, 0, 0];
    if (kind === SurfaceEventKind.GpuComplete) return [0, pendingEvents > 0 ? 1 : 0, 0];
    throw new Error(`unexpected schedule fact ${kind}`);
  };

  surface._applyGesture({
    mx: 100, my: 110, dx: 3, dy: -2, wheelDelta: 0,
    wheelMode: 0, buttons: 1, timestamp: 1,
  });
  surface._applyGesture({
    mx: 104, my: 118, dx: 4, dy: 8, wheelDelta: -120,
    wheelMode: 1, buttons: 1, timestamp: 2,
  });
  surface._applyGesture({
    mx: 109, my: 117, dx: 5, dy: -1, wheelDelta: -40,
    wheelMode: 1, buttons: 0, timestamp: 3,
  });

  assert.equal(transitionCalls, 0);
  assert.equal(frameCallbacks.length, 1);
  surface._gestureFrame = null;
  await surface._flushGestureFrame(10);

  assert.equal(transitionCalls, 1);
  assert.equal(renders, 1);
  assert.deepEqual(allocations, [[208, 4]]);
  assert.deepEqual(transportedEvents, [
    {
      mx: 100, my: 110, dx: 3, dy: -2, wheelDelta: 0,
      wheelMode: 0, buttons: 1, timestamp: 1, width: 512, height: 256,
      eventKind: SurfaceEventKind.Gesture, paramIndex: 0, paramValue: 0,
    },
    {
      mx: 104, my: 118, dx: 4, dy: 8, wheelDelta: -120,
      wheelMode: 1, buttons: 1, timestamp: 2, width: 512, height: 256,
      eventKind: SurfaceEventKind.Gesture, paramIndex: 0, paramValue: 0,
    },
    {
      mx: 109, my: 117, dx: 5, dy: -1, wheelDelta: -40,
      wheelMode: 1, buttons: 0, timestamp: 3, width: 512, height: 256,
      eventKind: SurfaceEventKind.Gesture, paramIndex: 0, paramValue: 0,
    },
    {
      mx: 0, my: 0, dx: 0, dy: 0, wheelDelta: 0,
      wheelMode: 0, buttons: 0, timestamp: 10, width: 512, height: 256,
      eventKind: SurfaceEventKind.AnimationFrame, paramIndex: 0, paramValue: 0,
    },
  ]);
  assert.deepEqual(surface._uniforms, [42]);
  assert.deepEqual(surface._pendingSurfaceEvents, []);
  assert.deepEqual(stateReplacementCalls, [[41]]);
  assert.deepEqual(transitionArgCounts, [2]);
  assert.deepEqual(scheduleCalls[0], [SurfaceEventKind.Gesture, 1, 1]);
  assert.deepEqual(scheduleCalls[1], [SurfaceEventKind.Gesture, 2, 2]);
  assert.deepEqual(scheduleCalls[2], [SurfaceEventKind.Gesture, 3, 3]);
  assert.deepEqual(scheduleCalls[3], [SurfaceEventKind.AnimationFrame, 10, 3]);
  assert.equal(scheduleCalls[4][0], SurfaceEventKind.GpuComplete);
  assert.equal(scheduleCalls[4][2], 0);

  // A second frame advances the state held by the Fe instance. The browser
  // presents the returned snapshot but never feeds 42 back as a frame arg or
  // state-replacement call.
  surface._applyGesture({
    mx: 110, my: 116, dx: 1, dy: -1, wheelDelta: 0,
    wheelMode: 0, buttons: 1, timestamp: 4,
  });
  surface._gestureFrame = null;
  await surface._flushGestureFrame(20);
  assert.equal(transitionCalls, 2);
  assert.equal(renders, 2);
  assert.deepEqual(allocations, [[208, 4], [104, 4]]);
  assert.deepEqual(surface._uniforms, [43]);
  assert.deepEqual(stateReplacementCalls, [[41]]);
  assert.deepEqual(transitionArgCounts, [2, 2]);
  assert.deepEqual(scheduleCalls[5], [SurfaceEventKind.Gesture, 4, 1]);
  assert.deepEqual(scheduleCalls[6], [SurfaceEventKind.AnimationFrame, 20, 1]);
  assert.equal(scheduleCalls[7][0], SurfaceEventKind.GpuComplete);
  assert.equal(scheduleCalls[7][2], 0);
});

test("Fe policy backpressure retains input until GPU completion requests a frame", async () => {
  const frameCallbacks = [];
  globalThis.requestAnimationFrame = callback => {
    frameCallbacks.push(callback);
    return frameCallbacks.length;
  };
  const surface = Object.create(FeSurfaceElement.prototype);
  surface._fsm = "live";
  surface._surfaceTransitionSchedule = "resident";
  surface._surfaceScheduleHasQueueAction = true;
  surface._surfaceTransitionKernel = () => {};
  surface._pendingSurfaceEvents = [];
  surface._gestureFrame = null;
  surface._backingWidth = 64;
  surface._backingHeight = 64;
  surface._uniforms = [0];
  surface._refreshControlValues = () => {};
  const transitionBatches = [];
  surface._runSurfaceFrame = events => {
    transitionBatches.push(structuredClone(events));
    return [transitionBatches.length];
  };
  let renders = 0;
  surface._render = () => { renders += 1; };
  surface._mode = "webgpu";
  let finishFirst;
  const firstCompletion = new Promise(resolve => { finishFirst = resolve; });
  let submissions = 0;
  surface._gpu = { device: { queue: {
    onSubmittedWorkDone() {
      submissions += 1;
      return submissions === 1 ? firstCompletion : Promise.resolve();
    },
  } } };

  const decisions = [
    [0, 1, 0],
    [1, 0, 0],
    [0, 0, 0],
    [0, 0, 0],
    [0, 1, 0],
    [1, 0, 0],
    [0, 0, 0],
  ];
  const scheduleCalls = [];
  surface._surfaceScheduleKernel = (...facts) => {
    scheduleCalls.push(facts);
    const decision = decisions[scheduleCalls.length - 1];
    if (!decision) throw new Error(`unexpected policy call ${scheduleCalls.length}`);
    return decision;
  };
  const gesture = timestamp => ({
    mx: timestamp, my: 0, dx: 1, dy: 0, wheelDelta: 0,
    wheelMode: 0, buttons: 1, timestamp,
  });

  surface._applyGesture(gesture(1));
  surface._gestureFrame = null; // model the browser entering callback 1
  const firstFlush = surface._flushGestureFrame(10);
  await Promise.resolve(); // first submission is now awaiting GPU completion

  surface._applyGesture(gesture(2));
  surface._gestureFrame = null; // model callback 2 while submission 1 is live
  await surface._flushGestureFrame(11);
  assert.equal(renders, 1);
  assert.equal(transitionBatches.length, 1);
  assert.equal(surface._pendingSurfaceEvents.length, 1);

  finishFirst();
  await firstFlush;
  assert.equal(scheduleCalls[4][0], SurfaceEventKind.GpuComplete);
  assert.equal(scheduleCalls[4][2], 1);
  assert.notEqual(surface._gestureFrame, null, "host must realize Fe's request_frame decision");

  surface._gestureFrame = null; // model the requested callback 3
  await surface._flushGestureFrame(12);
  assert.equal(renders, 2);
  assert.equal(transitionBatches.length, 2);
  assert.equal(transitionBatches[0][0].timestamp, 1);
  assert.equal(transitionBatches[1][0].timestamp, 2);
  assert.equal(surface._pendingSurfaceEvents.length, 0);
  assert.deepEqual(
    scheduleCalls.map(([kind, _timestamp, pending]) => [kind, pending]),
    [
      [SurfaceEventKind.Gesture, 1],
      [SurfaceEventKind.AnimationFrame, 1],
      [SurfaceEventKind.Gesture, 1],
      [SurfaceEventKind.AnimationFrame, 1],
      [SurfaceEventKind.GpuComplete, 1],
      [SurfaceEventKind.AnimationFrame, 1],
      [SurfaceEventKind.GpuComplete, 0],
    ],
  );
});

test("the fixed host obeys resident Fe lifecycle scheduling decisions", () => {
  const surface = Object.create(FeSurfaceElement.prototype);
  surface._surfaceTransitionKernel = () => {
    throw new Error("lifecycle scheduling must not re-enter application state");
  };
  surface._surfaceTransitionSchedule = "resident";
  surface._surfaceScheduleHasQueueAction = true;
  surface._pendingSurfaceEvents = [{
    mx: 12, my: 9, dx: 3, dy: -2, wheelDelta: 0,
    wheelMode: 0, buttons: 1, timestamp: 8, width: 320, height: 200,
    eventKind: SurfaceEventKind.Gesture, paramIndex: 0, paramValue: 0,
  }];
  surface._backingWidth = 320;
  surface._backingHeight = 200;
  surface._uniforms = [11];
  const scheduleCalls = [];
  surface._surfaceScheduleKernel = (...args) => {
    scheduleCalls.push(args);
    return [0, 1, 0];
  };
  let requested = 0;
  surface._scheduleGestureFrame = () => { requested += 1; };

  assert.deepEqual(
    surface._deliverSurfaceLifecycle(SurfaceEventKind.GpuComplete, 21.5),
    {
      present: false,
      requestFrame: true,
      queueAction: 0,
    },
  );
  assert.deepEqual(scheduleCalls, [[SurfaceEventKind.GpuComplete, 21.5, 1]]);
  assert.equal(requested, 1);
  assert.equal(surface._pendingSurfaceEvents.length, 1);
  assert.deepEqual(surface._uniforms, [11]);
});

test("gesture and parameter edits stay in one ordered raw batch until Fe admits a frame", async () => {
  const surface = Object.create(FeSurfaceElement.prototype);
  surface._fsm = "live";
  surface._surfaceTransitionKernel = () => {};
  surface._surfaceTransitionSchedule = "resident";
  surface._surfaceScheduleHasQueueAction = true;
  surface._surfaceScheduleKernel = kind => {
    if (kind === SurfaceEventKind.ParamEdit) return [0, 1, 0];
    if (kind === SurfaceEventKind.AnimationFrame) return [1, 0, 0];
    return [0, 0, 0];
  };
  surface._pendingSurfaceEvents = [{ eventKind: SurfaceEventKind.Gesture, marker: "prior gesture" }];
  surface._backingWidth = 640;
  surface._backingHeight = 480;
  surface._uniforms = [3];
  surface._memberIndexByName = new Map([["steps", 0]]);
  surface._surfaceParamIndexByName = new Map([["steps", 0]]);
  const transported = [];
  surface._runSurfaceFrame = events => {
    transported.push(structuredClone(events));
    return [5]; // stand-in for the generated Wasm's ordered Fe fold
  };
  let renders = 0;
  surface._render = next => {
    assert.equal(next, undefined, "Fe results must not re-enter the replacement boundary");
    renders += 1;
  };
  surface._replaceSurfaceState = () => {
    throw new Error("parameter edits must not replace resident state from JavaScript");
  };
  surface._refreshControlValues = () => {};
  let scheduled = 0;
  surface._scheduleGestureFrame = () => { scheduled += 1; };

  surface.params.steps = 4.6;

  assert.equal(renders, 0);
  assert.deepEqual(surface._uniforms, [3]);
  assert.deepEqual(transported, []);
  assert.equal(surface._pendingSurfaceEvents.length, 2);
  assert.deepEqual(surface._pendingSurfaceEvents[0], {
    eventKind: SurfaceEventKind.Gesture,
    marker: "prior gesture",
  });
  assert.deepEqual(
    {
      eventKind: surface._pendingSurfaceEvents[1].eventKind,
      paramIndex: surface._pendingSurfaceEvents[1].paramIndex,
      paramValue: surface._pendingSurfaceEvents[1].paramValue,
      width: surface._pendingSurfaceEvents[1].width,
      height: surface._pendingSurfaceEvents[1].height,
    },
    { eventKind: SurfaceEventKind.ParamEdit, paramIndex: 0, paramValue: 4.6, width: 640, height: 480 },
  );
  assert.equal(scheduled, 1);

  await surface._flushGestureFrame(10);
  assert.equal(renders, 1);
  assert.deepEqual(surface._uniforms, [5]);
  assert.equal(surface._pendingSurfaceEvents.length, 0);
  assert.equal(transported.length, 1);
  assert.deepEqual(
    transported[0].map(event => event.eventKind),
    [
      SurfaceEventKind.Gesture,
      SurfaceEventKind.ParamEdit,
      SurfaceEventKind.AnimationFrame,
    ],
  );
});

test("the fixed host realizes Fe-authored sample and drop queue effects", () => {
  const surface = Object.create(FeSurfaceElement.prototype);
  surface._surfaceScheduleHasQueueAction = true;
  surface._pendingSurfaceEvents = [
    { marker: "oldest" },
    { marker: "middle" },
    { marker: "newest" },
  ];

  surface._applySurfaceQueueAction({ queueAction: SurfaceQueueAction.KeepLatest });
  assert.deepEqual(surface._pendingSurfaceEvents, [{ marker: "newest" }]);

  surface._pendingSurfaceEvents.push({ marker: "later" });
  surface._applySurfaceQueueAction({ queueAction: SurfaceQueueAction.Drop });
  assert.deepEqual(surface._pendingSurfaceEvents, []);

  surface._surfaceScheduleKernel = () => [0, 0, 3];
  assert.throws(
    () => surface._runSurfaceSchedule(SurfaceEventKind.Gesture, 1, 1),
    /must return present\/request_frame and a valid queue action/,
  );
});

test("a scheduled typed surface cannot enter the legacy JavaScript scheduler", () => {
  const surface = Object.create(FeSurfaceElement.prototype);
  surface._fsm = "live";
  surface._surfaceTransitionKernel = () => {};
  surface._surfaceTransitionSchedule = "resident";
  surface._surfaceScheduleKernel = null;
  surface._pendingSurfaceEvents = [];
  surface._gestureDirty = false;
  surface._backingWidth = 64;
  surface._backingHeight = 64;

  assert.throws(
    () => surface._applyGesture({
      mx: 1,
      my: 2,
      dx: 3,
      dy: 4,
      wheelDelta: 0,
      wheelMode: 0,
      buttons: 1,
      timestamp: 5,
    }),
    /cannot fall back to JavaScript scheduling/,
  );
  assert.deepEqual(surface._pendingSurfaceEvents, []);
  assert.equal(surface._gestureDirty, false);
});

test("pointer lifecycle identity crosses the fixed host boundary untouched", () => {
  const seen = [];
  const surface = Object.create(FeSurfaceElement.prototype);
  surface._fsm = "live";
  surface._surfaceTransitionKernel = event => {
    seen.push(event);
    return [0];
  };
  surface._surfaceTransitionSchedule = "direct";
  surface._backingWidth = 320;
  surface._backingHeight = 180;
  surface._runSurfaceTransition = event => {
    seen.push(event);
    return [0];
  };
  surface._queueGestureRender = () => {};

  for (const eventKind of [
    SurfaceEventKind.PointerDown,
    SurfaceEventKind.PointerMove,
    SurfaceEventKind.PointerUp,
  ]) {
    surface._applyGesture({
      mx: 12,
      my: 34,
      dx: eventKind === SurfaceEventKind.PointerMove ? 5 : 0,
      dy: eventKind === SurfaceEventKind.PointerMove ? -2 : 0,
      wheelDelta: 0,
      wheelMode: 0,
      buttons: eventKind === SurfaceEventKind.PointerUp ? 0 : 1,
      timestamp: 9,
      eventKind,
    });
  }

  assert.deepEqual(seen.map(event => event.eventKind), [8, 9, 10]);
  assert.deepEqual(seen.map(event => event.buttons), [1, 1, 0]);
  assert.deepEqual(seen.map(event => [event.width, event.height]), [
    [320, 180],
    [320, 180],
    [320, 180],
  ]);
});
