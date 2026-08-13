import { createMaterializedTaskMachine } from "./materialized-task.js";

function required(value, name) {
  if (typeof value !== "function") throw new TypeError(`missing materialized task export ${name}`);
  return value;
}

export function createMaterializedTaskRegistry(wasmExports) {
  const registry = Object.create(null);
  const task0StartName = "__fe_task_start_activate_surfaces";
  const task0Start = required(wasmExports[task0StartName], task0StartName);
  const task0Resume0Name = "__fe_task_resume_activate_surfaces_1";
  const task0Resume0 = required(wasmExports[task0Resume0Name], task0Resume0Name);
  const task0Resume1Name = "__fe_task_resume_activate_surfaces_2";
  const task0Resume1 = required(wasmExports[task0Resume1Name], task0Resume1Name);
  registry["activate_surfaces"] = createMaterializedTaskMachine({
    input: [],
    step: [{ kind: "enum_tag", bits: 8, variants: 3 }, { kind: "unsigned", bits: 32 }, { kind: "unsigned", bits: 32 }, { kind: "unsigned", bits: 32 }, { kind: "bool", bits: 1 }, { kind: "unsigned", bits: 32 }, { kind: "unsigned", bits: 32 }, { kind: "bool", bits: 1 }],
    complete: { start: 1, count: 1 },
    start: (...lanes) => task0Start(...lanes),
    continuations: [
      { state: 1, range: { start: 2, count: 3 }, pending: { start: 2, count: 1 }, frame: { start: 3, count: 2 }, delivery: { lanes: [{ kind: "enum_tag", bits: 8, variants: 3 }, { kind: "unsigned", bits: 32 }, { kind: "unsigned", bits: 64 }], failure: { start: 1, count: 1 }, success: { start: 2, count: 1 } }, invoke: (...lanes) => task0Resume0(...lanes) },
      { state: 2, range: { start: 5, count: 3 }, pending: { start: 5, count: 1 }, frame: { start: 6, count: 2 }, delivery: { lanes: [{ kind: "enum_tag", bits: 8, variants: 3 }, { kind: "unsigned", bits: 32 }, { kind: "enum_tag", bits: 8, variants: 2 }, { kind: "unsigned", bits: 64 }, { kind: "unsigned", bits: 64 }], failure: { start: 1, count: 1 }, success: { start: 2, count: 3 } }, invoke: (...lanes) => task0Resume1(...lanes) },
    ],
  });
  return Object.freeze(registry);
}

export { createHostCompletionBroker } from "./host-completion.js";
