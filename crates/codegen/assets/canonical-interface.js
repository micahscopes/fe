export const CANONICAL_INTERFACE_PROTOCOL = "fe-canonical-browser-interface";
export const CANONICAL_INTERFACE_VERSION = 4;

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
  if (layout.kind === "list") {
    exactKeys(layout, [
      ...baseKeys, "element", "length_offset", "max", "pointer_offset", "stride",
    ], path);
    if (layout.size !== 8 || layout.align !== 4
        || !["u32", "f32"].includes(layout.element)
        || layout.pointer_offset !== 0 || layout.length_offset !== 4
        || layout.stride !== 4 || !Number.isSafeInteger(layout.max)
        || layout.max < 0 || layout.max > Math.floor(0xffffffff / layout.stride)) {
      throw new TypeError(`${path} has non-canonical bounded list descriptor`);
    }
    return layout;
  }
  if (layout.kind === "variant") {
    exactKeys(layout, [...baseKeys, "tag_offset", "variants"], path);
    if (layout.tag_offset !== 0 || layout.align < 4 || layout.size < 4
        || !Number.isSafeInteger(layout.size) || !Number.isSafeInteger(layout.align)
        || (layout.align & (layout.align - 1)) !== 0
        || layout.size % layout.align !== 0) {
      throw new TypeError(`${path} has non-canonical variant envelope`);
    }
    if (!Array.isArray(layout.variants) || layout.variants.length === 0) {
      throw new TypeError(`${path}.variants must be a non-empty array`);
    }
    const names = new Set();
    let variantAlign = 4;
    let variantSize = 4;
    for (let index = 0; index < layout.variants.length; index += 1) {
      state.nodes += 1;
      if (state.nodes > 4096) {
        throw new TypeError(`${path} exceeds maximum type node count`);
      }
      const variant = layout.variants[index];
      const variantPath = `${path}.variants[${index}]`;
      exactKeys(variant, ["fields", "name", "tag"], variantPath);
      validateName(variant.name, `${variantPath}.name`);
      if (names.has(variant.name)) throw new TypeError(`${path} has duplicate variant ${variant.name}`);
      names.add(variant.name);
      if (variant.tag !== index) throw new TypeError(`${variantPath}.tag is non-canonical`);
      if (!Array.isArray(variant.fields)) throw new TypeError(`${variantPath}.fields must be an array`);
      const fieldNames = new Set();
      let offset = 4;
      for (let fieldIndex = 0; fieldIndex < variant.fields.length; fieldIndex += 1) {
        const field = variant.fields[fieldIndex];
        const fieldPath = `${variantPath}.fields[${fieldIndex}]`;
        exactKeys(field, ["layout", "name", "offset"], fieldPath);
        validateName(field.name, `${fieldPath}.name`);
        if (field.name === "tag" || fieldNames.has(field.name)) {
          throw new TypeError(`${variantPath} has reserved or duplicate field ${field.name}`);
        }
        fieldNames.add(field.name);
        validateLayout(field.layout, `${fieldPath}.layout`, depth + 1, state);
        offset = alignUp(offset, field.layout.align, fieldPath);
        if (field.offset !== offset) throw new TypeError(`${fieldPath}.offset is non-canonical`);
        offset += field.layout.size;
        if (offset > layout.size) throw new TypeError(`${fieldPath} exceeds variant envelope`);
        variantAlign = Math.max(variantAlign, field.layout.align);
      }
      variantSize = Math.max(variantSize, offset);
    }
    variantSize = alignUp(variantSize, variantAlign, path);
    if (layout.align !== variantAlign || layout.size !== variantSize) {
      throw new TypeError(`${path} has non-canonical variant size or alignment`);
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

function listClass(layout) {
  return layout.element === "u32" ? Uint32Array : Float32Array;
}

function writeList(layout, value, memory, offset, allocate, path) {
  const Expected = listClass(layout);
  if (!(value instanceof Expected)) {
    throw new TypeError(`${path} must be a ${Expected.name}`);
  }
  if (value.length > layout.max) {
    throw new RangeError(`${path} exceeds maximum length ${layout.max}`);
  }
  const byteLength = value.length * layout.stride;
  let pointer = 0;
  if (byteLength > 0) {
    if (typeof allocate !== "function") {
      throw new TypeError(`${path} requires an allocate(length, align) callback`);
    }
    pointer = uint(allocate(byteLength, layout.stride), `${path} allocation pointer`);
    if (pointer % layout.stride !== 0) {
      throw new RangeError(`${path} allocation pointer is misaligned`);
    }
    const bytes = memoryBytes(memory);
    checkedEnd(pointer, byteLength, bytes, `${path} allocation`);
    const payload = new DataView(bytes.buffer, bytes.byteOffset + pointer, byteLength);
    for (let index = 0; index < value.length; index += 1) {
      if (layout.element === "u32") payload.setUint32(index * 4, value[index], true);
      else payload.setFloat32(index * 4, value[index], true);
    }
  }
  const descriptor = viewFor(memory, offset, layout.size, path);
  descriptor.setUint32(layout.pointer_offset, pointer, true);
  descriptor.setUint32(layout.length_offset, value.length, true);
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
    case "list": writeList(layout, value, memory, offset, allocate, path); return;
    case "record": {
      exactKeys(value, layout.fields.map((field) => field.name), path);
      for (const field of layout.fields) {
        writeLayout(field.layout, value[field.name], memory, offset + field.offset, allocate,
          `${path}.${field.name}`);
      }
      return;
    }
    case "variant": {
      const variant = layout.variants.find((candidate) => candidate.name === value?.tag);
      if (!variant) throw new TypeError(`${path}.tag is not a known variant`);
      exactKeys(value, ["tag", ...variant.fields.map((field) => field.name)], path);
      view.setUint32(layout.tag_offset, variant.tag, true);
      // Canonicalize inactive payload bytes so equal values have one wire image
      // and stale arena data cannot cross the boundary.
      memoryBytes(memory).fill(0, offset + 4, offset + layout.size);
      for (const field of variant.fields) {
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
    case "list": {
      const pointer = view.getUint32(layout.pointer_offset, true);
      const length = view.getUint32(layout.length_offset, true);
      if (length > layout.max) {
        throw new RangeError(`${path} exceeds maximum length ${layout.max}`);
      }
      if (length === 0) return new (listClass(layout))(0);
      if (pointer % layout.stride !== 0) {
        throw new RangeError(`${path} descriptor pointer is misaligned`);
      }
      const byteLength = length * layout.stride;
      const bytes = memoryBytes(memory);
      checkedEnd(pointer, byteLength, bytes, `${path} descriptor`);
      const payload = new DataView(bytes.buffer, bytes.byteOffset + pointer, byteLength);
      const result = new (listClass(layout))(length);
      for (let index = 0; index < length; index += 1) {
        result[index] = layout.element === "u32"
          ? payload.getUint32(index * 4, true)
          : payload.getFloat32(index * 4, true);
      }
      return result;
    }
    case "record":
      return Object.fromEntries(layout.fields.map((field) => [
        field.name,
        readLayout(field.layout, memory, offset + field.offset, `${path}.${field.name}`),
      ]));
    case "variant": {
      const tag = view.getUint32(layout.tag_offset, true);
      const variant = layout.variants[tag];
      if (!variant || variant.tag !== tag) {
        throw new TypeError(`${path} contains invalid variant tag ${tag}`);
      }
      return Object.fromEntries([
        ["tag", variant.name],
        ...variant.fields.map((field) => [
          field.name,
          readLayout(field.layout, memory, offset + field.offset, `${path}.${field.name}`),
        ]),
      ]);
    }
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

function mailboxValueWidth(layout, path) {
  switch (layout.kind) {
    case "bool":
    case "u8":
    case "i32":
    case "u32":
    case "i64":
    case "u64":
    case "f32": return 1;
    case "bytes":
    case "string":
    case "list": return 2;
    case "record": return layout.fields.reduce(
      (width, field) => width + mailboxValueWidth(field.layout, `${path}.${field.name}`),
      0,
    );
    case "variant": return 1 + layout.variants.reduce(
      (width, variant) => width + variant.fields.reduce(
        (variantWidth, field) => variantWidth
          + mailboxValueWidth(field.layout, `${path}.${variant.name}.${field.name}`),
        0,
      ),
      0,
    );
    default:
      throw new TypeError(`${path} is not a canonical mailbox value`);
  }
}

function mailboxHasDescriptors(layout) {
  if (layout.kind === "bytes" || layout.kind === "string" || layout.kind === "list") {
    return true;
  }
  if (layout.kind === "record") {
    return layout.fields.some(field => mailboxHasDescriptors(field.layout));
  }
  if (layout.kind === "variant") {
    return layout.variants.some(variant =>
      variant.fields.some(field => mailboxHasDescriptors(field.layout)));
  }
  return false;
}

function mailboxCarrier(carriers, state, path) {
  if (state.index >= carriers.length) throw new TypeError(`${path} has no Wasm carrier`);
  return carriers[state.index++];
}

function mailboxU32Carrier(carriers, state, path) {
  const raw = mailboxCarrier(carriers, state, path);
  if (!Number.isInteger(raw)) throw new TypeError(`${path} is not a Fe u32`);
  return raw >>> 0;
}

function mailboxMemory(binding, path) {
  if (!(binding.memory instanceof WebAssembly.Memory)) {
    throw new TypeError(`${path} requires an attached parent task Wasm memory`);
  }
  return new Uint8Array(binding.memory.buffer);
}

function liftMailboxDescriptor(layout, carriers, state, binding, path) {
  const pointer = mailboxU32Carrier(carriers, state, `${path}.ptr`);
  const length = mailboxU32Carrier(carriers, state, `${path}.len`);
  const memory = mailboxMemory(binding, path);
  if (layout.kind === "list") {
    if (length > layout.max) {
      throw new RangeError(`${path} exceeds maximum length ${layout.max}`);
    }
    if (length !== 0 && pointer % layout.stride !== 0) {
      throw new RangeError(`${path} descriptor pointer is misaligned`);
    }
    const byteLength = length * layout.stride;
    checkedEnd(pointer, byteLength, memory, `${path} descriptor`);
    const result = new (listClass(layout))(length);
    const payload = new DataView(
      memory.buffer, memory.byteOffset + pointer, byteLength,
    );
    for (let index = 0; index < length; index += 1) {
      result[index] = layout.element === "u32"
        ? payload.getUint32(index * 4, true)
        : payload.getFloat32(index * 4, true);
    }
    return result;
  }
  const end = checkedEnd(pointer, length, memory, `${path} descriptor`);
  const copy = memory.slice(pointer, end);
  return layout.kind === "string" ? textDecoder.decode(copy) : copy;
}

function liftMailboxValue(layout, carriers, state, binding, path) {
  if (layout.kind === "record") {
    return Object.fromEntries(layout.fields.map((field) => [
      field.name,
      liftMailboxValue(field.layout, carriers, state, binding, `${path}.${field.name}`),
    ]));
  }
  if (layout.kind === "variant") {
    if (state.index >= carriers.length) throw new TypeError(`${path}.tag has no Wasm carrier`);
    const rawTag = carriers[state.index++];
    if (!Number.isInteger(rawTag) || rawTag < 0 || rawTag >= layout.variants.length) {
      throw new TypeError(`${path}.tag is not a known variant`);
    }
    const active = layout.variants[rawTag];
    const value = { tag: active.name };
    for (const variant of layout.variants) {
      for (const field of variant.fields) {
        const fieldPath = `${path}.${variant.name}.${field.name}`;
        if (variant.tag === rawTag) {
          value[field.name] = liftMailboxValue(
            field.layout, carriers, state, binding, fieldPath,
          );
        } else {
          consumeMailboxZeros(field.layout, carriers, state, binding, fieldPath);
        }
      }
    }
    return value;
  }
  if (layout.kind === "bytes" || layout.kind === "string" || layout.kind === "list") {
    return liftMailboxDescriptor(layout, carriers, state, binding, path);
  }
  const raw = mailboxCarrier(carriers, state, path);
  switch (layout.kind) {
    case "bool":
      if (raw !== 0 && raw !== 1) throw new TypeError(`${path} is not a Fe bool`);
      return raw === 1;
    case "u8": {
      if (!Number.isInteger(raw) || (raw >>> 0) > 0xff) {
        throw new TypeError(`${path} is not a Fe u8`);
      }
      return raw >>> 0;
    }
    case "i32":
      if (!Number.isInteger(raw)) throw new TypeError(`${path} is not a Fe i32`);
      return raw | 0;
    case "u32":
      if (!Number.isInteger(raw)) throw new TypeError(`${path} is not a Fe u32`);
      return raw >>> 0;
    case "i64":
      if (typeof raw !== "bigint") throw new TypeError(`${path} is not a Fe i64`);
      return BigInt.asIntN(64, raw);
    case "u64":
      if (typeof raw !== "bigint") throw new TypeError(`${path} is not a Fe u64`);
      return BigInt.asUintN(64, raw);
    case "f32":
      if (typeof raw !== "number") throw new TypeError(`${path} is not a Fe f32`);
      return Math.fround(raw);
    default:
      throw new TypeError(`${path} is not a canonical mailbox value`);
  }
}

function consumeMailboxZeros(layout, carriers, state, binding, path) {
  if (layout.kind === "record") {
    for (const field of layout.fields) {
      consumeMailboxZeros(field.layout, carriers, state, binding, `${path}.${field.name}`);
    }
    return;
  }
  if (layout.kind === "variant") {
    if (state.index >= carriers.length) throw new TypeError(`${path}.tag has no Wasm carrier`);
    const tag = carriers[state.index++];
    if (tag !== 0) throw new TypeError(`${path}.tag inactive lane is not canonical zero`);
    for (const variant of layout.variants) {
      for (const field of variant.fields) {
        consumeMailboxZeros(
          field.layout,
          carriers,
          state,
          binding,
          `${path}.${variant.name}.${field.name}`,
        );
      }
    }
    return;
  }
  if (layout.kind === "bytes" || layout.kind === "string" || layout.kind === "list") {
    const pointer = mailboxU32Carrier(carriers, state, `${path}.ptr`);
    const length = mailboxU32Carrier(carriers, state, `${path}.len`);
    if (pointer !== 0 || length !== 0) {
      throw new TypeError(`${path} inactive descriptor is not canonical zero`);
    }
    return;
  }
  const value = liftMailboxValue(layout, carriers, state, binding, path);
  if (value !== false && value !== 0 && value !== 0n) {
    throw new TypeError(`${path} inactive lane is not canonical zero`);
  }
  if (typeof value === "number" && Object.is(value, -0)) {
    throw new TypeError(`${path} inactive lane is not canonical positive zero`);
  }
}

function appendMailboxZeros(layout, output) {
  if (layout.kind === "record") {
    for (const field of layout.fields) appendMailboxZeros(field.layout, output);
    return;
  }
  if (layout.kind === "variant") {
    output.push(0);
    for (const variant of layout.variants) {
      for (const field of variant.fields) appendMailboxZeros(field.layout, output);
    }
    return;
  }
  if (layout.kind === "bytes" || layout.kind === "string" || layout.kind === "list") {
    output.push(0, 0);
    return;
  }
  output.push(layout.kind === "i64" || layout.kind === "u64" ? 0n : 0);
}

function allocateMailboxPayload(binding, byteLength, align, allocations, path) {
  if (byteLength === 0) return 0;
  if (typeof binding.realloc !== "function" || typeof binding.postReturn !== "function") {
    throw new TypeError(`${path} requires an attached canonical allocation stack`);
  }
  const rawPointer = binding.realloc(0, 0, align, byteLength);
  if (!Number.isInteger(rawPointer)) {
    throw new TypeError(`${path} canonical allocator returned a non-integer pointer`);
  }
  const pointer = rawPointer >>> 0;
  allocations.push(Object.freeze({ align, pointer, size: byteLength }));
  checkedEnd(pointer, byteLength, mailboxMemory(binding, path), `${path} allocation`);
  return pointer;
}

function lowerMailboxDescriptor(layout, value, output, binding, allocations, path) {
  if (layout.kind === "list") {
    const Expected = listClass(layout);
    if (!(value instanceof Expected)) {
      throw new TypeError(`${path} must be a ${Expected.name}`);
    }
    if (value.length > layout.max) {
      throw new RangeError(`${path} exceeds maximum length ${layout.max}`);
    }
    const byteLength = value.length * layout.stride;
    const pointer = allocateMailboxPayload(
      binding, byteLength, layout.stride, allocations, path,
    );
    if (byteLength !== 0) {
      const memory = mailboxMemory(binding, path);
      const payload = new DataView(
        memory.buffer, memory.byteOffset + pointer, byteLength,
      );
      for (let index = 0; index < value.length; index += 1) {
        if (layout.element === "u32") payload.setUint32(index * 4, value[index], true);
        else payload.setFloat32(index * 4, value[index], true);
      }
    }
    output.push(pointer, value.length);
    return;
  }
  const bytes = layout.kind === "string"
    ? (() => {
        if (typeof value !== "string") throw new TypeError(`${path} must be a string`);
        return textEncoder.encode(value);
      })()
    : (() => {
        if (!(value instanceof Uint8Array)) {
          throw new TypeError(`${path} must be a Uint8Array`);
        }
        return value;
      })();
  const pointer = allocateMailboxPayload(binding, bytes.byteLength, 1, allocations, path);
  if (bytes.byteLength !== 0) mailboxMemory(binding, path).set(bytes, pointer);
  output.push(pointer, bytes.byteLength);
}

function lowerMailboxValue(layout, value, output, binding, allocations, path) {
  if (layout.kind === "record") {
    exactKeys(value, layout.fields.map((field) => field.name), path);
    for (const field of layout.fields) {
      lowerMailboxValue(
        field.layout, value[field.name], output, binding, allocations, `${path}.${field.name}`,
      );
    }
    return;
  }
  if (layout.kind === "variant") {
    const active = layout.variants.find((variant) => variant.name === value?.tag);
    if (!active) throw new TypeError(`${path}.tag is not a known variant`);
    exactKeys(value, ["tag", ...active.fields.map((field) => field.name)], path);
    output.push(active.tag);
    for (const variant of layout.variants) {
      for (const field of variant.fields) {
        if (variant.tag === active.tag) {
          lowerMailboxValue(
            field.layout,
            value[field.name],
            output,
            binding,
            allocations,
            `${path}.${variant.name}.${field.name}`,
          );
        } else {
          appendMailboxZeros(field.layout, output);
        }
      }
    }
    return;
  }
  if (layout.kind === "bytes" || layout.kind === "string" || layout.kind === "list") {
    lowerMailboxDescriptor(layout, value, output, binding, allocations, path);
    return;
  }
  switch (layout.kind) {
    case "bool":
      if (typeof value !== "boolean") throw new TypeError(`${path} must be boolean`);
      output.push(value ? 1 : 0); return;
    case "u8": output.push(uint(value, path, 0xff)); return;
    case "i32":
      if (!Number.isInteger(value) || value < -0x80000000 || value > 0x7fffffff) {
        throw new TypeError(`${path} must be an i32`);
      }
      output.push(value); return;
    case "u32": output.push(uint(value, path)); return;
    case "i64":
      if (typeof value !== "bigint" || value < -(1n << 63n) || value >= (1n << 63n)) {
        throw new TypeError(`${path} must be an i64 bigint`);
      }
      output.push(value); return;
    case "u64":
      if (typeof value !== "bigint" || value < 0n || value >= (1n << 64n)) {
        throw new TypeError(`${path} must be a u64 bigint`);
      }
      output.push(value); return;
    case "f32":
      if (typeof value !== "number") throw new TypeError(`${path} must be a number`);
      output.push(Math.fround(value)); return;
    default:
      throw new TypeError(`${path} is not a canonical mailbox value`);
  }
}

function createMailboxResponseSession(layout, width, binding, path) {
  const allocations = [];
  let lowered = false;
  let released = false;
  return Object.freeze({
    lower(value) {
      if (lowered) throw new TypeError(`${path} was lowered more than once`);
      lowered = true;
      const output = [];
      lowerMailboxValue(layout, value, output, binding, allocations, path);
      if (output.length !== width) {
        throw new TypeError(`${path} canonical width drifted`);
      }
      return output;
    },
    release() {
      if (released) throw new TypeError(`${path} allocations were released more than once`);
      released = true;
      while (allocations.length !== 0) {
        const allocation = allocations.pop();
        binding.postReturn(allocation.pointer, allocation.size, allocation.align);
      }
    },
  });
}

// Compile the parent-Wasm to canonical child-value bridge from the same
// interface which owns the child Wasm ABI. No request name, field list,
// response width, memory layout, or lane selector is supplied by application
// JavaScript.
export function compileCanonicalActorMailbox(manifest) {
  if (!manifest || !Array.isArray(manifest.lanes)) {
    throw new TypeError("canonical actor interface required");
  }
  const lanes = Object.create(null);
  const binding = {
    memory: undefined,
    postReturn: undefined,
    realloc: undefined,
    exports: undefined,
  };
  let needsMemory = false;
  let needsAllocation = false;
  for (const lane of manifest.lanes) {
    if (Object.hasOwn(lanes, lane.name)) {
      throw new TypeError(`duplicate canonical mailbox lane ${lane.name}`);
    }
    const requestWidth = mailboxValueWidth(lane.request, `${lane.name} request`);
    const responseWidth = mailboxValueWidth(lane.response, `${lane.name} response`);
    needsMemory ||= mailboxHasDescriptors(lane.request);
    needsMemory ||= mailboxHasDescriptors(lane.response);
    needsAllocation ||= mailboxHasDescriptors(lane.response);
    lanes[lane.name] = Object.freeze({
      requestWidth,
      responseWidth,
      liftRequest(carriers) {
        if (!Array.isArray(carriers) || carriers.length !== requestWidth) {
          throw new TypeError(
            `${lane.name} request must contain exactly ${requestWidth} Wasm carriers`,
          );
        }
        const state = { index: 0 };
        const value = liftMailboxValue(
          lane.request, carriers, state, binding, `${lane.name} request`,
        );
        if (state.index !== carriers.length) {
          throw new TypeError(`${lane.name} request left unconsumed Wasm carriers`);
        }
        return value;
      },
      createResponseSession() {
        return createMailboxResponseSession(
          lane.response, responseWidth, binding, `${lane.name} response`,
        );
      },
    });
  }
  Object.defineProperty(lanes, "attach", {
    enumerable: false,
    value(exports) {
      if (!exports || typeof exports !== "object" || Array.isArray(exports)) {
        throw new TypeError("canonical mailbox attachment requires Wasm exports");
      }
      if (binding.exports !== undefined && binding.exports !== exports) {
        throw new TypeError("canonical mailbox cannot be rebound to another Wasm instance");
      }
      if (needsMemory && !(exports.memory instanceof WebAssembly.Memory)) {
        throw new TypeError("canonical mailbox parent task exports no Wasm memory");
      }
      const canonicalRealloc = exports.cabi_realloc;
      const arenaAlloc = exports.fe_cabi_alloc;
      const realloc = typeof canonicalRealloc === "function"
        ? canonicalRealloc
        : typeof arenaAlloc === "function"
          ? (_oldPointer, _oldSize, align, size) => arenaAlloc(size, align)
          : undefined;
      if (needsAllocation && typeof realloc !== "function") {
        throw new TypeError("canonical mailbox parent task exports no canonical allocator");
      }
      if (needsAllocation && typeof exports.fe_cabi_post_return !== "function") {
        throw new TypeError("canonical mailbox parent task exports no post-return stack release");
      }
      binding.exports = exports;
      binding.memory = exports.memory;
      binding.realloc = realloc;
      binding.postReturn = exports.fe_cabi_post_return;
    },
  });
  return Object.freeze(lanes);
}

function canonicalTransferList(layout, value, name, output, seen) {
  switch (layout.kind) {
    case "bytes": {
      if (!(value instanceof Uint8Array)
          || !(value.buffer instanceof ArrayBuffer)
          || value.byteOffset !== 0 || value.byteLength !== value.buffer.byteLength) {
        throw actorError("FE_ACTOR_TRANSFER", `${name} bytes are not an owned full-span Uint8Array`);
      }
      const prior = seen.get(value.buffer);
      if (prior !== undefined && prior !== "bytes") {
        throw actorError(
          "FE_ACTOR_TRANSFER",
          `${name} aliases a buffer through incompatible canonical transfer layouts`,
        );
      }
      if (prior === undefined) {
        seen.set(value.buffer, "bytes");
        output.push(value.buffer);
      }
      return;
    }
    case "list": {
      const Expected = listClass(layout);
      if (!(value instanceof Expected)
          || !(value.buffer instanceof ArrayBuffer)
          || value.byteOffset !== 0 || value.byteLength !== value.buffer.byteLength) {
        throw actorError(
          "FE_ACTOR_TRANSFER", `${name} is not an owned full-span ${Expected.name}`,
        );
      }
      if (value.length > layout.max) {
        throw actorError("FE_ACTOR_TRANSFER", `${name} exceeds maximum length ${layout.max}`);
      }
      const signature = `list:${layout.element}:${layout.max}`;
      const prior = seen.get(value.buffer);
      if (prior !== undefined && prior !== signature) {
        throw actorError(
          "FE_ACTOR_TRANSFER",
          `${name} aliases a buffer through incompatible canonical transfer layouts`,
        );
      }
      if (prior === undefined) {
        seen.set(value.buffer, signature);
        output.push(value.buffer);
      }
      return;
    }
    case "record":
      for (const field of layout.fields) {
        canonicalTransferList(field.layout, value[field.name], `${name}.${field.name}`, output, seen);
      }
      return;
    case "variant": {
      const variant = layout.variants.find((candidate) => candidate.name === value?.tag);
      if (!variant) {
        throw actorError("FE_ACTOR_TRANSFER", `${name}.tag is not a known variant`);
      }
      for (const field of variant.fields) {
        canonicalTransferList(
          field.layout, value[field.name], `${name}.${field.name}`, output, seen,
        );
      }
      return;
    }
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
  const requestLayouts = {};
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
    requestLayouts[lane.name] = lane.request;
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
      canonicalTransferList(layout, value, `${request.lane} response`, output, new Map());
      return output;
    },
    transferRequest(value, request) {
      const layout = requestLayouts[request?.lane];
      if (!layout) throw actorError("FE_ACTOR_UNKNOWN_LANE", "unknown canonical actor lane");
      const output = [];
      canonicalTransferList(layout, value, `${request.lane} request`, output, new Map());
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

  const finishEntry = (entry) => {
    entry.signal?.removeEventListener("abort", entry.onAbort);
  };
  const run = (lane, state, entry) => {
    if (entry.cancelled) {
      const next = state.pending.shift();
      if (next) run(lane, state, next);
      else state.active = false;
      return;
    }
    state.active = true;
    entry.active = true;
    Promise.resolve().then(() => invoke(lane, entry.payload, {
      signal: entry.signal,
    })).then(
      (value) => {
        if (entry.cancelled) return;
        try {
          adapter.responseValidators[lane](value);
          entry.resolve(value);
        } catch {
          entry.reject(actorError(
            "FE_ACTOR_INVALID_RESPONSE", `${lane} result does not match its canonical layout`,
          ));
        }
      },
      (error) => {
        if (!entry.cancelled) {
          entry.reject(
            error?.name === "CanonicalActorError"
              ? error
              : actorError(failureCode, `${lane} ${failureDescription}`),
          );
        }
      },
    ).finally(() => {
      finishEntry(entry);
      const next = state.pending.shift();
      if (next) run(lane, state, next);
      else state.active = false;
    });
  };

  return Object.freeze({
    ...adapter,
    dispatch(request, { signal } = {}) {
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
      if (signal !== undefined
          && (!signal || typeof signal.aborted !== "boolean"
            || typeof signal.addEventListener !== "function"
            || typeof signal.removeEventListener !== "function")) {
        return Promise.reject(new TypeError("canonical dispatch signal must be an AbortSignal"));
      }
      if (signal?.aborted) {
        return Promise.reject(actorError("FE_ACTOR_ABORTED", `${lane} request was aborted`));
      }
      const state = states.get(lane) ?? { active: false, pending: [] };
      states.set(lane, state);
      return new Promise((resolve, reject) => {
        const entry = {
          payload: request.payload,
          resolve,
          reject,
          signal,
          active: false,
          cancelled: false,
          onAbort: null,
        };
        entry.onAbort = () => {
          if (entry.cancelled) return;
          entry.cancelled = true;
          if (!entry.active) {
            const index = state.pending.indexOf(entry);
            if (index >= 0) state.pending.splice(index, 1);
            finishEntry(entry);
          }
          reject(actorError("FE_ACTOR_ABORTED", `${lane} request was aborted`));
        };
        signal?.addEventListener("abort", entry.onAbort, { once: true });
        if (!state.active) {
          run(lane, state, entry);
          return;
        }
        if (maxPendingPerLane === 0) {
          finishEntry(entry);
          reject(actorError("FE_ACTOR_BUSY", `${lane} already has an active request`));
          return;
        }
        while (state.pending.length >= maxPendingPerLane) {
          const superseded = state.pending.shift();
          finishEntry(superseded);
          superseded.reject(actorError(
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
  if (!handlers || typeof handlers !== "object" || Array.isArray(handlers)) {
    throw new TypeError("canonical host-effect handlers must be an object");
  }
  const handlerLanes = Object.keys(handlers);
  for (const lane of handlerLanes) {
    if (adapter.intents[lane]?.execution !== "host_effect") {
      throw new TypeError(`unknown canonical host-effect lane ${lane}`);
    }
  }
  const declaredPlacements = new Set(
    handlerLanes.map((lane) => adapter.intents[lane].placement),
  );
  let placement;
  if (options?.placement !== undefined) {
    placement = canonicalRuntimePlacement(options);
    if ([...declaredPlacements].some((declared) => declared !== placement)) {
      throw new TypeError("canonical host-effect handlers disagree with explicit placement");
    }
  } else {
    if (declaredPlacements.size !== 1) {
      throw new TypeError(
        "canonical host-effect handlers must declare one shared placement",
      );
    }
    [placement] = declaredPlacements;
  }
  const selected = Object.create(null);
  const hostLanes = Object.entries(adapter.intents)
    .filter(([, intent]) => intent.execution === "host_effect"
      && intent.placement === placement)
    .map(([lane]) => lane);
  for (const [lane, handler] of Object.entries(handlers)) {
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
    (lane, payload, context) => {
      const handler = selected[lane];
      if (!handler) {
        throw actorError("FE_ACTOR_UNHANDLED_EFFECT", `${lane} has no host-effect handler`);
      }
      return Promise.resolve().then(() => handler(payload, context)).catch((error) => {
        if (error?.name === "CanonicalActorError") throw error;
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
