export const CANONICAL_INTERFACE_PROTOCOL = "fe-canonical-browser-interface";
export const CANONICAL_INTERFACE_VERSION = 1;

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
    exactKeys(lane, ["export", "name", "request", "response"], path);
    validateName(lane.name, `${path}.name`);
    if (names.has(lane.name)) throw new TypeError(`duplicate canonical lane ${lane.name}`);
    if (typeof lane.export !== "string" || lane.export.length === 0 || lane.export.length > 128
        || !/^[A-Za-z0-9_.-]+$/.test(lane.export)) {
      throw new TypeError(`${path}.export is invalid`);
    }
    if (["memory", "fe_cabi_alloc", "fe_cabi_reset"].includes(lane.export)) {
      throw new TypeError(`${path}.export collides with reserved ABI export`);
    }
    if (exports.has(lane.export)) throw new TypeError(`duplicate canonical export ${lane.export}`);
    names.add(lane.name); exports.add(lane.export);
    const layoutState = { nodes: 0 };
    validateLayout(lane.request, `${path}.request`, 0, layoutState);
    validateLayout(lane.response, `${path}.response`, 0, layoutState);
    lanes[lane.name] = Object.freeze({
      export: lane.export,
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
