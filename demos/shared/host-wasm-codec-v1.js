const CONTRACT = "fe:host-wasm-codec/v1";

export function createHostWasmCodecSession({
  plan,
  memory,
  realloc,
  postReturn,
  lowerHandle,
}) {
  if (plan?.contract !== CONTRACT || plan?.abi !== "fe-host-wasm" || plan?.abi_version !== 1) {
    throw new TypeError(`unsupported Fe host codec contract`);
  }
  if (!(memory instanceof WebAssembly.Memory)) {
    throw new TypeError(`codec requires WebAssembly.Memory`);
  }
  if (plan?.function?.requirements?.includes("realloc") && typeof realloc !== "function") {
    throw new TypeError(`codec requires cabi_realloc`);
  }
  const requirements = new Set(plan.function.requirements);
  for (const unsupported of ["callback_table", "future_table"]) {
    if (requirements.has(unsupported)) {
      throw new TypeError(`${unsupported} execution is not implemented by codec v1`);
    }
  }
  if (requirements.has("post_return") && typeof postReturn !== "function") {
    throw new TypeError(`codec plan requires a post-return surface`);
  }

  const cleanups = new Map();
  const view = () => new DataView(memory.buffer);

  function handleToken(value) {
    const token = typeof value === "number" ? value : lowerHandle?.(value);
    if (!Number.isInteger(token) || token < -0x80000000 || token > 0xffffffff) {
      throw new TypeError("expected i32 handle token");
    }
    return token | 0;
  }

  function remember(ptr, size, align) {
    if (size !== 0) cleanups.set(`${ptr}:${size}:${align}`, { ptr, size, align });
  }

  function region(offset, size, align) {
    if (!Number.isSafeInteger(offset) || !Number.isSafeInteger(size) || offset < 0 || size < 0) {
      throw new RangeError(`invalid canonical memory region`);
    }
    if (!Number.isInteger(align) || align <= 0 || (align & (align - 1)) !== 0 || offset % align !== 0) {
      throw new RangeError(`unaligned canonical memory region`);
    }
    if (offset + size > memory.buffer.byteLength) {
      throw new RangeError(`canonical memory region is out of bounds`);
    }
  }

  function allocate(size, align) {
    if (size === 0) return 0;
    const ptr = Number(realloc(0, 0, align, size));
    region(ptr, size, align);
    remember(ptr, size, align);
    return ptr;
  }

  function writeScalar(kind, offset, value) {
    const data = view();
    switch (kind) {
      case "bool":
        if (typeof value !== "boolean") throw new TypeError("expected bool");
        data.setUint8(offset, value ? 1 : 0);
        break;
      case "i8": data.setInt8(offset, value); break;
      case "u8": data.setUint8(offset, value); break;
      case "i16": data.setInt16(offset, value, true); break;
      case "u16": data.setUint16(offset, value, true); break;
      case "i32": data.setInt32(offset, value, true); break;
      case "u32":
      case "char": data.setUint32(offset, value, true); break;
      case "i64": data.setBigInt64(offset, BigInt(value), true); break;
      case "u64": data.setBigUint64(offset, BigInt(value), true); break;
      case "f32": data.setFloat32(offset, value, true); break;
      case "f64": data.setFloat64(offset, value, true); break;
      default: throw new TypeError(`unknown scalar kind ${kind}`);
    }
  }

  function readScalar(kind, offset) {
    const data = view();
    switch (kind) {
      case "bool": {
        const value = data.getUint8(offset);
        if (value > 1) throw new TypeError(`malformed bool ${value}`);
        return value === 1;
      }
      case "i8": return data.getInt8(offset);
      case "u8": return data.getUint8(offset);
      case "i16": return data.getInt16(offset, true);
      case "u16": return data.getUint16(offset, true);
      case "i32": return data.getInt32(offset, true);
      case "u32": return data.getUint32(offset, true);
      case "char": {
        const value = data.getUint32(offset, true);
        if (value > 0x10ffff || (value >= 0xd800 && value <= 0xdfff)) {
          throw new TypeError(`malformed Unicode scalar ${value}`);
        }
        return value;
      }
      case "i64": return data.getBigInt64(offset, true);
      case "u64": return data.getBigUint64(offset, true);
      case "f32": return data.getFloat32(offset, true);
      case "f64": return data.getFloat64(offset, true);
      default: throw new TypeError(`unknown scalar kind ${kind}`);
    }
  }

  function encodeString(encoding, value) {
    if (typeof value !== "string") throw new TypeError("expected string");
    if (encoding === "utf8") return [new TextEncoder().encode(value), 1];
    if (encoding === "utf16") {
      const bytes = new Uint8Array(value.length * 2);
      const data = new DataView(bytes.buffer);
      for (let index = 0; index < value.length; index++) data.setUint16(index * 2, value.charCodeAt(index), true);
      return [bytes, 2];
    }
    if (encoding === "latin1") {
      const bytes = new Uint8Array(value.length);
      for (let index = 0; index < value.length; index++) {
        const code = value.charCodeAt(index);
        if (code > 0xff) throw new TypeError("string is not Latin-1");
        bytes[index] = code;
      }
      return [bytes, 1];
    }
    throw new TypeError(`unknown string encoding ${encoding}`);
  }

  function decodeString(encoding, bytes) {
    if (encoding === "utf8") return new TextDecoder("utf-8", { fatal: true }).decode(bytes);
    if (encoding === "utf16") {
      if (bytes.byteLength % 2 !== 0) throw new TypeError("malformed UTF-16");
      const data = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
      let value = "";
      for (let index = 0; index < bytes.byteLength; index += 2) value += String.fromCharCode(data.getUint16(index, true));
      // Round-trip validation rejects unpaired surrogates.
      const canonical = encodeString("utf16", value)[0];
      new TextDecoder("utf-16le", { fatal: true }).decode(canonical);
      return value;
    }
    if (encoding === "latin1") return String.fromCharCode(...bytes);
    throw new TypeError(`unknown string encoding ${encoding}`);
  }

  function bufferSpec(kind) {
    switch (kind) {
      case "i8": return { bytes: 1, align: 1, ctor: Int8Array, get: "getInt8", set: "setInt8", bigint: false };
      case "u8": return { bytes: 1, align: 1, ctor: Uint8Array, get: "getUint8", set: "setUint8", bigint: false };
      case "i16": return { bytes: 2, align: 2, ctor: Int16Array, get: "getInt16", set: "setInt16", bigint: false };
      case "u16": return { bytes: 2, align: 2, ctor: Uint16Array, get: "getUint16", set: "setUint16", bigint: false };
      case "i32": return { bytes: 4, align: 4, ctor: Int32Array, get: "getInt32", set: "setInt32", bigint: false };
      case "u32": return { bytes: 4, align: 4, ctor: Uint32Array, get: "getUint32", set: "setUint32", bigint: false };
      case "i64": return { bytes: 8, align: 8, ctor: BigInt64Array, get: "getBigInt64", set: "setBigInt64", bigint: true };
      case "u64": return { bytes: 8, align: 8, ctor: BigUint64Array, get: "getBigUint64", set: "setBigUint64", bigint: true };
      case "f32": return { bytes: 4, align: 4, ctor: Float32Array, get: "getFloat32", set: "setFloat32", bigint: false };
      case "f64": return { bytes: 8, align: 8, ctor: Float64Array, get: "getFloat64", set: "setFloat64", bigint: false };
      default: throw new TypeError(`unknown typed buffer element ${kind}`);
    }
  }

  function lowerBuffer(kind, value, offset) {
    const spec = bufferSpec(kind);
    if (!(value instanceof spec.ctor)) {
      throw new TypeError(`expected ${spec.ctor.name}`);
    }
    const byteLength = value.length * spec.bytes;
    if (!Number.isSafeInteger(byteLength)) throw new RangeError("typed buffer size overflow");
    const ptr = allocate(byteLength, spec.align);
    const data = view();
    for (let index = 0; index < value.length; index++) {
      const item = spec.bigint ? BigInt(value[index]) : value[index];
      if (spec.bytes === 1) data[spec.set](ptr + index, item);
      else data[spec.set](ptr + index * spec.bytes, item, true);
    }
    writeDescriptor(offset, ptr, value.length);
  }

  function liftBuffer(kind, offset, ownership) {
    const spec = bufferSpec(kind);
    const [ptr, len] = readDescriptor(offset);
    const byteLength = len * spec.bytes;
    if (!Number.isSafeInteger(byteLength)) throw new RangeError("typed buffer size overflow");
    region(ptr, byteLength, spec.align);
    const result = new spec.ctor(len);
    const data = view();
    for (let index = 0; index < len; index++) {
      result[index] = spec.bytes === 1
        ? data[spec.get](ptr + index)
        : data[spec.get](ptr + index * spec.bytes, true);
    }
    if (ownership !== "borrow") remember(ptr, byteLength, spec.align);
    return result;
  }

  function writeDescriptor(offset, ptr, len) {
    view().setUint32(offset, ptr, true);
    view().setUint32(offset + 4, len, true);
  }

  function readDescriptor(offset) {
    return [view().getUint32(offset, true), view().getUint32(offset + 4, true)];
  }

  function lower(layout, value, offset, ownership = "value") {
    region(offset, layout.size, layout.align);
    const shape = layout.shape;
    switch (shape.kind) {
      case "scalar": writeScalar(shape.value, offset, value); return;
      case "string": {
        const [bytes, unit] = encodeString(shape.value, value);
        const ptr = allocate(bytes.byteLength, unit);
        new Uint8Array(memory.buffer, ptr, bytes.byteLength).set(bytes);
        writeDescriptor(offset, ptr, bytes.byteLength / unit);
        return;
      }
      case "list": {
        if (!Array.isArray(value)) throw new TypeError("expected list");
        const element = shape.value;
        const size = Math.multiplyExact ? Math.multiplyExact(value.length, element.size) : value.length * element.size;
        if (!Number.isSafeInteger(size)) throw new RangeError("list size overflow");
        const ptr = allocate(size, element.align);
        value.forEach((item, index) => lower(element, item, ptr + index * element.size));
        writeDescriptor(offset, ptr, value.length);
        return;
      }
      case "buffer":
        lowerBuffer(shape.value, value, offset);
        return;
      case "handle":
        view().setUint32(offset, handleToken(value), true);
        return;
      case "future_handle":
        throw new TypeError("future execution is not implemented by codec v1");
      case "record":
        for (const field of shape.value) lower(field.layout, value[field.name], offset + field.offset);
        return;
      case "tuple":
        if (!Array.isArray(value)) throw new TypeError("expected tuple");
        shape.value.forEach((field, index) => lower(field.layout, value[index], offset + field.offset));
        return;
      case "enum":
        if (!Number.isInteger(value) || value < 0 || value >= shape.value.cases) throw new TypeError("malformed enum tag");
        view().setUint32(offset, value, true);
        return;
      case "flags": {
        const allowed = shape.value.count === 32 ? 0xffffffff : (2 ** shape.value.count) - 1;
        if (!Number.isInteger(value) || value < 0 || (value & ~allowed) !== 0) throw new TypeError("malformed flags");
        view().setUint32(offset, value, true);
        return;
      }
      case "variant": {
        const tag = value?.case;
        const variant = shape.value;
        if (!Number.isInteger(tag) || tag < 0 || tag >= variant.cases.length) throw new TypeError("malformed variant tag");
        view().setUint32(offset, tag, true);
        const payload = variant.cases[tag].payload;
        if (payload) lower(payload, value.payload, offset + variant.payload_offset);
        else if (value.payload !== undefined && value.payload !== null) throw new TypeError("unexpected variant payload");
        return;
      }
      default: throw new TypeError(`unknown layout shape ${shape.kind}`);
    }
  }

  function lift(layout, offset, ownership = "value", rememberAllocations = true) {
    region(offset, layout.size, layout.align);
    const shape = layout.shape;
    switch (shape.kind) {
      case "scalar": return readScalar(shape.value, offset);
      case "string": {
        const [ptr, len] = readDescriptor(offset);
        const unit = shape.value === "utf16" ? 2 : 1;
        region(ptr, len * unit, unit);
        if (rememberAllocations) remember(ptr, len * unit, unit);
        return decodeString(shape.value, new Uint8Array(memory.buffer, ptr, len * unit));
      }
      case "list": {
        const [ptr, len] = readDescriptor(offset);
        const element = shape.value;
        region(ptr, len * element.size, element.align);
        if (rememberAllocations) remember(ptr, len * element.size, element.align);
        return Array.from(
          { length: len },
          (_, index) => lift(
            element,
            ptr + index * element.size,
            "value",
            rememberAllocations,
          ),
        );
      }
      case "buffer": return liftBuffer(shape.value, offset, ownership);
      case "handle": return view().getInt32(offset, true);
      case "future_handle": throw new TypeError("future execution is not implemented by codec v1");
      case "record": return Object.fromEntries(shape.value.map((field) => [
        field.name,
        lift(field.layout, offset + field.offset, "value", rememberAllocations),
      ]));
      case "tuple": return shape.value.map((field) =>
        lift(field.layout, offset + field.offset, "value", rememberAllocations));
      case "enum": {
        const tag = view().getUint32(offset, true);
        if (tag >= shape.value.cases) throw new TypeError("malformed enum tag");
        return tag;
      }
      case "flags": {
        const bits = view().getUint32(offset, true);
        const allowed = shape.value.count === 32 ? 0xffffffff : (2 ** shape.value.count) - 1;
        if ((bits & ~allowed) !== 0) throw new TypeError("malformed flags");
        return bits;
      }
      case "variant": {
        const tag = view().getUint32(offset, true);
        const variant = shape.value;
        if (tag >= variant.cases.length) throw new TypeError("malformed variant tag");
        const payload = variant.cases[tag].payload;
        return {
          case: tag,
          payload: payload
            ? lift(payload, offset + variant.payload_offset, "value", rememberAllocations)
            : null,
        };
      }
      default: throw new TypeError(`unknown layout shape ${shape.kind}`);
    }
  }

  function valuePlan(position, index = 0) {
    if (position === "parameter") return plan.function.params[index];
    if (position === "result") return plan.function.result;
    throw new TypeError(`unknown value position`);
  }

  function liftCoreScalar(kind, value) {
    switch (kind) {
      case "bool":
        if (value !== 0 && value !== 1) throw new TypeError(`malformed bool ${value}`);
        return value === 1;
      case "i8": return (Number(value) << 24) >> 24;
      case "u8": return Number(value) & 0xff;
      case "i16": return (Number(value) << 16) >> 16;
      case "u16": return Number(value) & 0xffff;
      case "i32": return Number(value) | 0;
      case "u32": return Number(value) >>> 0;
      case "char": {
        const scalar = Number(value) >>> 0;
        if (scalar > 0x10ffff || (scalar >= 0xd800 && scalar <= 0xdfff)) {
          throw new TypeError(`malformed Unicode scalar ${scalar}`);
        }
        return scalar;
      }
      case "i64": return BigInt.asIntN(64, BigInt(value));
      case "u64": return BigInt.asUintN(64, BigInt(value));
      case "f32": return Math.fround(Number(value));
      case "f64": return Number(value);
      default: throw new TypeError(`unknown scalar kind ${kind}`);
    }
  }

  function liftFlat(layout, coreArgs, state, ownership = "value") {
    if (layout.flat.mode === "indirect") {
      throw new TypeError("indirect values are represented by their pointer");
    }
    const shape = layout.shape;
    switch (shape.kind) {
      case "scalar": return liftCoreScalar(shape.value, coreArgs[state.index++]);
      case "handle": return Number(coreArgs[state.index++]) | 0;
      case "string": {
        const ptr = Number(coreArgs[state.index++]) >>> 0;
        const len = Number(coreArgs[state.index++]) >>> 0;
        const unit = shape.value === "utf16" ? 2 : 1;
        const byteLength = len * unit;
        if (!Number.isSafeInteger(byteLength)) throw new RangeError("string size overflow");
        region(ptr, byteLength, unit);
        return decodeString(
          shape.value,
          new Uint8Array(memory.buffer, ptr, byteLength),
        );
      }
      case "list": {
        const ptr = Number(coreArgs[state.index++]) >>> 0;
        const len = Number(coreArgs[state.index++]) >>> 0;
        const element = shape.value;
        const byteLength = len * element.size;
        if (!Number.isSafeInteger(byteLength)) throw new RangeError("list size overflow");
        region(ptr, byteLength, element.align);
        return Array.from(
          { length: len },
          (_, index) => lift(element, ptr + index * element.size, "value", false),
        );
      }
      case "buffer": {
        const ptr = Number(coreArgs[state.index++]) >>> 0;
        const len = Number(coreArgs[state.index++]) >>> 0;
        const spec = bufferSpec(shape.value);
        const byteLength = len * spec.bytes;
        if (!Number.isSafeInteger(byteLength)) throw new RangeError("buffer size overflow");
        region(ptr, byteLength, spec.bytes);
        const result = new spec.ctor(len);
        const data = view();
        for (let index = 0; index < len; index++) {
          result[index] = spec.bytes === 1
            ? data[spec.get](ptr + index)
            : data[spec.get](ptr + index * spec.bytes, true);
        }
        if (ownership !== "borrow") remember(ptr, byteLength, spec.bytes);
        return result;
      }
      case "record": return Object.fromEntries(shape.value.map((field) => [
        field.name,
        liftFlat(field.layout, coreArgs, state),
      ]));
      case "tuple": return shape.value.map((field) =>
        liftFlat(field.layout, coreArgs, state));
      case "enum": {
        const tag = Number(coreArgs[state.index++]) >>> 0;
        if (tag >= shape.value.cases) throw new TypeError("malformed enum tag");
        return tag;
      }
      case "flags": {
        const bits = Number(coreArgs[state.index++]) >>> 0;
        const allowed = shape.value.count === 32 ? 0xffffffff : (2 ** shape.value.count) - 1;
        if ((bits & ~allowed) !== 0) throw new TypeError("malformed flags");
        return bits;
      }
      default:
        throw new TypeError(`${shape.kind} cannot use a direct core signature`);
    }
  }

  function lowerFlat(layout, value, output, ownership = "value") {
    if (layout.flat.mode === "indirect") {
      throw new TypeError("indirect values are represented by their pointer");
    }
    const shape = layout.shape;
    switch (shape.kind) {
      case "scalar": {
        const normalized = liftCoreScalar(
          shape.value,
          shape.value === "bool" ? (value ? 1 : 0) : value,
        );
        output.push(shape.value === "bool" ? (normalized ? 1 : 0) : normalized);
        return;
      }
      case "handle":
        output.push(handleToken(value));
        return;
      case "string": {
        const [bytes, unit] = encodeString(shape.value, value);
        const ptr = allocate(bytes.byteLength, unit);
        new Uint8Array(memory.buffer, ptr, bytes.byteLength).set(bytes);
        output.push(ptr, bytes.byteLength / unit);
        return;
      }
      case "list": {
        if (!Array.isArray(value)) throw new TypeError("expected list");
        const element = shape.value;
        const byteLength = value.length * element.size;
        if (!Number.isSafeInteger(byteLength)) throw new RangeError("list size overflow");
        const ptr = allocate(byteLength, element.align);
        value.forEach((item, index) => lower(element, item, ptr + index * element.size));
        output.push(ptr, value.length);
        return;
      }
      case "buffer": {
        const spec = bufferSpec(shape.value);
        if (!(value instanceof spec.ctor)) throw new TypeError(`expected ${spec.ctor.name}`);
        const byteLength = value.length * spec.bytes;
        if (!Number.isSafeInteger(byteLength)) throw new RangeError("buffer size overflow");
        const ptr = allocate(byteLength, spec.bytes);
        const data = view();
        for (let index = 0; index < value.length; index++) {
          const item = spec.bigint ? BigInt(value[index]) : value[index];
          if (spec.bytes === 1) data[spec.set](ptr + index, item);
          else data[spec.set](ptr + index * spec.bytes, item, true);
        }
        output.push(ptr, value.length);
        return;
      }
      case "record":
        for (const field of shape.value) {
          lowerFlat(field.layout, value[field.name], output);
        }
        return;
      case "tuple":
        if (!Array.isArray(value)) throw new TypeError("expected tuple");
        shape.value.forEach((field, index) =>
          lowerFlat(field.layout, value[index], output));
        return;
      case "enum":
        if (!Number.isInteger(value) || value < 0 || value >= shape.value.cases) {
          throw new TypeError("malformed enum tag");
        }
        output.push(value);
        return;
      case "flags": {
        const allowed = shape.value.count === 32 ? 0xffffffff : (2 ** shape.value.count) - 1;
        if (!Number.isInteger(value) || value < 0 || (value & ~allowed) !== 0) {
          throw new TypeError("malformed flags");
        }
        output.push(value >>> 0);
        return;
      }
      default:
        throw new TypeError(`${shape.kind} cannot use a direct core signature`);
    }
  }

  function liftArguments(coreArgs) {
    const args = [];
    const state = { index: 0 };
    for (const parameter of plan.function.params) {
      if (parameter.layout.flat.mode === "indirect") {
        args.push(lift(
          parameter.layout,
          Number(coreArgs[state.index++]),
          parameter.ownership,
          false,
        ));
      } else {
        args.push(liftFlat(parameter.layout, coreArgs, state, parameter.ownership));
      }
    }
    if (state.index !== coreArgs.length) {
      throw new TypeError(`core argument count does not match codec plan`);
    }
    return args;
  }

  function lowerResult(value) {
    const result = plan.function.result;
    if (!result) return undefined;
    if (result.layout.flat.mode === "direct") {
      const core = [];
      lowerFlat(result.layout, value, core, result.ownership);
      return core.length === 1 ? core[0] : core;
    }
    const ptr = allocate(result.layout.size, result.layout.align);
    lower(result.layout, value, ptr, result.ownership);
    return ptr;
  }

  return Object.freeze({
    contract: CONTRACT,
    plan,
    lower(position, value, offset, index = 0) {
      const selected = valuePlan(position, index);
      if (!selected) throw new TypeError(`missing codec value plan`);
      lower(selected.layout, value, offset, selected.ownership);
    },
    lift(position, offset, index = 0) {
      const selected = valuePlan(position, index);
      if (!selected) throw new TypeError(`missing codec value plan`);
      return lift(selected.layout, offset, selected.ownership);
    },
    allocateAndLower(position, value, index = 0) {
      const selected = valuePlan(position, index);
      if (!selected) throw new TypeError(`missing codec value plan`);
      const ptr = allocate(selected.layout.size, selected.layout.align);
      lower(selected.layout, value, ptr, selected.ownership);
      return ptr;
    },
    liftArguments,
    lowerResult,
    finish() {
      if (typeof postReturn === "function") {
        for (const { ptr, size, align } of [...cleanups.values()].reverse()) {
          postReturn(ptr, size, align);
        }
      }
      cleanups.clear();
    },
  });
}

/// Binder-facing facade consumed by generated Web IDL core-Wasm transports.
///
/// `plans` is keyed by the transport function identity used by the generated
/// binder. Layouts remain entirely Rust-emitted data.
export function createFeHostWasmCodec(plans, options = {}) {
  const byIdentity = plans instanceof Map ? plans : new Map(Object.entries(plans));
  const needsRealloc = [...byIdentity.values()].some(plan =>
    plan?.function?.requirements?.includes("realloc"));
  const resources = options.resources;
  if (resources !== undefined && typeof resources?.toCore !== "function") {
    throw new TypeError("codec resources must expose toCore(handle)");
  }
  const supported = new Set([
    "realloc",
    "post_return",
    "resource_transfer",
    "borrow_scope",
  ]);
  return Object.freeze({
    protocol: CONTRACT,
    supports(feature) {
      return supported.has(feature);
    },
    createSession() {
      let surface = null;
      const sessions = new Map();
      function selected(identity) {
        if (!surface) throw new TypeError("codec session is not attached");
        const plan = byIdentity.get(identity);
        if (!plan) throw new TypeError(`missing codec plan for ${identity}`);
        let session = sessions.get(identity);
        if (!session) {
          session = createHostWasmCodecSession({
            plan,
            memory: surface.memory,
            realloc: surface.realloc,
            postReturn: surface.postReturns?.[identity],
            lowerHandle: resources === undefined
              ? undefined
              : handle => resources.toCore(handle),
          });
          sessions.set(identity, session);
        }
        return session;
      }
      return Object.freeze({
        attach(nextSurface) {
          if (!(nextSurface?.memory instanceof WebAssembly.Memory)) {
            throw new TypeError("codec attach requires WebAssembly.Memory");
          }
          if (needsRealloc && typeof nextSurface.realloc !== "function") {
            throw new TypeError("codec attach requires cabi_realloc");
          }
          surface = nextSurface;
          sessions.clear();
        },
        liftArguments(identity, coreArgs) {
          return selected(identity).liftArguments(coreArgs);
        },
        lowerResult(identity, value) {
          return selected(identity).lowerResult(value);
        },
        finish(identity) {
          selected(identity).finish();
        },
      });
    },
  });
}
