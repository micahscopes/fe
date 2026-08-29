import assert from "node:assert/strict";
import { constants as fsConstants } from "node:fs";
import { access, mkdtemp, readFile, readdir, rm, writeFile } from "node:fs/promises";
import { createServer } from "node:http";
import { extname, resolve, sep } from "node:path";
import { pathToFileURL } from "node:url";

const siteRoot = resolve(process.argv[2] ?? "");
const remoteBrowserUrl = process.env.FE_BROWSER_URL ?? null;
const browserSiteHost = process.env.FE_BROWSER_HOST ?? "127.0.0.1";
const interceptedOrigin = process.env.FE_BROWSER_ORIGIN ?? null;

async function exists(path) {
  try {
    await access(path, fsConstants.R_OK);
    return true;
  } catch {
    return false;
  }
}

if (!process.argv[2] || !(await exists(resolve(siteRoot, "index.html")))) {
  throw new Error("usage: node mandelbrot_proof_lab.browser.mjs <precompiled-site>");
}

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

async function chromiumPath() {
  if (process.env.FE_CHROMIUM_BIN) return resolve(process.env.FE_CHROMIUM_BIN);
  for (const name of ["chromium", "google-chrome", "chrome"]) {
    for (const directory of (process.env.PATH ?? "").split(":")) {
      const candidate = resolve(directory, name);
      if (await exists(candidate)) return candidate;
    }
  }
  const wrapped = await firstNixStorePath("-chromium-", "bin/chromium");
  if (wrapped) return wrapped;
  throw new Error("Chromium is unavailable");
}

const contentTypes = new Map([
  [".html", "text/html; charset=utf-8"],
  [".js", "text/javascript; charset=utf-8"],
  [".json", "application/json; charset=utf-8"],
  [".wasm", "application/wasm"],
  [".wgsl", "text/plain; charset=utf-8"],
  [".fe", "text/plain; charset=utf-8"],
]);

async function siteAsset(value) {
  const url = new URL(value, "http://fe-proof.test");
  if (url.pathname === "/favicon.ico") {
    return { status: 204, body: Buffer.alloc(0) };
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
  server.listen(0, remoteBrowserUrl ? "0.0.0.0" : "127.0.0.1", resolvePromise);
});
const address = server.address();
if (!address || typeof address === "string") throw new Error("test server has no port");

const imported = await loadPuppeteer();
const puppeteer = imported.puppeteer ?? imported.default ?? imported;
const localProfile = remoteBrowserUrl
  ? null
  : await mkdtemp(resolve(
    process.env.FE_BROWSER_PROFILE_ROOT ?? "/workspace/scratch",
    "mb2-proof-browser-profile-",
  ));
const browserErrors = [];
let browser;
let page;
try {
  browser = remoteBrowserUrl
    ? await puppeteer.connect({ browserURL: remoteBrowserUrl })
    : await puppeteer.launch({
      executablePath: await chromiumPath(),
      userDataDir: localProfile,
      headless: true,
      args: [
        "--no-sandbox",
        "--disable-dev-shm-usage",
        "--enable-unsafe-webgpu",
        "--use-angle=swiftshader",
        "--enable-features=Vulkan,UseSkiaRenderer",
      ],
    });
  page = await browser.newPage();
  if (interceptedOrigin) {
    const origin = new URL(interceptedOrigin).origin;
    await page.setRequestInterception(true);
    page.on("request", request => {
      const url = new URL(request.url());
      if (url.origin !== origin) {
        void request.continue();
        return;
      }
      void siteAsset(url).then(asset => request.respond({
        status: asset.status,
        contentType: asset.contentType,
        body: asset.body,
      })).catch(() => request.abort("failed"));
    });
  }
  page.setDefaultTimeout(600_000);
  page.on("console", message => {
    if (message.type() === "error") browserErrors.push(`console: ${message.text()}`);
  });
  page.on("pageerror", error => browserErrors.push(`page: ${error.message}`));
  await page.evaluateOnNewDocument(() => {
    globalThis.__feProofLab = { errors: [], ready: 0, frames: 0, states: [] };
    const recordError = error => globalThis.__feProofLab.errors.push(
      String(error?.stack ?? error),
    );
    globalThis.addEventListener("fe:bootstrap-error", event => recordError(event.detail));
    document.addEventListener("fe:error", event => recordError(event.detail), true);
    document.addEventListener("fe-error", event => recordError(event.detail), true);
    document.addEventListener("fe-ready", event => {
      if (event.target?.tagName === "FE-SURFACE") globalThis.__feProofLab.ready += 1;
    }, true);
    document.addEventListener("fe-frame", event => {
      if (event.target?.tagName === "FE-SURFACE") globalThis.__feProofLab.frames += 1;
    }, true);
    document.addEventListener("fe-statechange", event => {
      if (event.target?.tagName === "FE-SURFACE") {
        globalThis.__feProofLab.states.push(event.detail?.state ?? event.target.state);
      }
    }, true);
  });

  const pageUrl = interceptedOrigin ?? `http://${browserSiteHost}:${address.port}/`;
  await page.goto(pageUrl, { waitUntil: "domcontentloaded", timeout: 120_000 });
  await page.waitForFunction(() => {
    const surface = document.querySelector("fe-surface");
    return surface?.state === "ready" || surface?.state === "error";
  });

  const boot = await page.evaluate(async () => {
    const surface = document.querySelector("fe-surface");
    const adapter = await navigator.gpu?.requestAdapter();
    return {
      state: surface?.state,
      mode: surface?.mode,
      notice: surface?.shadowRoot?.querySelector(".notice")?.textContent ?? null,
      surfaces: document.querySelectorAll("fe-surface").length,
      passes: surface?.manifest?.passes?.map(pass => ({
        entry: pass.source_entry,
        repeat: pass.repeat ?? 1,
        workgroup: pass.layout.workgroup_size,
      })),
      adapter: adapter?.info ? {
        vendor: adapter.info.vendor,
        architecture: adapter.info.architecture,
        device: adapter.info.device,
        description: adapter.info.description,
      } : null,
    };
  });
  assert.equal(boot.surfaces, 1, "the focused lab must mount exactly one Fe surface");
  if (boot.state === "error") throw new Error(`proof surface failed: ${boot.notice}`);
  assert.equal(boot.mode, "webgpu", "the proof graph must execute through WebGPU");
  assert.deepEqual(
    boot.passes.map(pass => pass.entry),
    [
      "derive_witness",
      "prepare_lde_inverse",
      "advance_lde_inverse",
      "prepare_lde_forward",
      "advance_lde_forward",
      "finish_lde",
      "initialize_commitments",
      "advance_commitment_rounds",
      "finalize_commitments",
      "initialize_fri_schedule",
      "advance_fri_round_1",
      "advance_fri_round_2",
      "advance_fri_round_3",
      "advance_fri_round_4",
      "finalize_fri_schedule",
      "display",
    ],
    "the browser did not receive the expected Fe-derived proof schedule",
  );
  assert.equal(boot.passes[7].repeat, 396);
  assert.deepEqual(
    boot.passes.slice(10, 14).map(pass => pass.repeat),
    [403, 358, 313, 268],
  );
  assert.deepEqual(
    boot.passes.slice(9, 14).map(pass => pass.workgroup),
    Array.from({ length: 5 }, () => [256, 1, 1]),
  );

  const BLUE = [87, 117, 226, 255];
  const PINK = [255, 176, 222, 255];
  async function setModeAndSample(tamper) {
    return page.evaluate(async requested => {
      const surface = document.querySelector("fe-surface");
      await surface.live();
      const frame = new Promise(resolvePromise => {
        surface.addEventListener("fe-frame", resolvePromise, { once: true });
      });
      surface.params.tamper = requested;
      await frame;
      const gpu = surface._gpu;
      const source = gpu?.resourceBuffers?.get("proof");
      const proofResource = surface.manifest?.resources?.find(resource =>
        resource.name === "proof"
      );
      const context = surface._liveContext;
      if (!gpu?.device || !source || !proofResource || !context) {
        throw new Error("the live Fe proof graph is unavailable for test-only readback");
      }
      if (proofResource.element !== "U32" ||
          proofResource.stride !== Uint32Array.BYTES_PER_ELEMENT) {
        throw new Error("the Fe proof resource is not a canonical u32 tape");
      }
      const byteLength = proofResource.length * proofResource.stride;
      const proofStaging = gpu.device.createBuffer({
        size: byteLength,
        usage: GPUBufferUsage.COPY_DST | GPUBufferUsage.MAP_READ,
      });
      const canvas = surface._liveCanvas;
      const width = canvas.width;
      const height = canvas.height;
      const bytesPerRow = Math.ceil((width * 4) / 256) * 256;
      const pixelStaging = gpu.device.createBuffer({
        size: bytesPerRow * height,
        usage: GPUBufferUsage.COPY_DST | GPUBufferUsage.MAP_READ,
      });
      const encoder = gpu.device.createCommandEncoder();
      encoder.copyBufferToBuffer(source, 0, proofStaging, 0, byteLength);
      encoder.copyTextureToBuffer(
        { texture: context.getCurrentTexture() },
        { buffer: pixelStaging, bytesPerRow, rowsPerImage: height },
        { width, height, depthOrArrayLayers: 1 },
      );
      gpu.device.queue.submit([encoder.finish()]);
      await Promise.all([
        proofStaging.mapAsync(GPUMapMode.READ),
        pixelStaging.mapAsync(GPUMapMode.READ),
      ]);
      const proof = Array.from(new Uint32Array(proofStaging.getMappedRange()).slice());
      const pixels = new Uint8Array(pixelStaging.getMappedRange());
      const bgra = gpu.format.startsWith("bgra");
      const y = Math.floor(height * 0.09);
      const bands = [0.1, 0.3, 0.5, 0.7, 0.9].map(fraction => {
        const offset = y * bytesPerRow + Math.floor(width * fraction) * 4;
        return [
          pixels[offset + (bgra ? 2 : 0)],
          pixels[offset + 1],
          pixels[offset + (bgra ? 0 : 2)],
          pixels[offset + 3],
        ];
      });
      proofStaging.unmap();
      proofStaging.destroy();
      pixelStaging.unmap();
      pixelStaging.destroy();
      return {
        bands,
        state: surface.state,
        mode: surface.mode,
        tamper: surface.params.tamper,
        proof,
      };
    }, tamper);
  }

  const clean = await setModeAndSample(0);
  assert.deepEqual(clean.bands, [BLUE, BLUE, BLUE, BLUE, BLUE]);
  assert.equal(clean.state, "live");
  assert.equal(clean.mode, "webgpu");
  assert.equal(clean.tamper, 0);

  const tampered = await setModeAndSample(1);
  assert.deepEqual(tampered.bands, [BLUE, BLUE, BLUE, PINK, PINK]);
  assert.equal(tampered.tamper, 1);

  const recovered = await setModeAndSample(0);
  assert.deepEqual(recovered.bands, [BLUE, BLUE, BLUE, BLUE, BLUE]);
  assert.equal(recovered.tamper, 0);
  assert.deepEqual(recovered.proof, clean.proof, "clean recovery must reproduce the receipt");
  const frozenState = await page.evaluate(async () => {
    const surface = document.querySelector("fe-surface");
    await surface.freeze();
    return surface.state;
  });
  assert.equal(frozenState, "frozen");

  if (process.env.MB2_BROWSER_PROOF_RECEIPTS) {
    await writeFile(
      resolve(process.env.MB2_BROWSER_PROOF_RECEIPTS),
      `${JSON.stringify({
        clean: clean.proof,
        tampered: tampered.proof,
        recovered: recovered.proof,
      })}\n`,
    );
  }

  const evidence = await page.evaluate(() => globalThis.__feProofLab);
  assert.equal(evidence.ready, 1);
  assert.deepEqual(evidence.errors, []);
  assert.deepEqual(browserErrors, []);
  console.log(JSON.stringify({
    ok: true,
    adapter: boot.adapter,
    passes: boot.passes.length,
    clean: clean.bands,
    tampered: tampered.bands,
    recovered: recovered.bands,
    receiptWords: clean.proof.length,
    frames: evidence.frames,
    states: evidence.states,
    finalState: frozenState,
  }, null, 2));
} finally {
  if (page) await page.close();
  if (browser && remoteBrowserUrl) browser.disconnect();
  else if (browser) await browser.close();
  await new Promise(resolvePromise => server.close(resolvePromise));
  if (localProfile) await rm(localProfile, { recursive: true, force: true });
}
