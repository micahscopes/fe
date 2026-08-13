import { describe, expect, test } from "bun:test";
import { createHostCompletionBroker } from "./host-completion.js";
import { createMaterializedTaskMachine } from "./materialized-task.js";

const u32 = Object.freeze({ kind: "unsigned", bits: 32 });
const u64 = Object.freeze({ kind: "unsigned", bits: 64 });
const state = Object.freeze({ kind: "enum_tag", bits: 8, variants: 2 });
const outcome = Object.freeze({ kind: "enum_tag", bits: 8, variants: 3 });
const race = Object.freeze({ kind: "enum_tag", bits: 8, variants: 2 });

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

describe("browser HostTimer/Recv completion broker", () => {
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
