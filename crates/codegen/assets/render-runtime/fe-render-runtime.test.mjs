import assert from "node:assert/strict";
import test from "node:test";

// The module defines a custom element at load time. These minimal standards
// stubs let this host-policy unit gate import the fixed runtime without
// constructing a surface or pretending to be a browser.
globalThis.HTMLElement = class HTMLElement {};
globalThis.customElements = { define() {} };

const { FeSurfaceElement, SurfaceEventKind, SurfaceQueueAction, writeSurfaceEventBatch } =
  await import("./fe-render-runtime.js");

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
