// Fixed browser mechanics for compiler-materialized Fe continuations.
//
// Application code never supplies the definition consumed below. The Fe
// compiler emits it beside the Wasm module from the same target-neutral
// suspension machine which emitted `__fe_task_start_*` / `__fe_task_resume_*`.
// This file owns only validation, opaque affine frame custody, and invocation;
// it does not name a task, effect, handler, export, or value layout.

const frameDetails = new WeakMap();
const outcomeDetails = new WeakMap();

function exactKeys(value, expected, name) {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    throw new TypeError(`${name} must be a plain object`);
  }
  const actual = Object.keys(value).sort();
  const wanted = [...expected].sort();
  if (actual.join("\0") !== wanted.join("\0")) {
    throw new TypeError(`${name} has unexpected or missing fields`);
  }
}

function integer(value, min, max, name) {
  if (!Number.isInteger(value) || value < min || value > max) {
    throw new TypeError(`${name} is outside its Fe integer lane`);
  }
  return value;
}

function bigint(value, bits, signed, name) {
  if (typeof value !== "bigint") {
    throw new TypeError(`${name} must be a bigint`);
  }
  const width = BigInt(bits);
  const lower = signed ? -(1n << (width - 1n)) : 0n;
  const upper = signed ? (1n << (width - 1n)) - 1n : (1n << width) - 1n;
  if (value < lower || value > upper) {
    throw new TypeError(`${name} is outside its Fe integer lane`);
  }
  return value;
}

function validateLane(value, lane, name) {
  switch (lane.kind) {
    case "bool":
      if (typeof value !== "boolean") throw new TypeError(`${name} must be boolean`);
      return value;
    case "signed": {
      if (lane.bits === 64) return bigint(value, lane.bits, true, name);
      const half = 2 ** (lane.bits - 1);
      return integer(value, -half, half - 1, name);
    }
    case "unsigned":
    case "fixed_bytes":
    case "enum_tag": {
      if (lane.bits === 64) return bigint(value, lane.bits, false, name);
      const checked = integer(value, 0, 2 ** lane.bits - 1, name);
      if (lane.kind === "enum_tag" && checked >= lane.variants) {
        throw new TypeError(`${name} is not a declared Fe enum variant`);
      }
      return checked;
    }
    case "f32":
      if (typeof value !== "number") throw new TypeError(`${name} must be a number`);
      return value;
    default:
      throw new TypeError(`${name} has unknown compiler lane kind ${String(lane.kind)}`);
  }
}

function encodeLane(value, lane, name) {
  const checked = validateLane(value, lane, name);
  switch (lane.kind) {
    case "bool": return checked ? 1 : 0;
    case "unsigned":
    case "fixed_bytes":
    case "enum_tag":
      return lane.bits === 64 ? BigInt.asIntN(64, checked) : checked | 0;
    case "signed":
      return checked;
    case "f32":
      return Math.fround(checked);
    default:
      throw new TypeError(`${name} has unknown compiler lane kind ${String(lane.kind)}`);
  }
}

function decodeLane(value, lane, name) {
  switch (lane.kind) {
    case "bool":
      if (value !== 0 && value !== 1) throw new TypeError(`${name} is not a Fe bool`);
      return value === 1;
    case "signed":
      if (lane.bits === 64) return bigint(value, 64, true, name);
      if (!Number.isInteger(value)) throw new TypeError(`${name} is not an integer`);
      return lane.bits === 32 ? value | 0 : (value << (32 - lane.bits)) >> (32 - lane.bits);
    case "unsigned":
    case "fixed_bytes":
    case "enum_tag": {
      let decoded;
      if (lane.bits === 64) {
        if (typeof value !== "bigint") throw new TypeError(`${name} must be a bigint`);
        decoded = BigInt.asUintN(64, value);
      } else {
        if (!Number.isInteger(value)) throw new TypeError(`${name} is not an integer`);
        decoded = (value >>> 0) % (2 ** lane.bits);
      }
      if (lane.kind === "enum_tag" && decoded >= lane.variants) {
        throw new TypeError(`${name} is not a declared Fe enum variant`);
      }
      return decoded;
    }
    case "f32":
      if (typeof value !== "number") throw new TypeError(`${name} must be a number`);
      return value;
    default:
      throw new TypeError(`${name} has unknown compiler lane kind ${String(lane.kind)}`);
  }
}

function laneZero(value, lane) {
  return lane.bits === 64 ? value === 0n : value === 0 || value === false;
}

function vector(value, lanes, name, encode) {
  if (!Array.isArray(value) || value.length !== lanes.length) {
    throw new TypeError(`${name} must contain exactly ${lanes.length} lanes`);
  }
  return value.map((lane, index) => encode(lane, lanes[index], `${name}[${index}]`));
}

function resultVector(value, lanes, name) {
  const values = lanes.length === 1 && !Array.isArray(value) ? [value] : value;
  return vector(values, lanes, name, decodeLane);
}

function rangeIndices(range) {
  return Array.from({ length: range.count }, (_, index) => range.start + index);
}

function validateDefinition(definition) {
  exactKeys(definition, ["complete", "continuations", "input", "start", "step"], "task definition");
  if (typeof definition.start !== "function") throw new TypeError("task start must be callable");
  if (!Array.isArray(definition.input) || !Array.isArray(definition.step)) {
    throw new TypeError("task input and step layouts must be lane arrays");
  }
  if (!Array.isArray(definition.continuations)) {
    throw new TypeError("task continuations must be an array");
  }
  const states = new Set();
  for (const continuation of definition.continuations) {
    exactKeys(
      continuation,
      ["delivery", "frame", "invoke", "pending", "range", "state"],
      "task continuation",
    );
    if (!Number.isSafeInteger(continuation.state) || continuation.state < 1
        || states.has(continuation.state)) {
      throw new TypeError("task continuation states must be unique positive integers");
    }
    if (typeof continuation.invoke !== "function") {
      throw new TypeError("task continuation invoke must be callable");
    }
    states.add(continuation.state);
  }
}

function makeOutcome(kind, lanes) {
  if (!Array.isArray(lanes)) throw new TypeError(`${kind} task outcome must carry a lane array`);
  const outcome = Object.freeze({ kind });
  outcomeDetails.set(outcome, { kind, lanes: [...lanes] });
  return outcome;
}

export function taskSuccess(lanes) { return makeOutcome("success", lanes); }
export function taskFailure(lanes) { return makeOutcome("failure", lanes); }
export function taskCancelled() {
  const outcome = Object.freeze({ kind: "cancelled" });
  outcomeDetails.set(outcome, { kind: "cancelled", lanes: [] });
  return outcome;
}

export function createMaterializedTaskMachine(definition) {
  validateDefinition(definition);
  const byState = new Map(definition.continuations.map(continuation => [continuation.state, {
    ...continuation,
    handler: Object.freeze({}),
  }]));

  const decodeStep = (raw) => {
    const lanes = resultVector(raw, definition.step, "task step");
    const state = lanes[0];
    const continuation = state === 0 ? null : byState.get(state);
    if (state !== 0 && continuation === undefined) {
      throw new TypeError(`Wasm returned unknown continuation state ${state}`);
    }
    const active = new Set([0, ...rangeIndices(
      state === 0 ? definition.complete : continuation.range,
    )]);
    for (let index = 1; index < lanes.length; index += 1) {
      if (!active.has(index) && !laneZero(lanes[index], definition.step[index])) {
        throw new TypeError(`Wasm returned a nonzero inactive task lane at ${index}`);
      }
    }
    if (state === 0) {
      return Object.freeze({
        kind: "complete",
        output: Object.freeze(lanes.slice(
          definition.complete.start,
          definition.complete.start + definition.complete.count,
        )),
      });
    }
    const pending = Object.freeze({
      handler: continuation.handler,
      lanes: Object.freeze(lanes.slice(
        continuation.pending.start,
        continuation.pending.start + continuation.pending.count,
      )),
    });
    const frame = Object.freeze({});
    frameDetails.set(frame, {
      continuation,
      lanes: lanes.slice(
        continuation.frame.start,
        continuation.frame.start + continuation.frame.count,
      ),
    });
    return Object.freeze({ kind: "suspended", pending, frame });
  };

  return Object.freeze({
    start(input) {
      const lanes = vector(input, definition.input, "task input", encodeLane);
      return decodeStep(definition.start(...lanes));
    },
    resume(frame, outcome) {
      const saved = frameDetails.get(frame);
      if (saved === undefined) throw new TypeError("task frame is stale, forged, or already resumed");
      const delivered = outcomeDetails.get(outcome);
      if (delivered === undefined) throw new TypeError("task outcome was not constructed by this runtime");
      frameDetails.delete(frame);
      const { continuation, lanes: frameLanes } = saved;
      const delivery = continuation.delivery;
      const encoded = new Array(delivery.lanes.length).fill(0);
      encoded[0] = delivered.kind === "failure" ? 0 : delivered.kind === "success" ? 1 : 2;
      const payload = delivered.kind === "failure"
        ? delivery.failure
        : delivered.kind === "success" ? delivery.success : { start: 0, count: 0 };
      const payloadLanes = delivered.kind === "cancelled"
        ? []
        : vector(
          delivered.lanes,
          delivery.lanes.slice(payload.start, payload.start + payload.count),
          `task ${delivered.kind}`,
          encodeLane,
        );
      for (let index = 0; index < payloadLanes.length; index += 1) {
        encoded[payload.start + index] = payloadLanes[index];
      }
      const frameSchemas = definition.step.slice(
        continuation.frame.start,
        continuation.frame.start + continuation.frame.count,
      );
      const encodedFrame = vector(frameLanes, frameSchemas, "task frame", encodeLane);
      return decodeStep(continuation.invoke(...encodedFrame, ...encoded));
    },
  });
}
