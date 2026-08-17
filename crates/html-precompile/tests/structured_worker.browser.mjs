import assert from "node:assert/strict";
import { constants as fsConstants } from "node:fs";
import { access, readFile, readdir } from "node:fs/promises";
import { createServer } from "node:http";
import { extname, resolve, sep } from "node:path";
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
  throw new Error(
    "usage: node structured_worker.browser.mjs <precompiled-structured-worker-site>",
  );
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

const served = [];
const server = createServer(async (request, response) => {
  try {
    const url = new URL(request.url, "http://127.0.0.1");
    const pathname = decodeURIComponent(url.pathname === "/" ? "/index.html" : url.pathname);
    if (pathname === "/favicon.ico") {
      response.writeHead(204).end();
      return;
    }
    const candidate = resolve(siteRoot, `.${pathname}`);
    if (candidate !== siteRoot && !candidate.startsWith(`${siteRoot}${sep}`)) {
      response.writeHead(403).end("outside site root");
      return;
    }
    if (!(await exists(candidate))) {
      response.writeHead(404).end("not found");
      return;
    }
    served.push(pathname);
    response.writeHead(200, {
      "content-type": contentTypes.get(extname(candidate)) ?? "application/octet-stream",
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

const errors = [];
try {
  const page = await browser.newPage();
  page.setDefaultTimeout(15_000);
  page.on("console", message => {
    if (message.type() === "error") errors.push(`console: ${message.text()}`);
  });
  page.on("pageerror", error => errors.push(`page: ${error.message}`));
  page.on("requestfailed", request => {
    errors.push(`request: ${request.url()} (${request.failure()?.errorText ?? "failed"})`);
  });
  await page.evaluateOnNewDocument(() => {
    globalThis.__feStructuredWorkerErrors = [];
    document.addEventListener("fe-error", event => {
      globalThis.__feStructuredWorkerErrors.push(
        String(event.detail?.stack ?? event.detail),
      );
    }, true);
    document.addEventListener("fe:error", event => {
      globalThis.__feStructuredWorkerErrors.push(
        String(event.detail?.stack ?? event.detail),
      );
    }, true);
    globalThis.addEventListener("unhandledrejection", event => {
      globalThis.__feStructuredWorkerErrors.push(
        String(event.reason?.stack ?? event.reason),
      );
    });
  });
  await page.goto(`http://127.0.0.1:${address.port}/`, {
    waitUntil: "domcontentloaded",
  });
  try {
    await page.waitForFunction(() => {
      const script = document.querySelector("script[data-fe-component]");
      const component = document.querySelector("#app");
      return script?.dataset.feState === "complete" && component?._active === true;
    });
  } catch (error) {
    const snapshot = await page.evaluate(() => {
      const script = document.querySelector("script[data-fe-component]");
      const component = document.querySelector("#app");
      return {
        state: script?.dataset.feState ?? null,
        active: component?._active ?? null,
        errors: globalThis.__feStructuredWorkerErrors,
      };
    });
    throw new Error(
      `structured Worker component did not become ready: ${JSON.stringify({
        snapshot,
        errors,
        served,
        cause: String(error),
      })}`,
    );
  }
  try {
    await page.waitForFunction(() => {
      const component = document.querySelector("#app");
      return component?._state?.[0] === 42;
    });
  } catch (error) {
    const snapshot = await page.evaluate(() => ({
      errors: globalThis.__feStructuredWorkerErrors,
      state: Array.from(document.querySelector("#app")?._state ?? []),
    }));
    throw new Error(
      `typed Worker mailbox did not produce resident state 42: ${JSON.stringify({
        snapshot,
        errors,
        served,
        cause: String(error),
      })}`,
    );
  }
  const observed = await page.evaluate(() => ({
    errors: globalThis.__feStructuredWorkerErrors,
    state: Array.from(document.querySelector("#app")?._state ?? []),
  }));
  const componentErrors = observed.errors;
  assert.deepEqual(componentErrors, []);
  assert.deepEqual(errors, []);
  assert.equal(observed.state[0], 42, "typed Worker mailbox must compute 21 -> 42");
  assert.equal(served.filter(path => path.endsWith("/child.wasm")).length, 1);
  assert.equal(served.filter(path => path.endsWith("/runtime/worker-host.js")).length, 1);
  assert.ok(served.filter(path => path.endsWith("/interface.js")).length >= 2);
  console.log("structured Fe parent and compiler-derived Worker child: ok");
} finally {
  await browser.close();
  await new Promise(resolvePromise => server.close(resolvePromise));
}
