import { describe, expect, test } from "bun:test";
import { createHostCompletionBroker } from "./host-completion.js";
import { createMaterializedTaskMachine } from "./materialized-task.js";

const u32 = Object.freeze({ kind: "unsigned", bits: 32 });
const u64 = Object.freeze({ kind: "unsigned", bits: 64 });
const state = Object.freeze({ kind: "enum_tag", bits: 8, variants: 2 });
const outcome = Object.freeze({ kind: "enum_tag", bits: 8, variants: 3 });

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

describe("browser HostTimer/Recv completion broker", () => {
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
