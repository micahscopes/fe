import assert from "node:assert/strict";
import test from "node:test";
import {
  bootFeArtifacts,
  createComponentEventSource,
  createDocumentEventSource,
  createGpuDeviceEventSource,
  createGpuQueueIdleEventSource,
  createWindowEventSource,
  decodeComponentCommands,
  FeComponentElement,
  registerFeImportProvider,
} from "../assets/bootstrap.js";

if (!globalThis.CustomEvent) {
  globalThis.CustomEvent = class CustomEvent {
    constructor(type, init) {
      this.type = type;
      this.detail = init?.detail;
    }
  };
}

function element() {
  return {
    baseURI: "https://example.test/",
    dataset: {
      feManifest: "assets/app.json",
      feSrc: "assets/app.wasm",
      feEntry: "main",
    },
    dispatchEvent(event) {
      this.lastEvent = event;
    },
  };
}

async function digest(bytes) {
  const value = new Uint8Array(await crypto.subtle.digest("SHA-256", bytes));
  return Array.from(value, byte => byte.toString(16).padStart(2, "0")).join("");
}

test("document visibility adapter reports state changes without a permanent listener", async () => {
  const documentTarget = new EventTarget();
  documentTarget.visibilityState = "hidden";
  const source = createDocumentEventSource(documentTarget);

  assert.equal(await source.visibility(false, false), 1);

  const visible = source.visibility(true, true);
  documentTarget.visibilityState = "visible";
  documentTarget.dispatchEvent(new Event("visibilitychange"));
  assert.equal(await visible, 0);

  // A change between Fe pulls is observed synchronously from current state.
  documentTarget.visibilityState = "hidden";
  assert.equal(await source.visibility(true, false), 1);

  const controller = new AbortController();
  const cancelled = source.visibility(true, true, controller.signal);
  controller.abort();
  await assert.rejects(cancelled, error => error.name === "AbortError");

  documentTarget.visibilityState = "prerender";
  assert.throws(() => source.visibility(false, false), /unsupported Document\.visibilityState/);
});

test("window animation-frame adapter resolves or cancels exactly one request", async () => {
  let nextHandle = 1;
  const callbacks = new Map();
  const cancelled = [];
  const windowTarget = new EventTarget();
  Object.assign(windowTarget, {
    innerWidth: 800,
    innerHeight: 600,
    devicePixelRatio: 2,
    requestAnimationFrame(callback) {
      const handle = nextHandle++;
      callbacks.set(handle, callback);
      return handle;
    },
    cancelAnimationFrame(handle) {
      cancelled.push(handle);
      callbacks.delete(handle);
    },
  });
  const source = createWindowEventSource(windowTarget);

  const first = source.animationFrame();
  callbacks.get(1)(12.5);
  callbacks.delete(1);
  assert.equal(await first, 12.5);

  const controller = new AbortController();
  const second = source.animationFrame(controller.signal);
  controller.abort();
  await assert.rejects(second, error => error.name === "AbortError");
  assert.deepEqual(cancelled, [2]);
  assert.equal(callbacks.size, 0);
});

test("window viewport adapter reports typed changes without permanent host state", async () => {
  const windowTarget = new EventTarget();
  Object.assign(windowTarget, {
    innerWidth: 800,
    innerHeight: 600,
    devicePixelRatio: 2,
    requestAnimationFrame() { return 1; },
    cancelAnimationFrame() {},
  });
  const source = createWindowEventSource(windowTarget);

  assert.deepEqual(await source.viewport(false, 0, 0, 0), {
    width: 800,
    height: 600,
    devicePixelRatio: 2,
  });

  const resized = source.viewport(true, 800, 600, 2);
  windowTarget.innerWidth = 720;
  windowTarget.dispatchEvent(new Event("resize"));
  assert.deepEqual(await resized, {
    width: 720,
    height: 600,
    devicePixelRatio: 2,
  });

  // A change between Fe pulls is observed synchronously from current state.
  windowTarget.devicePixelRatio = 3;
  assert.deepEqual(await source.viewport(true, 720, 600, 2), {
    width: 720,
    height: 600,
    devicePixelRatio: 3,
  });

  const controller = new AbortController();
  const cancelled = source.viewport(true, 720, 600, 3, controller.signal);
  controller.abort();
  await assert.rejects(cancelled, error => error.name === "AbortError");
});

test("GPU device adapter observes the shared render runtime without acquiring another device", async () => {
  const calls = [];
  const source = createGpuDeviceEventSource({
    observeSharedGpuDevice(seen, previousSequence, signal) {
      calls.push([seen, previousSequence, signal]);
      return { kind: 2, reason: 1, generation: 4, sequence: 9, missed: 0 };
    },
  });
  const controller = new AbortController();
  assert.deepEqual(await source.observe(true, 8, controller.signal), {
    kind: 2, reason: 1, generation: 4, sequence: 9, missed: 0,
  });
  assert.deepEqual(calls, [[true, 8, controller.signal]]);
  assert.throws(
    () => createGpuDeviceEventSource({}),
    /fixed render runtime lifecycle export/,
  );
});

test("GPU queue-idle adapter exposes only the shared runtime's typed completion facts", async () => {
  const calls = [];
  const source = createGpuQueueIdleEventSource({
    observeSharedGpuQueueIdle(seen, previousSequence, signal) {
      calls.push([seen, previousSequence, signal]);
      return { generation: 4, sequence: 12, missed: 2 };
    },
  });
  const controller = new AbortController();
  assert.deepEqual(await source.observe(true, 9, controller.signal), {
    generation: 4, sequence: 12, missed: 2,
  });
  assert.deepEqual(calls, [[true, 9, controller.signal]]);
  assert.throws(
    () => createGpuQueueIdleEventSource({}),
    /fixed render runtime completion export/,
  );
});

test("component pointer and wheel adapters own exactly one pending pull", async () => {
  const component = new EventTarget();
  const attached = [];
  const detached = [];
  const add = component.addEventListener.bind(component);
  const remove = component.removeEventListener.bind(component);
  component.addEventListener = (type, listener, options) => {
    attached.push(type);
    add(type, listener, options);
  };
  component.removeEventListener = (type, listener, options) => {
    detached.push(type);
    remove(type, listener, options);
  };
  const source = createComponentEventSource(() => component);

  const pendingPointer = source.pointer();
  const pointer = new Event("pointermove");
  Object.defineProperties(pointer, {
    pointerType: { value: "touch" },
    pointerId: { value: 17 },
    clientX: { value: -2.5 },
    clientY: { value: 91.25 },
    buttons: { value: 1 },
    isPrimary: { value: true },
    pressure: { value: 0.625 },
  });
  component.dispatchEvent(pointer);
  assert.deepEqual(await pendingPointer, {
    phase: 1,
    device: 2,
    pointerId: 17,
    clientX: -2.5,
    clientY: 91.25,
    buttons: 1,
    primary: true,
    pressure: 0.625,
    timestamp: Math.fround(pointer.timeStamp),
  });
  assert.deepEqual(attached, ["pointerdown", "pointermove", "pointerup", "pointercancel"]);
  assert.deepEqual(detached, attached);

  const controller = new AbortController();
  const pendingWheel = source.wheel(controller.signal);
  controller.abort();
  await assert.rejects(pendingWheel, error => error.name === "AbortError");
  assert.deepEqual(attached, [
    "pointerdown", "pointermove", "pointerup", "pointercancel", "wheel",
  ]);
  assert.deepEqual(detached, attached);
});

test("Fe-selected primary pointer capture has an exact scoped lifecycle", async () => {
  const component = new EventTarget();
  const captured = new Set();
  const operations = [];
  component.setPointerCapture = pointerId => {
    captured.add(pointerId);
    operations.push(["capture", pointerId]);
  };
  component.hasPointerCapture = pointerId => captured.has(pointerId);
  component.releasePointerCapture = pointerId => {
    captured.delete(pointerId);
    operations.push(["release", pointerId]);
  };
  const source = createComponentEventSource(() => component);

  const pointer = (type, pointerId, primary = true) => {
    const event = new Event(type);
    Object.defineProperties(event, {
      pointerType: { value: "touch" },
      pointerId: { value: pointerId },
      clientX: { value: 12 },
      clientY: { value: 34 },
      buttons: { value: type === "pointerup" ? 0 : 1 },
      isPrimary: { value: primary },
      pressure: { value: type === "pointerup" ? 0 : 0.5 },
    });
    return event;
  };

  const down = source.capturedPointer();
  component.dispatchEvent(pointer("pointerdown", 17));
  assert.equal((await down).phase, 0);
  assert.deepEqual(operations, [["capture", 17]]);

  const secondary = source.capturedPointer();
  component.dispatchEvent(pointer("pointerdown", 18, false));
  assert.equal((await secondary).pointerId, 18);
  assert.deepEqual(operations, [["capture", 17]]);

  const up = source.capturedPointer();
  component.dispatchEvent(pointer("pointerup", 17));
  assert.equal((await up).phase, 2);
  assert.deepEqual(operations, [["capture", 17], ["release", 17]]);

  const secondDown = source.capturedPointer();
  component.dispatchEvent(pointer("pointerdown", 23));
  await secondDown;
  const controller = new AbortController();
  const cancelled = source.capturedPointer(controller.signal);
  controller.abort();
  await assert.rejects(cancelled, error => error.name === "AbortError");
  assert.deepEqual(operations, [
    ["capture", 17], ["release", 17], ["capture", 23], ["release", 23],
  ]);

  const thirdDown = source.capturedPointer();
  component.dispatchEvent(pointer("pointerdown", 29));
  await thirdDown;
  const lost = source.capturedPointer();
  component.dispatchEvent(pointer("lostpointercapture", 29));
  assert.equal((await lost).phase, 4);
  assert.deepEqual(operations, [
    ["capture", 17], ["release", 17], ["capture", 23], ["release", 23],
    ["capture", 29],
  ]);
});

function installFetch(bytes) {
  return async url => String(url).endsWith(".json")
    ? {
        ok: true,
        json: async () => ({
          entry: "main",
          artifacts: [{
            kind: "wasm_module",
            byte_len: bytes.byteLength,
            sha256: await digest(bytes),
          }],
        }),
      }
    : { ok: true, arrayBuffer: async () => bytes.buffer };
}

test("published bootstrap executes a real precompiled artifact without a compiler", async () => {
  const wasm = Uint8Array.from([
    0, 97, 115, 109, 1, 0, 0, 0,
    1, 5, 1, 96, 0, 1, 127,
    3, 2, 1, 0,
    7, 8, 1, 4, 109, 97, 105, 110, 0, 0,
    10, 6, 1, 4, 0, 65, 42, 11,
  ]);
  globalThis.fetch = installFetch(wasm);
  const script = element();
  const [result] = await bootFeArtifacts({
    querySelectorAll: () => [script],
  });
  assert.equal(result.value, 42);
  assert.equal(script.dataset.feState, "complete");
  assert.equal(script.lastEvent.type, "fe:load");
});

test("published bootstrap fails preflight before instantiation on unresolved imports", async () => {
  const wasm = Uint8Array.from([
    0, 97, 115, 109, 1, 0, 0, 0,
    1, 4, 1, 96, 0, 0,
    2, 11, 1, 3, 101, 110, 118, 3, 108, 111, 103, 0, 0,
    7, 8, 1, 4, 109, 97, 105, 110, 0, 0,
  ]);
  globalThis.fetch = installFetch(wasm);
  const script = element();
  await assert.rejects(
    bootFeArtifacts({ querySelectorAll: () => [script] }),
    /missing Wasm import: env\.log/,
  );
  assert.equal(script.dataset.feState, "error");
  assert.equal(script.lastEvent.type, "fe:error");
});

test("one failed artifact does not prevent sibling artifacts from booting", async () => {
  const invalid = Uint8Array.from([
    0, 97, 115, 109, 1, 0, 0, 0,
    1, 4, 1, 96, 0, 0,
    2, 11, 1, 3, 101, 110, 118, 3, 108, 111, 103, 0, 0,
    7, 8, 1, 4, 109, 97, 105, 110, 0, 0,
  ]);
  const valid = Uint8Array.from([
    0, 97, 115, 109, 1, 0, 0, 0,
    1, 5, 1, 96, 0, 1, 127,
    3, 2, 1, 0,
    7, 8, 1, 4, 109, 97, 105, 110, 0, 0,
    10, 6, 1, 4, 0, 65, 42, 11,
  ]);
  globalThis.fetch = async url => {
    const href = String(url);
    const bytes = href.includes("invalid") ? invalid : valid;
    return href.endsWith(".json")
      ? {
          ok: true,
          json: async () => ({
            entry: "main",
            artifacts: [{
              kind: "wasm_module",
              byte_len: bytes.byteLength,
              sha256: await digest(bytes),
            }],
          }),
        }
      : { ok: true, arrayBuffer: async () => bytes.buffer };
  };
  const failed = element();
  failed.dataset.feManifest = "assets/invalid.json";
  failed.dataset.feSrc = "assets/invalid.wasm";
  const sibling = element();
  sibling.dataset.feManifest = "assets/valid.json";
  sibling.dataset.feSrc = "assets/valid.wasm";

  await assert.rejects(
    bootFeArtifacts({ querySelectorAll: () => [failed, sibling] }),
    /missing Wasm import: env\.log/,
  );
  assert.equal(failed.dataset.feState, "error");
  assert.equal(sibling.dataset.feState, "complete");
  assert.equal(sibling.feResult.value, 42);
});

test("retired component resource opcodes fail closed", () => {
  const encoder = new TextEncoder();
  const command = (opcode, id, value) => {
    const text = encoder.encode(value);
    const bytes = new Uint8Array(9 + text.length);
    const view = new DataView(bytes.buffer);
    bytes[0] = opcode;
    view.setUint32(1, id, true);
    view.setUint32(5, text.length, true);
    bytes.set(text, 9);
    return bytes;
  };
  assert.deepEqual(
    decodeComponentCommands(command(11, 9, "https://example.test/a.fe")),
    [{ opcode: 11, target: 9, value: "https://example.test/a.fe" }],
  );
  const activation = new Uint8Array(9);
  const activationView = new DataView(activation.buffer);
  activation[0] = 14;
  activationView.setUint32(1, 7, true);
  activationView.setUint32(5, 30_000, true);
  assert.deepEqual(
    decodeComponentCommands(activation),
    [{ opcode: 14, sequence: 7, timeout: 30_000 }],
  );
  assert.throws(
    () => decodeComponentCommands(command(12, 41, "https://example.test/a.fe")),
    /unknown command opcode 12/,
  );
  assert.throws(
    () => decodeComponentCommands(command(13, 42, "https://example.test/a.wasm")),
    /unknown command opcode 13/,
  );
});

test("component scoped tasks start per connection and cancel with their actor scope", async () => {
  const component = new FeComponentElement();
  const machine = {
    inputWidth: 2,
    liftInput(input) {
      assert.deepEqual(input, [17, 1]);
      return [17, true];
    },
    start() {},
    resume() {},
  };
  const signals = [];
  const broker = {
    run(received, input, { signal }) {
      assert.equal(received, machine);
      assert.deepEqual(input, [17, true]);
      signals.push(signal);
      return new Promise((resolve, reject) => {
        signal.addEventListener("abort", () => {
          const error = new Error("cancelled");
          error.name = "AbortError";
          reject(error);
        }, { once: true });
      });
    },
    cancelAll() { return 0; },
  };

  component._active = true;
  component._state = [17, 1];
  component.attachFeScopedTasks([machine], broker);
  assert.equal(signals.length, 1);
  assert.equal(signals[0].aborted, false);

  component._active = false;
  component.disconnectedCallback();
  assert.equal(signals[0].aborted, true);
  await Promise.resolve();

  component._active = true;
  component._startScopedTasks();
  assert.equal(signals.length, 2);
  assert.equal(signals[1].aborted, false);
});

test("scoped task events cross opaquely into the resident transition and project once", () => {
  const component = new FeComponentElement();
  const transitions = [];
  const patches = [];
  component._instance = {
    exports: {
      fe_actor_transition_v1(kind, value, stamp) {
        transitions.push([kind, value, stamp]);
        return [41];
      },
      fe_actor_project_v1() {
        return [0, 0, 0, 0, 0];
      },
    },
  };
  component._initialized = true;
  component._active = true;
  component._applyPatch = patch => patches.push(patch);

  const controller = new AbortController();
  component._sendScopedTaskEvent([3, 0.25, 9n], controller.signal);
  assert.deepEqual(transitions, [[3, 0.25, 9n]]);
  assert.deepEqual(patches, [[0, 0, 0, 0, 0]]);

  controller.abort();
  assert.throws(
    () => component._sendScopedTaskEvent([4], controller.signal),
    error => error.name === "AbortError",
  );
  component._active = false;
  assert.throws(
    () => component._sendScopedTaskEvent([5], new AbortController().signal),
    error => error.name === "AbortError",
  );
  assert.equal(transitions.length, 1, "stale actor events must not enter Fe");
});

test("browser component events zero-fill only a compiler-derived task payload tail", () => {
  const component = new FeComponentElement();
  const transitions = [];
  component._instance = {
    exports: {
      fe_actor_transition_v1(
        kind, target, request, key, detail, value, timestamp, textPointer, textLength,
        taskWidth, taskHeight, taskRatio, taskError,
      ) {
        transitions.push([
          kind, target, request, key, detail, value, timestamp, textPointer, textLength,
          taskWidth, taskHeight, taskRatio, taskError,
        ]);
        return [17];
      },
      fe_actor_project_v1() {
        return [0, 0, 0, 0, 0];
      },
    },
  };
  component._initialized = true;
  component._active = true;
  component._applyPatch = () => {};

  component._send(0, 0, 0, 0, 0, 0, 12.5);
  assert.deepEqual(transitions, [[
    0, 0, 0, 0, 0, 0, 12.5, 0, 0,
    0, 0, 0, 0,
  ]]);
  assert.throws(
    () => component._sendScopedTaskEvent(
      [0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 2, 3],
      new AbortController().signal,
    ),
    /has 12 lanes; transition expects 13/,
  );
});

test("bootstrap registers a selected adapter before real Wasm import preflight", async () => {
  const wasm = Uint8Array.from([
    0, 97, 115, 109, 1, 0, 0, 0,
    1, 4, 1, 96, 0, 0,
    2, 11, 1, 3, 101, 110, 118, 3, 108, 111, 103, 0, 0,
    7, 8, 1, 4, 109, 97, 105, 110, 0, 0,
  ]);
  globalThis.fetch = installFetch(wasm);
  let calls = 0;
  globalThis.selectedAdapterLog = () => { calls += 1; };
  const selectedModule = [
    "export function createFeBrowserCoreAdapter() {",
    "  let attached = false;",
    "  return {",
    "    imports: { env: { log: () => { if (!attached) throw new Error('adapter was not attached'); globalThis.selectedAdapterLog(); } } },",
    "    attach() { attached = true; },",
    "  };",
    "}",
  ].join("\n");
  const script = element();
  script.dataset.feAdapter =
    `data:text/javascript;base64,${Buffer.from(selectedModule).toString("base64")}`;
  const [result] = await bootFeArtifacts({
    querySelectorAll: () => [script],
  });
  assert.equal(result.value, undefined);
  assert.equal(calls, 1);
  delete globalThis.selectedAdapterLog;
});

test("application import providers satisfy preflight outside compiler code", async () => {
  const wasm = Uint8Array.from([
    0, 97, 115, 109, 1, 0, 0, 0,
    1, 4, 1, 96, 0, 0,
    2, 11, 1, 3, 101, 110, 118, 3, 108, 111, 103, 0, 0,
    7, 8, 1, 4, 109, 97, 105, 110, 0, 0,
  ]);
  globalThis.fetch = installFetch(wasm);
  let calls = 0;
  registerFeImportProvider({
    env: {
      log() {
        calls += 1;
      },
    },
  });
  const [result] = await bootFeArtifacts({
    querySelectorAll: () => [element()],
  });
  assert.equal(result.value, undefined);
  assert.equal(calls, 1);
});
