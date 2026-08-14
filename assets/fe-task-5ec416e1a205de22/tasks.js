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
  const task0Resume2Name = "__fe_task_resume_activate_surfaces_3";
  const task0Resume2 = required(wasmExports[task0Resume2Name], task0Resume2Name);
  const task0Resume3Name = "__fe_task_resume_activate_surfaces_4";
  const task0Resume3 = required(wasmExports[task0Resume3Name], task0Resume3Name);
  const task0Resume4Name = "__fe_task_resume_activate_surfaces_5";
  const task0Resume4 = required(wasmExports[task0Resume4Name], task0Resume4Name);
  const task0Resume5Name = "__fe_task_resume_activate_surfaces_6";
  const task0Resume5 = required(wasmExports[task0Resume5Name], task0Resume5Name);
  const task0Resume6Name = "__fe_task_resume_activate_surfaces_7";
  const task0Resume6 = required(wasmExports[task0Resume6Name], task0Resume6Name);
  registry["activate_surfaces"] = createMaterializedTaskMachine({
    input: [],
    step: [{ kind: "enum_tag", bits: 8, variants: 8 }, { kind: "unsigned", bits: 32 }, { kind: "unsigned", bits: 32 }, { kind: "bool", bits: 1 }, { kind: "bool", bits: 1 }, { kind: "enum_tag", bits: 8, variants: 5 }, { kind: "enum_tag", bits: 8, variants: 2 }, { kind: "unsigned", bits: 32 }, { kind: "bool", bits: 1 }, { kind: "unsigned", bits: 32 }, { kind: "unsigned", bits: 32 }, { kind: "unsigned", bits: 32 }, { kind: "bool", bits: 1 }, { kind: "unsigned", bits: 32 }, { kind: "bool", bits: 1 }, { kind: "enum_tag", bits: 8, variants: 5 }, { kind: "unsigned", bits: 64 }, { kind: "unsigned", bits: 32 }, { kind: "bool", bits: 1 }, { kind: "unsigned", bits: 32 }, { kind: "unsigned", bits: 32 }, { kind: "unsigned", bits: 32 }, { kind: "unsigned", bits: 32 }, { kind: "bool", bits: 1 }, { kind: "unsigned", bits: 32 }, { kind: "bool", bits: 1 }, { kind: "unsigned", bits: 32 }, { kind: "unsigned", bits: 32 }, { kind: "unsigned", bits: 32 }, { kind: "bool", bits: 1 }, { kind: "unsigned", bits: 32 }, { kind: "unsigned", bits: 32 }, { kind: "unsigned", bits: 32 }, { kind: "unsigned", bits: 32 }, { kind: "bool", bits: 1 }, { kind: "unsigned", bits: 32 }, { kind: "unsigned", bits: 32 }, { kind: "unsigned", bits: 32 }, { kind: "unsigned", bits: 32 }, { kind: "bool", bits: 1 }, { kind: "unsigned", bits: 32 }, { kind: "bool", bits: 1 }, { kind: "unsigned", bits: 32 }, { kind: "unsigned", bits: 32 }, { kind: "bool", bits: 1 }, { kind: "unsigned", bits: 32 }, { kind: "bool", bits: 1 }, { kind: "enum_tag", bits: 8, variants: 5 }, { kind: "f32", bits: 32 }, { kind: "unsigned", bits: 32 }, { kind: "bool", bits: 1 }, { kind: "unsigned", bits: 32 }],
    complete: { start: 1, count: 1 },
    start: (...lanes) => task0Start(...lanes),
    continuations: [
      { state: 1, range: { start: 2, count: 8 }, pending: { start: 2, count: 1 }, frame: { start: 3, count: 7 }, delivery: { lanes: [{ kind: "enum_tag", bits: 8, variants: 3 }, { kind: "unsigned", bits: 32 }, { kind: "enum_tag", bits: 8, variants: 2 }], failure: { start: 1, count: 1 }, success: { start: 2, count: 1 } }, invoke: (...lanes) => task0Resume0(...lanes) },
      { state: 2, range: { start: 10, count: 10 }, pending: { start: 10, count: 1 }, frame: { start: 11, count: 9 }, delivery: { lanes: [{ kind: "enum_tag", bits: 8, variants: 3 }, { kind: "unsigned", bits: 32 }, { kind: "unsigned", bits: 64 }], failure: { start: 1, count: 1 }, success: { start: 2, count: 1 } }, invoke: (...lanes) => task0Resume1(...lanes) },
      { state: 3, range: { start: 20, count: 6 }, pending: { start: 20, count: 1 }, frame: { start: 21, count: 5 }, delivery: { lanes: [{ kind: "enum_tag", bits: 8, variants: 3 }, { kind: "unsigned", bits: 32 }, { kind: "enum_tag", bits: 8, variants: 2 }, { kind: "unsigned", bits: 64 }, { kind: "unsigned", bits: 64 }], failure: { start: 1, count: 1 }, success: { start: 2, count: 3 } }, invoke: (...lanes) => task0Resume2(...lanes) },
      { state: 4, range: { start: 26, count: 5 }, pending: { start: 26, count: 1 }, frame: { start: 27, count: 4 }, delivery: { lanes: [{ kind: "enum_tag", bits: 8, variants: 3 }, { kind: "unsigned", bits: 32 }], failure: { start: 1, count: 1 }, success: { start: 2, count: 0 } }, invoke: (...lanes) => task0Resume3(...lanes) },
      { state: 5, range: { start: 31, count: 5 }, pending: { start: 31, count: 1 }, frame: { start: 32, count: 4 }, delivery: { lanes: [{ kind: "enum_tag", bits: 8, variants: 3 }, { kind: "unsigned", bits: 32 }], failure: { start: 1, count: 1 }, success: { start: 2, count: 0 } }, invoke: (...lanes) => task0Resume4(...lanes) },
      { state: 6, range: { start: 36, count: 6 }, pending: { start: 36, count: 1 }, frame: { start: 37, count: 5 }, delivery: { lanes: [{ kind: "enum_tag", bits: 8, variants: 3 }, { kind: "unsigned", bits: 32 }], failure: { start: 1, count: 1 }, success: { start: 2, count: 0 } }, invoke: (...lanes) => task0Resume5(...lanes) },
      { state: 7, range: { start: 42, count: 10 }, pending: { start: 42, count: 1 }, frame: { start: 43, count: 9 }, delivery: { lanes: [{ kind: "enum_tag", bits: 8, variants: 3 }, { kind: "unsigned", bits: 32 }, { kind: "f32", bits: 32 }], failure: { start: 1, count: 1 }, success: { start: 2, count: 1 } }, invoke: (...lanes) => task0Resume6(...lanes) },
    ],
  });
  return Object.freeze(registry);
}

export { createHostCompletionBroker } from "./host-completion.js";
