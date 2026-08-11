import assert from "node:assert/strict";
import test from "node:test";

// The module defines a custom element at load time. These minimal standards
// stubs let this host-policy unit gate import the fixed runtime without
// constructing a surface or pretending to be a browser.
globalThis.HTMLElement = class HTMLElement {};
globalThis.customElements = { define() {} };

const { FeSurfaceElement, coalesceLatestSurfaceEvent } =
  await import("./fe-render-runtime.js");

test("latest-per-frame preserves accumulated motion and the newest raw facts", () => {
  const first = {
    mx: 10,
    my: 20,
    dx: 4,
    dy: -3,
    wheelDelta: 0,
    wheelMode: 0,
    buttons: 1,
    timestamp: 1,
  };
  const second = {
    mx: 14,
    my: 17,
    dx: -1,
    dy: 9,
    wheelDelta: -120,
    wheelMode: 1,
    buttons: 3,
    timestamp: 2,
  };
  const third = {
    mx: 18,
    my: 30,
    dx: 8,
    dy: 2,
    wheelDelta: -40,
    wheelMode: 2,
    buttons: 0,
    timestamp: 3,
  };

  const pending = coalesceLatestSurfaceEvent(
    coalesceLatestSurfaceEvent(null, first),
    second,
  );
  const got = coalesceLatestSurfaceEvent(pending, third);

  assert.deepEqual(got, {
    ...third,
    dx: 11,
    dy: 8,
    wheelDelta: -160,
  });
  assert.deepEqual(first, {
    mx: 10,
    my: 20,
    dx: 4,
    dy: -3,
    wheelDelta: 0,
    wheelMode: 0,
    buttons: 1,
    timestamp: 1,
  });
});

test("opposite movement and wheel facts retain their algebraic totals", () => {
  const got = coalesceLatestSurfaceEvent(
    { mx: 1, my: 2, dx: 7, dy: -5, wheelDelta: -12, timestamp: 1 },
    { mx: 9, my: 8, dx: -7, dy: 5, wheelDelta: 12, timestamp: 2 },
  );
  assert.equal(got.dx, 0);
  assert.equal(got.dy, 0);
  assert.equal(got.wheelDelta, 0);
  assert.equal(got.mx, 9);
  assert.equal(got.timestamp, 2);
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
  surface._pendingSurfaceEvent = null;
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
  surface._mode = "wasm";
  surface._refreshControlValues = () => {};
  let renders = 0;
  surface._render = () => { renders += 1; };
  let transitionCalls = 0;
  let eventArgs;
  surface._surfaceTransitionKernel = (...args) => {
    transitionCalls += 1;
    eventArgs = args.slice(0, 10);
    return [args[10] + 1];
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
  assert.deepEqual(eventArgs, [109, 117, 12, 5, -160, 1, 0, 3, 512, 256]);
  assert.deepEqual(surface._uniforms, [42]);
  assert.equal(surface._pendingSurfaceEvent, null);
});
