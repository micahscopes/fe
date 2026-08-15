import { describe, expect, test } from "bun:test";
import { createHostCompletionBroker } from "./host-completion.js";
import { createMaterializedTaskMachine } from "./materialized-task.js";

const u32 = Object.freeze({ kind: "unsigned", bits: 32 });
const u64 = Object.freeze({ kind: "unsigned", bits: 64 });
const f32 = Object.freeze({ kind: "f32", bits: 32 });
const bool = Object.freeze({ kind: "bool", bits: 1 });
const state = Object.freeze({ kind: "enum_tag", bits: 8, variants: 2 });
const outcome = Object.freeze({ kind: "enum_tag", bits: 8, variants: 3 });
const race = Object.freeze({ kind: "enum_tag", bits: 8, variants: 2 });
const selection = Object.freeze({ kind: "enum_tag", bits: 8, variants: 6 });
const visibility = Object.freeze({ kind: "enum_tag", bits: 8, variants: 2 });
const enum4 = Object.freeze({ kind: "enum_tag", bits: 8, variants: 4 });

function machine(start, cancelled = 77n, failed = 88n, onCancel = () => {}) {
  return createMaterializedTaskMachine({
    input: [],
    step: [state, u64, u32],
    complete: { start: 1, count: 1 },
    start,
    continuations: [{
      state: 1,
      range: { start: 2, count: 1 },
      pending: { start: 2, count: 1 },
      frame: { start: 3, count: 0 },
      delivery: {
        lanes: [outcome, u32, u64],
        failure: { start: 1, count: 1 },
        success: { start: 2, count: 1 },
      },
      invoke(tag, error, value) {
        if (tag === 0) {
          if (value !== 0n) throw new Error("inactive success lane was not an i64 zero");
          return [0, failed + BigInt(error), 0];
        }
        if (tag === 2) {
          if (error !== 0 || value !== 0n) throw new Error("cancelled payload was not typed zero");
          onCancel();
          return [0, cancelled, 0];
        }
        return [0, value, 0];
      },
    }],
  });
}

function raceMachine(broker, delay) {
  return createMaterializedTaskMachine({
    input: [],
    step: [state, u64, u32],
    complete: { start: 1, count: 1 },
    start() {
      const receive = broker.imports["fe:host"].recv_begin();
      const timer = broker.imports["fe:host"].sleep_begin(delay);
      return [1, 0n, broker.imports["fe:host"].race_begin(receive, timer) >>> 0];
    },
    continuations: [{
      state: 1,
      range: { start: 2, count: 1 },
      pending: { start: 2, count: 1 },
      frame: { start: 3, count: 0 },
      delivery: {
        lanes: [outcome, u32, race, u64, u64],
        failure: { start: 1, count: 1 },
        success: { start: 2, count: 3 },
      },
      invoke(tag, error, winner, left, right) {
        if (tag === 0) return [0, BigInt(error), 0];
        if (tag === 2) return [0, 0n, 0];
        return [0, winner === 0 ? left : right + 10_000n, 0];
      },
    }],
  });
}

function selectTerminalMachine(broker, delay) {
  return createMaterializedTaskMachine({
    input: [],
    step: [state, u64, u32],
    complete: { start: 1, count: 1 },
    start() {
      const receive = broker.imports["fe:host"].recv_begin();
      const timer = broker.imports["fe:host"].sleep_begin(delay);
      return [1, 0n, broker.imports["fe:host"].select_begin(receive, timer) >>> 0];
    },
    continuations: [{
      state: 1,
      range: { start: 2, count: 1 },
      pending: { start: 2, count: 1 },
      frame: { start: 3, count: 0 },
      delivery: {
        lanes: [outcome, u32, selection, u64, u32, u32, u64, u32, u32],
        failure: { start: 1, count: 1 },
        success: { start: 2, count: 7 },
      },
      invoke(tag, outerError, selected, leftValue, rightToken, leftToken,
        rightValue, leftError, rightError) {
        if (tag === 0) return [0, 10_000n + BigInt(outerError), 0];
        if (tag === 2) return [0, 11_000n, 0];
        if (selected === 0) return [0, leftValue + BigInt(rightToken), 0];
        if (selected === 1) return [0, rightValue + BigInt(leftToken), 0];
        if (selected === 2) return [0, 20_000n + BigInt(leftError), 0];
        if (selected === 3) return [0, 30_000n + BigInt(rightError), 0];
        if (selected === 4) return [0, 40_000n, 0];
        return [0, 50_000n, 0];
      },
    }],
  });
}

function visibilityMachine(start, onCancel = () => {}) {
  return createMaterializedTaskMachine({
    input: [],
    step: [state, u32, u32],
    complete: { start: 1, count: 1 },
    start,
    continuations: [{
      state: 1,
      range: { start: 2, count: 1 },
      pending: { start: 2, count: 1 },
      frame: { start: 3, count: 0 },
      delivery: {
        lanes: [outcome, u32, visibility],
        failure: { start: 1, count: 1 },
        success: { start: 2, count: 1 },
      },
      invoke(tag, error, value) {
        if (tag === 0) return [0, 100 + error, 0];
        if (tag === 2) {
          onCancel();
          return [0, 200, 0];
        }
        return [0, value, 0];
      },
    }],
  });
}

function animationFrameMachine(start, onCancel = () => {}) {
  return createMaterializedTaskMachine({
    input: [],
    step: [state, f32, u32],
    complete: { start: 1, count: 1 },
    start,
    continuations: [{
      state: 1,
      range: { start: 2, count: 1 },
      pending: { start: 2, count: 1 },
      frame: { start: 3, count: 0 },
      delivery: {
        lanes: [outcome, u32, f32],
        failure: { start: 1, count: 1 },
        success: { start: 2, count: 1 },
      },
      invoke(tag, error, timestamp) {
        if (tag === 0) return [0, -100 - error, 0];
        if (tag === 2) {
          onCancel();
          return [0, -200, 0];
        }
        return [0, timestamp, 0];
      },
    }],
  });
}

function viewportMachine(start, onCancel = () => {}) {
  return createMaterializedTaskMachine({
    input: [],
    step: [state, f32, f32, f32, u32],
    complete: { start: 1, count: 3 },
    start,
    continuations: [{
      state: 1,
      range: { start: 4, count: 1 },
      pending: { start: 4, count: 1 },
      frame: { start: 5, count: 0 },
      delivery: {
        lanes: [outcome, u32, f32, f32, f32],
        failure: { start: 1, count: 1 },
        success: { start: 2, count: 3 },
      },
      invoke(tag, error, width, height, devicePixelRatio) {
        if (tag === 0) return [0, -100 - error, 0, 0, 0];
        if (tag === 2) {
          onCancel();
          return [0, -200, 0, 0, 0];
        }
        return [0, width, height, devicePixelRatio, 0];
      },
    }],
  });
}

function browserRecordMachine(fields, start, onCancel = () => {}) {
  const pending = 1 + fields.length;
  return createMaterializedTaskMachine({
    input: [],
    step: [state, ...fields, u32],
    complete: { start: 1, count: fields.length },
    start,
    continuations: [{
      state: 1,
      range: { start: pending, count: 1 },
      pending: { start: pending, count: 1 },
      frame: { start: pending + 1, count: 0 },
      delivery: {
        lanes: [outcome, u32, ...fields],
        failure: { start: 1, count: 1 },
        success: { start: 2, count: fields.length },
      },
      invoke(tag, error, ...values) {
        if (tag === 0) return [0, ...fields.map(() => 0), 0];
        if (tag === 2) {
          onCancel();
          return [0, ...fields.map(() => 0), 0];
        }
        return [0, ...values, 0];
      },
    }],
  });
}

function actorSendMachine(start, onCancel = () => {}) {
  return createMaterializedTaskMachine({
    input: [],
    step: [state, u32, u32],
    complete: { start: 1, count: 1 },
    start,
    continuations: [{
      state: 1,
      range: { start: 2, count: 1 },
      pending: { start: 2, count: 1 },
      frame: { start: 3, count: 0 },
      delivery: {
        lanes: [outcome, u32],
        failure: { start: 1, count: 1 },
        success: { start: 2, count: 0 },
      },
      invoke(tag, error) {
        if (tag === 0) return [0, 100 + error, 0];
        if (tag === 2) {
          onCancel();
          return [0, 200, 0];
        }
        return [0, 1, 0];
      },
    }],
  });
}

describe("browser HostTimer/Recv completion broker", () => {
  test("typed actor sends remain opaque and resume only after acceptance", async () => {
    const accepted = [];
    const broker = createHostCompletionBroker({
      actorEvents: {
        async send(event, signal) {
          expect(signal.aborted).toBeFalse();
          accepted.push(event);
        },
      },
    });
    const sending = actorSendMachine(() => [
      1, 0, broker.imports["fe:actor"].send_begin(3, 0.25, 9n) >>> 0,
    ]);
    expect(await broker.run(sending, [])).toEqual([1]);
    expect(accepted).toEqual([[3, 0.25, 9n]]);
    expect(Object.isFrozen(accepted[0])).toBeTrue();
    expect(broker.activeCount()).toBe(0);
  });

  test("actor-send scope cancellation suppresses stale delivery exactly once", async () => {
    let hostAborts = 0;
    let feCancellations = 0;
    const broker = createHostCompletionBroker({
      actorEvents: {
        send(_event, signal) {
          return new Promise((_resolve, reject) => {
            signal.addEventListener("abort", () => {
              hostAborts += 1;
              reject(new DOMException("cancelled", "AbortError"));
            }, { once: true });
          });
        },
      },
    });
    const sending = actorSendMachine(() => [
      1, 0, broker.imports["fe:actor"].send_begin(7) >>> 0,
    ], () => { feCancellations += 1; });
    const controller = new AbortController();
    const result = broker.run(sending, [], { signal: controller.signal });
    await Promise.resolve();
    controller.abort();
    await expect(result).rejects.toBeInstanceOf(Error);
    expect(hostAborts).toBe(1);
    expect(feCancellations).toBe(1);
    expect(broker.activeCount()).toBe(0);
  });

  test("typed surface hooks resume Fe with opaque u64 results and failures", async () => {
    const seen = [];
    const broker = createHostCompletionBroker({
      surface: {
        next: async signal => {
          expect(signal.aborted).toBeFalse();
          return 41n;
        },
        load: async (token, signal) => {
          seen.push(token);
          expect(signal.aborted).toBeFalse();
          if (token === 7n) throw new Error("browser surface failed");
          return token + 1n;
        },
      },
    });
    const next = machine(() => [
      1, 0n, broker.imports["fe:web-surface"].next_begin() >>> 0,
    ]);
    expect(await broker.run(next, [])).toEqual([41n]);

    const loaded = machine(() => [
      1, 0n, broker.imports["fe:web-surface"].load_begin(5n) >>> 0,
    ]);
    expect(await broker.run(loaded, [])).toEqual([6n]);

    const failed = machine(() => [
      1, 0n, broker.imports["fe:web-surface"].load_begin(7n) >>> 0,
    ]);
    expect(await broker.run(failed, [])).toEqual([89n]);
    expect(seen).toEqual([5n, 7n]);
    expect(broker.activeCount()).toBe(0);
  });

  test("surface hook cancellation aborts host observation exactly once", async () => {
    let aborts = 0;
    const broker = createHostCompletionBroker({
      surface: {
        next: () => 0n,
        load: (_token, signal) => new Promise((_resolve, reject) => {
          signal.addEventListener("abort", () => {
            aborts += 1;
            reject(new DOMException("cancelled", "AbortError"));
          }, { once: true });
        }),
      },
    });
    const loading = machine(() => [
      1, 0n, broker.imports["fe:web-surface"].load_begin(3n) >>> 0,
    ]);
    const controller = new AbortController();
    const result = broker.run(loading, [], { signal: controller.signal });
    await Promise.resolve();
    controller.abort();
    await expect(result).rejects.toBeInstanceOf(Error);
    expect(aborts).toBe(1);
    expect(broker.activeCount()).toBe(0);
  });

  test("typed document visibility resumes Fe and preserves observation state", async () => {
    let current = 0;
    const calls = [];
    const broker = createHostCompletionBroker({
      documentEvents: {
        visibility: async (seen, previousHidden, signal) => {
          expect(signal.aborted).toBeFalse();
          calls.push([seen, previousHidden]);
          return current;
        },
      },
    });
    const initial = visibilityMachine(() => [
      1, 0, broker.imports["fe:web-document"].visibility_begin(0, 0) >>> 0,
    ]);
    expect(await broker.run(initial, [])).toEqual([0]);

    current = 1;
    const changed = visibilityMachine(() => [
      1, 0, broker.imports["fe:web-document"].visibility_begin(1, 0) >>> 0,
    ]);
    expect(await broker.run(changed, [])).toEqual([1]);
    expect(calls).toEqual([[false, false], [true, false]]);
    expect(broker.activeCount()).toBe(0);
  });

  test("document visibility cancellation aborts observation exactly once", async () => {
    let hostAborts = 0;
    let feCancellations = 0;
    const broker = createHostCompletionBroker({
      documentEvents: {
        visibility: (_seen, _previous, signal) => new Promise((_resolve, reject) => {
          signal.addEventListener("abort", () => {
            hostAborts += 1;
            reject(new DOMException("cancelled", "AbortError"));
          }, { once: true });
        }),
      },
    });
    const waiting = visibilityMachine(() => [
      1, 0, broker.imports["fe:web-document"].visibility_begin(1, 0) >>> 0,
    ], () => { feCancellations += 1; });
    const controller = new AbortController();
    const result = broker.run(waiting, [], { signal: controller.signal });
    await Promise.resolve();
    controller.abort();
    await expect(result).rejects.toBeInstanceOf(Error);
    expect(hostAborts).toBe(1);
    expect(feCancellations).toBe(1);
    expect(broker.activeCount()).toBe(0);
  });

  test("typed animation-frame timestamps resume Fe", async () => {
    let calls = 0;
    const broker = createHostCompletionBroker({
      windowEvents: {
        animationFrame: async signal => {
          expect(signal.aborted).toBeFalse();
          calls += 1;
          return 17.25;
        },
        viewport: async () => ({ width: 800, height: 600, devicePixelRatio: 2 }),
      },
    });
    const frame = animationFrameMachine(() => [
      1, 0, broker.imports["fe:web-window"].animation_frame_begin() >>> 0,
    ]);
    expect(await broker.run(frame, [])).toEqual([17.25]);
    expect(calls).toBe(1);
    expect(broker.activeCount()).toBe(0);
  });

  test("animation-frame cancellation reaches host and Fe exactly once", async () => {
    let hostAborts = 0;
    let feCancellations = 0;
    const broker = createHostCompletionBroker({
      windowEvents: {
        animationFrame: signal => new Promise((_resolve, reject) => {
          signal.addEventListener("abort", () => {
            hostAborts += 1;
            reject(new DOMException("cancelled", "AbortError"));
          }, { once: true });
        }),
        viewport: async () => ({ width: 800, height: 600, devicePixelRatio: 2 }),
      },
    });
    const frame = animationFrameMachine(() => [
      1, 0, broker.imports["fe:web-window"].animation_frame_begin() >>> 0,
    ], () => { feCancellations += 1; });
    const controller = new AbortController();
    const result = broker.run(frame, [], { signal: controller.signal });
    await Promise.resolve();
    controller.abort();
    await expect(result).rejects.toBeInstanceOf(Error);
    expect(hostAborts).toBe(1);
    expect(feCancellations).toBe(1);
    expect(broker.activeCount()).toBe(0);
  });

  test("typed viewport values resume Fe without a host-side subscription graph", async () => {
    const calls = [];
    const broker = createHostCompletionBroker({
      windowEvents: {
        animationFrame: async () => 0,
        viewport: async (seen, width, height, devicePixelRatio, signal) => {
          expect(signal.aborted).toBeFalse();
          calls.push([seen, width, height, devicePixelRatio]);
          return { width: 720, height: 480, devicePixelRatio: 2.5 };
        },
      },
    });
    const viewport = viewportMachine(() => [
      1, 0, 0, 0,
      broker.imports["fe:web-window"].viewport_begin(1, 800, 600, 2) >>> 0,
    ]);
    expect(await broker.run(viewport, [])).toEqual([720, 480, 2.5]);
    expect(calls).toEqual([[true, 800, 600, 2]]);
    expect(broker.activeCount()).toBe(0);
  });

  test("viewport cancellation reaches host and Fe exactly once", async () => {
    let hostAborts = 0;
    let feCancellations = 0;
    const broker = createHostCompletionBroker({
      windowEvents: {
        animationFrame: async () => 0,
        viewport: (_seen, _width, _height, _dpr, signal) =>
          new Promise((_resolve, reject) => {
            signal.addEventListener("abort", () => {
              hostAborts += 1;
              reject(new DOMException("cancelled", "AbortError"));
            }, { once: true });
          }),
      },
    });
    const viewport = viewportMachine(() => [
      1, 0, 0, 0,
      broker.imports["fe:web-window"].viewport_begin(1, 800, 600, 2) >>> 0,
    ], () => { feCancellations += 1; });
    const controller = new AbortController();
    const result = broker.run(viewport, [], { signal: controller.signal });
    await Promise.resolve();
    controller.abort();
    await expect(result).rejects.toBeInstanceOf(Error);
    expect(hostAborts).toBe(1);
    expect(feCancellations).toBe(1);
    expect(broker.activeCount()).toBe(0);
  });

  test("typed component pointer and wheel facts resume Fe without a JS gesture policy", async () => {
    const broker = createHostCompletionBroker({
      componentEvents: {
        pointer: async signal => {
          expect(signal.aborted).toBeFalse();
          return {
            phase: 1,
            device: 2,
            pointerId: 41,
            clientX: -3.5,
            clientY: 72.25,
            buttons: 1,
            primary: true,
            pressure: 0.75,
            timestamp: 19.5,
          };
        },
        wheel: async signal => {
          expect(signal.aborted).toBeFalse();
          return {
            deltaX: -1.25,
            deltaY: 8.5,
            deltaZ: 0,
            mode: 1,
            clientX: 21,
            clientY: -7,
            control: false,
            timestamp: 23,
          };
        },
      },
    });
    const pointer = browserRecordMachine(
      [enum4, enum4, u32, f32, f32, u32, bool, f32, f32],
      () => [
        1, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        broker.imports["fe:web-component-events"].pointer_begin() >>> 0,
      ],
    );
    expect(await broker.run(pointer, [])).toEqual([
      1, 2, 41, -3.5, 72.25, 1, true, 0.75, 19.5,
    ]);

    const wheel = browserRecordMachine(
      [f32, f32, f32, enum4, f32, f32, bool, f32],
      () => [
        1, 0, 0, 0, 0, 0, 0, 0, 0,
        broker.imports["fe:web-component-events"].wheel_begin() >>> 0,
      ],
    );
    expect(await broker.run(wheel, [])).toEqual([
      -1.25, 8.5, 0, 1, 21, -7, false, 23,
    ]);
    expect(broker.activeCount()).toBe(0);
  });

  test("component pointer cancellation reaches the listener source and Fe once", async () => {
    let hostAborts = 0;
    let feCancellations = 0;
    const broker = createHostCompletionBroker({
      componentEvents: {
        pointer: signal => new Promise((_resolve, reject) => {
          signal.addEventListener("abort", () => {
            hostAborts += 1;
            reject(new DOMException("cancelled", "AbortError"));
          }, { once: true });
        }),
        wheel: async () => ({
          deltaX: 0, deltaY: 0, deltaZ: 0, mode: 0,
          clientX: 0, clientY: 0, control: false, timestamp: 0,
        }),
      },
    });
    const pointer = browserRecordMachine(
      [enum4, enum4, u32, f32, f32, u32, bool, f32, f32],
      () => [
        1, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        broker.imports["fe:web-component-events"].pointer_begin() >>> 0,
      ],
      () => { feCancellations += 1; },
    );
    const controller = new AbortController();
    const result = broker.run(pointer, [], { signal: controller.signal });
    await Promise.resolve();
    controller.abort();
    await expect(result).rejects.toBeInstanceOf(Error);
    expect(hostAborts).toBe(1);
    expect(feCancellations).toBe(1);
    expect(broker.activeCount()).toBe(0);
  });

  test("a real timer resumes the opaque Fe continuation", async () => {
    const broker = createHostCompletionBroker();
    const timer = machine(() => [1, 0n, broker.imports["fe:host"].sleep_begin(1n) >>> 0]);
    const before = BigInt(Math.trunc(performance.now()));
    const output = await broker.run(timer, []);
    const after = BigInt(Math.trunc(performance.now()));
    expect(output[0] >= before).toBeTrue();
    expect(output[0] <= after).toBeTrue();
    expect(broker.activeCount()).toBe(0);
  });

  test("receive success and failure remain typed Fe branches", async () => {
    const broker = createHostCompletionBroker();
    const receive = machine(() => [1, 0n, broker.imports["fe:host"].recv_begin() >>> 0]);
    const success = broker.run(receive, []);
    expect(() => broker.post(7)).toThrow(/u64 bigint/);
    expect(broker.post(0xe7e7n)).toBeTrue();
    expect(await success).toEqual([0xe7e7n]);

    const failure = broker.run(receive, []);
    expect(broker.failNextReceive(3)).toBeTrue();
    expect(await failure).toEqual([91n]);
    expect(broker.post(1n)).toBeFalse();
  });

  test("typed receive/timer race cancels the loser and lets Fe match the winner", async () => {
    const broker = createHostCompletionBroker();
    const receiveWins = broker.run(raceMachine(broker, 10_000n), []);
    expect(broker.post(17n)).toBeTrue();
    expect(await receiveWins).toEqual([17n]);
    expect(broker.activeCount()).toBe(0);

    const before = BigInt(Math.trunc(performance.now()));
    const timerWins = await broker.run(raceMachine(broker, 1n), []);
    const after = BigInt(Math.trunc(performance.now()));
    expect(timerWins[0] - 10_000n >= before).toBeTrue();
    expect(timerWins[0] - 10_000n <= after).toBeTrue();
    expect(broker.activeCount()).toBe(0);
    expect(broker.post(99n)).toBeFalse();
  });

  test("select side-tags child failure and cancels its unreachable loser", async () => {
    const broker = createHostCompletionBroker();
    const selected = broker.run(selectTerminalMachine(broker, 10_000n), []);
    expect(broker.failNextReceive(7)).toBeTrue();
    expect(await selected).toEqual([20_007n]);
    expect(broker.activeCount()).toBe(0);
    expect(broker.post(99n)).toBeFalse();
  });

  test("race inputs are distinct affine tokens and failed starts clean them", async () => {
    const broker = createHostCompletionBroker();
    const duplicate = {
      start() {
        const pending = broker.imports["fe:host"].recv_begin();
        broker.imports["fe:host"].race_begin(pending, pending);
      },
      resume() { throw new Error("unreachable"); },
    };
    await expect(broker.run(duplicate, [])).rejects.toThrow(/distinct affine/);
    expect(broker.activeCount()).toBe(0);

    const claimedTwice = {
      start() {
        const left = broker.imports["fe:host"].recv_begin();
        const right = broker.imports["fe:host"].recv_begin();
        broker.imports["fe:host"].race_begin(left, right);
        broker.imports["fe:host"].race_begin(left, right);
      },
      resume() { throw new Error("unreachable"); },
    };
    await expect(broker.run(claimedTwice, [])).rejects.toThrow(/already claimed/);
    expect(broker.activeCount()).toBe(0);
  });

  test("AbortSignal cancellation wins once and clears timer work", async () => {
    const broker = createHostCompletionBroker();
    let cleanupCount = 0;
    const timer = machine(
      () => [1, 0n, broker.imports["fe:host"].sleep_begin(10_000n) >>> 0],
      77n,
      88n,
      () => { cleanupCount += 1; },
    );
    const controller = new AbortController();
    const output = broker.run(timer, [], { signal: controller.signal });
    controller.abort();
    let cancellation;
    try { await output; }
    catch (error) { cancellation = error; }
    expect(cancellation?.name).toBe("AbortError");
    expect(cleanupCount).toBe(1);
    expect(broker.activeCount()).toBe(0);
    expect(broker.cancelAll()).toBe(0);
  });

  test("cancellation discards host work attempted by the cleanup continuation", async () => {
    const broker = createHostCompletionBroker();
    const machineWithHostWorkInCleanup = machine(
      () => [1, 0n, broker.imports["fe:host"].recv_begin() >>> 0],
      77n,
      88n,
      () => { broker.imports["fe:host"].sleep_begin(10_000n); },
    );
    const controller = new AbortController();
    const output = broker.run(machineWithHostWorkInCleanup, [], { signal: controller.signal });
    controller.abort();
    let cancellation;
    try { await output; }
    catch (error) { cancellation = error; }
    expect(cancellation?.name).toBe("AbortError");
    expect(broker.activeCount()).toBe(0);
  });

  test("a trapping start cancels host work minted by that invocation", async () => {
    const broker = createHostCompletionBroker();
    const crashing = {
      start() {
        broker.imports["fe:host"].sleep_begin(10_000n);
        throw new Error("Fe trap");
      },
      resume() { throw new Error("unreachable"); },
    };
    let trapped = false;
    try { await broker.run(crashing, []); }
    catch (error) { trapped = /Fe trap/.test(String(error)); }
    expect(trapped).toBeTrue();
    expect(broker.activeCount()).toBe(0);
  });

  test("blocking wait is rejected on the browser broker", () => {
    const broker = createHostCompletionBroker();
    expect(() => broker.imports["fe:host"].wait()).toThrow(/unavailable/);
  });
});
