export const CANONICAL_INTERFACE_PROTOCOL = "fe-canonical-browser-interface";
export const CANONICAL_INTERFACE_VERSION = 2;

const textEncoder = new TextEncoder();
const textDecoder = new TextDecoder("utf-8", { fatal: true });

function exactKeys(value, expected, path) {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    throw new TypeError(`${path} must be an object`);
  }
  const actual = Object.keys(value).sort();
  if (actual.join("\0") !== [...expected].sort().join("\0")) {
    throw new TypeError(`${path} has unexpected or missing fields`);
  }
}

function uint(value, path, maximum = 0xffffffff) {
  if (!Number.isSafeInteger(value) || value < 0 || value > maximum) {
    throw new TypeError(`${path} must be an unsigned integer <= ${maximum}`);
  }
  return value;
}

function checkedEnd(offset, length, memory, path) {
  memory = memoryBytes(memory);
  uint(offset, `${path} offset`);
  uint(length, `${path} length`);
  const end = offset + length;
  if (!Number.isSafeInteger(end) || end > memory.byteLength) {
    throw new RangeError(`${path} is outside canonical memory`);
  }
  return end;
}

function memoryBytes(memory) {
  if (typeof memory === "function") memory = memory();
  if (!(memory instanceof Uint8Array)) {
    throw new TypeError("canonical memory must be a Uint8Array or memory-view provider");
  }
  return memory;
}

function alignUp(value, align, path) {
  const answer = Math.ceil(value / align) * align;
  if (!Number.isSafeInteger(answer) || answer > 0xffffffff) {
    throw new TypeError(`${path} layout overflows wasm32`);
  }
  return answer;
}

function validateName(name, path, maximum = 64) {
  if (typeof name !== "string" || name.length === 0 || name.length > maximum
      || !/^[a-z][a-z0-9_]*$/.test(name)) {
    throw new TypeError(`${path} must be a lowercase ASCII identifier`);
  }
}

function validateLayout(layout, path, depth = 0, state = { nodes: 0 }) {
  if (depth > 64) throw new TypeError(`${path} exceeds maximum nesting depth`);
  state.nodes += 1;
  if (state.nodes > 4096) throw new TypeError(`${path} exceeds maximum type node count`);
  const baseKeys = ["align", "kind", "size"];
  if (!layout || typeof layout.kind !== "string") {
    throw new TypeError(`${path} must be a canonical layout`);
  }
  const primitive = {
    bool: [1, 1], u8: [1, 1], i32: [4, 4], u32: [4, 4],
    i64: [8, 8], u64: [8, 8], f32: [4, 4],
  };
  if (Object.hasOwn(primitive, layout.kind)) {
    exactKeys(layout, baseKeys, path);
    const [size, align] = primitive[layout.kind];
    if (layout.size !== size || layout.align !== align) {
      throw new TypeError(`${path} has non-canonical ${layout.kind} layout`);
    }
    return layout;
  }
  if (layout.kind === "bytes") {
    exactKeys(layout, [...baseKeys, "length_offset", "pointer_offset"], path);
    if (layout.size !== 8 || layout.align !== 4
        || layout.pointer_offset !== 0 || layout.length_offset !== 4) {
      throw new TypeError(`${path} has non-canonical bytes descriptor`);
    }
    return layout;
  }
  if (layout.kind === "string") {
    exactKeys(layout, [...baseKeys, "encoding", "length_offset", "pointer_offset"], path);
    if (layout.size !== 8 || layout.align !== 4 || layout.encoding !== "utf-8"
        || layout.pointer_offset !== 0 || layout.length_offset !== 4) {
      throw new TypeError(`${path} has non-canonical string descriptor`);
    }
    return layout;
  }
  if (layout.kind !== "record") throw new TypeError(`${path}.kind is unsupported`);
  exactKeys(layout, [...baseKeys, "fields"], path);
  if (!Array.isArray(layout.fields) || layout.fields.length === 0) {
    throw new TypeError(`${path}.fields must be a non-empty array`);
  }
  const names = new Set();
  let offset = 0;
  let recordAlign = 1;
  for (let index = 0; index < layout.fields.length; index += 1) {
    const field = layout.fields[index];
    const fieldPath = `${path}.fields[${index}]`;
    exactKeys(field, ["layout", "name", "offset"], fieldPath);
    validateName(field.name, `${fieldPath}.name`);
    if (names.has(field.name)) throw new TypeError(`${path} has duplicate field ${field.name}`);
    names.add(field.name);
    validateLayout(field.layout, `${fieldPath}.layout`, depth + 1, state);
    offset = alignUp(offset, field.layout.align, fieldPath);
    if (field.offset !== offset) throw new TypeError(`${fieldPath}.offset is non-canonical`);
    offset += field.layout.size;
    if (!Number.isSafeInteger(offset) || offset > 0xffffffff) {
      throw new TypeError(`${fieldPath} layout overflows wasm32`);
    }
    recordAlign = Math.max(recordAlign, field.layout.align);
  }
  const size = alignUp(offset, recordAlign, path);
  if (layout.align !== recordAlign || layout.size !== size) {
    throw new TypeError(`${path} has non-canonical record size or alignment`);
  }
  return layout;
}

function viewFor(memory, offset, size, path) {
  memory = memoryBytes(memory);
  checkedEnd(offset, size, memory, path);
  return new DataView(memory.buffer, memory.byteOffset + offset, size);
}

function writeDescriptor(layout, value, memory, offset, allocate, path) {
  const bytes = layout.kind === "string"
    ? (() => {
        if (typeof value !== "string") throw new TypeError(`${path} must be a string`);
        return textEncoder.encode(value);
      })()
    : (() => {
        if (!(value instanceof Uint8Array)) throw new TypeError(`${path} must be a Uint8Array`);
        return value;
      })();
  let pointer = 0;
  if (bytes.byteLength > 0) {
    if (typeof allocate !== "function") {
      throw new TypeError(`${path} requires an allocate(length, align) callback`);
    }
    pointer = uint(allocate(bytes.byteLength, 1), `${path} allocation pointer`);
    const currentMemory = memoryBytes(memory);
    checkedEnd(pointer, bytes.byteLength, currentMemory, `${path} allocation`);
    currentMemory.set(bytes, pointer);
  }
  const view = viewFor(memory, offset, layout.size, path);
  view.setUint32(layout.pointer_offset, pointer, true);
  view.setUint32(layout.length_offset, bytes.byteLength, true);
}

function writeLayout(layout, value, memory, offset, allocate, path) {
  const view = viewFor(memory, offset, layout.size, path);
  switch (layout.kind) {
    case "bool":
      if (typeof value !== "boolean") throw new TypeError(`${path} must be boolean`);
      view.setUint8(0, value ? 1 : 0); return;
    case "u8": view.setUint8(0, uint(value, path, 0xff)); return;
    case "i32":
      if (!Number.isInteger(value) || value < -0x80000000 || value > 0x7fffffff) {
        throw new TypeError(`${path} must be an i32`);
      }
      view.setInt32(0, value, true); return;
    case "u32": view.setUint32(0, uint(value, path), true); return;
    case "i64":
      if (typeof value !== "bigint" || value < -(1n << 63n) || value >= (1n << 63n)) {
        throw new TypeError(`${path} must be an i64 bigint`);
      }
      view.setBigInt64(0, value, true); return;
    case "u64":
      if (typeof value !== "bigint" || value < 0n || value >= (1n << 64n)) {
        throw new TypeError(`${path} must be a u64 bigint`);
      }
      view.setBigUint64(0, value, true); return;
    case "f32":
      if (typeof value !== "number") throw new TypeError(`${path} must be a number`);
      view.setFloat32(0, value, true); return;
    case "bytes":
    case "string": writeDescriptor(layout, value, memory, offset, allocate, path); return;
    case "record": {
      exactKeys(value, layout.fields.map((field) => field.name), path);
      for (const field of layout.fields) {
        writeLayout(field.layout, value[field.name], memory, offset + field.offset, allocate,
          `${path}.${field.name}`);
      }
      return;
    }
    default: throw new TypeError(`${path}.kind is unsupported`);
  }
}

function readLayout(layout, memory, offset, path) {
  const view = viewFor(memory, offset, layout.size, path);
  switch (layout.kind) {
    case "bool": {
      const value = view.getUint8(0);
      if (value > 1) throw new TypeError(`${path} contains invalid bool ${value}`);
      return value === 1;
    }
    case "u8": return view.getUint8(0);
    case "i32": return view.getInt32(0, true);
    case "u32": return view.getUint32(0, true);
    case "i64": return view.getBigInt64(0, true);
    case "u64": return view.getBigUint64(0, true);
    case "f32": return view.getFloat32(0, true);
    case "bytes":
    case "string": {
      const pointer = view.getUint32(layout.pointer_offset, true);
      const length = view.getUint32(layout.length_offset, true);
      const currentMemory = memoryBytes(memory);
      const end = checkedEnd(pointer, length, currentMemory, `${path} descriptor`);
      const copy = currentMemory.slice(pointer, end);
      return layout.kind === "string" ? textDecoder.decode(copy) : copy;
    }
    case "record":
      return Object.fromEntries(layout.fields.map((field) => [
        field.name,
        readLayout(field.layout, memory, offset + field.offset, `${path}.${field.name}`),
      ]));
    default: throw new TypeError(`${path}.kind is unsupported`);
  }
}

export function compileCanonicalInterfaceManifest(manifest) {
  exactKeys(manifest, ["abi", "lanes", "protocol", "version"], "canonical interface");
  if (manifest.protocol !== CANONICAL_INTERFACE_PROTOCOL) {
    throw new TypeError("unsupported canonical interface protocol");
  }
  if (manifest.version !== CANONICAL_INTERFACE_VERSION) {
    throw new TypeError("unsupported canonical interface version");
  }
  exactKeys(manifest.abi, [
    "alloc_export", "endianness", "memory_export", "pointer_width", "reset_export",
  ], "canonical interface ABI");
  if (manifest.abi.pointer_width !== 32 || manifest.abi.endianness !== "little"
      || manifest.abi.memory_export !== "memory"
      || manifest.abi.alloc_export !== "fe_cabi_alloc"
      || manifest.abi.reset_export !== "fe_cabi_reset") {
    throw new TypeError("unsupported canonical interface ABI");
  }
  if (!Array.isArray(manifest.lanes) || manifest.lanes.length === 0) {
    throw new TypeError("canonical interface lanes must be a non-empty array");
  }
  const names = new Set();
  const exports = new Set();
  const lanes = Object.create(null);
  for (let index = 0; index < manifest.lanes.length; index += 1) {
    const lane = manifest.lanes[index];
    const path = `canonical interface lanes[${index}]`;
    exactKeys(lane, ["export", "intent", "name", "request", "response"], path);
    validateName(lane.name, `${path}.name`);
    if (names.has(lane.name)) throw new TypeError(`duplicate canonical lane ${lane.name}`);
    exactKeys(lane.intent, ["capabilities", "execution", "placement"], `${path}.intent`);
    if (!["wasm", "host_effect"].includes(lane.intent.execution)) {
      throw new TypeError(`${path}.intent.execution is unsupported`);
    }
    if (!["any", "main_thread", "worker"].includes(lane.intent.placement)) {
      throw new TypeError(`${path}.intent.placement is unsupported`);
    }
    if (!Array.isArray(lane.intent.capabilities)) {
      throw new TypeError(`${path}.intent.capabilities must be an array`);
    }
    const capabilities = new Set();
    for (let capabilityIndex = 0;
      capabilityIndex < lane.intent.capabilities.length;
      capabilityIndex += 1) {
      const requirement = lane.intent.capabilities[capabilityIndex];
      const capabilityPath = `${path}.intent.capabilities[${capabilityIndex}]`;
      exactKeys(requirement, ["capability", "mutable"], capabilityPath);
      if (requirement.capability !== "webgpu_dispatch"
          || typeof requirement.mutable !== "boolean") {
        throw new TypeError(`${capabilityPath} is unsupported`);
      }
      if (capabilities.has(requirement.capability)) {
        throw new TypeError(`${path}.intent has duplicate capability ${requirement.capability}`);
      }
      capabilities.add(requirement.capability);
    }
    const isWasm = lane.intent.execution === "wasm";
    if (isWasm && (typeof lane.export !== "string" || lane.export.length === 0
        || lane.export.length > 128 || !/^[A-Za-z0-9_.-]+$/.test(lane.export))) {
      throw new TypeError(`${path}.export is invalid`);
    }
    if (!isWasm && lane.export !== null) {
      throw new TypeError(`${path}.export must be null for a host effect`);
    }
    if (!isWasm && lane.intent.placement === "any") {
      throw new TypeError(`${path}.intent host effect requires explicit placement`);
    }
    if (isWasm && ["memory", "fe_cabi_alloc", "fe_cabi_reset"].includes(lane.export)) {
      throw new TypeError(`${path}.export collides with reserved ABI export`);
    }
    if (isWasm && exports.has(lane.export)) {
      throw new TypeError(`duplicate canonical export ${lane.export}`);
    }
    names.add(lane.name);
    if (isWasm) exports.add(lane.export);
    const layoutState = { nodes: 0 };
    validateLayout(lane.request, `${path}.request`, 0, layoutState);
    validateLayout(lane.response, `${path}.response`, 0, layoutState);
    lanes[lane.name] = Object.freeze({
      export: lane.export,
      intent: Object.freeze({
        execution: lane.intent.execution,
        placement: lane.intent.placement,
        capabilities: Object.freeze(lane.intent.capabilities.map(Object.freeze)),
      }),
      request: Object.freeze({
        size: lane.request.size,
        align: lane.request.align,
        write(value, { memory, offset, allocate } = {}) {
          writeLayout(lane.request, value, memory, offset, allocate, `${lane.name} request`);
        },
        read({ memory, offset } = {}) {
          return readLayout(lane.request, memory, offset, `${lane.name} request`);
        },
      }),
      response: Object.freeze({
        size: lane.response.size,
        align: lane.response.align,
        write(value, { memory, offset, allocate } = {}) {
          writeLayout(lane.response, value, memory, offset, allocate, `${lane.name} response`);
        },
        read({ memory, offset } = {}) {
          return readLayout(lane.response, memory, offset, `${lane.name} response`);
        },
      }),
    });
  }
  return Object.freeze({ abi: Object.freeze({ ...manifest.abi }), lanes: Object.freeze(lanes) });
}

export function createCanonicalInterfaceCaller(compiled, exports) {
  if (!compiled || !compiled.abi || !compiled.lanes) {
    throw new TypeError("compiled canonical interface required");
  }
  if (!exports || typeof exports !== "object") {
    throw new TypeError("canonical Wasm exports object required");
  }
  const memory = exports[compiled.abi.memory_export];
  if (!memory || !("buffer" in memory)) {
    throw new TypeError(`missing canonical memory export ${compiled.abi.memory_export}`);
  }
  const allocate = exports[compiled.abi.alloc_export];
  const reset = exports[compiled.abi.reset_export];
  if (typeof allocate !== "function" || typeof reset !== "function") {
    throw new TypeError("missing canonical arena allocator/reset exports");
  }
  for (const lane of Object.values(compiled.lanes)) {
    if (lane.intent.execution !== "wasm") continue;
    if (typeof exports[lane.export] !== "function") {
      throw new TypeError(`missing canonical lane export ${lane.export}`);
    }
  }

  const currentMemory = () => new Uint8Array(memory.buffer);
  const arenaAllocate = (size, align) => uint(
    allocate(size, align),
    `canonical allocation (${size} bytes, align ${align})`,
  );
  let tail = Promise.resolve();

  const invoke = (laneName, value) => {
    if (!Object.hasOwn(compiled.lanes, laneName)) {
      throw new TypeError(`unknown canonical lane ${laneName}`);
    }
    const lane = compiled.lanes[laneName];
    if (lane.intent.execution !== "wasm") {
      throw new TypeError(`canonical lane ${laneName} is a host effect`);
    }
    try {
      const requestPointer = arenaAllocate(lane.request.size, lane.request.align);
      lane.request.write(value, {
        memory: currentMemory,
        offset: requestPointer,
        allocate: arenaAllocate,
      });
      const responsePointer = uint(
        exports[lane.export](requestPointer),
        `${laneName} response pointer`,
      );
      return lane.response.read({ memory: currentMemory, offset: responsePointer });
    } finally {
      reset();
    }
  };

  return Object.freeze({
    call(laneName, value) {
      const result = tail.then(() => invoke(laneName, value));
      tail = result.catch(() => {});
      return result;
    },
  });
}

const actorError = (code, message) => {
  const error = new Error(`${code}: ${message}`);
  error.name = "CanonicalActorError";
  return error;
};

function canonicalActorValidator(codec, name) {
  return (value) => {
    let cursor = codec.size;
    let memory = new Uint8Array(Math.max(64, codec.size));
    const allocate = (size, align) => {
      cursor = Math.ceil(cursor / align) * align;
      const end = cursor + size;
      if (end > memory.byteLength) {
        const grown = new Uint8Array(Math.max(end, memory.byteLength * 2));
        grown.set(memory);
        memory = grown;
      }
      const result = cursor;
      cursor = end;
      return result;
    };
    try {
      codec.write(value, { memory: () => memory, offset: 0, allocate });
    } catch {
      throw actorError("FE_ACTOR_INVALID_PAYLOAD", `${name} does not match its canonical layout`);
    }
  };
}

function canonicalTransferList(layout, value, name, output, seen) {
  switch (layout.kind) {
    case "bytes": {
      if (!(value instanceof Uint8Array)
          || !(value.buffer instanceof ArrayBuffer)
          || value.byteOffset !== 0 || value.byteLength !== value.buffer.byteLength) {
        throw actorError("FE_ACTOR_TRANSFER", `${name} bytes are not an owned full-span Uint8Array`);
      }
      if (!seen.has(value.buffer)) {
        seen.add(value.buffer);
        output.push(value.buffer);
      }
      return;
    }
    case "record":
      for (const field of layout.fields) {
        canonicalTransferList(field.layout, value[field.name], `${name}.${field.name}`, output, seen);
      }
      return;
    default:
      return;
  }
}

export function compileCanonicalActorAdapter(manifest, compiled) {
  if (!manifest || !Array.isArray(manifest.lanes) || !compiled?.lanes) {
    throw new TypeError("canonical manifest and compiled interface required");
  }
  const requestSchema = {};
  const resultSchema = {};
  const responseValidators = {};
  const responseLayouts = {};
  const intents = {};
  for (const lane of manifest.lanes) {
    const compiledLane = compiled.lanes[lane.name];
    if (!compiledLane) throw new TypeError(`missing compiled canonical lane ${lane.name}`);
    requestSchema[lane.name] = canonicalActorValidator(
      compiledLane.request, `${lane.name} request`,
    );
    const validateResponse = canonicalActorValidator(
      compiledLane.response, `${lane.name} response`,
    );
    responseValidators[lane.name] = validateResponse;
    resultSchema[lane.name] = (payload) => {
      if (!payload || typeof payload !== "object" || Array.isArray(payload)) {
        throw actorError("FE_ACTOR_INVALID_RESULT", "result payload must be an object");
      }
      if (payload.ok === true
          && Object.keys(payload).sort().join("\0") === "ok\0value") {
        validateResponse(payload.value);
        return;
      }
      if (payload.ok === false
          && typeof payload.error === "string"
          && Object.keys(payload).sort().join("\0") === "error\0ok") {
        return;
      }
      throw actorError(
        "FE_ACTOR_INVALID_RESULT", "result payload must be a canonical discriminated result",
      );
    };
    responseLayouts[lane.name] = lane.response;
    intents[lane.name] = compiledLane.intent;
  }
  return Object.freeze({
    requestSchema: Object.freeze(requestSchema),
    resultSchema: Object.freeze(resultSchema),
    responseValidators: Object.freeze(responseValidators),
    intents: Object.freeze(intents),
    transferResult(value, request) {
      const layout = responseLayouts[request?.lane];
      if (!layout) throw actorError("FE_ACTOR_UNKNOWN_LANE", "unknown canonical actor lane");
      const output = [];
      canonicalTransferList(layout, value, `${request.lane} response`, output, new Set());
      return output;
    },
  });
}

function createCanonicalDispatchAdapter(adapter, invoke, {
  maxPendingPerLane = 1,
  failureCode,
  failureDescription,
  accepts = () => true,
} = {}) {
  if (!Number.isSafeInteger(maxPendingPerLane) || maxPendingPerLane < 0) {
    throw new TypeError("maxPendingPerLane must be a non-negative safe integer");
  }
  const states = new Map();

  const run = (lane, state, entry) => {
    state.active = true;
    Promise.resolve().then(() => invoke(lane, entry.payload)).then(
      (value) => {
        try {
          adapter.responseValidators[lane](value);
          entry.resolve(value);
        } catch {
          entry.reject(actorError(
            "FE_ACTOR_INVALID_RESPONSE", `${lane} result does not match its canonical layout`,
          ));
        }
      },
      (error) => entry.reject(
        error?.name === "CanonicalActorError"
          ? error
          : actorError(failureCode, `${lane} ${failureDescription}`),
      ),
    ).finally(() => {
      const next = state.pending.shift();
      if (next) run(lane, state, next);
      else state.active = false;
    });
  };

  return Object.freeze({
    ...adapter,
    dispatch(request) {
      const lane = request?.lane;
      if (!Object.hasOwn(adapter.requestSchema, lane)) {
        return Promise.reject(actorError(
          "FE_ACTOR_UNKNOWN_LANE", "unknown canonical actor lane",
        ));
      }
      if (!accepts(lane, adapter.intents[lane])) {
        return Promise.reject(actorError(
          "FE_ACTOR_WRONG_EXECUTION", `${lane} is not owned by this adapter`,
        ));
      }
      try {
        adapter.requestSchema[lane](request.payload);
      } catch (error) {
        return Promise.reject(error);
      }
      const state = states.get(lane) ?? { active: false, pending: [] };
      states.set(lane, state);
      return new Promise((resolve, reject) => {
        const entry = { payload: request.payload, resolve, reject };
        if (!state.active) {
          run(lane, state, entry);
          return;
        }
        if (maxPendingPerLane === 0) {
          reject(actorError("FE_ACTOR_BUSY", `${lane} already has an active request`));
          return;
        }
        while (state.pending.length >= maxPendingPerLane) {
          state.pending.shift().reject(actorError(
            "FE_ACTOR_SUPERSEDED", `${lane} pending request was superseded`,
          ));
        }
        state.pending.push(entry);
      });
    },
  });
}

function canonicalRuntimePlacement(options) {
  const placement = options?.placement ?? (
    typeof WorkerGlobalScope !== "undefined" && globalThis instanceof WorkerGlobalScope
      ? "worker"
      : "main_thread"
  );
  if (!["main_thread", "worker"].includes(placement)) {
    throw new TypeError("canonical adapter placement must be main_thread or worker");
  }
  return placement;
}

export function createCanonicalActorAdapter(manifest, compiled, exports, options = {}) {
  const adapter = compileCanonicalActorAdapter(manifest, compiled);
  const caller = createCanonicalInterfaceCaller(compiled, exports);
  const placement = canonicalRuntimePlacement(options);
  return createCanonicalDispatchAdapter(
    adapter,
    (lane, payload) => caller.call(lane, payload),
    {
      ...options,
      accepts: (_lane, intent) => intent.execution === "wasm"
        && (intent.placement === "any" || intent.placement === placement),
      failureCode: "FE_ACTOR_CANONICAL_CALL",
      failureDescription: "canonical call failed",
    },
  );
}

export function createCanonicalHostEffectAdapter(
  manifest,
  compiled,
  handlers,
  options = {},
) {
  const adapter = compileCanonicalActorAdapter(manifest, compiled);
  const placement = canonicalRuntimePlacement(options);
  if (!handlers || typeof handlers !== "object" || Array.isArray(handlers)) {
    throw new TypeError("canonical host-effect handlers must be an object");
  }
  const selected = Object.create(null);
  const hostLanes = Object.entries(adapter.intents)
    .filter(([, intent]) => intent.execution === "host_effect"
      && intent.placement === placement)
    .map(([lane]) => lane);
  for (const [lane, handler] of Object.entries(handlers)) {
    if (!hostLanes.includes(lane)) {
      throw new TypeError(`unknown canonical host-effect lane ${lane}`);
    }
    if (typeof handler !== "function") {
      throw new TypeError(`canonical host-effect handler ${lane} must be a function`);
    }
    selected[lane] = handler;
  }
  const missing = hostLanes.filter((lane) => !Object.hasOwn(selected, lane));
  if (missing.length !== 0) {
    throw new TypeError(`missing canonical host-effect handlers: ${missing.join(", ")}`);
  }
  if (hostLanes.length === 0) {
    throw new TypeError("canonical interface declares no host-effect lanes");
  }
  return createCanonicalDispatchAdapter(
    adapter,
    (lane, payload) => {
      const handler = selected[lane];
      if (!handler) {
        throw actorError("FE_ACTOR_UNHANDLED_EFFECT", `${lane} has no host-effect handler`);
      }
      return Promise.resolve().then(() => handler(payload)).catch(() => {
        throw actorError("FE_ACTOR_HOST_EFFECT", `${lane} host-effect handler failed`);
      });
    },
    {
      ...options,
      accepts: (_lane, intent) => intent.execution === "host_effect"
        && intent.placement === placement,
      failureCode: "FE_ACTOR_HOST_EFFECT",
      failureDescription: "host-effect handler failed",
    },
  );
}

export const canonicalInterfaceManifest = Object.freeze({"protocol":"fe-canonical-browser-interface","version":2,"abi":{"pointer_width":32,"endianness":"little","memory_export":"memory","alloc_export":"fe_cabi_alloc","reset_export":"fe_cabi_reset"},"lanes":[{"name":"render","export":null,"request":{"size":24,"align":4,"kind":"record","fields":[{"name":"generation","offset":0,"layout":{"size":4,"align":4,"kind":"u32"}},{"name":"cam_x","offset":4,"layout":{"size":4,"align":4,"kind":"f32"}},{"name":"cam_y","offset":8,"layout":{"size":4,"align":4,"kind":"f32"}},{"name":"zoom","offset":12,"layout":{"size":4,"align":4,"kind":"f32"}},{"name":"inv_cx","offset":16,"layout":{"size":4,"align":4,"kind":"f32"}},{"name":"inv_cy","offset":20,"layout":{"size":4,"align":4,"kind":"f32"}}]},"response":{"size":1,"align":1,"kind":"record","fields":[{"name":"submitted","offset":0,"layout":{"size":1,"align":1,"kind":"bool"}}]},"intent":{"execution":"host_effect","placement":"main_thread","capabilities":[{"capability":"webgpu_dispatch","mutable":true}]}},{"name":"verify","export":null,"request":{"size":24,"align":4,"kind":"record","fields":[{"name":"generation","offset":0,"layout":{"size":4,"align":4,"kind":"u32"}},{"name":"cam_x","offset":4,"layout":{"size":4,"align":4,"kind":"f32"}},{"name":"cam_y","offset":8,"layout":{"size":4,"align":4,"kind":"f32"}},{"name":"zoom","offset":12,"layout":{"size":4,"align":4,"kind":"f32"}},{"name":"inv_cx","offset":16,"layout":{"size":4,"align":4,"kind":"f32"}},{"name":"inv_cy","offset":20,"layout":{"size":4,"align":4,"kind":"f32"}}]},"response":{"size":8,"align":4,"kind":"bytes","pointer_offset":0,"length_offset":4},"intent":{"execution":"host_effect","placement":"main_thread","capabilities":[{"capability":"webgpu_dispatch","mutable":true}]}},{"name":"oracle","export":null,"request":{"size":24,"align":4,"kind":"record","fields":[{"name":"generation","offset":0,"layout":{"size":4,"align":4,"kind":"u32"}},{"name":"cam_x","offset":4,"layout":{"size":4,"align":4,"kind":"f32"}},{"name":"cam_y","offset":8,"layout":{"size":4,"align":4,"kind":"f32"}},{"name":"zoom","offset":12,"layout":{"size":4,"align":4,"kind":"f32"}},{"name":"inv_cx","offset":16,"layout":{"size":4,"align":4,"kind":"f32"}},{"name":"inv_cy","offset":20,"layout":{"size":4,"align":4,"kind":"f32"}}]},"response":{"size":8,"align":4,"kind":"bytes","pointer_offset":0,"length_offset":4},"intent":{"execution":"host_effect","placement":"worker","capabilities":[]}},{"name":"oracle_pixel","export":"fe_cabi_oracle_pixel","request":{"size":28,"align":4,"kind":"record","fields":[{"name":"x","offset":0,"layout":{"size":4,"align":4,"kind":"i32"}},{"name":"y","offset":4,"layout":{"size":4,"align":4,"kind":"i32"}},{"name":"cam_x","offset":8,"layout":{"size":4,"align":4,"kind":"f32"}},{"name":"cam_y","offset":12,"layout":{"size":4,"align":4,"kind":"f32"}},{"name":"zoom","offset":16,"layout":{"size":4,"align":4,"kind":"f32"}},{"name":"inv_cx","offset":20,"layout":{"size":4,"align":4,"kind":"f32"}},{"name":"inv_cy","offset":24,"layout":{"size":4,"align":4,"kind":"f32"}}]},"response":{"size":4,"align":4,"kind":"record","fields":[{"name":"rgba","offset":0,"layout":{"size":4,"align":4,"kind":"u32"}}]},"intent":{"execution":"wasm","placement":"any","capabilities":[]}}]});
export const compiledCanonicalInterface = compileCanonicalInterfaceManifest(canonicalInterfaceManifest);
export function createInterfaceCaller(exports) {
  return createCanonicalInterfaceCaller(compiledCanonicalInterface, exports);
}
export function compileActorAdapter() {
  return compileCanonicalActorAdapter(canonicalInterfaceManifest, compiledCanonicalInterface);
}
export function createActorAdapter(exports, options) {
  return createCanonicalActorAdapter(canonicalInterfaceManifest, compiledCanonicalInterface, exports, options);
}
export function createHostEffectAdapter(handlers, options) {
  return createCanonicalHostEffectAdapter(canonicalInterfaceManifest, compiledCanonicalInterface, handlers, options);
}
