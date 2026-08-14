import assert from "node:assert/strict";
import { constants as fsConstants } from "node:fs";
import { access, readFile, readdir } from "node:fs/promises";
import { createServer } from "node:http";
import { resolve, sep } from "node:path";
import { pathToFileURL } from "node:url";

const siteRoot = resolve(process.argv[2] ?? "");

async function exists(path) {
  try {
    await access(path, fsConstants.R_OK);
    return true;
  } catch {
    return false;
  }
}

if (!process.argv[2] || !(await exists(resolve(siteRoot, "index.html")))) {
  throw new Error("usage: node event_studio.browser.mjs <precompiled-event-studio-site>");
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
  if (process.env.FE_PUPPETEER_MODULE) {
    return import(pathToFileURL(resolve(process.env.FE_PUPPETEER_MODULE)).href);
  }
  try {
    return await import("puppeteer");
  } catch {
    const bundled = await firstNixStorePath(
      "-chrome-devtools-mcp-",
      "lib/chrome-devtools-mcp/build/src/third_party/index.js",
    );
    if (!bundled) throw new Error("Puppeteer is unavailable; set FE_PUPPETEER_MODULE");
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
  throw new Error("Chromium is unavailable; set FE_CHROMIUM_BIN");
}

const contentTypes = new Map([
  [".html", "text/html; charset=utf-8"],
  [".js", "text/javascript; charset=utf-8"],
  [".json", "application/json; charset=utf-8"],
  [".wasm", "application/wasm"],
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
    response.writeHead(200, {
      "content-type": contentTypes.get(extension) ?? "application/octet-stream",
    });
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
  page.setDefaultTimeout(15_000);
  await page.setViewport({ width: 640, height: 480, deviceScaleFactor: 1.5 });
  page.on("console", message => {
    if (message.type() === "error") browserErrors.push(`console: ${message.text()}`);
  });
  page.on("pageerror", error => browserErrors.push(`page: ${error.message}`));
  await page.evaluateOnNewDocument(() => {
    globalThis.__feEventStudioE2E = { states: [], errors: [] };
    document.addEventListener("fe-state", event => {
      if (event.target?.id === "event-studio") {
        globalThis.__feEventStudioE2E.states.push(event.detail.state.slice());
      }
    });
    document.addEventListener("fe-error", event => {
      if (event.target?.id === "event-studio") {
        globalThis.__feEventStudioE2E.errors.push(String(event.detail?.stack ?? event.detail));
      }
    }, true);
    globalThis.addEventListener("unhandledrejection", event => {
      globalThis.__feEventStudioE2E.errors.push(String(event.reason?.stack ?? event.reason));
    });
  });
  await page.goto(`http://127.0.0.1:${address.port}/`, { waitUntil: "domcontentloaded" });

  const readStudio = () => page.evaluate(() => {
    const values = document.querySelectorAll("#event-studio .event-studio-grid strong");
    return {
      width: Number(values[0]?.textContent),
      height: Number(values[1]?.textContent),
      devicePixelRatioPercent: Number(values[2]?.textContent),
      observations: Number(values[3]?.textContent),
      failures: Number(values[4]?.textContent),
      states: globalThis.__feEventStudioE2E.states.length,
      errors: globalThis.__feEventStudioE2E.errors,
    };
  });
  await page.waitForFunction(() => {
    const script = document.querySelector('script[data-fe-mount="#event-studio"]');
    const component = document.querySelector("#event-studio");
    const values = component?.querySelectorAll(".event-studio-grid strong");
    return script?.dataset.feState === "complete" && component?._active === true
      && Number(values?.[3]?.textContent) >= 1;
  });
  const initial = await readStudio();
  assert.deepEqual(initial, {
    width: 640,
    height: 480,
    devicePixelRatioPercent: 150,
    observations: 1,
    failures: 0,
    states: 5,
    errors: [],
  });

  await page.setViewport({ width: 777, height: 555, deviceScaleFactor: 2 });
  await page.waitForFunction(() => {
    const values = document.querySelectorAll("#event-studio .event-studio-grid strong");
    return values[0]?.textContent === "777" && values[1]?.textContent === "555"
      && values[2]?.textContent === "200" && values[3]?.textContent === "2";
  });
  assert.deepEqual(await readStudio(), {
    width: 777,
    height: 555,
    devicePixelRatioPercent: 200,
    observations: 2,
    failures: 0,
    states: 9,
    errors: [],
  });

  // Reconnection cancels the old affine pull, starts one fresh scoped task,
  // and observes the current standards state exactly once.
  await page.evaluate(() => {
    const component = document.querySelector("#event-studio");
    const marker = document.createComment("event-studio-position");
    component.before(marker);
    component.remove();
    marker.replaceWith(component);
  });
  await page.waitForFunction(() => {
    const values = document.querySelectorAll("#event-studio .event-studio-grid strong");
    return document.querySelector("#event-studio")?._active === true
      && values[3]?.textContent === "3";
  });
  assert.deepEqual(await readStudio(), {
    width: 777,
    height: 555,
    devicePixelRatioPercent: 200,
    observations: 3,
    failures: 0,
    // The disconnected projection is dispatched after detachment, so the
    // document-level oracle observes reconnect + four task messages only.
    states: 14,
    errors: [],
  });
  assert.deepEqual(browserErrors, []);
  console.log("ok: Fe Event Studio typed viewport stream, resize, DPR, and lifecycle");
} finally {
  await browser.close();
  await new Promise(resolvePromise => server.close(resolvePromise));
}
