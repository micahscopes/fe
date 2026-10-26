// Tests of the host checks, with no WebAssembly module of Fe: the imports are
// called directly, as a module that breaks the contract would call them.
//
//   node --test examples/wasm/host.test.mjs
import assert from "node:assert/strict";
import test from "node:test";
import {
  ContractError,
  FILE_ERRORS,
  MAX_TEXT_BYTES,
  createFilesHost,
  decodeReply,
  encodeReply,
} from "./host.mjs";

const PAGE = 65536;

/** A host over two files, bound to a memory that holds the names `hello.txt` and `nope` at 100. */
function fixture() {
  const memory = new WebAssembly.Memory({ initial: 1 });
  const host = createFilesHost({ "hello.txt": "Hello, world", empty: "" });
  host.bind({ exports: { memory } });
  const bytes = new Uint8Array(memory.buffer);
  bytes.set(new TextEncoder().encode("hello.txt"), 100);
  bytes.set(new TextEncoder().encode("nope"), 200);
  return { host, env: host.imports.env, memory };
}

const open = (env, address, length) => decodeReply(env.sys_wasm_files_open(address, length));

test("a present file, a missing file and a bad name are replies", () => {
  const { env } = fixture();
  assert.deepEqual(open(env, 100, 9), { ok: true, value: 1 });
  assert.deepEqual(open(env, 200, 4), { ok: false, code: 1, error: "NotFound" });
  assert.deepEqual(open(env, 100, 0), { ok: false, code: 3, error: "BadName" });
});

test("size and read_byte, and a read past the end is OutOfRange", () => {
  const { env } = fixture();
  const handle = open(env, 100, 9).value;
  assert.deepEqual(decodeReply(env.sys_wasm_files_size(handle)), { ok: true, value: 12 });
  assert.deepEqual(decodeReply(env.sys_wasm_files_read_byte(handle, 1)), { ok: true, value: 101 });
  assert.deepEqual(decodeReply(env.sys_wasm_files_read_byte(handle, 11)), { ok: true, value: 100 });
  assert.deepEqual(decodeReply(env.sys_wasm_files_read_byte(handle, 12)), { ok: false, code: 2, error: "OutOfRange" });
  // An index is unsigned: the host receives an i32 and does not read it as negative.
  assert.deepEqual(decodeReply(env.sys_wasm_files_read_byte(handle, -1)), { ok: false, code: 2, error: "OutOfRange" });
});

test("handles count up from 1 and are not reused", () => {
  const { env } = fixture();
  const first = open(env, 100, 9).value;
  env.sys_wasm_files_close(first);
  const second = open(env, 100, 9).value;
  assert.deepEqual([first, second], [1, 2]);
});

test("a forged handle is refused by every operation", () => {
  const { host, env } = fixture();
  open(env, 100, 9);
  for (const forged of [0, 2, 77, -1, 0x7fffffff]) {
    for (const call of [
      () => env.sys_wasm_files_size(forged),
      () => env.sys_wasm_files_read_byte(forged, 0),
      () => env.sys_wasm_files_close(forged),
    ]) {
      assert.throws(call, (error) => error instanceof ContractError && /never issued.*forged handle/.test(error.message));
    }
  }
  assert.equal(host.openHandles, 1, "a refused call changes nothing");
  assert.equal(host.closed, 0);
});

test("a closed handle is refused: double close and use after close", () => {
  const { host, env } = fixture();
  const handle = open(env, 100, 9).value;
  env.sys_wasm_files_close(handle);
  for (const call of [
    () => env.sys_wasm_files_close(handle),
    () => env.sys_wasm_files_size(handle),
    () => env.sys_wasm_files_read_byte(handle, 0),
  ]) {
    assert.throws(call, (error) => error instanceof ContractError && /handle 1 is closed/.test(error.message));
  }
  assert.equal(host.closed, 1, "the second close is not counted");
});

test("text outside the memory, or too long, is refused", () => {
  const { host, env, memory } = fixture();
  const size = memory.buffer.byteLength;
  assert.equal(size, PAGE);
  for (const [address, length] of [
    [size - 4, 5],
    [size, 1],
    [0xffffff00, 100],
    [-1, 2], // the i32 -1 is the address 0xffffffff
    [0xffffffff, 1],
  ]) {
    assert.throws(
      () => env.sys_wasm_files_open(address, length),
      (error) => error instanceof ContractError && /lies outside the memory of 65536 bytes/.test(error.message),
      `${address}+${length}`,
    );
  }
  // A text that ends at the last byte of the memory is inside it, and so is an empty one at the end.
  assert.deepEqual(open(env, size - 4, 4), { ok: false, code: 1, error: "NotFound" });
  assert.deepEqual(open(env, size, 0), { ok: false, code: 3, error: "BadName" });
  assert.throws(
    () => env.sys_wasm_files_open(0, MAX_TEXT_BYTES + 1),
    (error) => error instanceof ContractError && /above the limit of 65536 bytes/.test(error.message),
  );
  // A negative i32 length is a large unsigned one.
  for (const length of [-1, 0xffffffff, 0x80000000]) {
    assert.throws(
      () => env.sys_wasm_files_open(0, length),
      (error) => error instanceof ContractError && /above the limit/.test(error.message),
    );
  }
  assert.equal(host.opened, 0, "a refused call opens nothing");
});

test("an unknown error code is refused when it is made and when it is read", () => {
  for (const code of [4, 9, 255, 0xffffffff]) {
    assert.throws(() => encodeReply(code), (error) => error instanceof ContractError && new RegExp(`unknown error code ${code}\\b`).test(error.message));
    const word = BigInt(code);
    assert.throws(() => decodeReply(word), (error) => error instanceof ContractError && /unknown error code/.test(error.message));
  }
  for (const code of Object.keys(FILE_ERRORS)) {
    assert.deepEqual(decodeReply(encodeReply(Number(code))), { ok: false, code: Number(code), error: FILE_ERRORS[code] });
  }
  // An error carries no payload, and a reply is a bigint.
  assert.throws(() => decodeReply((5n << 32n) | 2n), /carries no payload/);
  assert.throws(() => decodeReply(3), /64-bit word/);
  assert.deepEqual(decodeReply(encodeReply(0, 0xffffffff)), { ok: true, value: 0xffffffff });
});

test("open handles are counted and the leak check names them", () => {
  const { host, env } = fixture();
  assert.equal(host.openHandles, 0);
  host.assertNoOpenHandles();
  const a = open(env, 100, 9).value;
  const b = open(env, 100, 9).value;
  assert.equal(host.openHandles, 2);
  assert.throws(() => host.assertNoOpenHandles(), /2 handle\(s\) were not closed: 1, 2/);
  env.sys_wasm_files_close(a);
  env.sys_wasm_files_close(b);
  assert.equal(host.openHandles, 0);
  assert.deepEqual([host.opened, host.closed], [2, 2]);
  host.assertNoOpenHandles();
});

test("a host that was not bound refuses to read", () => {
  const host = createFilesHost({});
  assert.throws(() => host.imports.env.sys_wasm_files_open(0, 0), /not bound/);
  assert.throws(() => host.bind({ exports: {} }), /does not export its memory/);
});

test("names are compared as bytes", () => {
  const memory = new WebAssembly.Memory({ initial: 1 });
  const host = createFilesHost({ "grüß": "x", [new Uint8Array([0xff, 0xfe]).toString()]: "y" });
  host.bind({ exports: { memory } });
  const bytes = new Uint8Array(memory.buffer);
  bytes.set(new TextEncoder().encode("grüß"), 0);
  bytes.set([0xc3, 0xbc], 20); // "ü" alone
  assert.deepEqual(open(host.imports.env, 0, 6), { ok: true, value: 1 });
  assert.deepEqual(open(host.imports.env, 20, 2), { ok: false, code: 1, error: "NotFound" });
});
