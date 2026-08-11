import assert from "node:assert/strict";
import { constants as fsConstants } from "node:fs";
import { access, readFile, readdir } from "node:fs/promises";
import { createServer } from "node:http";
import { resolve, sep } from "node:path";
import { pathToFileURL } from "node:url";

const siteRoot = resolve(process.argv[2] ?? "");
const tolerateUnavailableWebGpu = process.argv.includes("--allow-unavailable-webgpu");
async function exists(path) {
  try {
    await access(path, fsConstants.R_OK);
    return true;
  } catch {
    return false;
  }
}
if (!process.argv[2] || !(await exists(resolve(siteRoot, "index.html")))) {
  throw new Error("usage: node source_inspector.browser.mjs <precompiled-site>");
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
const server = createServer(async (request, response) => {
  try {
    const url = new URL(request.url, "http://127.0.0.1");
    const pathname = decodeURIComponent(url.pathname === "/" ? "/index.html" : url.pathname);
    const candidate = resolve(siteRoot, `.${pathname}`);
    if (candidate !== siteRoot && !candidate.startsWith(`${siteRoot}${sep}`)) {
      response.writeHead(403).end("outside site root");
      return;
    }
    if (!(await exists(candidate))) {
      response.writeHead(404).end("not found");
      return;
    }
    const extension = candidate.slice(candidate.lastIndexOf("."));
    response.writeHead(200, { "content-type": contentTypes.get(extension) ?? "application/octet-stream" });
    response.end(await readFile(candidate));
  } catch (error) {
    response.writeHead(500).end(String(error));
  }
});
await new Promise((resolvePromise, reject) => {
  server.once("error", reject);
  server.listen(0, "127.0.0.1", resolvePromise);
});
const address = server.address();
if (!address || typeof address === "string") throw new Error("test server has no port");

const imported = await loadPuppeteer();
const puppeteer = imported.puppeteer ?? imported.default ?? imported;
const browser = await puppeteer.launch({
  executablePath: await chromiumPath(),
  headless: true,
  args: ["--no-sandbox", "--disable-dev-shm-usage"],
});

const browserErrors = [];
try {
  const page = await browser.newPage();
  page.setDefaultTimeout(20_000);
  page.on("console", message => {
    if (message.type() === "error") browserErrors.push(`console: ${message.text()}`);
  });
  page.on("pageerror", error => browserErrors.push(`page: ${error.message}`));
  await page.evaluateOnNewDocument(() => {
    globalThis.__feInspectorE2E = { errors: [], states: [], prevented: [] };
    document.addEventListener("fe-error", event => {
      if (event.target?.id === "source-inspector" || event.target?.id === "gallery-shell") {
        globalThis.__feInspectorE2E.errors.push(String(event.detail?.stack ?? event.detail));
      }
    }, true);
    document.addEventListener("fe-state", event => {
      if (event.target?.id === "source-inspector" || event.target?.id === "gallery-shell") {
        globalThis.__feInspectorE2E.states.push(event.detail.state.slice());
      }
    });
    document.addEventListener("click", event => {
      const action = event.composedPath().find(node => node?.getAttribute?.("data-fe-action"))
        ?.getAttribute("data-fe-action");
      if (action) globalThis.__feInspectorE2E.prevented.push([action, event.defaultPrevented]);
    });
  });
  await page.goto(`http://127.0.0.1:${address.port}/`, { waitUntil: "networkidle0" });
  await page.waitForFunction(() => {
    const script = document.querySelector('script[data-fe-mount="#source-inspector"], script[data-fe-mount="#gallery-shell"]');
    const component = document.querySelector("#source-inspector, #gallery-shell");
    const surfaces = Array.from(document.querySelectorAll("fe-surface"));
    return script?.dataset.feState === "complete" && component?._active === true &&
      surfaces.length === 10 &&
      surfaces.every(surface => surface.shadowRoot?.querySelector('[data-fe-action="101"]'));
  });

  assert.deepEqual(await page.evaluate(() => ({
    title: document.title,
    pageMarker: Boolean(document.querySelector("script[data-fe-page]")),
    figures: document.querySelectorAll(".grid > figure").length,
    surfaces: document.querySelectorAll("fe-surface").length,
    components: document.querySelectorAll("fe-component").length,
    captions: Array.from(document.querySelectorAll(".grid > figure > figcaption > b"),
      node => node.textContent),
  })), {
    title: "Fe · GPU gallery",
    pageMarker: false,
    figures: 11,
    surfaces: 10,
    components: 2,
    captions: [
      "known color",
      "rollcall pipeline",
      "cga3d",
      "qcga",
      "desargues",
      "plasma",
      "mandelbrot",
      "perturbation mandelbrot",
      "dec",
      "gradient",
      "TodoMVC",
    ],
  });

  assert.equal(await page.$eval(".inspector, .source-inspector", node => node.hidden), true);
  await page.click('.gallery-head [data-fe-action="100"]');
  await page.waitForFunction(() =>
    !document.querySelector('[data-fe-view="1"]').hidden &&
    document.querySelector('[data-fe-node="100"]').textContent.includes("actor GalleryPage") &&
    document.querySelector('[data-fe-node="100"]').textContent.includes("struct GalleryBuilder")
  );
  assert.deepEqual(await page.evaluate(() => ({
    open: !document.querySelector(".inspector, .source-inspector").hidden,
    sourceTitle: !document.querySelector('[data-fe-view="5"]').hidden,
    focused: document.activeElement === document.querySelector('[data-fe-action="104"]'),
    stayed: location.pathname === "/",
  })), { open: true, sourceTitle: true, focused: true, stayed: true });

  await page.click('[data-fe-action="104"]');
  assert.equal(await page.$eval(".inspector, .source-inspector", node => node.hidden), true);
  await page.evaluate(() => Array.from(document.querySelectorAll("fe-surface"))
    .map(surface => surface.shadowRoot?.querySelector('[data-fe-action="101"]'))
    .find(Boolean).click());
  await page.waitForFunction(() =>
    !document.querySelector('[data-fe-view="1"]').hidden &&
    document.querySelector('[data-fe-node="100"]').textContent.includes("@fragment")
  );
  assert.equal(await page.$eval('[data-fe-view="6"]', node => node.hidden), false);

  const wasmExpected = await page.evaluate(async () => {
    const link = Array.from(document.querySelectorAll("fe-surface"))
      .map(surface => surface.shadowRoot?.querySelector('[data-fe-action="102"]'))
      .find(Boolean);
    const length = (await (await fetch(link.href)).arrayBuffer()).byteLength;
    link.click();
    return length;
  });
  await page.waitForFunction(expected =>
    !document.querySelector('[data-fe-view="2"]').hidden &&
    Number(document.querySelector('[data-fe-node="101"]').textContent) === expected,
    {}, wasmExpected,
  );
  assert.equal(await page.$eval('[data-fe-view="7"]', node => node.hidden), false);

  await page.evaluate(() => Array.from(document.querySelectorAll("fe-surface"))
    .map(surface => surface.shadowRoot?.querySelector('[data-fe-action="103"]'))
    .find(Boolean).click());
  await page.waitForFunction(() =>
    !document.querySelector('[data-fe-view="1"]').hidden &&
    document.querySelector('[data-fe-node="100"]').textContent.includes('"protocol": "fe-web-bundle"')
  );
  assert.equal(await page.$eval('[data-fe-view="8"]', node => node.hidden), false);

  await page.keyboard.press("Escape");
  assert.equal(await page.$eval(".inspector, .source-inspector", node => node.hidden), true);
  const evidence = await page.evaluate(() => globalThis.__feInspectorE2E);
  assert.deepEqual(evidence.errors, []);
  assert.ok(evidence.states.length >= 10, `too few Fe states: ${evidence.states.length}`);
  for (const action of ["100", "101", "102", "103", "104"]) {
    assert.ok(evidence.prevented.some(value => value[0] === action && value[1] === true),
      `Fe did not prevent default for action ${action}`);
  }
  const unexpectedBrowserErrors = tolerateUnavailableWebGpu
    ? browserErrors.filter(error => !error.includes("no WebGPU adapter is available"))
    : browserErrors;
  assert.deepEqual(unexpectedBrowserErrors, []);
  console.log("ok: Fe-composed gallery and resident SourceInspector behavior");
} finally {
  await browser.close();
  await new Promise(resolvePromise => server.close(resolvePromise));
}
