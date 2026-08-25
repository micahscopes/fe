// Fixed browser mechanics for compiler-materialized Fe continuations.
//
// Application code never supplies the definition consumed below. The Fe
// compiler emits it beside the Wasm module from the same target-neutral
// suspension machine which emitted `__fe_task_start_*` / `__fe_task_resume_*`.
// This file owns only validation, opaque affine frame custody, and invocation;
// it does not name a task, effect, handler, export, or value layout.

const frameDetails = new WeakMap();
const outcomeDetails = new WeakMap();
const pendingDetails = new WeakMap();

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
    case "borrowed_pointer":
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
    case "borrowed_pointer":
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
    case "borrowed_pointer":
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

function encodedZero(lane) {
  if (lane.kind === "bool") return false;
  return lane.bits === 64 ? 0n : 0;
}

function vector(value, lanes, name, encode) {
  if (!Array.isArray(value) || value.length !== lanes.length) {
    throw new TypeError(`${name} must contain exactly ${lanes.length} lanes`);
  }
  return value.map((lane, index) => encode(lane, lanes[index], `${name}[${index}]`));
}

function descriptorLengthSchema(schemas, index, name) {
  const length = schemas[index + 1];
  if (length?.kind !== "unsigned" || length.bits !== 32) {
    throw new TypeError(`${name}[${index}] borrowed pointer is not followed by a u32 length`);
  }
  return length;
}

function descriptorSize(length, pointerSchema, name) {
  const checked = integer(length, 0, pointerSchema.max, `${name} length`);
  const size = checked * pointerSchema.stride;
  if (!Number.isSafeInteger(size) || size > 0xffff_ffff) {
    throw new TypeError(`${name} byte length exceeds wasm32 memory`);
  }
  return { length: checked, size };
}

function memoryRange(memory, pointer, size, align, name) {
  const checkedPointer = integer(pointer, 0, 0xffff_ffff, `${name} pointer`);
  if (size === 0) return checkedPointer;
  if (checkedPointer === 0 || checkedPointer % align !== 0) {
    throw new TypeError(`${name} pointer is null or misaligned`);
  }
  const end = checkedPointer + size;
  if (!Number.isSafeInteger(end) || end > memory.buffer.byteLength) {
    throw new RangeError(`${name} payload is outside wasm memory`);
  }
  return checkedPointer;
}

function createBorrowedFrameStorage(wasmExports, required) {
  if (!required) return undefined;
  if (!wasmExports || typeof wasmExports !== "object" || Array.isArray(wasmExports)) {
    throw new TypeError("borrowed task frames require their Wasm exports");
  }
  const { memory } = wasmExports;
  if (!(memory instanceof WebAssembly.Memory)) {
    throw new TypeError("borrowed task frames require Wasm memory");
  }
  const canonicalRealloc = wasmExports.cabi_realloc;
  const arenaAlloc = wasmExports.fe_cabi_alloc;
  const checkpoint = wasmExports.fe_cabi_checkpoint;
  const rewind = wasmExports.fe_cabi_rewind;
  const realloc = typeof canonicalRealloc === "function"
    ? (_size, align) => canonicalRealloc(0, 0, align, _size)
    : typeof arenaAlloc === "function"
      ? (size, align) => arenaAlloc(size, align)
      : undefined;
  if (realloc === undefined || typeof wasmExports.fe_cabi_post_return !== "function"
      || typeof checkpoint !== "function" || typeof rewind !== "function") {
    throw new TypeError(
      "borrowed task frames require canonical allocation, post-return, checkpoint, and rewind",
    );
  }

  return Object.freeze({
    invoke(action) {
      const cursor = integer(
        Number(checkpoint()) >>> 0, 0, 0xffff_ffff, "task arena checkpoint",
      );
      try {
        return action();
      } finally {
        rewind(cursor);
      }
    },
    capture(values, schemas, name) {
      const captured = [...values];
      for (let index = 0; index < schemas.length; index += 1) {
        const pointerSchema = schemas[index];
        if (pointerSchema.kind !== "borrowed_pointer") continue;
        descriptorLengthSchema(schemas, index, name);
        const { length, size } = descriptorSize(values[index + 1], pointerSchema, name);
        const pointer = memoryRange(
          memory, values[index], size, pointerSchema.align, `${name}[${index}]`,
        );
        const bytes = size === 0
          ? new Uint8Array()
          : new Uint8Array(memory.buffer, pointer, size).slice();
        captured[index] = Object.freeze({ bytes, length });
        captured[index + 1] = length;
        index += 1;
      }
      return captured;
    },
    lower(values, schemas, allocations, name) {
      const lowered = [];
      for (let index = 0; index < schemas.length; index += 1) {
        const pointerSchema = schemas[index];
        if (pointerSchema.kind !== "borrowed_pointer") {
          lowered.push(encodeLane(values[index], pointerSchema, `${name}[${index}]`));
          continue;
        }
        descriptorLengthSchema(schemas, index, name);
        const owned = values[index];
        if (!owned || typeof owned !== "object" || Array.isArray(owned)
            || !(owned.bytes instanceof Uint8Array)) {
          throw new TypeError(`${name}[${index}] is not runtime-owned borrowed storage`);
        }
        const { length, size } = descriptorSize(owned.length, pointerSchema, name);
        if (owned.bytes.byteLength !== size) {
          throw new TypeError(`${name}[${index}] payload length drifted`);
        }
        let pointer = 0;
        if (size !== 0) {
          pointer = Number(realloc(size, pointerSchema.align)) >>> 0;
          memoryRange(memory, pointer, size, pointerSchema.align, `${name}[${index}]`);
          new Uint8Array(memory.buffer, pointer, size).set(owned.bytes);
          allocations.push(Object.freeze({ pointer, size, align: pointerSchema.align }));
        }
        lowered.push(pointer, length);
        index += 1;
      }
      return lowered;
    },
    release(allocations) {
      while (allocations.length !== 0) {
        const allocation = allocations.pop();
        wasmExports.fe_cabi_post_return(
          allocation.pointer, allocation.size, allocation.align,
        );
      }
    },
  });
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

/// Inspect only the terminal class of a runtime-owned task outcome. The fixed
/// completion broker uses this to decide whether a non-destructive select may
/// hand its loser back to Fe; application payloads remain opaque lanes.
export function taskOutcomeKind(outcome) {
  const delivered = outcomeDetails.get(outcome);
  if (delivered === undefined) {
    throw new TypeError("task outcome was not constructed by this runtime");
  }
  return delivered.kind;
}

/// Lift one same-payload child completion into the compiler-derived
/// `RaceOutcome<T>` success layout of `pending`. Failure/cancellation retain
/// their outer `TaskOutcome` meaning. The schema and outcome are both opaque
/// runtime authorities, so the host cannot invent payload widths or tags.
function raceTaskOutcomeForSchemas(schemas, outcome, side) {
  const delivered = outcomeDetails.get(outcome);
  if (delivered === undefined) {
    throw new TypeError("task race requires a runtime-owned outcome value");
  }
  if (side !== "left" && side !== "right") {
    throw new TypeError("task race side must be left or right");
  }
  if (delivered.kind !== "success") return outcome;
  if (schemas.length < 3 || schemas[0].kind !== "enum_tag" || schemas[0].variants !== 2
      || (schemas.length - 1) % 2 !== 0) {
    throw new TypeError("task race success is not RaceOutcome<T>");
  }
  const width = (schemas.length - 1) / 2;
  for (let index = 0; index < width; index += 1) {
    if (JSON.stringify(schemas[1 + index]) !== JSON.stringify(schemas[1 + width + index])) {
      throw new TypeError("task race variants do not carry the same payload layout");
    }
  }
  if (delivered.lanes.length !== width) {
    throw new TypeError(`task race child must contain exactly ${width} lanes`);
  }
  const lanes = schemas.map(encodedZero);
  lanes[0] = side === "left" ? 0 : 1;
  const start = side === "left" ? 1 : 1 + width;
  for (let index = 0; index < width; index += 1) {
    lanes[start + index] = delivered.lanes[index];
  }
  return taskSuccess(lanes);
}

export function raceTaskOutcome(pending, outcome, side) {
  const continuation = pendingDetails.get(pending);
  if (continuation === undefined) {
    throw new TypeError("task race requires a runtime-owned pending value");
  }
  const { delivery } = continuation;
  return raceTaskOutcomeForSchemas(delivery.lanes.slice(
    delivery.success.start,
    delivery.success.start + delivery.success.count,
  ), outcome, side);
}

/// Lift one heterogeneous child success into the compiler-derived
/// `SelectOutcome<B, E, L, R>` layout while returning the still-affine loser
/// token in the winning variant. Child failure/cancellation remains the outer
/// `TaskOutcome` terminal, so the broker can cancel the otherwise unreachable
/// loser. No application schema or payload width is supplied by the host: the
/// primitive width comes from the fixed effect ABI and the complete envelope
/// comes from the continuation compiled from Fe.
function selectTaskOutcomeForSchemas(
  schemas,
  errorWidth,
  outcome,
  side,
  loserToken,
  leftWidth,
  rightWidth,
) {
  const delivered = outcomeDetails.get(outcome);
  if (delivered === undefined) {
    throw new TypeError("task select requires a runtime-owned outcome value");
  }
  if (side !== "left" && side !== "right") {
    throw new TypeError("task select side must be left or right");
  }
  if (schemas.length < 3 + 2 * errorWidth || schemas[0].kind !== "enum_tag"
      || schemas[0].variants !== 6) {
    throw new TypeError("task select success is not SelectOutcome<B, E, L, R>");
  }
  if (!Number.isSafeInteger(leftWidth) || leftWidth < 0
      || !Number.isSafeInteger(rightWidth) || rightWidth < 0
      || schemas.length !== 3 + leftWidth + rightWidth + 2 * errorWidth) {
    throw new TypeError("task select variants do not fill the compiler-derived envelope");
  }

  const lanes = schemas.map(encodedZero);
  if (delivered.kind === "failure") {
    if (delivered.lanes.length !== errorWidth) {
      throw new TypeError(`task select child failure must contain ${errorWidth} lanes`);
    }
    lanes[0] = side === "left" ? 2 : 3;
    const errorStart = schemas.length - 2 * errorWidth
      + (side === "left" ? 0 : errorWidth);
    for (let index = 0; index < errorWidth; index += 1) {
      lanes[errorStart + index] = delivered.lanes[index];
    }
    return taskSuccess(lanes);
  }
  if (delivered.kind === "cancelled") {
    lanes[0] = side === "left" ? 4 : 5;
    return taskSuccess(lanes);
  }
  if (!Number.isInteger(loserToken) || loserToken < 0 || loserToken > 0xffff_ffff) {
    throw new TypeError("task select loser must be a u32 affine token");
  }

  const winnerWidth = side === "left" ? leftWidth : rightWidth;
  if (delivered.lanes.length !== winnerWidth) {
    throw new TypeError(
      `task select ${side} child must contain exactly ${winnerWidth} lanes`,
    );
  }
  const leftLoserIndex = 1 + leftWidth;
  const rightVariantStart = leftLoserIndex + 1;
  const rightLoserIndex = rightVariantStart;
  const leftTokenSchema = schemas[leftLoserIndex];
  const rightTokenSchema = schemas[rightLoserIndex];
  for (const tokenSchema of [leftTokenSchema, rightTokenSchema]) {
    if (tokenSchema?.kind !== "unsigned" || tokenSchema.bits !== 32) {
      throw new TypeError("task select variants must carry one u32 Pending loser token");
    }
  }
  lanes[0] = side === "left" ? 0 : 1;
  if (side === "left") {
    for (let index = 0; index < leftWidth; index += 1) {
      lanes[1 + index] = delivered.lanes[index];
    }
    lanes[leftLoserIndex] = loserToken;
  } else {
    lanes[rightLoserIndex] = loserToken;
    for (let index = 0; index < rightWidth; index += 1) {
      lanes[rightVariantStart + 1 + index] = delivered.lanes[index];
    }
  }
  return taskSuccess(lanes);
}

export function selectTaskOutcome(pending, outcome, side, loserToken) {
  const continuation = pendingDetails.get(pending);
  const delivered = outcomeDetails.get(outcome);
  if (continuation === undefined || delivered === undefined) {
    throw new TypeError("task select requires runtime-owned pending and outcome values");
  }
  const { delivery } = continuation;
  const schemas = delivery.lanes.slice(
    delivery.success.start,
    delivery.success.start + delivery.success.count,
  );
  const errorWidth = delivery.failure.count;
  const winnerWidth = delivered.lanes.length;
  const otherWidth = schemas.length - winnerWidth - 3 - 2 * errorWidth;
  if (otherWidth < 0) {
    throw new TypeError("task select child is wider than its SelectOutcome envelope");
  }
  return selectTaskOutcomeForSchemas(
    schemas,
    errorWidth,
    outcome,
    side,
    loserToken,
    side === "left" ? winnerWidth : otherWidth,
    side === "right" ? winnerWidth : otherWidth,
  );
}

function materializeTrace(trace, schemas, errorWidth) {
  if (!trace || typeof trace !== "object" || Array.isArray(trace)) {
    throw new TypeError("task completion trace must be a runtime-owned object");
  }
  if (trace.kind === "terminal") {
    const delivered = outcomeDetails.get(trace.outcome);
    if (delivered === undefined) {
      throw new TypeError("task completion trace contains a foreign outcome");
    }
    const expectedWidth = delivered.kind === "success"
      ? schemas.length
      : delivered.kind === "failure" ? errorWidth : 0;
    if (delivered.lanes.length !== expectedWidth) {
      throw new TypeError(
        `task ${delivered.kind} trace must contain exactly ${expectedWidth} lanes`,
      );
    }
    return trace.outcome;
  }
  if (trace.kind === "race") {
    const width = trace.width;
    if (!Number.isSafeInteger(width) || width < 0 || schemas.length !== 1 + 2 * width) {
      throw new TypeError("task race trace does not match its compiler-derived envelope");
    }
    const childStart = trace.side === "left" ? 1 : 1 + width;
    const outcome = materializeTrace(
      trace.winner,
      schemas.slice(childStart, childStart + width),
      errorWidth,
    );
    return raceTaskOutcomeForSchemas(schemas, outcome, trace.side);
  }
  if (trace.kind === "select") {
    const { leftWidth, rightWidth } = trace;
    if (!Number.isSafeInteger(leftWidth) || leftWidth < 0
        || !Number.isSafeInteger(rightWidth) || rightWidth < 0
        || schemas.length !== 3 + leftWidth + rightWidth + 2 * errorWidth) {
      throw new TypeError("task select trace does not match its compiler-derived envelope");
    }
    const childStart = trace.side === "left" ? 1 : leftWidth + 3;
    const childWidth = trace.side === "left" ? leftWidth : rightWidth;
    const outcome = materializeTrace(
      trace.winner,
      schemas.slice(childStart, childStart + childWidth),
      errorWidth,
    );
    return selectTaskOutcomeForSchemas(
      schemas,
      errorWidth,
      outcome,
      trace.side,
      trace.loserToken,
      leftWidth,
      rightWidth,
    );
  }
  throw new TypeError(`unknown task completion trace kind ${String(trace.kind)}`);
}

/// Materialize a possibly nested race/select completion only when the outer
/// compiler-generated continuation supplies the complete scalar envelope.
/// The broker records token custody and winner structure; it never guesses a
/// payload width or fills application lanes from host-authored metadata.
export function materializeTaskOutcome(pending, trace) {
  const continuation = pendingDetails.get(pending);
  if (continuation === undefined) {
    throw new TypeError("task completion requires a runtime-owned pending value");
  }
  const { delivery } = continuation;
  return materializeTrace(
    trace,
    delivery.lanes.slice(
      delivery.success.start,
      delivery.success.start + delivery.success.count,
    ),
    delivery.failure.count,
  );
}

export function createMaterializedTaskMachine(definition, wasmExports) {
  validateDefinition(definition);
  const byState = new Map(definition.continuations.map(continuation => [continuation.state, {
    ...continuation,
    handler: Object.freeze({}),
  }]));
  const frameHasBorrowedStorage = continuation => definition.step.slice(
    continuation.frame.start,
    continuation.frame.start + continuation.frame.count,
  ).some(lane => lane.kind === "borrowed_pointer");
  const frameStorage = createBorrowedFrameStorage(
    wasmExports,
    definition.continuations.some(frameHasBorrowedStorage),
  );

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
    pendingDetails.set(pending, continuation);
    const frame = Object.freeze({});
    const frameSchemas = definition.step.slice(
      continuation.frame.start,
      continuation.frame.start + continuation.frame.count,
    );
    const frameLanes = lanes.slice(
      continuation.frame.start,
      continuation.frame.start + continuation.frame.count,
    );
    frameDetails.set(frame, {
      continuation,
      lanes: frameHasBorrowedStorage(continuation)
        ? frameStorage.capture(frameLanes, frameSchemas, "task frame")
        : frameLanes,
    });
    return Object.freeze({ kind: "suspended", pending, frame });
  };

  return Object.freeze({
    inputWidth: definition.input.length,
    liftInput(input) {
      return Object.freeze(vector(input, definition.input, "task core input", decodeLane));
    },
    start(input) {
      const lanes = vector(input, definition.input, "task input", encodeLane);
      const invoke = () => decodeStep(definition.start(...lanes));
      return frameStorage === undefined ? invoke() : frameStorage.invoke(invoke);
    },
    resume(frame, outcome) {
      const saved = frameDetails.get(frame);
      if (saved === undefined) throw new TypeError("task frame is stale, forged, or already resumed");
      const delivered = outcomeDetails.get(outcome);
      if (delivered === undefined) throw new TypeError("task outcome was not constructed by this runtime");
      frameDetails.delete(frame);
      const { continuation, lanes: frameLanes } = saved;
      const delivery = continuation.delivery;
      const encoded = delivery.lanes.map(encodedZero);
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
      const allocations = [];
      try {
        const encodedFrame = frameHasBorrowedStorage(continuation)
          ? frameStorage.lower(frameLanes, frameSchemas, allocations, "task frame")
          : vector(frameLanes, frameSchemas, "task frame", encodeLane);
        const invoke = () => decodeStep(continuation.invoke(...encodedFrame, ...encoded));
        return frameStorage === undefined ? invoke() : frameStorage.invoke(invoke);
      } finally {
        frameStorage?.release(allocations);
      }
    },
  });
}
