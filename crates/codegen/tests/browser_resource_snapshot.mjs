// Generic real-browser evidence observer for compiler-declared Fe surface
// resources. This module knows no application resource layout or protocol
// semantics. It enters one surface, copies only caller-selected u32 buffers,
// and emits raw little-endian tapes for an independent host oracle.

import assert from "node:assert/strict";
import { mkdir, writeFile } from "node:fs/promises";
import { resolve } from "node:path";

const pageUrl = process.argv[2];
const receiptDir = process.argv[3] ? resolve(process.argv[3]) : null;
const resourceNames = process.argv.slice(4);
const browserUrl = process.env.FE_BROWSER_URL ?? "http://10.0.0.1:9222";

if (!pageUrl || !receiptDir || resourceNames.length === 0) {
  throw new Error(
    "usage: bun browser_resource_snapshot.mjs <page-url> <new-receipt-dir> <resource>...",
  );
}
if (new Set(resourceNames).size !== resourceNames.length) {
  throw new Error("resource names must be unique");
}
if (resourceNames.some(name => !/^[A-Za-z_][A-Za-z0-9_]*$/.test(name))) {
  throw new Error("resource names must be safe Fe identifiers");
}

function delay(milliseconds) {
  return new Promise(resolvePromise => setTimeout(resolvePromise, milliseconds));
}

await mkdir(receiptDir);
const target = await fetch(new URL("/json/new?about:blank", browserUrl), {
  method: "PUT",
}).then(response => response.json());
const socket = new WebSocket(target.webSocketDebuggerUrl);
await new Promise((resolvePromise, reject) => {
  socket.addEventListener("open", resolvePromise, { once: true });
  socket.addEventListener("error", reject, { once: true });
});

let nextId = 1;
const pending = new Map();
const browserErrors = [];
socket.addEventListener("message", event => {
  const message = JSON.parse(event.data);
  const waiter = pending.get(message.id);
  if (waiter) {
    pending.delete(message.id);
    clearTimeout(waiter.timeout);
    if (message.error) waiter.reject(new Error(JSON.stringify(message.error)));
    else waiter.resolve(message.result);
    return;
  }
  if (message.method === "Runtime.exceptionThrown") {
    browserErrors.push(
      message.params.exceptionDetails.exception?.description ??
        message.params.exceptionDetails.text,
    );
  } else if (
    message.method === "Runtime.consoleAPICalled" &&
    message.params.type === "error"
  ) {
    browserErrors.push(message.params.args.map(argument =>
      argument.value ?? argument.description ?? argument.type
    ).join(" "));
  } else if (
    message.method === "Log.entryAdded" &&
    message.params.entry.level === "error"
  ) {
    browserErrors.push(message.params.entry.text);
  } else if (message.method === "Inspector.targetCrashed") {
    browserErrors.push("the Chrome target crashed");
  }
});

function call(method, params = {}, timeoutMilliseconds = 10_000) {
  const id = nextId++;
  return new Promise((resolvePromise, reject) => {
    const timeout = setTimeout(() => {
      pending.delete(id);
      reject(new Error(`${method} timed out`));
    }, timeoutMilliseconds);
    pending.set(id, { resolve: resolvePromise, reject, timeout });
    socket.send(JSON.stringify({ id, method, params }));
  });
}

async function evaluate(expression, timeoutMilliseconds = 10_000) {
  const evaluation = await call("Runtime.evaluate", {
    expression,
    awaitPromise: true,
    returnByValue: true,
  }, timeoutMilliseconds);
  if (evaluation.exceptionDetails) {
    throw new Error(
      evaluation.exceptionDetails.exception?.description ??
        evaluation.exceptionDetails.text,
    );
  }
  return evaluation.result.value;
}

try {
  await call("Runtime.enable");
  await call("Page.enable");
  await call("Log.enable");
  await call("Page.navigate", { url: pageUrl }, 120_000);

  let state;
  const readyDeadline = Date.now() + 600_000;
  while (Date.now() < readyDeadline) {
    state = await evaluate(
      `document.querySelector("fe-surface")?.state ?? null`,
      600_000,
    );
    if (state === "ready" || state === "live" || state === "error") break;
    await delay(250);
  }
  if (state !== "ready" && state !== "live" && state !== "error") {
    throw new Error("the Fe surface did not finish its initial browser frame");
  }

  const names = JSON.stringify(resourceNames);
  const surface = await evaluate(`(async () => {
    const names = ${names};
    const surface = document.querySelector("fe-surface");
    if (!surface) throw new Error("the page has no fe-surface");
    if (surface.state === "error") {
      const notices = [...(surface.shadowRoot?.querySelectorAll(".notice") ?? [])];
      const notice = notices.at(-1)?.textContent;
      throw new Error(
        \`Fe surface failed before resource readback: \${notice ?? "unknown error"}\`,
      );
    }
    await surface.live();
    const gpu = surface._gpu;
    if (surface.state !== "live" || surface.mode !== "webgpu" || !gpu?.device) {
      throw new Error("the Fe surface did not enter a live WebGPU state");
    }
    const declared = new Map((surface.manifest?.resources ?? []).map(resource => [
      resource.name,
      resource,
    ]));
    const resources = names.map(name => {
      const resource = declared.get(name);
      if (!resource || !gpu.resourceBuffers?.get(name)) {
        throw new Error(\`resource \\\`\${name}\\\` is unavailable\`);
      }
      if (
        resource.element !== "U32" ||
        resource.stride !== Uint32Array.BYTES_PER_ELEMENT ||
        !Number.isSafeInteger(resource.length) ||
        resource.length < 1
      ) {
        throw new Error(\`resource \\\`\${name}\\\` is not a canonical nonempty u32 tape\`);
      }
      return { name, words: resource.length };
    });
    return {
      resources,
      state: surface.state,
      mode: surface.mode,
      passes: surface.manifest?.passes?.length ?? 0,
    };
  })()`, 600_000);

  assert.equal(surface.state, "live");
  assert.equal(surface.mode, "webgpu");
  assert.deepEqual(surface.resources.map(resource => resource.name), resourceNames);
  for (const resource of surface.resources) {
    const name = JSON.stringify(resource.name);
    const prepared = await evaluate(`(async () => {
      const name = ${name};
      const surface = document.querySelector("fe-surface");
      const gpu = surface?._gpu;
      const declared = surface?.manifest?.resources?.find(resource => resource.name === name);
      const source = gpu?.resourceBuffers?.get(name);
      if (!gpu?.device || !declared || !source) {
        throw new Error(\`resource \\\`\${name}\\\` disappeared before readback\`);
      }
      const byteLength = declared.length * declared.stride;
      const staging = gpu.device.createBuffer({
        size: byteLength,
        usage: GPUBufferUsage.COPY_DST | GPUBufferUsage.MAP_READ,
      });
      const encoder = gpu.device.createCommandEncoder();
      encoder.copyBufferToBuffer(source, 0, staging, 0, byteLength);
      gpu.device.queue.submit([encoder.finish()]);
      await staging.mapAsync(GPUMapMode.READ);
      globalThis.__feResourceSnapshot = {
        name,
        staging,
        bytes: new Uint8Array(staging.getMappedRange()),
      };
      return { bytes: byteLength };
    })()`, 600_000);
    assert.equal(prepared.bytes, resource.words * Uint32Array.BYTES_PER_ELEMENT);

    const transferBytes = 512 * 1024;
    const chunks = [];
    try {
      for (let offset = 0; offset < prepared.bytes; offset += transferBytes) {
        const length = Math.min(transferBytes, prepared.bytes - offset);
        const encoded = await evaluate(`(() => {
          const snapshot = globalThis.__feResourceSnapshot;
          const offset = ${offset};
          const length = ${length};
          if (!snapshot || snapshot.name !== ${name}) {
            throw new Error("the mapped resource snapshot is unavailable");
          }
          const bytes = snapshot.bytes.subarray(offset, offset + length);
          let binary = "";
          const stringChunkBytes = 32 * 1024;
          for (let start = 0; start < bytes.length; start += stringChunkBytes) {
            binary += String.fromCharCode(
              ...bytes.subarray(start, start + stringChunkBytes),
            );
          }
          return { base64: btoa(binary), bytes: bytes.length };
        })()`, 60_000);
        const bytes = Buffer.from(encoded.base64, "base64");
        assert.equal(bytes.length, encoded.bytes);
        assert.equal(bytes.length, length);
        chunks.push(bytes);
      }
    } finally {
      await evaluate(`(() => {
        const snapshot = globalThis.__feResourceSnapshot;
        if (snapshot) {
          snapshot.staging.unmap();
          snapshot.staging.destroy();
          delete globalThis.__feResourceSnapshot;
        }
      })()`).catch(() => {});
    }
    const bytes = Buffer.concat(chunks);
    assert.equal(bytes.length, prepared.bytes);
    await writeFile(resolve(receiptDir, `${resource.name}.u32le`), bytes);
  }
  assert.deepEqual(browserErrors, []);
  console.log(JSON.stringify({
    ok: true,
    page: pageUrl,
    mode: surface.mode,
    passes: surface.passes,
    resources: surface.resources,
    receiptDir,
  }, null, 2));
} finally {
  for (const waiter of pending.values()) {
    clearTimeout(waiter.timeout);
    waiter.reject(new Error("Chrome session closed"));
  }
  pending.clear();
  socket.close();
  await fetch(new URL(`/json/close/${target.id}`, browserUrl)).catch(() => {});
}
