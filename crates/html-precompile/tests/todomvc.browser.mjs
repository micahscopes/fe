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
  throw new Error("usage: node todomvc.browser.mjs <precompiled-todomvc-site>");
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
    if (!bundled) {
      throw new Error("Puppeteer is unavailable; set FE_PUPPETEER_MODULE");
    }
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
    let pathname;
    try {
      pathname = decodeURIComponent(url.pathname === "/" ? "/index.html" : url.pathname);
    } catch {
      response.writeHead(400).end("bad path");
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
const serverAddress = server.address();
if (!serverAddress || typeof serverAddress === "string") throw new Error("test server has no TCP port");

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
  page.on("console", message => {
    if (message.type() === "error") browserErrors.push(`console: ${message.text()}`);
  });
  page.on("pageerror", error => browserErrors.push(`page: ${error.message}`));
  await page.evaluateOnNewDocument(() => {
    globalThis.__feTodoE2E = { stateCount: 0, states: [], errors: [] };
    document.addEventListener("fe-state", event => {
      if (event.target?.id !== "todo-app" && event.target?.id !== "gallery-todomvc") return;
      globalThis.__feTodoE2E.stateCount += 1;
      globalThis.__feTodoE2E.states.push({
        state: event.detail.state.slice(),
        patch: event.detail.patch.slice(),
      });
    });
    document.addEventListener("fe-error", event => {
      if (event.target?.id === "todo-app" || event.target?.id === "gallery-todomvc") {
        globalThis.__feTodoE2E.errors.push(String(event.detail?.stack ?? event.detail));
      }
    }, true);
    globalThis.addEventListener("fe:bootstrap-error", event => {
      if (!globalThis.__feTodoTolerateUnavailableWebGpu) {
        globalThis.__feTodoE2E.errors.push(String(event.detail?.stack ?? event.detail));
      }
    });
    globalThis.addEventListener("unhandledrejection", event => {
      globalThis.__feTodoE2E.errors.push(String(event.reason?.stack ?? event.reason));
    });
  });
  await page.evaluateOnNewDocument(value => {
    globalThis.__feTodoTolerateUnavailableWebGpu = value;
  }, tolerateUnavailableWebGpu);

  await page.goto(`http://127.0.0.1:${serverAddress.port}/`, { waitUntil: "networkidle0" });
  try {
    await page.waitForFunction(() => {
      const script = document.querySelector('script[type="application/fe+wasm"][data-fe-mount="#todo-app"], script[type="application/fe+wasm"][data-fe-mount="#gallery-todomvc"]');
      const component = document.querySelector("#todo-app, #gallery-todomvc");
      return script?.dataset.feState === "complete" && component?._active === true;
    });
  } catch (error) {
    const diagnosis = await page.evaluate(() => {
      const script = document.querySelector('script[type="application/fe+wasm"][data-fe-mount="#todo-app"], script[type="application/fe+wasm"][data-fe-mount="#gallery-todomvc"]');
      const component = document.querySelector("#todo-app, #gallery-todomvc");
      return {
        scriptState: script?.dataset.feState ?? null,
        componentActive: component?._active ?? null,
        componentError: component?._error?.stack ?? String(component?._error ?? ""),
        testErrors: globalThis.__feTodoE2E?.errors ?? [],
      };
    });
    throw new Error(`TodoMVC did not boot: ${JSON.stringify(diagnosis)}`, { cause: error });
  }

  const initial = await page.evaluate(() => ({
    mainHidden: document.querySelector(".main").hidden,
    footerHidden: document.querySelector(".footer").hidden,
    rows: document.querySelectorAll(".todo-list > li").length,
    stateCount: globalThis.__feTodoE2E.stateCount,
  }));
  assert.deepEqual(initial, { mainHidden: true, footerHidden: true, rows: 0, stateCount: 1 });

  // Real keyboard input covers the controlled-input/caret path as well as the
  // fixed host's key event transport.
  await page.focus(".new-todo");
  await page.keyboard.type("  alpha  ");
  await page.keyboard.press("Enter");
  if (process.env.FE_E2E_DEBUG) {
    console.log(await page.evaluate(() => ({
      html: document.querySelector("#todo-app, #gallery-todomvc").innerHTML,
      latest: globalThis.__feTodoE2E.states.at(-1),
      stateCount: globalThis.__feTodoE2E.stateCount,
    })));
  }
  assert.deepEqual(await page.evaluate(() => ({
    value: document.querySelector(".new-todo").value,
    titles: Array.from(document.querySelectorAll(".todo-list label"), node => node.textContent),
    keys: Array.from(document.querySelectorAll(".todo-list > li"), node => node.dataset.feKey),
    active: document.querySelector(".todo-count strong").textContent,
    mainHidden: document.querySelector(".main").hidden,
  })), {
    value: "",
    titles: ["alpha"],
    keys: ["1"],
    active: "1",
    mainHidden: false,
  });

  const unicodeCanceled = await page.evaluate(() => {
    const input = document.querySelector(".new-todo");
    input.value = "  βeta 🌍  ";
    input.dispatchEvent(new InputEvent("input", { bubbles: true, inputType: "insertText" }));
    return !input.dispatchEvent(new KeyboardEvent("keydown", {
      key: "Enter", code: "Enter", keyCode: 13, which: 13, bubbles: true, cancelable: true,
    }));
  });
  assert.equal(unicodeCanceled, true, "Fe requested preventDefault for new-todo Enter");
  assert.deepEqual(await page.evaluate(() => ({
    titles: Array.from(document.querySelectorAll(".todo-list label"), node => node.textContent),
    keys: Array.from(document.querySelectorAll(".todo-list > li"), node => node.dataset.feKey),
    active: document.querySelector(".todo-count strong").textContent,
  })), { titles: ["alpha", "βeta 🌍"], keys: ["1", "2"], active: "2" });

  await page.click(".toggle-all-label");
  assert.deepEqual(await page.evaluate(() => ({
    completed: Array.from(document.querySelectorAll(".todo-list > li"), node =>
      node.classList.contains("completed")),
    checked: Array.from(document.querySelectorAll(".todo-list .toggle"), node => node.checked),
    master: document.querySelector(".toggle-all").checked,
    active: document.querySelector(".todo-count strong").textContent,
  })), { completed: [true, true], checked: [true, true], master: true, active: "0" });
  await page.click(".toggle-all-label");
  assert.deepEqual(await page.evaluate(() => ({
    completed: Array.from(document.querySelectorAll(".todo-list > li"), node =>
      node.classList.contains("completed")),
    checked: Array.from(document.querySelectorAll(".todo-list .toggle"), node => node.checked),
    master: document.querySelector(".toggle-all").checked,
    active: document.querySelector(".todo-count strong").textContent,
  })), { completed: [false, false], checked: [false, false], master: false, active: "2" });

  await page.evaluate(() => {
    // Row two remains projected when the active filter removes completed row
    // one, so its identity must survive the keyed reconciliation.
    document.querySelector('[data-fe-key="2"]').dataset.e2eIdentity = "kept";
  });
  await page.click('[data-fe-key="1"] .toggle');
  assert.deepEqual(await page.evaluate(() => ({
    completed: document.querySelector('[data-fe-key="1"]').classList.contains("completed"),
    checked: document.querySelector('[data-fe-key="1"] .toggle').checked,
    active: document.querySelector(".todo-count strong").textContent,
  })), { completed: true, checked: true, active: "1" });

  await page.click('[data-fe-action="5"]');
  assert.deepEqual(await page.evaluate(() => ({
    keys: Array.from(document.querySelectorAll(".todo-list > li"), node => node.dataset.feKey),
    identity: document.querySelector('[data-fe-key="2"]').dataset.e2eIdentity,
  })), { keys: ["2"], identity: "kept" });
  await page.click('[data-fe-action="4"]');
  assert.deepEqual(await page.evaluate(() =>
    Array.from(document.querySelectorAll(".todo-list > li"), node => node.dataset.feKey)
  ), ["1", "2"]);

  await page.click('[data-fe-key="1"] .edit-button');
  assert.deepEqual(await page.evaluate(() => ({
    editing: document.querySelector('[data-fe-key="1"]').classList.contains("editing"),
    focused: document.activeElement === document.querySelector('[data-fe-key="1"] .edit'),
    value: document.querySelector('[data-fe-key="1"] .edit').value,
  })), { editing: true, focused: true, value: "alpha" });

  // Every input event reprojects the Fe draft. If the generic adapter rewrites
  // an unchanged value and resets the caret, this becomes a scrambled title.
  await page.keyboard.down("Control");
  await page.keyboard.press("KeyA");
  await page.keyboard.up("Control");
  await page.keyboard.type("  gamma  ");
  await page.keyboard.press("Enter");
  assert.deepEqual(await page.evaluate(() => ({
    title: document.querySelector('[data-fe-key="1"] label').textContent,
    editing: document.querySelector('[data-fe-key="1"]').classList.contains("editing"),
  })), { title: "gamma", editing: false });

  await page.click('[data-fe-key="1"] label', { count: 2 });
  await page.keyboard.down("Control");
  await page.keyboard.press("KeyA");
  await page.keyboard.up("Control");
  await page.keyboard.type("discard me");
  await page.keyboard.press("Escape");
  assert.deepEqual(await page.evaluate(() => ({
    title: document.querySelector('[data-fe-key="1"] label').textContent,
    editing: document.querySelector('[data-fe-key="1"]').classList.contains("editing"),
  })), { title: "gamma", editing: false });

  // Repeated disconnect/reconnect must retain Fe state and replace, not stack,
  // the browser listener subscription.
  await page.evaluate(async () => {
    const component = document.querySelector("#todo-app, #gallery-todomvc");
    for (let index = 0; index < 3; index += 1) {
      const marker = document.createComment("component-position");
      component.before(marker);
      component.remove();
      marker.replaceWith(component);
      await Promise.resolve();
    }
    globalThis.__feTodoE2E.beforeSingleClick = globalThis.__feTodoE2E.stateCount;
    document.querySelector('[data-fe-action="6"]').click();
  });
  assert.deepEqual(await page.evaluate(() => ({
    delta: globalThis.__feTodoE2E.stateCount - globalThis.__feTodoE2E.beforeSingleClick,
    keys: Array.from(document.querySelectorAll(".todo-list > li"), node => node.dataset.feKey),
    title: document.querySelector('[data-fe-key="1"] label').textContent,
  })), { delta: 1, keys: ["1"], title: "gamma" });

  await page.click('[data-fe-action="4"]');
  await page.click('[data-fe-key="2"] .destroy');
  assert.deepEqual(await page.evaluate(() =>
    Array.from(document.querySelectorAll(".todo-list > li"), node => node.dataset.feKey)
  ), ["1"]);
  await page.click('[data-fe-action="3"]');
  assert.deepEqual(await page.evaluate(() => ({
    rows: document.querySelectorAll(".todo-list > li").length,
    mainHidden: document.querySelector(".main").hidden,
    footerHidden: document.querySelector(".footer").hidden,
    stateShapes: globalThis.__feTodoE2E.states.every(entry =>
      entry.state.length === 13 && entry.patch.length === 5),
    componentErrors: globalThis.__feTodoE2E.errors,
  })), {
    rows: 0,
    mainHidden: true,
    footerHidden: true,
    stateShapes: true,
    componentErrors: [],
  });

  const boundary = await page.evaluate(() => {
    const input = document.querySelector(".new-todo");
    input.value = `${"a".repeat(95)}🌍`;
    input.dispatchEvent(new InputEvent("input", { bubbles: true, inputType: "insertText" }));
    const canceled = !input.dispatchEvent(new KeyboardEvent("keydown", {
      key: "Enter", code: "Enter", keyCode: 13, which: 13, bubbles: true, cancelable: true,
    }));
    const title = document.querySelector(".todo-list label")?.textContent;
    return { canceled, title, codePoints: Array.from(title ?? "").length };
  });
  assert.deepEqual(boundary, {
    canceled: true,
    title: "a".repeat(95),
    codePoints: 95,
  });
  await page.click(".todo-list .destroy");

  const unexpectedBrowserErrors = tolerateUnavailableWebGpu
    ? browserErrors.filter(error => !error.includes("no WebGPU adapter is available"))
    : browserErrors;
  assert.deepEqual(unexpectedBrowserErrors, []);
  console.log("ok: Fe TodoMVC browser behavior, keyed identity, focus, UTF-8, and lifecycle");
} finally {
  await browser.close();
  await new Promise(resolvePromise => server.close(resolvePromise));
}
