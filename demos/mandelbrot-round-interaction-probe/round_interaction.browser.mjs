import assert from "node:assert/strict";
import { constants as fsConstants } from "node:fs";
import { access, open, readFile, readdir } from "node:fs/promises";
import { createServer } from "node:http";
import { extname, resolve, sep } from "node:path";
import { pathToFileURL } from "node:url";

const siteRoot = resolve(process.argv[2] ?? "");
const remoteBrowserUrl = process.env.FE_BROWSER_URL ?? "http://10.0.0.1:9222";
const browserSiteHost = process.env.FE_BROWSER_HOST ?? "10.0.0.2";
const browserSitePort = Number.parseInt(process.env.FE_BROWSER_PORT ?? "8000", 10);
const healthOnly = process.env.FE_BROWSER_HEALTH_ONLY === "1";
const computeStageOnly = process.env.FE_BROWSER_COMPUTE_STAGE ?? null;
const tracePath = process.env.FE_BROWSER_TRACE_PATH
  ? resolve(process.env.FE_BROWSER_TRACE_PATH)
  : null;
const harnessStarted = performance.now();

const traceCategories = [
  "blink.user_timing",
  "gpu",
  "disabled-by-default-gpu.debug",
  "disabled-by-default-gpu.device",
  "disabled-by-default-gpu.dawn",
  "disabled-by-default-gpu.service",
];

function reportPhase(phase, details = {}) {
  console.log(JSON.stringify({
    kind: "WebGPU probe phase",
    phase,
    elapsedMs: Number((performance.now() - harnessStarted).toFixed(2)),
    ...details,
  }));
}

async function startBrowserTrace(browser) {
  if (!tracePath) return null;
  const session = await browser.target().createCDPSession();
  await session.send("Tracing.start", {
    transferMode: "ReturnAsStream",
    traceConfig: {
      recordMode: "recordUntilFull",
      includedCategories: traceCategories,
    },
  });
  return session;
}

async function stopBrowserTrace(session) {
  if (!session || !tracePath) return null;
  const completed = new Promise(resolvePromise => {
    session.once("Tracing.tracingComplete", resolvePromise);
  });
  await session.send("Tracing.end");
  const { stream } = await completed;
  if (!stream) throw new Error("Chrome trace completed without an IO stream");

  const output = await open(tracePath, "w");
  let bytes = 0;
  try {
    while (true) {
      const chunk = await session.send("IO.read", { handle: stream });
      const data = chunk.base64Encoded
        ? Buffer.from(chunk.data, "base64")
        : Buffer.from(chunk.data);
      await output.write(data);
      bytes += data.byteLength;
      if (chunk.eof) break;
    }
  } finally {
    await output.close();
    await session.send("IO.close", { handle: stream }).catch(() => {});
    await session.detach().catch(() => {});
  }
  return { path: tracePath, bytes, categories: traceCategories };
}

async function exists(path) {
  try {
    await access(path, fsConstants.R_OK);
    return true;
  } catch {
    return false;
  }
}

if (!process.argv[2] || !(await exists(resolve(siteRoot, "index.html")))) {
  throw new Error("usage: node round_interaction.browser.mjs <precompiled-site>");
}
if (!Number.isInteger(browserSitePort) || browserSitePort < 1 || browserSitePort > 65_535) {
  throw new Error("FE_BROWSER_PORT must be an integer from 1 through 65535");
}
if (
  computeStageOnly !== null &&
  !new Set(["compile", "one", "full", "readback"]).has(computeStageOnly)
) {
  throw new Error("FE_BROWSER_COMPUTE_STAGE must be compile, one, full, or readback");
}

async function computeProbeConfiguration() {
  const assetDirectory = resolve(siteRoot, "assets");
  const manifestNames = (await readdir(assetDirectory)).filter(name => name.endsWith(".json"));
  if (manifestNames.length !== 1) {
    throw new Error(`expected one render manifest, found ${manifestNames.length}`);
  }
  const manifest = JSON.parse(await readFile(resolve(assetDirectory, manifestNames[0]), "utf8"));
  const computePasses = manifest.passes.filter(pass => pass.layout?.mode === "compute");
  if (computePasses.length !== 1) {
    throw new Error(`expected one compute pass, found ${computePasses.length}`);
  }
  const pass = computePasses[0];
  const resources = new Map(manifest.resources.map(resource => [resource.name, resource]));
  return {
    shaderUrl: `/assets/${pass.shader}`,
    shaderBytes: pass.shader_bytes,
    entryPoint: pass.layout.entry_point,
    workgroup: pass.layout.workgroup_size,
    dispatch: pass.dispatch,
    bindings: pass.layout.bindings.map(binding => {
      const resource = resources.get(binding.name);
      return {
        group: binding.group,
        binding: binding.binding,
        name: binding.name,
        role: binding.role,
        byteLength: resource ? resource.length * resource.stride : binding.span,
      };
    }),
  };
}

const computeProbe = computeStageOnly === null ? null : await computeProbeConfiguration();

async function firstNixStorePath(nameFragment, suffix) {
  for (const entry of (await readdir("/nix/store")).sort()) {
    if (!entry.includes(nameFragment)) continue;
    const candidate = resolve("/nix/store", entry, suffix);
    if (await exists(candidate)) return candidate;
  }
  return null;
}

async function loadPuppeteer() {
  try {
    return await import("puppeteer");
  } catch {
    const bundled = await firstNixStorePath(
      "-chrome-devtools-mcp-",
      "lib/chrome-devtools-mcp/build/src/third_party/index.js",
    );
    if (!bundled) throw new Error("Puppeteer is unavailable");
    return import(pathToFileURL(bundled).href);
  }
}

const contentTypes = new Map([
  [".html", "text/html; charset=utf-8"],
  [".js", "text/javascript; charset=utf-8"],
  [".json", "application/json; charset=utf-8"],
  [".wasm", "application/wasm"],
  [".wgsl", "text/plain; charset=utf-8"],
]);

async function siteAsset(value) {
  const url = new URL(value, "http://fe-round-probe.test");
  if (url.pathname === "/favicon.ico") {
    return { status: 204, body: Buffer.alloc(0) };
  }
  if (url.pathname === "/health.html") {
    return {
      status: 200,
      contentType: "text/html; charset=utf-8",
      body: Buffer.from("<!doctype html><meta charset=utf-8><title>WebGPU health</title>"),
    };
  }
  const pathname = decodeURIComponent(url.pathname === "/" ? "/index.html" : url.pathname);
  const candidate = resolve(siteRoot, `.${pathname}`);
  if (candidate !== siteRoot && !candidate.startsWith(`${siteRoot}${sep}`)) {
    return { status: 403, body: Buffer.from("outside site root") };
  }
  if (!(await exists(candidate))) {
    return { status: 404, body: Buffer.from("not found") };
  }
  return {
    status: 200,
    contentType: contentTypes.get(extname(candidate)) ?? "application/octet-stream",
    body: await readFile(candidate),
  };
}

const server = createServer(async (request, response) => {
  try {
    const asset = await siteAsset(request.url);
    response.writeHead(
      asset.status,
      asset.contentType ? { "content-type": asset.contentType } : undefined,
    );
    response.end(asset.body);
  } catch (error) {
    response.writeHead(500).end(String(error));
  }
});

await new Promise((resolvePromise, reject) => {
  server.once("error", reject);
  server.listen(browserSitePort, "0.0.0.0", resolvePromise);
});

const imported = await loadPuppeteer();
const puppeteer = imported.puppeteer ?? imported.default ?? imported;
const browserErrors = [];
let browser;
let page;
let traceSession;
try {
  reportPhase("connect", { remoteBrowserUrl });
  browser = await puppeteer.connect({
    browserURL: remoteBrowserUrl,
    protocolTimeout: 600_000,
  });
  reportPhase("new-page");
  page = await browser.newPage();
  page.setDefaultTimeout(600_000);
  page.on("console", message => {
    if (message.type() === "error" || message.type() === "warn") {
      browserErrors.push(`console ${message.type()}: ${message.text()}`);
    }
  });
  page.on("pageerror", error => browserErrors.push(`page: ${error.message}`));
  await page.evaluateOnNewDocument(() => {
    globalThis.__feRoundProbe = { errors: [], ready: 0, frames: 0, lost: null };
    const recordError = error => globalThis.__feRoundProbe.errors.push(
      String(error?.stack ?? error),
    );
    globalThis.addEventListener("fe:bootstrap-error", event => recordError(event.detail));
    document.addEventListener("fe-error", event => recordError(event.detail), true);
    document.addEventListener("fe-ready", event => {
      if (event.target?.tagName === "FE-SURFACE") globalThis.__feRoundProbe.ready += 1;
    }, true);
    document.addEventListener("fe-frame", event => {
      if (event.target?.tagName === "FE-SURFACE") globalThis.__feRoundProbe.frames += 1;
    }, true);
  });

  const isolatedProbe = healthOnly || computeStageOnly !== null;
  const pageUrl = `http://${browserSiteHost}:${browserSitePort}/${isolatedProbe ? "health.html" : ""}`;
  reportPhase("navigate", { pageUrl });
  await page.goto(pageUrl, { waitUntil: "domcontentloaded", timeout: 120_000 });
  reportPhase("navigated", { pageUrl });
  traceSession = await startBrowserTrace(browser);
  if (traceSession) reportPhase("trace-started", { tracePath });
  if (healthOnly) {
    reportPhase("health-control");
    const health = await page.evaluate(async () => {
      const started = performance.now();
      const adapter = await navigator.gpu?.requestAdapter();
      if (!adapter) throw new Error("the browser returned no WebGPU adapter");
      const device = await adapter.requestDevice();
      device.pushErrorScope("out-of-memory");
      device.pushErrorScope("internal");
      device.pushErrorScope("validation");
      const module = device.createShaderModule({
        code: `
          @group(0) @binding(0) var<storage, read_write> receipt: array<u32>;
          @compute @workgroup_size(1) fn main() { receipt[0] = 42u; }
        `,
      });
      const pipeline = await device.createComputePipelineAsync({
        layout: "auto",
        compute: { module, entryPoint: "main" },
      });
      const output = device.createBuffer({
        size: 4,
        usage: GPUBufferUsage.STORAGE | GPUBufferUsage.COPY_SRC,
      });
      const staging = device.createBuffer({
        size: 4,
        usage: GPUBufferUsage.COPY_DST | GPUBufferUsage.MAP_READ,
      });
      const bindGroup = device.createBindGroup({
        layout: pipeline.getBindGroupLayout(0),
        entries: [{ binding: 0, resource: { buffer: output } }],
      });
      const encoder = device.createCommandEncoder();
      const pass = encoder.beginComputePass();
      pass.setPipeline(pipeline);
      pass.setBindGroup(0, bindGroup);
      pass.dispatchWorkgroups(1);
      pass.end();
      encoder.copyBufferToBuffer(output, 0, staging, 0, 4);
      device.queue.submit([encoder.finish()]);
      await staging.mapAsync(GPUMapMode.READ);
      const value = new Uint32Array(staging.getMappedRange())[0];
      staging.unmap();
      staging.destroy();
      output.destroy();
      const scopedErrors = [
        await device.popErrorScope(),
        await device.popErrorScope(),
        await device.popErrorScope(),
      ].filter(Boolean).map(error => error.message);
      const loss = await Promise.race([
        device.lost.then(info => ({ reason: info.reason, message: info.message })),
        new Promise(resolvePromise => setTimeout(() => resolvePromise(null), 500)),
      ]);
      return {
        secureContext: window.isSecureContext,
        value,
        scopedErrors,
        loss,
        elapsedMs: performance.now() - started,
        adapter: adapter.info ? {
          vendor: adapter.info.vendor,
          architecture: adapter.info.architecture,
          device: adapter.info.device,
          description: adapter.info.description,
        } : null,
      };
    });
    assert.equal(health.secureContext, true);
    assert.equal(health.value, 42);
    assert.deepEqual(health.scopedErrors, []);
    assert.equal(health.loss, null);
    assert.deepEqual(browserErrors, []);
    reportPhase("health-control-complete", { elapsedMs: health.elapsedMs });
    console.log(JSON.stringify({ ok: true, kind: "WebGPU health", ...health }, null, 2));
  } else if (computeStageOnly !== null) {
    reportPhase("compute-stage", { stage: computeStageOnly });
    const result = await page.evaluate(async ({ stage, probe }) => {
      const started = performance.now();
      const uncaptured = [];
      const adapter = await navigator.gpu?.requestAdapter();
      if (!adapter) throw new Error("the browser returned no WebGPU adapter");
      const device = await adapter.requestDevice();
      device.addEventListener("uncapturederror", event => {
        uncaptured.push(event.error?.message ?? String(event.error));
      });
      let lost = null;
      const lostPromise = device.lost.then(info => {
        lost = { reason: info.reason, message: info.message };
        return "lost";
      });

      const shaderStarted = performance.now();
      const shaderResponse = await fetch(probe.shaderUrl);
      if (!shaderResponse.ok) {
        throw new Error(`could not fetch ${probe.shaderUrl}: ${shaderResponse.status}`);
      }
      const shaderSource = await shaderResponse.text();
      const module = device.createShaderModule({ code: shaderSource });
      const compilation = await module.getCompilationInfo();
      const compilationMessages = compilation.messages.map(message => ({
        type: message.type,
        line: message.lineNum,
        column: message.linePos,
        message: message.message,
      }));
      const shaderElapsedMs = performance.now() - shaderStarted;

      device.pushErrorScope("out-of-memory");
      device.pushErrorScope("internal");
      device.pushErrorScope("validation");
      const pipelineStarted = performance.now();
      const pipeline = await device.createComputePipelineAsync({
        layout: "auto",
        compute: { module, entryPoint: probe.entryPoint },
      });
      const pipelineElapsedMs = performance.now() - pipelineStarted;

      let dispatchElapsedMs = null;
      let readbackElapsedMs = null;
      let readbackWord = null;
      if (stage !== "compile") {
        const buffers = probe.bindings.map(binding => ({
          binding,
          buffer: device.createBuffer({
            size: Math.max(4, binding.byteLength),
            usage: GPUBufferUsage.STORAGE | GPUBufferUsage.COPY_SRC | GPUBufferUsage.COPY_DST,
          }),
        }));
        const bindGroup = device.createBindGroup({
          layout: pipeline.getBindGroupLayout(0),
          entries: buffers.map(({ binding, buffer }) => ({
            binding: binding.binding,
            resource: { buffer },
          })),
        });
        const dispatch = stage === "one" ? [1, 1, 1] : probe.dispatch;
        const encoder = device.createCommandEncoder();
        const pass = encoder.beginComputePass();
        pass.setPipeline(pipeline);
        pass.setBindGroup(0, bindGroup);
        pass.dispatchWorkgroups(...dispatch);
        pass.end();
        const dispatchStarted = performance.now();
        device.queue.submit([encoder.finish()]);
        const completion = await Promise.race([
          device.queue.onSubmittedWorkDone().then(() => "done"),
          lostPromise,
          new Promise(resolvePromise => setTimeout(() => resolvePromise("timeout"), 120_000)),
        ]);
        dispatchElapsedMs = performance.now() - dispatchStarted;
        if (completion !== "done") {
          for (const { buffer } of buffers) buffer.destroy();
          return {
            ok: false,
            stage,
            completion,
            lost,
            uncaptured,
            compilationMessages,
            shaderBytes: shaderSource.length,
            shaderElapsedMs,
            pipelineElapsedMs,
            dispatchElapsedMs,
          };
        }
        if (stage === "readback") {
          const source = buffers.find(({ binding }) => binding.name === "validity") ?? buffers[0];
          const staging = device.createBuffer({
            size: 4,
            usage: GPUBufferUsage.COPY_DST | GPUBufferUsage.MAP_READ,
          });
          const readbackEncoder = device.createCommandEncoder();
          readbackEncoder.copyBufferToBuffer(source.buffer, 0, staging, 0, 4);
          const readbackStarted = performance.now();
          device.queue.submit([readbackEncoder.finish()]);
          await staging.mapAsync(GPUMapMode.READ);
          readbackWord = new Uint32Array(staging.getMappedRange())[0];
          staging.unmap();
          staging.destroy();
          readbackElapsedMs = performance.now() - readbackStarted;
        }
        for (const { buffer } of buffers) buffer.destroy();
      }

      const scopedErrors = [
        await device.popErrorScope(),
        await device.popErrorScope(),
        await device.popErrorScope(),
      ].filter(Boolean).map(error => error.message);
      await new Promise(resolvePromise => setTimeout(resolvePromise, 250));
      return {
        ok: lost === null && uncaptured.length === 0 && scopedErrors.length === 0,
        stage,
        secureContext: window.isSecureContext,
        adapter: adapter.info ? {
          vendor: adapter.info.vendor,
          architecture: adapter.info.architecture,
          device: adapter.info.device,
          description: adapter.info.description,
        } : null,
        declaredShaderBytes: probe.shaderBytes,
        shaderBytes: shaderSource.length,
        compilationMessages,
        scopedErrors,
        uncaptured,
        lost,
        timingsMs: {
          shaderModule: shaderElapsedMs,
          pipeline: pipelineElapsedMs,
          dispatch: dispatchElapsedMs,
          readback: readbackElapsedMs,
          total: performance.now() - started,
        },
        readbackWord,
      };
    }, { stage: computeStageOnly, probe: computeProbe });
    assert.equal(result.secureContext ?? true, true);
    assert.equal(result.shaderBytes, result.declaredShaderBytes);
    assert.equal(result.compilationMessages.filter(message => message.type === "error").length, 0);
    assert.equal(result.ok, true, JSON.stringify(result));
    assert.deepEqual(browserErrors, []);
    reportPhase("compute-stage-complete", {
      stage: computeStageOnly,
      timingsMs: result.timingsMs,
    });
    console.log(JSON.stringify(result, null, 2));
  } else {
  reportPhase("surface-mount");
  await page.waitForFunction(() => {
    const surface = document.querySelector("fe-surface");
    return surface?.state === "ready" || surface?.state === "error";
  });
  const readyElapsedMs = performance.now() - harnessStarted;

  const boot = await page.evaluate(() => {
    const surface = document.querySelector("fe-surface");
    return {
      state: surface?.state,
      mode: surface?.mode,
      notice: surface?.shadowRoot?.querySelector(".notice")?.textContent ?? null,
      surfaces: document.querySelectorAll("fe-surface").length,
      passes: surface?.manifest?.passes?.map(pass => ({
        entry: pass.source_entry,
        dispatch: pass.dispatch ?? null,
        workgroup: pass.layout.workgroup_size,
      })),
      computeShaderBytes: surface?.manifest?.passes?.find(
        pass => pass.layout.mode === "compute",
      )?.shader_bytes ?? null,
      resources: surface?.manifest?.resources?.map(resource => ({
        name: resource.name,
        length: resource.length,
        element: resource.element,
        stride: resource.stride,
      })),
      evidence: globalThis.__feRoundProbe,
    };
  });
  assert.equal(boot.surfaces, 1, "the focused probe must mount exactly one Fe surface");
  if (boot.state === "error") {
    throw new Error(
      `round-interaction surface failed: ${boot.notice}\n` +
      `Fe events: ${JSON.stringify(boot.evidence)}\n` +
      `browser diagnostics: ${JSON.stringify(browserErrors)}`,
    );
  }
  assert.equal(boot.mode, "webgpu", "the round-interaction actor must use WebGPU");
  assert.deepEqual(
    boot.passes,
    [
      { entry: "write_round_locals", dispatch: [64, 1, 1], workgroup: [64, 1, 1] },
      { entry: "paint", dispatch: null, workgroup: [0, 0, 0] },
    ],
  );
  assert.deepEqual(
    boot.resources,
    [
      { name: "base_trace", length: 1_064_960, element: "U32", stride: 4 },
      { name: "challenge_output", length: 40, element: "U32", stride: 4 },
      { name: "interaction", length: 622_592, element: "U32", stride: 4 },
      { name: "validity", length: 4_096, element: "U32", stride: 4 },
    ],
  );

  const liveStarted = performance.now();
  await page.evaluate(async () => {
    const surface = document.querySelector("fe-surface");
    await surface.live();
    const gpu = surface._gpu;
    if (!gpu?.device) throw new Error("the live Fe surface has no WebGPU device");
    gpu.device.lost.then(info => {
      globalThis.__feRoundProbe.lost = {
        reason: info?.reason ?? null,
        message: info?.message ?? null,
      };
    });
    await gpu.device.queue.onSubmittedWorkDone();
  });
  const liveElapsedMs = performance.now() - liveStarted;

  const readback = await page.evaluate(async () => {
    const started = performance.now();
    const surface = document.querySelector("fe-surface");
    const gpu = surface._gpu;
    if (!gpu?.device || !gpu.resourceBuffers) {
      throw new Error("the live Fe pass graph is unavailable for test-only readback");
    }

    async function snapshot(name) {
      const resource = surface.manifest.resources.find(candidate => candidate.name === name);
      const source = gpu.resourceBuffers.get(name);
      if (!resource || !source || resource.element !== "U32" || resource.stride !== 4) {
        throw new Error(`resource ${name} is not a live canonical u32 buffer`);
      }
      const byteLength = resource.length * resource.stride;
      const staging = gpu.device.createBuffer({
        size: byteLength,
        usage: GPUBufferUsage.COPY_DST | GPUBufferUsage.MAP_READ,
      });
      const encoder = gpu.device.createCommandEncoder();
      encoder.copyBufferToBuffer(source, 0, staging, 0, byteLength);
      gpu.device.queue.submit([encoder.finish()]);
      await staging.mapAsync(GPUMapMode.READ);
      const bytes = new Uint8Array(staging.getMappedRange());
      const words = new Uint32Array(bytes.buffer, bytes.byteOffset, resource.length);
      let nonzero = 0;
      let ones = 0;
      for (const word of words) {
        if (word !== 0) nonzero += 1;
        if (word === 1) ones += 1;
      }
      const digestBytes = new Uint8Array(await crypto.subtle.digest("SHA-256", bytes));
      const digest = Array.from(
        digestBytes,
        byte => byte.toString(16).padStart(2, "0"),
      ).join("");
      const first = Array.from(words.slice(0, 16));
      const last = Array.from(words.slice(Math.max(0, words.length - 16)));
      staging.unmap();
      staging.destroy();
      return { name, words: resource.length, nonzero, ones, sha256: digest, first, last };
    }

    const [interaction, validity] = await Promise.all([
      snapshot("interaction"),
      snapshot("validity"),
    ]);
    await gpu.device.queue.onSubmittedWorkDone();
    return {
      interaction,
      validity,
      elapsedMs: performance.now() - started,
      adapter: gpu.adapter?.info ? {
        vendor: gpu.adapter.info.vendor,
        architecture: gpu.adapter.info.architecture,
        device: gpu.adapter.info.device,
        description: gpu.adapter.info.description,
      } : null,
      state: surface.state,
      mode: surface.mode,
    };
  });

  const evidence = await page.evaluate(() => globalThis.__feRoundProbe);
  assert.equal(readback.state, "live");
  assert.equal(readback.mode, "webgpu");
  assert.equal(evidence.ready, 1);
  assert.equal(evidence.lost, null, "the WebGPU device was lost during the probe");
  assert.deepEqual(evidence.errors, []);
  assert.deepEqual(browserErrors, []);

  console.log(JSON.stringify({
    ok: true,
    pageUrl,
    adapter: readback.adapter,
    shaderBytes: boot.computeShaderBytes,
    timingsMs: {
      launchToReady: Number(readyElapsedMs.toFixed(2)),
      readyToLiveAndIdle: Number(liveElapsedMs.toFixed(2)),
      readbackAndHash: Number(readback.elapsedMs.toFixed(2)),
    },
    interaction: readback.interaction,
    validity: readback.validity,
    events: evidence,
    exactness: "not asserted by this feasibility probe",
  }, null, 2));
  reportPhase("surface-complete", {
    timingsMs: {
      launchToReady: Number(readyElapsedMs.toFixed(2)),
      readyToLiveAndIdle: Number(liveElapsedMs.toFixed(2)),
      readbackAndHash: Number(readback.elapsedMs.toFixed(2)),
    },
  });
  }
} finally {
  if (page) await page.close().catch(() => {});
  if (traceSession) {
    try {
      const trace = await stopBrowserTrace(traceSession);
      console.log(JSON.stringify({ ok: true, kind: "Chrome trace", ...trace }, null, 2));
    } catch (error) {
      console.error(`could not collect Chrome trace: ${error?.stack ?? error}`);
    }
  }
  if (browser) browser.disconnect();
  const serverClosed = new Promise(resolvePromise => server.close(resolvePromise));
  server.closeAllConnections?.();
  await serverClosed;
}
