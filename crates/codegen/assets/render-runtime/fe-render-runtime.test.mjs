import assert from "node:assert/strict";
import test from "node:test";

// The module defines a custom element at load time. These minimal standards
// stubs let this host-policy unit gate import the fixed runtime without
// constructing a surface or pretending to be a browser.
globalThis.HTMLElement = class HTMLElement {};
globalThis.customElements = { define() {} };

const { FeSurfaceElement, writeSurfaceEventBatch } =
  await import("./fe-render-runtime.js");

test("fixed host writes untouched SurfaceEvent records in the versioned memory layout", () => {
  const memory = new WebAssembly.Memory({ initial: 1 });
  const events = [
    {
      mx: 10, my: 20, dx: 4, dy: -3, wheelDelta: -120,
      wheelMode: 2, buttons: 3, timestamp: 1.25, width: 512, height: 256,
    },
    {
      mx: 14, my: 17, dx: -1, dy: 9, wheelDelta: 40,
      wheelMode: 1, buttons: 0, timestamp: 2.5, width: 640, height: 480,
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
  });
  assert.deepEqual([decode(64), decode(104)], events);
  assert.deepEqual(events, before);
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
  surface._surfaceTransitionSchedule = "latest_per_frame";
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
  surface._surfaceTransitionMemory = new WebAssembly.Memory({ initial: 1 });
  surface._surfaceEventBufferPtr = 0;
  surface._surfaceEventBufferCapacity = 0;
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
  let transportedEvents;
  surface._surfaceTransitionKernel = (pointer, count, state) => {
    transitionCalls += 1;
    const view = new DataView(surface._surfaceTransitionMemory.buffer);
    transportedEvents = Array.from({ length: count }, (_, index) => {
      const base = pointer + index * 40;
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
      };
    });
    return [state + 1];
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
  await surface._flushGestureFrame();

  assert.equal(transitionCalls, 1);
  assert.equal(renders, 1);
  assert.deepEqual(allocations, [[160, 4]]);
  assert.deepEqual(transportedEvents, [
    {
      mx: 100, my: 110, dx: 3, dy: -2, wheelDelta: 0,
      wheelMode: 0, buttons: 1, timestamp: 1, width: 512, height: 256,
    },
    {
      mx: 104, my: 118, dx: 4, dy: 8, wheelDelta: -120,
      wheelMode: 1, buttons: 1, timestamp: 2, width: 512, height: 256,
    },
    {
      mx: 109, my: 117, dx: 5, dy: -1, wheelDelta: -40,
      wheelMode: 1, buttons: 0, timestamp: 3, width: 512, height: 256,
    },
  ]);
  assert.deepEqual(surface._uniforms, [42]);
  assert.deepEqual(surface._pendingSurfaceEvents, []);
});
