import { createMaterializedTaskMachine } from "./materialized-task.js";

function required(value, name) {
  if (typeof value !== "function") throw new TypeError(`missing materialized task export ${name}`);
  return value;
}

export function createMaterializedTaskRegistry(wasmExports) {
  const registry = Object.create(null);
  const task0StartName = "__fe_task_start_watch_pointer";
  const task0Start = required(wasmExports[task0StartName], task0StartName);
  const task0Resume0Name = "__fe_task_resume_watch_pointer_1";
  const task0Resume0 = required(wasmExports[task0Resume0Name], task0Resume0Name);
  const task0Resume1Name = "__fe_task_resume_watch_pointer_2";
  const task0Resume1 = required(wasmExports[task0Resume1Name], task0Resume1Name);
  const task0Resume2Name = "__fe_task_resume_watch_pointer_3";
  const task0Resume2 = required(wasmExports[task0Resume2Name], task0Resume2Name);
  registry["watch_pointer"] = createMaterializedTaskMachine({
    input: [],
    step: [{ kind: "enum_tag", bits: 8, variants: 4 }, { kind: "unsigned", bits: 32 }, { kind: "unsigned", bits: 32 }, { kind: "unsigned", bits: 32 }, { kind: "bool", bits: 1 }, { kind: "enum_tag", bits: 8, variants: 5 }, { kind: "enum_tag", bits: 8, variants: 4 }, { kind: "enum_tag", bits: 8, variants: 4 }, { kind: "unsigned", bits: 32 }, { kind: "f32", bits: 32 }, { kind: "f32", bits: 32 }, { kind: "unsigned", bits: 32 }, { kind: "bool", bits: 1 }, { kind: "f32", bits: 32 }, { kind: "f32", bits: 32 }, { kind: "unsigned", bits: 32 }, { kind: "bool", bits: 1 }, { kind: "unsigned", bits: 32 }, { kind: "bool", bits: 1 }, { kind: "enum_tag", bits: 8, variants: 4 }, { kind: "enum_tag", bits: 8, variants: 4 }, { kind: "unsigned", bits: 32 }, { kind: "f32", bits: 32 }, { kind: "f32", bits: 32 }, { kind: "unsigned", bits: 32 }, { kind: "bool", bits: 1 }, { kind: "f32", bits: 32 }, { kind: "f32", bits: 32 }, { kind: "unsigned", bits: 32 }, { kind: "unsigned", bits: 32 }, { kind: "bool", bits: 1 }, { kind: "enum_tag", bits: 8, variants: 4 }, { kind: "enum_tag", bits: 8, variants: 4 }, { kind: "unsigned", bits: 32 }, { kind: "f32", bits: 32 }, { kind: "f32", bits: 32 }, { kind: "unsigned", bits: 32 }, { kind: "bool", bits: 1 }, { kind: "f32", bits: 32 }, { kind: "f32", bits: 32 }, { kind: "unsigned", bits: 32 }, { kind: "unsigned", bits: 32 }, { kind: "unsigned", bits: 32 }, { kind: "bool", bits: 1 }, { kind: "enum_tag", bits: 8, variants: 4 }, { kind: "enum_tag", bits: 8, variants: 4 }, { kind: "unsigned", bits: 32 }, { kind: "f32", bits: 32 }, { kind: "f32", bits: 32 }, { kind: "unsigned", bits: 32 }, { kind: "bool", bits: 1 }, { kind: "f32", bits: 32 }, { kind: "f32", bits: 32 }, { kind: "unsigned", bits: 32 }],
    complete: { start: 1, count: 1 },
    start: (...lanes) => task0Start(...lanes),
    continuations: [
      { state: 1, range: { start: 2, count: 26 }, pending: { start: 2, count: 1 }, frame: { start: 3, count: 25 }, delivery: { lanes: [{ kind: "enum_tag", bits: 8, variants: 3 }, { kind: "unsigned", bits: 32 }, { kind: "enum_tag", bits: 8, variants: 4 }, { kind: "enum_tag", bits: 8, variants: 4 }, { kind: "unsigned", bits: 32 }, { kind: "f32", bits: 32 }, { kind: "f32", bits: 32 }, { kind: "unsigned", bits: 32 }, { kind: "bool", bits: 1 }, { kind: "f32", bits: 32 }, { kind: "f32", bits: 32 }], failure: { start: 1, count: 1 }, success: { start: 2, count: 9 } }, invoke: (...lanes) => task0Resume0(...lanes) },
      { state: 2, range: { start: 28, count: 13 }, pending: { start: 28, count: 1 }, frame: { start: 29, count: 12 }, delivery: { lanes: [{ kind: "enum_tag", bits: 8, variants: 3 }, { kind: "unsigned", bits: 32 }], failure: { start: 1, count: 1 }, success: { start: 2, count: 0 } }, invoke: (...lanes) => task0Resume1(...lanes) },
      { state: 3, range: { start: 41, count: 13 }, pending: { start: 41, count: 1 }, frame: { start: 42, count: 12 }, delivery: { lanes: [{ kind: "enum_tag", bits: 8, variants: 3 }, { kind: "unsigned", bits: 32 }], failure: { start: 1, count: 1 }, success: { start: 2, count: 0 } }, invoke: (...lanes) => task0Resume2(...lanes) },
    ],
  });
  const task1StartName = "__fe_task_start_watch_viewport";
  const task1Start = required(wasmExports[task1StartName], task1StartName);
  const task1Resume0Name = "__fe_task_resume_watch_viewport_1";
  const task1Resume0 = required(wasmExports[task1Resume0Name], task1Resume0Name);
  const task1Resume1Name = "__fe_task_resume_watch_viewport_2";
  const task1Resume1 = required(wasmExports[task1Resume1Name], task1Resume1Name);
  const task1Resume2Name = "__fe_task_resume_watch_viewport_3";
  const task1Resume2 = required(wasmExports[task1Resume2Name], task1Resume2Name);
  registry["watch_viewport"] = createMaterializedTaskMachine({
    input: [],
    step: [{ kind: "enum_tag", bits: 8, variants: 4 }, { kind: "unsigned", bits: 32 }, { kind: "unsigned", bits: 32 }, { kind: "unsigned", bits: 32 }, { kind: "bool", bits: 1 }, { kind: "enum_tag", bits: 8, variants: 5 }, { kind: "f32", bits: 32 }, { kind: "f32", bits: 32 }, { kind: "f32", bits: 32 }, { kind: "unsigned", bits: 32 }, { kind: "bool", bits: 1 }, { kind: "unsigned", bits: 32 }, { kind: "bool", bits: 1 }, { kind: "f32", bits: 32 }, { kind: "f32", bits: 32 }, { kind: "f32", bits: 32 }, { kind: "unsigned", bits: 32 }, { kind: "unsigned", bits: 32 }, { kind: "bool", bits: 1 }, { kind: "f32", bits: 32 }, { kind: "f32", bits: 32 }, { kind: "f32", bits: 32 }, { kind: "unsigned", bits: 32 }, { kind: "unsigned", bits: 32 }, { kind: "unsigned", bits: 32 }, { kind: "bool", bits: 1 }, { kind: "f32", bits: 32 }, { kind: "f32", bits: 32 }, { kind: "f32", bits: 32 }, { kind: "unsigned", bits: 32 }],
    complete: { start: 1, count: 1 },
    start: (...lanes) => task1Start(...lanes),
    continuations: [
      { state: 1, range: { start: 2, count: 14 }, pending: { start: 2, count: 1 }, frame: { start: 3, count: 13 }, delivery: { lanes: [{ kind: "enum_tag", bits: 8, variants: 3 }, { kind: "unsigned", bits: 32 }, { kind: "f32", bits: 32 }, { kind: "f32", bits: 32 }, { kind: "f32", bits: 32 }], failure: { start: 1, count: 1 }, success: { start: 2, count: 3 } }, invoke: (...lanes) => task1Resume0(...lanes) },
      { state: 2, range: { start: 16, count: 7 }, pending: { start: 16, count: 1 }, frame: { start: 17, count: 6 }, delivery: { lanes: [{ kind: "enum_tag", bits: 8, variants: 3 }, { kind: "unsigned", bits: 32 }], failure: { start: 1, count: 1 }, success: { start: 2, count: 0 } }, invoke: (...lanes) => task1Resume1(...lanes) },
      { state: 3, range: { start: 23, count: 7 }, pending: { start: 23, count: 1 }, frame: { start: 24, count: 6 }, delivery: { lanes: [{ kind: "enum_tag", bits: 8, variants: 3 }, { kind: "unsigned", bits: 32 }], failure: { start: 1, count: 1 }, success: { start: 2, count: 0 } }, invoke: (...lanes) => task1Resume2(...lanes) },
    ],
  });
  const task2StartName = "__fe_task_start_watch_wheel";
  const task2Start = required(wasmExports[task2StartName], task2StartName);
  const task2Resume0Name = "__fe_task_resume_watch_wheel_1";
  const task2Resume0 = required(wasmExports[task2Resume0Name], task2Resume0Name);
  const task2Resume1Name = "__fe_task_resume_watch_wheel_2";
  const task2Resume1 = required(wasmExports[task2Resume1Name], task2Resume1Name);
  const task2Resume2Name = "__fe_task_resume_watch_wheel_3";
  const task2Resume2 = required(wasmExports[task2Resume2Name], task2Resume2Name);
  registry["watch_wheel"] = createMaterializedTaskMachine({
    input: [],
    step: [{ kind: "enum_tag", bits: 8, variants: 4 }, { kind: "unsigned", bits: 32 }, { kind: "unsigned", bits: 32 }, { kind: "unsigned", bits: 32 }, { kind: "bool", bits: 1 }, { kind: "enum_tag", bits: 8, variants: 5 }, { kind: "f32", bits: 32 }, { kind: "f32", bits: 32 }, { kind: "f32", bits: 32 }, { kind: "enum_tag", bits: 8, variants: 4 }, { kind: "f32", bits: 32 }, { kind: "f32", bits: 32 }, { kind: "bool", bits: 1 }, { kind: "f32", bits: 32 }, { kind: "unsigned", bits: 32 }, { kind: "bool", bits: 1 }, { kind: "unsigned", bits: 32 }, { kind: "bool", bits: 1 }, { kind: "f32", bits: 32 }, { kind: "f32", bits: 32 }, { kind: "f32", bits: 32 }, { kind: "enum_tag", bits: 8, variants: 4 }, { kind: "f32", bits: 32 }, { kind: "f32", bits: 32 }, { kind: "bool", bits: 1 }, { kind: "f32", bits: 32 }, { kind: "unsigned", bits: 32 }, { kind: "unsigned", bits: 32 }, { kind: "bool", bits: 1 }, { kind: "f32", bits: 32 }, { kind: "f32", bits: 32 }, { kind: "f32", bits: 32 }, { kind: "enum_tag", bits: 8, variants: 4 }, { kind: "f32", bits: 32 }, { kind: "f32", bits: 32 }, { kind: "bool", bits: 1 }, { kind: "f32", bits: 32 }, { kind: "unsigned", bits: 32 }, { kind: "unsigned", bits: 32 }, { kind: "unsigned", bits: 32 }, { kind: "bool", bits: 1 }, { kind: "f32", bits: 32 }, { kind: "f32", bits: 32 }, { kind: "f32", bits: 32 }, { kind: "enum_tag", bits: 8, variants: 4 }, { kind: "f32", bits: 32 }, { kind: "f32", bits: 32 }, { kind: "bool", bits: 1 }, { kind: "f32", bits: 32 }, { kind: "unsigned", bits: 32 }],
    complete: { start: 1, count: 1 },
    start: (...lanes) => task2Start(...lanes),
    continuations: [
      { state: 1, range: { start: 2, count: 24 }, pending: { start: 2, count: 1 }, frame: { start: 3, count: 23 }, delivery: { lanes: [{ kind: "enum_tag", bits: 8, variants: 3 }, { kind: "unsigned", bits: 32 }, { kind: "f32", bits: 32 }, { kind: "f32", bits: 32 }, { kind: "f32", bits: 32 }, { kind: "enum_tag", bits: 8, variants: 4 }, { kind: "f32", bits: 32 }, { kind: "f32", bits: 32 }, { kind: "bool", bits: 1 }, { kind: "f32", bits: 32 }], failure: { start: 1, count: 1 }, success: { start: 2, count: 8 } }, invoke: (...lanes) => task2Resume0(...lanes) },
      { state: 2, range: { start: 26, count: 12 }, pending: { start: 26, count: 1 }, frame: { start: 27, count: 11 }, delivery: { lanes: [{ kind: "enum_tag", bits: 8, variants: 3 }, { kind: "unsigned", bits: 32 }], failure: { start: 1, count: 1 }, success: { start: 2, count: 0 } }, invoke: (...lanes) => task2Resume1(...lanes) },
      { state: 3, range: { start: 38, count: 12 }, pending: { start: 38, count: 1 }, frame: { start: 39, count: 11 }, delivery: { lanes: [{ kind: "enum_tag", bits: 8, variants: 3 }, { kind: "unsigned", bits: 32 }], failure: { start: 1, count: 1 }, success: { start: 2, count: 0 } }, invoke: (...lanes) => task2Resume2(...lanes) },
    ],
  });
  return Object.freeze(registry);
}

export { createHostCompletionBroker } from "./host-completion.js";
