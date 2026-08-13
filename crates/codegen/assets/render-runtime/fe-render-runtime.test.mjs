import assert from "node:assert/strict";
import test from "node:test";

// The module defines a custom element at load time. These minimal standards
// stubs let this host-policy unit gate import the fixed runtime without
// constructing a surface or pretending to be a browser.
globalThis.HTMLElement = class HTMLElement {};
globalThis.customElements = { define() {} };

const { FeSurfaceElement, SurfaceEventKind, SurfaceQueueAction, fitBackingExtent, rasterDrawVertexCount, requiresGpuPassGraph, unpackCanvasReadback, writeSurfaceEventBatch } =
  await import("./fe-render-runtime.js");

test("runtime backing ceilings preserve aspect instead of cropping mobile work", () => {
  assert.deepEqual(fitBackingExtent(512, 256, 128), { width: 128, height: 64 });
  assert.deepEqual(fitBackingExtent(96, 64, 128), { width: 96, height: 64 });
  assert.deepEqual(fitBackingExtent(512, 256), { width: 512, height: 256 });
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

test("poster copy is encoded after rendering in the same GPU submission", () => {
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

  const readback = surface._presentOn(
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

test("fixed host consumes the Fe-derived authored-raster draw count", () => {
  assert.equal(rasterDrawVertexCount({ draw_vertices: 7 }), 7);
  assert.equal(rasterDrawVertexCount({}), 3, "legacy fullscreen render remains three vertices");
  assert.throws(
    () => rasterDrawVertexCount({ draw_vertices: 0 }),
    /invalid compiler-derived raster vertex count/,
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
  assert.deepEqual(allocations, [[156, 4]]);
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
  assert.deepEqual(allocations, [[156, 4], [52, 4]]);
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
    { present: false, requestFrame: true, queueAction: 0 },
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
    [SurfaceEventKind.Gesture, SurfaceEventKind.ParamEdit],
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
    /valid queue action/,
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
