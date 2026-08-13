import { describe, expect, test } from "bun:test";
import {
  createMaterializedTaskMachine,
  taskCancelled,
  taskFailure,
  taskSuccess,
} from "./materialized-task.js";

const u32 = Object.freeze({ kind: "unsigned", bits: 32 });
const state = Object.freeze({ kind: "enum_tag", bits: 8, variants: 3 });
const outcome = Object.freeze({ kind: "enum_tag", bits: 8, variants: 3 });

function definition(overrides = {}) {
  return {
    input: [u32, u32, u32],
    step: [state, u32, u32, u32, u32, u32, u32],
    complete: { start: 1, count: 1 },
    start(firstPending, secondPending, seed) {
      return [1, 0, firstPending, secondPending, seed, 0, 0];
    },
    continuations: [
      {
        state: 1,
        range: { start: 2, count: 3 },
        pending: { start: 2, count: 1 },
        frame: { start: 3, count: 2 },
        delivery: {
          lanes: [outcome, u32, u32],
          failure: { start: 1, count: 1 },
          success: { start: 2, count: 1 },
        },
        invoke(secondPending, seed, tag, error, value) {
          if (tag === 0) return [0, error, 0, 0, 0, 0, 0];
          if (tag === 2) return [0, seed, 0, 0, 0, 0, 0];
          return [2, 0, 0, 0, 0, secondPending, value + seed];
        },
      },
      {
        state: 2,
        range: { start: 5, count: 2 },
        pending: { start: 5, count: 1 },
        frame: { start: 6, count: 1 },
        delivery: {
          lanes: [outcome, u32, u32],
          failure: { start: 1, count: 1 },
          success: { start: 2, count: 1 },
        },
        invoke(first, tag, error, value) {
          if (tag === 0) return [0, error, 0, 0, 0, 0, 0];
          if (tag === 2) return [0, first, 0, 0, 0, 0, 0];
          return [0, first + value, 0, 0, 0, 0, 0];
        },
      },
    ],
    ...overrides,
  };
}

describe("compiler-materialized browser task machine", () => {
  test("chains exact frames through success", () => {
    const machine = createMaterializedTaskMachine(definition());
    const first = machine.start([41, 42, 5]);
    expect(first.kind).toBe("suspended");
    expect(first.pending.lanes).toEqual([41]);

    const second = machine.resume(first.frame, taskSuccess([7]));
    expect(second.kind).toBe("suspended");
    expect(second.pending.lanes).toEqual([42]);
    expect(second.pending.handler).not.toBe(first.pending.handler);

    const complete = machine.resume(second.frame, taskSuccess([8]));
    expect(complete).toEqual({ kind: "complete", output: [20] });
  });

  test("delivers typed failure and cancellation", () => {
    const machine = createMaterializedTaskMachine(definition());
    const failed = machine.start([41, 42, 5]);
    expect(machine.resume(failed.frame, taskFailure([3]))).toEqual({
      kind: "complete",
      output: [3],
    });

    const cancelled = machine.start([41, 42, 5]);
    expect(machine.resume(cancelled.frame, taskCancelled())).toEqual({
      kind: "complete",
      output: [5],
    });
  });

  test("frames are opaque and affine", () => {
    const machine = createMaterializedTaskMachine(definition());
    const step = machine.start([1, 2, 3]);
    const frame = step.frame;
    machine.resume(frame, taskCancelled());
    expect(() => machine.resume(frame, taskCancelled())).toThrow(/stale, forged, or already resumed/);
    expect(() => machine.resume(Object.freeze({}), taskCancelled())).toThrow(/stale, forged/);
    expect(() => machine.resume(machine.start([1, 2, 3]).frame, { kind: "success" }))
      .toThrow(/not constructed by this runtime/);
  });

  test("inactive lanes and declared tags are validated", () => {
    const badInactive = createMaterializedTaskMachine(definition({
      start: () => [1, 9, 1, 2, 3, 0, 0],
    }));
    expect(() => badInactive.start([1, 2, 3])).toThrow(/nonzero inactive task lane/);

    const badTag = createMaterializedTaskMachine(definition({
      start: () => [3, 0, 0, 0, 0, 0, 0],
    }));
    expect(() => badTag.start([1, 2, 3])).toThrow(/declared Fe enum variant/);
  });
});
