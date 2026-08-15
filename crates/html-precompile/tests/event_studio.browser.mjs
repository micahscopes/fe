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
      pointerX: Number(values[3]?.textContent),
      pointerY: Number(values[4]?.textContent),
      pointerEvents: Number(values[5]?.textContent),
      wheelEvents: Number(values[6]?.textContent),
      visible: Number(values[7]?.textContent),
      visibilityEvents: Number(values[8]?.textContent),
      frameEvents: Number(values[9]?.textContent),
      timerEvents: Number(values[10]?.textContent),
      frameTimestamp: Number(values[11]?.textContent),
      boundedDrops: Number(values[12]?.textContent),
      latestValue: Number(values[13]?.textContent),
      observations: Number(values[14]?.textContent),
      failures: Number(values[15]?.textContent),
      deviceKind: Number(values[16]?.textContent),
      deviceReason: Number(values[17]?.textContent),
      deviceGeneration: Number(values[18]?.textContent),
      deviceEvents: Number(values[19]?.textContent),
      deviceMissed: Number(values[20]?.textContent),
      queueGeneration: Number(values[21]?.textContent),
      queueEvents: Number(values[22]?.textContent),
      queueMissed: Number(values[23]?.textContent),
      states: globalThis.__feEventStudioE2E.states.length,
      errors: globalThis.__feEventStudioE2E.errors,
    };
  });
  await page.waitForFunction(() => {
    const script = document.querySelector('script[data-fe-mount="#event-studio"]');
    const component = document.querySelector("#event-studio");
    const values = component?.querySelectorAll(".event-studio-grid strong");
    return script?.dataset.feState === "complete" && component?._active === true
      && Number(values?.[8]?.textContent) >= 1
      && Number(values?.[9]?.textContent) >= 1
      && Number(values?.[10]?.textContent) >= 1
      && Number(values?.[13]?.textContent) >= 4
      && Number(values?.[14]?.textContent) >= 1;
  });
  const initial = await readStudio();
  assert.equal(initial.width, 640);
  assert.equal(initial.height, 480);
  assert.equal(initial.devicePixelRatioPercent, 150);
  assert.equal(initial.pointerX, 0);
  assert.equal(initial.pointerY, 0);
  assert.equal(initial.pointerEvents, 0);
  assert.equal(initial.wheelEvents, 0);
  assert.equal(initial.visible, 1);
  assert.equal(initial.visibilityEvents, 1);
  assert.ok(initial.frameEvents >= 1);
  assert.ok(initial.timerEvents === initial.frameEvents ||
    initial.timerEvents === initial.frameEvents + 1);
  assert.ok(initial.frameTimestamp > 0);
  assert.equal(initial.boundedDrops, 0);
  assert.ok(initial.latestValue >= 4 && initial.latestValue % 4 === 0);
  assert.equal(initial.observations, 1);
  assert.equal(initial.failures, 0);
  assert.equal(initial.queueGeneration, 0);
  assert.equal(initial.queueEvents, 0);
  assert.equal(initial.queueMissed, 0);
  assert.deepEqual(initial.errors, []);

  // The fixed adapter reports only the standards state and event. Fe owns the
  // typed visibility stream, actor message, and projection policy.
  await page.evaluate(() => {
    Object.defineProperty(document, "visibilityState", {
      configurable: true,
      value: "hidden",
    });
    document.dispatchEvent(new Event("visibilitychange"));
  });
  await page.waitForFunction(previous => {
    const values = document.querySelectorAll("#event-studio .event-studio-grid strong");
    return values[7]?.textContent === "0"
      && Number(values[8]?.textContent) === previous + 1;
  }, {}, initial.visibilityEvents);
  await page.evaluate(() => {
    Object.defineProperty(document, "visibilityState", {
      configurable: true,
      value: "visible",
    });
    document.dispatchEvent(new Event("visibilitychange"));
  });
  await page.waitForFunction(previous => {
    const values = document.querySelectorAll("#event-studio .event-studio-grid strong");
    return values[7]?.textContent === "1"
      && Number(values[8]?.textContent) === previous + 2;
  }, {}, initial.visibilityEvents);

  // Deliberately hold the first generic actor acceptance while delivering six
  // real PointerEvents and one real WheelEvent. This is an independent merge
  // oracle: the fixed host knows neither the shared queue nor KeepLatest
  // policy, while Fe keeps both heterogeneous listeners pending and decides
  // which observations survive. A fast machine therefore cannot satisfy this
  // gate merely because every actor transition usually completes immediately.
  await page.evaluate(async () => {
    const component = document.querySelector("#event-studio");
    const originalSend = component._sendScopedTaskEvent.bind(component);
    let holdFirst = true;
    let releaseFirst;
    component._sendScopedTaskEvent = (event, signal) => {
      if (!holdFirst) return originalSend(event, signal);
      holdFirst = false;
      return new Promise((resolvePromise, reject) => {
        releaseFirst = () => {
          try { resolvePromise(originalSend(event, signal)); }
          catch (error) { reject(error); }
        };
        signal.addEventListener("abort", () => {
          reject(new DOMException("test-delayed actor send aborted", "AbortError"));
        }, { once: true });
      });
    };
    const dispatch = index => component.dispatchEvent(new PointerEvent("pointermove", {
      bubbles: true,
      composed: true,
      pointerId: 17,
      pointerType: "touch",
      clientX: 123.75 + index,
      clientY: 222.5 + index,
      buttons: 1,
      isPrimary: true,
      pressure: 0.625,
    }));
    for (let index = 0; index < 6; index += 1) {
      dispatch(index);
      await new Promise(resolvePromise => setTimeout(resolvePromise, 0));
    }
    if (typeof releaseFirst !== "function") {
      throw new Error("the first Fe actor send was not held in flight");
    }
    component.dispatchEvent(new WheelEvent("wheel", {
      bubbles: true,
      composed: true,
      deltaX: -1.25,
      deltaY: 8.5,
      deltaMode: WheelEvent.DOM_DELTA_LINE,
      clientX: 210.25,
      clientY: 111.75,
      ctrlKey: true,
    }));
    await new Promise(resolvePromise => setTimeout(resolvePromise, 0));
    releaseFirst();
  });
  try {
    await page.waitForFunction(() => {
      const values = document.querySelectorAll("#event-studio .event-studio-grid strong");
      return values[3]?.textContent === "210" && values[4]?.textContent === "111"
        && values[5]?.textContent === "3" && values[6]?.textContent === "1"
        && values[12]?.textContent === "3";
    });
  } catch (error) {
    throw new Error(`merged pointer/wheel receipt did not settle: ${JSON.stringify(await readStudio())}`, {
      cause: error,
    });
  }
  const afterGestures = await readStudio();
  assert.equal(afterGestures.pointerX, 210);
  assert.equal(afterGestures.pointerY, 111);
  assert.equal(afterGestures.pointerEvents, 3);
  assert.equal(afterGestures.wheelEvents, 1);
  assert.equal(afterGestures.boundedDrops, 3);
  assert.equal(afterGestures.visible, 1);
  assert.equal(afterGestures.visibilityEvents, initial.visibilityEvents + 2);
  assert.equal(afterGestures.failures, 0);
  assert.deepEqual(afterGestures.errors, []);

  await page.setViewport({ width: 777, height: 555, deviceScaleFactor: 2 });
  await page.waitForFunction(() => {
    const values = document.querySelectorAll("#event-studio .event-studio-grid strong");
    return values[0]?.textContent === "777" && values[1]?.textContent === "555"
      && values[2]?.textContent === "200" && values[14]?.textContent === "2";
  });
  const afterResize = await readStudio();
  assert.equal(afterResize.width, 777);
  assert.equal(afterResize.height, 555);
  assert.equal(afterResize.devicePixelRatioPercent, 200);
  assert.equal(afterResize.observations, 2);
  assert.equal(afterResize.failures, 0);
  assert.deepEqual(afterResize.errors, []);

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
      && values[8]?.textContent === "4"
      && values[14]?.textContent === "3";
  });
  const reconnected = await readStudio();
  assert.equal(reconnected.width, 777);
  assert.equal(reconnected.height, 555);
  assert.equal(reconnected.devicePixelRatioPercent, 200);
  assert.equal(reconnected.pointerEvents, 3);
  assert.equal(reconnected.wheelEvents, 1);
  assert.equal(reconnected.visible, 1);
  assert.equal(reconnected.visibilityEvents, initial.visibilityEvents + 3);
  assert.equal(reconnected.observations, 3);
  assert.equal(reconnected.failures, 0);
  assert.deepEqual(reconnected.errors, []);

  // Capture is selected by `BrowserPointerEvents::capture_primary()` in Fe.
  // The fixed adapter realizes only the standards operations. Holding capture
  // across a component disconnect then proves the affine task cancellation
  // releases it even though no pointerup/cancel event arrived.
  await page.evaluate(() => {
    const component = document.querySelector("#event-studio");
    const captured = new Set();
    globalThis.__feCaptureOperations = [];
    component.setPointerCapture = pointerId => {
      captured.add(pointerId);
      globalThis.__feCaptureOperations.push(["capture", pointerId]);
    };
    component.hasPointerCapture = pointerId => captured.has(pointerId);
    component.releasePointerCapture = pointerId => {
      captured.delete(pointerId);
      globalThis.__feCaptureOperations.push(["release", pointerId]);
    };
    component.dispatchEvent(new PointerEvent("pointerdown", {
      bubbles: true,
      composed: true,
      pointerId: 37,
      pointerType: "touch",
      clientX: 144.5,
      clientY: 211.25,
      buttons: 1,
      isPrimary: true,
      pressure: 0.5,
    }));
  });
  await page.waitForFunction(previousPointers => {
    const values = document.querySelectorAll("#event-studio .event-studio-grid strong");
    return Number(values[5]?.textContent) === previousPointers + 1
      && globalThis.__feCaptureOperations?.length === 1;
  }, {}, reconnected.pointerEvents);
  await page.evaluate(() => {
    const component = document.querySelector("#event-studio");
    const marker = document.createComment("captured-event-studio-position");
    component.before(marker);
    component.remove();
    marker.replaceWith(component);
  });
  await page.waitForFunction(() => document.querySelector("#event-studio")?._active === true
    && globalThis.__feCaptureOperations?.length === 2);
  assert.deepEqual(await page.evaluate(() => globalThis.__feCaptureOperations), [
    ["capture", 37], ["release", 37],
  ]);
  const afterCaptureCancellation = await readStudio();
  assert.equal(afterCaptureCancellation.failures, 0);
  assert.deepEqual(afterCaptureCancellation.errors, []);
  assert.deepEqual(browserErrors, []);
console.log("ok: Fe Event Studio viewport, Fe-selected scoped pointer capture, merged bounded pointer/wheel buffering, visibility, shared-device lifecycle/queue completion, paced frame/timer, Scan forwarding, latest values, and lifecycle streams");
} finally {
  await browser.close();
  await new Promise(resolvePromise => server.close(resolvePromise));
}
