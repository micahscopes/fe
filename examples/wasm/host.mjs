// A host for WebAssembly modules built from Fe with `std::wasm::files`.
//
// It supplies the four imports of the files contract, places text in the
// module's memory before a call, decodes the reply of an export, and counts
// open handles. It checks everything a module hands it before acting on it, and
// ends the call with a `ContractError` that names the problem when the module
// breaks the contract. The module is a plain ES module with no dependencies.
//
//   import { instantiateFiles } from "./host.mjs";
//   const guest = await instantiateFiles(bytes, { "hello.txt": "Hello" });
//   guest.callReply("file_size", "hello.txt");   // { ok: true, value: 5 }
//   guest.host.assertNoOpenHandles();
//
// The contract is described in `ingots/std/src/wasm/files.fe` and
// `ingots/std/src/wasm/boundary.fe`; the numbers below repeat it.

/** The errors of the contract, by number. A number outside this table is a fault. */
export const FILE_ERRORS = Object.freeze({
  1: "NotFound",
  2: "OutOfRange",
  3: "BadName",
});

/** The longest text, in bytes, that the host reads from the module or places in it. */
export const MAX_TEXT_BYTES = 65536;

/** The module, or the code that drives it, broke the contract. */
export class ContractError extends Error {
  constructor(message) {
    super(message);
    this.name = "ContractError";
  }
}

const MASK32 = 0xffffffffn;
const encoder = new TextEncoder();

/** The reply word of the contract: the tag in the low half, the payload in the high half. */
export function encodeReply(tag, payload = 0) {
  if (tag !== 0 && !(tag in FILE_ERRORS)) {
    throw new ContractError(`unknown error code ${tag}: the contract defines ${Object.keys(FILE_ERRORS).join(", ")}`);
  }
  if (tag !== 0 && payload !== 0) {
    throw new ContractError(`an error reply carries no payload, but error ${tag} has payload ${payload}`);
  }
  return (BigInt(payload >>> 0) << 32n) | BigInt(tag >>> 0);
}

/** `{ ok: true, value }` or `{ ok: false, code, error }` for the word an export returned. */
export function decodeReply(word) {
  if (typeof word !== "bigint") {
    throw new ContractError(`a reply is a 64-bit word, but the export returned ${typeof word}`);
  }
  const bits = BigInt.asUintN(64, word);
  const tag = Number(bits & MASK32);
  const payload = Number(bits >> 32n);
  if (tag === 0) return { ok: true, value: payload };
  if (!(tag in FILE_ERRORS)) {
    throw new ContractError(`unknown error code ${tag} in a reply: the contract defines ${Object.keys(FILE_ERRORS).join(", ")}`);
  }
  if (payload !== 0) {
    throw new ContractError(`an error reply carries no payload, but error ${tag} has payload ${payload}`);
  }
  return { ok: false, code: tag, error: FILE_ERRORS[tag] };
}

/** The bytes of a file or a name: a string is UTF-8, a Uint8Array is taken as it is. */
function toBytes(value, what) {
  if (typeof value === "string") return encoder.encode(value);
  if (value instanceof Uint8Array) return value;
  throw new TypeError(`${what} must be a string or a Uint8Array`);
}

/** A name is compared as bytes, so each byte becomes one character of the key. */
function keyOf(bytes) {
  let key = "";
  for (const byte of bytes) key += String.fromCharCode(byte);
  return key;
}

/**
 * The imports of the files contract, over an in-memory set of files.
 *
 * `files` maps a name to its contents. Handle numbers count up from 1 and are
 * never reused, so a closed handle is told from one that was never issued.
 * Call `bind` with the instance before the module runs.
 */
export function createFilesHost(files, extraImports = {}) {
  const table = new Map();
  for (const [name, contents] of Object.entries(files)) {
    table.set(keyOf(toBytes(name, "a file name")), toBytes(contents, `the contents of ${name}`));
  }
  const open = new Map(); // handle -> contents
  let issued = 0;
  let closed = 0;
  let memory = null;

  const bytesOfMemory = () => {
    if (memory === null) throw new ContractError("the host is not bound to an instance");
    return new Uint8Array(memory.buffer);
  };

  /** The text at `address` of `length` bytes, copied out of the module's memory. */
  const readText = (address, length, what) => {
    const addr = address >>> 0;
    const len = length >>> 0;
    if (len > MAX_TEXT_BYTES) {
      throw new ContractError(`${what}: text length ${len} is above the limit of ${MAX_TEXT_BYTES} bytes`);
    }
    const view = bytesOfMemory();
    if (addr + len > view.length) {
      throw new ContractError(`${what}: text at ${addr} of length ${len} lies outside the memory of ${view.length} bytes`);
    }
    return view.slice(addr, addr + len);
  };

  const lookup = (handle, what) => {
    const id = handle >>> 0;
    const contents = open.get(id);
    if (contents !== undefined) return contents;
    if (id >= 1 && id <= issued) {
      throw new ContractError(`${what}: handle ${id} is closed (closed twice, or used after it was closed)`);
    }
    throw new ContractError(`${what}: handle ${id} was never issued by this host (forged handle)`);
  };

  const env = {
    sys_wasm_files_open(address, length) {
      const name = readText(address, length, "open");
      if (name.length === 0) return encodeReply(3);
      const contents = table.get(keyOf(name));
      if (contents === undefined) return encodeReply(1);
      issued += 1;
      open.set(issued, contents);
      return encodeReply(0, issued);
    },
    sys_wasm_files_size(handle) {
      return encodeReply(0, lookup(handle, "size").length);
    },
    sys_wasm_files_read_byte(handle, index) {
      const contents = lookup(handle, "read_byte");
      const at = index >>> 0;
      if (at >= contents.length) return encodeReply(2);
      return encodeReply(0, contents[at]);
    },
    sys_wasm_files_close(handle) {
      lookup(handle, "close");
      open.delete(handle >>> 0);
      closed += 1;
    },
    ...(extraImports.env ?? {}),
  };

  return {
    imports: { ...extraImports, env },
    /** Gives the host the instance whose memory the module's text lives in. */
    bind(instance) {
      memory = instance.exports.memory;
      if (!(memory instanceof WebAssembly.Memory)) {
        throw new ContractError("the module does not export its memory as `memory`");
      }
    },
    get opened() { return issued; },
    get closed() { return closed; },
    /** Handles that were opened and not closed. */
    get openHandles() { return open.size; },
    assertNoOpenHandles() {
      if (open.size !== 0) {
        throw new ContractError(`${open.size} handle(s) were not closed: ${[...open.keys()].join(", ")}`);
      }
    },
  };
}

/**
 * A running module and its host.
 *
 * An argument of a call is a number or bigint, passed as it is, or a string or
 * Uint8Array, which is text: it is placed in the module's memory and passed as
 * an address and a length. The memory is released after the call.
 */
export class Guest {
  constructor(instance, host) {
    this.instance = instance;
    this.host = host;
    this.exports = instance.exports;
  }

  #requireAllocator() {
    for (const name of ["alloc", "arena_mark", "arena_release"]) {
      if (typeof this.exports[name] !== "function") {
        throw new ContractError(`the module does not export \`${name}\`, so it takes no text`);
      }
    }
  }

  /** Calls an export and returns what it returned. */
  call(name, ...args) {
    const func = this.exports[name];
    if (typeof func !== "function") throw new ContractError(`the module does not export a function \`${name}\``);
    if (!args.some((arg) => typeof arg === "string" || arg instanceof Uint8Array)) {
      return func(...args);
    }
    this.#requireAllocator();
    const mark = this.exports.arena_mark();
    try {
      const spread = [];
      for (const arg of args) {
        if (typeof arg === "string" || arg instanceof Uint8Array) {
          const bytes = toBytes(arg, "a text argument");
          if (bytes.length > MAX_TEXT_BYTES) {
            throw new ContractError(`text of ${bytes.length} bytes is above the limit of ${MAX_TEXT_BYTES} bytes`);
          }
          const address = this.exports.alloc(bytes.length) >>> 0;
          const memory = new Uint8Array(this.exports.memory.buffer);
          if (address + bytes.length > memory.length) {
            throw new ContractError(`alloc returned ${address}, which cannot hold ${bytes.length} bytes in a memory of ${memory.length}`);
          }
          memory.set(bytes, address);
          spread.push(address, bytes.length);
        } else {
          spread.push(arg);
        }
      }
      return func(...spread);
    } finally {
      this.exports.arena_release(mark);
    }
  }

  /** Calls an export that returns a reply and decodes it. */
  callReply(name, ...args) {
    return decodeReply(this.call(name, ...args));
  }
}

/** Instantiates a module that imports the files contract and returns it with its host. */
export async function instantiateFiles(bytes, files = {}, extraImports = {}) {
  const host = createFilesHost(files, extraImports);
  const { instance } = await WebAssembly.instantiate(bytes, host.imports);
  host.bind(instance);
  return new Guest(instance, host);
}
