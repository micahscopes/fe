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
    globalThis.__feInspectorE2E = {
      errors: [], states: [], prevented: [], surfaceReady: [], surfaceSettled: [],
      surfaceTerminal: [],
    };
    const recordSurface = (field, event) => {
      if (event.target?.tagName !== "FE-SURFACE") return;
      const sequence = Number(event.target.getAttribute("data-fe-sequence"));
      if (!Number.isInteger(sequence)) return;
      const values = globalThis.__feInspectorE2E[field];
      if (!values.includes(sequence)) values.push(sequence);
    };
    document.addEventListener("fe-ready", event => {
      recordSurface("surfaceReady", event);
      recordSurface("surfaceSettled", event);
    }, true);
    document.addEventListener("fe-live", event => recordSurface("surfaceTerminal", event), true);
    document.addEventListener("fe-error", event => {
      recordSurface("surfaceTerminal", event);
      recordSurface("surfaceSettled", event);
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
  await page.goto(`http://127.0.0.1:${address.port}/`, {
    waitUntil: "domcontentloaded",
    timeout: 120_000,
  });
  try {
    await page.waitForFunction(tolerateUnavailable => {
      const script = document.querySelector('script[data-fe-mount="#source-inspector"], script[data-fe-mount="#gallery-shell"]');
      const component = document.querySelector("#source-inspector, #gallery-shell");
      const surfaces = Array.from(document.querySelectorAll("fe-surface"));
      const expectedSurfaces = document.querySelector(".gallery-head") ? 12 : 1;
      return script?.dataset.feState === "complete" && component?._active === true &&
        surfaces.length === expectedSurfaces &&
        (tolerateUnavailable
          ? globalThis.__feInspectorE2E.surfaceSettled.length === expectedSurfaces
          : globalThis.__feInspectorE2E.surfaceReady.length === expectedSurfaces) &&
        surfaces.every(surface => surface.shadowRoot?.querySelector('a[href$=".wgsl"]'));
    }, { timeout: 120_000 }, tolerateUnavailableWebGpu);
  } catch (error) {
    const diagnosis = await page.evaluate(() => ({
      scriptState: document.querySelector('script[data-fe-mount="#source-inspector"], script[data-fe-mount="#gallery-shell"]')?.dataset.feState,
      componentActive: document.querySelector("#source-inspector, #gallery-shell")?._active,
      componentState: document.querySelector("#source-inspector, #gallery-shell")?._state,
      surfaces: Array.from(document.querySelectorAll("fe-surface"), surface => ({
        sequence: surface.getAttribute("data-fe-sequence"),
        state: surface.state,
        notice: surface.shadowRoot?.querySelector(".notice")?.textContent,
      })),
      events: globalThis.__feInspectorE2E,
    }));
    throw new Error(`gallery did not finish ordered poster loading: ${JSON.stringify({ diagnosis, browserErrors })}`, { cause: error });
  }

  const semanticActions = await page.evaluate(() => {
    const action = element => element?.getAttribute("data-fe-action");
    const artifactAction = suffix => action(Array.from(document.querySelectorAll("fe-surface"))
      .map(surface => surface.shadowRoot?.querySelector(`a[href$="${suffix}"]`))
      .find(Boolean));
    return [
      action(document.querySelector(".gallery-head a[href$='.fe'], .source[href$='.fe']")),
      artifactAction(".wgsl"),
      artifactAction(".wasm"),
      artifactAction(".json"),
      action(document.querySelector(".source-inspector .close")),
    ];
  });
  assert.ok(semanticActions.every(action => action !== null));
  assert.equal(new Set(semanticActions).size, semanticActions.length,
    "semantic inspector actions must have distinct derived transport identities");

  const isGallery = await page.$(".gallery-head") !== null;
  if (isGallery) {
    assert.deepEqual(
      await page.evaluate(() => globalThis.__feInspectorE2E.states.map(
        state => state.slice(14, 18),
      )),
      [
        [1, 0, 0, 0],
        ...Array.from({ length: 12 }, (_, index) => [index + 2, index + 1, 0, 0]),
        [14, 12, 1, 0],
      ],
      "scoped Fe task did not deliver exact progress/completion states to its resident actor",
    );
    assert.deepEqual(
      await page.evaluate(() => globalThis.__feInspectorE2E.surfaceSettled),
      Array.from({ length: 12 }, (_, index) => index),
      "Fe did not load every gallery poster exactly in compiler-derived order",
    );
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
      figures: 14,
      surfaces: 12,
      components: 3,
      captions: [
        "gradient",
        "TodoMVC",
        "Event Studio",
        "cga3d",
        "qcga",
        "qcga pencil",
        "desargues",
        "plasma",
        "distance field",
        "mandelbrot",
        "perturbation mandelbrot",
        "dec",
        "known color",
        "rollcall pipeline",
      ],
    });

    // Event Studio is the browser-event acceptance tile: real standards
    // resize/pointer/wheel facts cross fixed adapters, typed Fe EventSources,
    // affine subscriptions, actor-scoped tasks, and the resident projection.
    await page.waitForFunction(() => {
      const component = document.querySelector("#gallery-event-studio");
      const values = component?.querySelectorAll(".event-studio-grid strong");
      return component?._active === true && values?.length === 16
        && Number(values[8].textContent) >= 1
        && Number(values[9].textContent) >= 1
        && Number(values[10].textContent) >= 1
        && Number(values[13].textContent) >= 4
        && Number(values[14].textContent) >= 1;
    });
    const eventStudioBefore = await page.evaluate(() => {
      const values = document.querySelectorAll("#gallery-event-studio .event-studio-grid strong");
      return {
        width: Number(values[0].textContent),
        height: Number(values[1].textContent),
        devicePixelRatioPercent: Number(values[2].textContent),
        visible: Number(values[7].textContent),
        visibilityEvents: Number(values[8].textContent),
        frameEvents: Number(values[9].textContent),
        timerEvents: Number(values[10].textContent),
        boundedDrops: Number(values[12].textContent),
        latestValue: Number(values[13].textContent),
        observations: Number(values[14].textContent),
        failures: Number(values[15].textContent),
      };
    });
    assert.equal(eventStudioBefore.failures, 0);
    assert.equal(eventStudioBefore.visible, 1);
    assert.equal(eventStudioBefore.visibilityEvents, 1);
    assert.ok(eventStudioBefore.frameEvents >= 1);
    assert.ok(eventStudioBefore.timerEvents === eventStudioBefore.frameEvents ||
      eventStudioBefore.timerEvents === eventStudioBefore.frameEvents + 1);
    assert.equal(eventStudioBefore.boundedDrops, 0);
    assert.ok(eventStudioBefore.latestValue >= 4 && eventStudioBefore.latestValue % 4 === 0);
    // Hold the first generic actor acceptance while six genuine PointerEvents
    // arrive. The host knows neither the queue capacity nor KeepLatest policy;
    // Fe's Select keeps source and sink independently in flight and its
    // three-slot waiting backlog decides which observations survive.
    await page.evaluate(async () => {
      const component = document.querySelector("#gallery-event-studio");
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
        pointerId: 23,
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
      releaseFirst();
      component.dispatchEvent(new WheelEvent("wheel", {
        bubbles: true,
        composed: true,
        deltaY: -4.5,
        deltaMode: WheelEvent.DOM_DELTA_PIXEL,
        clientX: 166.25,
        clientY: 244.5,
      }));
    });
    await page.waitForFunction(() => {
      const values = document.querySelectorAll("#gallery-event-studio .event-studio-grid strong");
      return Number(values[3]?.textContent) === 128
        && Number(values[4]?.textContent) === 227
        && Number(values[5]?.textContent) === 4
        && Number(values[6]?.textContent) === 1
        && Number(values[12]?.textContent) === 2;
    });
    await page.setViewport({ width: 777, height: 555, deviceScaleFactor: 2 });
    await page.waitForFunction(previousObservations => {
      const values = document.querySelectorAll("#gallery-event-studio .event-studio-grid strong");
      return Number(values[0]?.textContent) === 777
        && Number(values[1]?.textContent) === 555
        && Number(values[2]?.textContent) === 200
        && Number(values[14]?.textContent) > previousObservations;
    }, {}, eventStudioBefore.observations);
    assert.equal(await page.$eval(
      "#gallery-event-studio .event-studio-grid p:last-child strong",
      node => Number(node.textContent),
    ), 0);

    // Exercise the DEC tile through the real generated browser actor path:
    // `<fe-surface>.post` -> generated canonical validators/router -> module
    // Worker -> Fe `d0` -> canonical response. This is semantic evidence, not
    // a publication-byte or manifest-presence proxy.
    await page.waitForFunction(() => {
      const figure = Array.from(document.querySelectorAll(".grid > figure"))
        .find(node => node.querySelector("figcaption > b")?.textContent === "dec");
      return figure?.querySelector("fe-surface")?._actor != null;
    });
    const decD0 = await page.evaluate(async () => {
      const figure = Array.from(document.querySelectorAll(".grid > figure"))
        .find(node => node.querySelector("figcaption > b")?.textContent === "dec");
      const surface = figure.querySelector("fe-surface");
      return surface.post("d0", {
        v0: 1, v1: 0, v2: 0, v3: 0, v4: 0, v5: 0, v6: 0,
      });
    });
    assert.deepEqual(decD0, {
      e0: -1, e1: -1, e2: -1, e3: -1, e4: -1, e5: -1,
      e6: 0, e7: 0, e8: 0, e9: 0, e10: 0, e11: 0,
    }, "DEC d0 did not execute through the generated Worker/message path");

    // Exercise the canonical QCGA DE tile through actual pointer events. The
    // browser contributes only coordinates; the changed yaw must
    // come back from the resident Fe transition, while the solver certificate
    // remains untouched. Skip only when this run explicitly permits a missing
    // WebGPU implementation and the GPU-only surface could not become ready.
    const qcgaPointer = await page.evaluate(async tolerateUnavailable => {
      const figure = Array.from(document.querySelectorAll(".grid > figure"))
        .find(node => node.querySelector("figcaption > b")?.textContent === "qcga pencil");
      const surface = figure?.querySelector("fe-surface");
      if (!surface) throw new Error("QCGA pencil surface is missing");
      if (surface._fsm === "error") {
        const detail = surface.shadowRoot?.querySelector(".notice")?.textContent ??
          "QCGA pencil failed to boot";
        const unavailable = /no WebGPU adapter is available|WebGPU is required|does not expose WebGPU/.test(detail);
        if (tolerateUnavailable && unavailable) return {
          skipped: true,
          detail,
          togglePresent: Boolean(surface.shadowRoot?.querySelector('input[type="checkbox"]')),
          bounded: surface.params.bounded,
        };
        throw new Error(detail);
      }
      await surface.live();
      surface.scrollIntoView({ block: "center" });
      await new Promise(resolvePromise => requestAnimationFrame(
        () => requestAnimationFrame(resolvePromise),
      ));
      const canvas = surface._adoptedCanvas || surface._liveCanvas || surface._posterCanvas;
      const rect = canvas.getBoundingClientRect();
      // Start in a canvas corner, deliberately outside the projected control
      // cluster. The previous centre click could correctly pick a control
      // point and enter Fe's drag/re-solve branch, making an orbit assertion
      // depend on the current scene geometry.
      surface.__qcgaTestFrames = 0;
      surface.addEventListener("fe-frame", () => { surface.__qcgaTestFrames += 1; });
      return {
        skipped: false,
        x: Math.max(24, Math.min(innerWidth - 24, rect.left + rect.width * 0.06)),
        y: Math.max(24, Math.min(innerHeight - 24, rect.top + rect.height * 0.06)),
        yaw: surface.params.yaw,
        certificate: surface._uniforms.slice(
          surface._memberIndexByName.get("generation"),
          surface._memberIndexByName.get("picked"),
        ),
        picked: surface._uniforms[surface._memberIndexByName.get("picked")],
        togglePresent: Boolean(surface.shadowRoot?.querySelector('input[type="checkbox"]')),
        bounded: surface.params.bounded,
      };
    }, tolerateUnavailableWebGpu);
    assert.equal(qcgaPointer.togglePresent, true,
      "the generic Fe toggle Param did not render as a browser checkbox");
    assert.equal(qcgaPointer.bounded, 0,
      "the QCGA pencil must default to its infinite-fade mode");
    if (!qcgaPointer.skipped) {
      await page.evaluate(() => {
        const figure = Array.from(document.querySelectorAll(".grid > figure"))
          .find(node => node.querySelector("figcaption > b")?.textContent === "qcga pencil");
        figure.querySelector("fe-surface").shadowRoot.querySelector('input[type="checkbox"]').click();
      });
      await page.waitForFunction(() => {
        const figure = Array.from(document.querySelectorAll(".grid > figure"))
          .find(node => node.querySelector("figcaption > b")?.textContent === "qcga pencil");
        const surface = figure.querySelector("fe-surface");
        return surface.params.bounded === 1 && surface._pendingSurfaceEvents.length === 0;
      });
      assert.equal(await page.evaluate(() => {
        const figure = Array.from(document.querySelectorAll(".grid > figure"))
          .find(node => node.querySelector("figcaption > b")?.textContent === "qcga pencil");
        const surface = figure.querySelector("fe-surface");
        const index = surface._memberIndexByName.get("bounded");
        return surface._uniforms[index];
      }), 1, "the checkbox did not cross the typed ParamEdit lane into Fe state");
      await page.evaluate(() => {
        const figure = Array.from(document.querySelectorAll(".grid > figure"))
          .find(node => node.querySelector("figcaption > b")?.textContent === "qcga pencil");
        figure.querySelector("fe-surface").shadowRoot.querySelector('input[type="checkbox"]').click();
      });
      await page.waitForFunction(() => {
        const figure = Array.from(document.querySelectorAll(".grid > figure"))
          .find(node => node.querySelector("figcaption > b")?.textContent === "qcga pencil");
        const surface = figure.querySelector("fe-surface");
        return surface.params.bounded === 0 && surface._pendingSurfaceEvents.length === 0;
      });
      await page.evaluate(() => {
        const figure = Array.from(document.querySelectorAll(".grid > figure"))
          .find(node => node.querySelector("figcaption > b")?.textContent === "qcga pencil");
        figure.querySelector("fe-surface").__qcgaTestFrames = 0;
      });
      await page.mouse.move(qcgaPointer.x, qcgaPointer.y);
      await page.mouse.down();
      await page.waitForFunction(() => {
        const figure = Array.from(document.querySelectorAll(".grid > figure"))
          .find(node => node.querySelector("figcaption > b")?.textContent === "qcga pencil");
        const surface = figure.querySelector("fe-surface");
        return surface.__qcgaTestFrames > 0 && surface._pendingSurfaceEvents.length === 0;
      });
      const qcgaAfterDown = await page.evaluate(() => {
        const figure = Array.from(document.querySelectorAll(".grid > figure"))
          .find(node => node.querySelector("figcaption > b")?.textContent === "qcga pencil");
        const surface = figure.querySelector("fe-surface");
        const picked = surface._memberIndexByName.get("picked");
        return { frames: surface.__qcgaTestFrames, picked: surface._uniforms[picked] };
      });
      assert.equal(qcgaAfterDown.picked, qcgaPointer.picked,
        "marker-free orbit press unexpectedly selected a QCGA control point");
      await page.mouse.move(qcgaPointer.x + 25, qcgaPointer.y);
      await page.waitForFunction(({ previousYaw, previousFrames }) => {
        const figure = Array.from(document.querySelectorAll(".grid > figure"))
          .find(node => node.querySelector("figcaption > b")?.textContent === "qcga pencil");
        const surface = figure.querySelector("fe-surface");
        return surface.__qcgaTestFrames > previousFrames
          && surface.params.yaw !== previousYaw;
      }, {}, { previousYaw: qcgaPointer.yaw, previousFrames: qcgaAfterDown.frames });
      await page.mouse.up();
      const qcgaAfter = await page.evaluate(() => {
        const figure = Array.from(document.querySelectorAll(".grid > figure"))
          .find(node => node.querySelector("figcaption > b")?.textContent === "qcga pencil");
        const surface = figure.querySelector("fe-surface");
        return {
          yaw: surface.params.yaw,
          certificate: surface._uniforms.slice(
            surface._memberIndexByName.get("generation"),
            surface._memberIndexByName.get("picked"),
          ),
        };
      });
      assert.notEqual(qcgaAfter.yaw, qcgaPointer.yaw,
        "raw pointer movement did not cross the resident Fe QCGA transition");
      assert.deepEqual(qcgaAfter.certificate, qcgaPointer.certificate,
        "camera interaction rewrote the Fe-owned solved-pencil certificate");
    }
  } else {
    assert.deepEqual(await page.evaluate(() => ({
      title: document.title,
      surfaces: document.querySelectorAll("fe-surface").length,
      components: document.querySelectorAll("fe-component").length,
      overlays: document.querySelectorAll('[data-fe-view="0"]').length,
    })), {
      title: "Fe · SourceInspector",
      surfaces: 1,
      components: 1,
      overlays: 1,
    });
  }

  assert.equal(await page.$eval(".inspector, .source-inspector", node => node.hidden), true);
  const nestedOwnership = await page.evaluate(() => {
    const owner = document.querySelector("#source-inspector, #gallery-shell");
    const statesBefore = globalThis.__feInspectorE2E.states.length;
    const nested = document.createElement("fe-component");
    nested.id = "nested-component-ownership-probe";
    const button = document.createElement("button");
    button.setAttribute("data-fe-action", document
      .querySelector(".gallery-head a[href$='.fe'], .source[href$='.fe']")
      .getAttribute("data-fe-action"));
    nested.append(button);
    owner.append(nested);
    button.click();
    const result = {
      statesBefore,
      statesAfter: globalThis.__feInspectorE2E.states.length,
      inspectorHidden: document.querySelector(".inspector, .source-inspector").hidden,
    };
    nested.remove();
    return result;
  });
  assert.deepEqual(nestedOwnership, {
    statesBefore: nestedOwnership.statesBefore,
    statesAfter: nestedOwnership.statesBefore,
    inspectorHidden: true,
  }, "an action owned by a nested Fe component reached its parent actor");
  if (isGallery) {
    await page.click(".gallery-head a[href$='.fe']");
    try {
      await page.waitForFunction(() =>
        !document.querySelector('[data-fe-view="1"]').hidden &&
        document.querySelector(".inspector-body pre:not(.error)").textContent.includes("actor GalleryPage") &&
        document.querySelector(".inspector-body pre:not(.error)").textContent.includes("struct GalleryBuilder")
      );
    } catch (error) {
      const diagnosis = await page.evaluate(() => ({
        evidence: globalThis.__feInspectorE2E,
        componentActive: document.querySelector("#gallery-shell")?._active,
        componentState: document.querySelector("#gallery-shell")?._state,
        inspectorHidden: document.querySelector(".source-inspector")?.hidden,
        inspectorText: document.querySelector(".inspector-body")?.textContent,
      }));
      throw new Error(`gallery SourceInspector did not load page source: ${JSON.stringify({ diagnosis, browserErrors })}`, { cause: error });
    }
  } else {
    await page.click(".source[href$='.fe']");
    await page.waitForFunction(() =>
      !document.querySelector('[data-fe-view="1"]').hidden &&
      document.querySelector(".inspector-body pre:not(.error)").textContent.includes("actor GradientSurface")
    );
  }
  assert.deepEqual(await page.evaluate(() => ({
    open: !document.querySelector(".inspector, .source-inspector").hidden,
    sourceTitle: !document.querySelector('[data-fe-view="5"]').hidden,
    focused: document.activeElement === document.querySelector(".source-inspector .close"),
    stayed: location.pathname === "/",
  })), { open: true, sourceTitle: true, focused: true, stayed: true });

  await page.click(".source-inspector .close");
  assert.equal(await page.$eval(".inspector, .source-inspector", node => node.hidden), true);
  await page.evaluate(() => Array.from(document.querySelectorAll("fe-surface"))
    .map(surface => surface.shadowRoot?.querySelector('a[href$=".wgsl"]'))
    .find(Boolean).click());
  await page.waitForFunction(() =>
    !document.querySelector('[data-fe-view="1"]').hidden &&
    document.querySelector(".inspector-body pre:not(.error)").textContent.includes("@fragment")
  );
  assert.equal(await page.$eval('[data-fe-view="6"]', node => node.hidden), false);

  const wasmExpected = await page.evaluate(async () => {
    const link = Array.from(document.querySelectorAll("fe-surface"))
      .map(surface => surface.shadowRoot?.querySelector('a[href$=".wasm"]'))
      .find(Boolean);
    const length = (await (await fetch(link.href)).arrayBuffer()).byteLength;
    link.click();
    return length;
  });
  await page.waitForFunction(expected =>
    !document.querySelector('[data-fe-view="2"]').hidden &&
    Number(document.querySelector(".inspector-body strong").textContent) === expected,
    {}, wasmExpected,
  );
  assert.equal(await page.$eval('[data-fe-view="7"]', node => node.hidden), false);

  await page.evaluate(() => Array.from(document.querySelectorAll("fe-surface"))
    .map(surface => surface.shadowRoot?.querySelector('a[href$=".json"]'))
    .find(Boolean).click());
  await page.waitForFunction(() =>
    !document.querySelector('[data-fe-view="1"]').hidden &&
    document.querySelector(".inspector-body pre:not(.error)").textContent.includes('"protocol": "fe-web-bundle"')
  );
  assert.equal(await page.$eval('[data-fe-view="8"]', node => node.hidden), false);

  await page.keyboard.press("Escape");
  assert.equal(await page.$eval(".inspector, .source-inspector", node => node.hidden), true);
  const evidence = await page.evaluate(() => globalThis.__feInspectorE2E);
  assert.deepEqual(evidence.errors, []);
  assert.ok(evidence.states.length >= 10, `too few Fe states: ${evidence.states.length}`);
  for (const action of semanticActions) {
    assert.ok(evidence.prevented.some(value => value[0] === action && value[1] === true),
      `Fe did not prevent default for action ${action}`);
  }
  const unexpectedBrowserErrors = tolerateUnavailableWebGpu
    ? browserErrors.filter(error => !error.includes("no WebGPU adapter is available"))
    : browserErrors;
  assert.deepEqual(unexpectedBrowserErrors, []);
  console.log(`ok: ${isGallery ? "Fe-composed gallery and " : ""}resident SourceInspector behavior`);
} finally {
  await browser.close();
  await new Promise(resolvePromise => server.close(resolvePromise));
}
