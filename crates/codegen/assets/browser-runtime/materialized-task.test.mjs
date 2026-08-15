import { describe, expect, test } from "bun:test";
import {
  createMaterializedTaskMachine,
  raceTaskOutcome,
  selectTaskOutcome,
  taskCancelled,
  taskFailure,
  taskSuccess,
} from "./materialized-task.js";

const u32 = Object.freeze({ kind: "unsigned", bits: 32 });
const u64 = Object.freeze({ kind: "unsigned", bits: 64 });
const state = Object.freeze({ kind: "enum_tag", bits: 8, variants: 3 });
const outcome = Object.freeze({ kind: "enum_tag", bits: 8, variants: 3 });
const race = Object.freeze({ kind: "enum_tag", bits: 8, variants: 2 });
const select = Object.freeze({ kind: "enum_tag", bits: 8, variants: 6 });

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

  test("same-payload races are packed from the continuation schema", () => {
    const machine = createMaterializedTaskMachine({
      input: [u32],
      step: [Object.freeze({ kind: "enum_tag", bits: 8, variants: 2 }), u64, u32],
      complete: { start: 1, count: 1 },
      start(token) { return [1, 0n, token]; },
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
          return [0, winner === 0 ? left : right + 100n, 0];
        },
      }],
    });
    const left = machine.start([41]);
    const leftOutcome = raceTaskOutcome(left.pending, taskSuccess([9n]), "left");
    expect(machine.resume(left.frame, leftOutcome)).toEqual({ kind: "complete", output: [9n] });

    const right = machine.start([42]);
    const rightOutcome = raceTaskOutcome(right.pending, taskSuccess([7n]), "right");
    expect(machine.resume(right.frame, rightOutcome)).toEqual({
      kind: "complete",
      output: [107n],
    });

    const failed = machine.start([43]);
    const failure = raceTaskOutcome(failed.pending, taskFailure([5]), "left");
    expect(machine.resume(failed.frame, failure)).toEqual({ kind: "complete", output: [5n] });
  });

  test("heterogeneous selects return one affine loser and side-tag child terminals", () => {
    const machine = createMaterializedTaskMachine({
      input: [u32],
      step: [Object.freeze({ kind: "enum_tag", bits: 8, variants: 2 }), u64, u32],
      complete: { start: 1, count: 1 },
      start(token) { return [1, 0n, token]; },
      continuations: [{
        state: 1,
        range: { start: 2, count: 1 },
        pending: { start: 2, count: 1 },
        frame: { start: 3, count: 0 },
        delivery: {
          // TaskOutcome<u32, SelectOutcome<B, u32, u64, u32>>
          lanes: [outcome, u32, select, u64, u32, u32, u32, u32, u32],
          failure: { start: 1, count: 1 },
          success: { start: 2, count: 7 },
        },
        invoke(tag, outerError, selected, leftValue, rightToken, leftToken,
          rightValue, leftError, rightError) {
          if (tag === 0) return [0, 90_000n + BigInt(outerError), 0];
          if (tag === 2) return [0, 91_000n, 0];
          if (selected === 0) return [0, leftValue * 100n + BigInt(rightToken), 0];
          if (selected === 1) return [0, BigInt(leftToken) * 100n + BigInt(rightValue), 0];
          if (selected === 2) return [0, 92_000n + BigInt(leftError), 0];
          if (selected === 3) return [0, 93_000n + BigInt(rightError), 0];
          if (selected === 4) return [0, 94_000n, 0];
          return [0, 95_000n, 0];
        },
      }],
    });

    const left = machine.start([51]);
    expect(machine.resume(
      left.frame,
      selectTaskOutcome(left.pending, taskSuccess([9n]), "left", 42),
    )).toEqual({ kind: "complete", output: [942n] });

    const right = machine.start([52]);
    expect(machine.resume(
      right.frame,
      selectTaskOutcome(right.pending, taskSuccess([7]), "right", 43),
    )).toEqual({ kind: "complete", output: [4307n] });

    const leftFailure = machine.start([53]);
    expect(machine.resume(
      leftFailure.frame,
      selectTaskOutcome(leftFailure.pending, taskFailure([5]), "left", 44),
    )).toEqual({ kind: "complete", output: [92_005n] });

    const rightCancelled = machine.start([54]);
    expect(machine.resume(
      rightCancelled.frame,
      selectTaskOutcome(rightCancelled.pending, taskCancelled(), "right", 45),
    )).toEqual({ kind: "complete", output: [95_000n] });
  });
});
