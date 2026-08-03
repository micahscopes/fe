import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

import {
  createFeHostWasmCodec,
  createHostWasmCodecSession,
} from "./host-wasm-codec-v1.js";
import { createFeCoreWasmTransport } from "./host-wasm-transport-v1.fixture.js";

const fixture = JSON.parse(
  readFileSync(
    fileURLToPath(new URL("./host-wasm-codec-v1.fixture.json", import.meta.url)),
    "utf8",
  ),
);
const bufferFixture = JSON.parse(
  readFileSync(
    fileURLToPath(new URL("./host-wasm-codec-v1.buffers.json", import.meta.url)),
    "utf8",
  ),
);

function bufferPlan(layout, ownership = "own") {
  return {
    contract: fixture.contract,
    abi: fixture.abi,
    abi_version: fixture.abi_version,
    function: {
      namespace: "fe:fixture",
      name: "buffer",
      direction: "host_to_guest",
      params: [
        {
          type_: {
            kind: "buffer",
            value: { element: layout.shape.value, ownership },
          },
          layout,
          position: "parameter",
          ownership,
        },
      ],
      result: null,
      requirements: ["realloc", "post_return"],
    },
  };
}

function runtime(plan = structuredClone(fixture)) {
  const memory = new WebAssembly.Memory({ initial: 1 });
  let cursor = 256;
  const released = [];
  const realloc = (_oldPtr, _oldSize, align, size) => {
    cursor = (cursor + align - 1) & ~(align - 1);
    const ptr = cursor;
    cursor += size;
    return ptr;
  };
  const session = createHostWasmCodecSession({
    plan,
    memory,
    realloc,
    postReturn(ptr, size, align) {
      released.push({ ptr, size, align });
    },
  });
  return { memory, released, session };
}

describe("fe:host-wasm-codec/v1", () => {
  test("executes the Rust-emitted scalar, handle, record, string, list, and variant plan", () => {
    const { memory, released, session } = runtime();

    session.lower("parameter", 17, 0, 0);
    expect(session.lift("parameter", 0, 0)).toBe(17);

    const request = { message: "héllo", values: [3, 5, 8] };
    session.lower("parameter", request, 16, 1);
    expect(session.lift("parameter", 16, 1)).toEqual(request);

    const reply = { case: 0, payload: "nope" };
    session.lower("result", reply, 64);
    expect(session.lift("result", 64)).toEqual(reply);

    session.finish();
    expect(released).toHaveLength(3);
    expect(released.every(({ ptr, size, align }) => ptr > 0 && size > 0 && ptr % align === 0)).toBe(true);
    expect(memory).toBeInstanceOf(WebAssembly.Memory);
  });

  test("rejects malformed tags, unaligned roots, and out-of-bounds descriptors", () => {
    const { memory, session } = runtime();
    new DataView(memory.buffer).setUint32(64, 9, true);
    expect(() => session.lift("result", 64)).toThrow("malformed variant tag");
    expect(() => session.lower("parameter", 1, 1, 0)).toThrow("unaligned");

    const data = new DataView(memory.buffer);
    data.setUint32(16, memory.buffer.byteLength - 1, true);
    data.setUint32(20, 12, true);
    data.setUint32(24, 0, true);
    data.setUint32(28, 0, true);
    expect(() => session.lift("parameter", 16, 1)).toThrow("out of bounds");
  });

  test("fails closed for callback, future, and missing post-return execution", () => {
    for (const requirement of ["callback_table", "future_table"]) {
      const plan = structuredClone(fixture);
      plan.function.requirements.push(requirement);
      expect(() =>
        createHostWasmCodecSession({
          plan,
          memory: new WebAssembly.Memory({ initial: 1 }),
          realloc() { return 0; },
          postReturn() {},
        }),
      ).toThrow("not implemented");
    }
    expect(() =>
      createHostWasmCodecSession({
        plan: fixture,
        memory: new WebAssembly.Memory({ initial: 1 }),
        realloc() { return 0; },
      }),
    ).toThrow("post-return");
  });

  test("executes every Rust-emitted typed buffer layout with correct numeric semantics", () => {
    const values = {
      i8: new Int8Array([-128, -1, 127]),
      u8: new Uint8Array([0, 1, 255]),
      i16: new Int16Array([-32768, -2, 32767]),
      u16: new Uint16Array([0, 2, 65535]),
      i32: new Int32Array([-2147483648, -3, 2147483647]),
      u32: new Uint32Array([0, 3, 0xffffffff]),
      i64: new BigInt64Array([-9223372036854775808n, -4n, 9223372036854775807n]),
      u64: new BigUint64Array([0n, 4n, 18446744073709551615n]),
      f32: new Float32Array([-1.5, 0, 3.25]),
      f64: new Float64Array([-Math.PI, 0, Number.MAX_VALUE]),
    };
    for (const [kind, layout] of Object.entries(bufferFixture.layouts)) {
      const { released, session } = runtime(bufferPlan(layout));
      session.lower("parameter", values[kind], 0);
      const decoded = session.lift("parameter", 0);
      expect(decoded).toBeInstanceOf(values[kind].constructor);
      expect([...decoded]).toEqual([...values[kind]]);
      session.finish();
      expect(released).toHaveLength(1);
      expect(released[0].align).toBe(values[kind].BYTES_PER_ELEMENT);
    }
  });

  test("does not claim post-return ownership for a borrowed typed buffer", () => {
    const layout = bufferFixture.layouts.u32;
    const owned = runtime(bufferPlan(layout));
    owned.session.lower("parameter", new Uint32Array([10, 20]), 0);
    owned.session.finish();
    expect(owned.released).toHaveLength(1);

    const borrowed = createHostWasmCodecSession({
      plan: bufferPlan(layout, "borrow"),
      memory: owned.memory,
      realloc() {
        throw new Error("borrow lift must not allocate");
      },
      postReturn(ptr, size, align) {
        owned.released.push({ ptr, size, align });
      },
    });
    expect([...borrowed.lift("parameter", 0)]).toEqual([10, 20]);
    borrowed.finish();
    expect(owned.released).toHaveLength(1);
  });

  test("executes a generated binder import against an attached Wasm instance", async () => {
    const directory = mkdtempSync(join(tmpdir(), "fe-host-codec-"));
    const wasmPath = join(directory, "fixture.wasm");
    try {
      const watPath = fileURLToPath(
        new URL("./host-wasm-codec-v1.fixture.wat", import.meta.url),
      );
      const compiled = spawnSync("wasm-tools", ["parse", watPath, "-o", wasmPath]);
      if (compiled.status !== 0) {
        throw new Error(compiled.stderr.toString());
      }
      const plan = structuredClone(fixture);
      const codec = createFeHostWasmCodec({ "fixture/send": plan });
      const seen = [];
      const transport = createFeCoreWasmTransport(codec, {
        imports: {
          "fe:fixture": {
            send(channel, request) {
              seen.push({ channel, request });
              return {
                case: 1,
                payload: channel + request.values.reduce((sum, value) => sum + value, 0),
              };
            },
          },
        },
      });
      const { instance } = await WebAssembly.instantiate(
        readFileSync(wasmPath),
        transport.imports,
      );
      transport.attach(instance);

      const requestCodec = createHostWasmCodecSession({
        plan,
        memory: instance.exports.memory,
        realloc: instance.exports.cabi_realloc,
        postReturn: instance.exports.cabi_post_fixture_send,
      });
      requestCodec.lower(
        "parameter",
        { message: "from wasm", values: [2, 3, 5] },
        32,
        1,
      );
      const data = new DataView(instance.exports.memory.buffer);
      const resultPtr = instance.exports.run(
        7,
        data.getUint32(32, true),
        data.getUint32(36, true),
        data.getUint32(40, true),
        data.getUint32(44, true),
      );
      expect(seen).toEqual([
        {
          channel: 7,
          request: { message: "from wasm", values: [2, 3, 5] },
        },
      ]);
      expect(requestCodec.lift("result", resultPtr)).toEqual({
        case: 1,
        payload: 17,
      });
      expect(instance).toBeInstanceOf(WebAssembly.Instance);
    } finally {
      rmSync(directory, { recursive: true, force: true });
    }
  });
});
