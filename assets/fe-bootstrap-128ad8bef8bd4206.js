const providers = globalThis.feImportProviders ??= [];

export function registerFeImportProvider(provider) {
  if (typeof provider !== "function" && (typeof provider !== "object" || provider === null)) {
    throw new TypeError("Fe import provider must be an object or function");
  }
  providers.push(provider);
}

globalThis.registerFeImportProvider ??= registerFeImportProvider;

function mergeImports(target, additions) {
  for (const [module, members] of Object.entries(additions || {})) {
    const namespace = target[module] ??= {};
    for (const [name, value] of Object.entries(members)) {
      if (Object.hasOwn(namespace, name) && namespace[name] !== value) {
        throw new Error(`duplicate Wasm import: ${module}.${name}`);
      }
      namespace[name] = value;
    }
  }
}

async function importsFor(context, additions) {
  const imports = {};
  mergeImports(imports, additions);
  for (const provider of providers) {
    mergeImports(
      imports,
      typeof provider === "function" ? await provider(context) : provider,
    );
  }
  return imports;
}

async function sha256Hex(bytes) {
  const digest = new Uint8Array(await crypto.subtle.digest("SHA-256", bytes));
  return Array.from(digest, byte => byte.toString(16).padStart(2, "0")).join("");
}

const ACTOR_INITIALIZE = "fe_actor_initialize_v1";
const ACTOR_TRANSITION = "fe_actor_transition_v1";
const ACTOR_PROJECT = "fe_actor_project_v1";
const COMPONENT_EVENT = Object.freeze({
  connected: 0,
  disconnected: 1,
  adopted: 2,
  activate: 3,
  input: 4,
  change: 5,
  submit: 6,
  keyDown: 7,
  resourceLoaded: 8,
  surfaceDiscovered: 9,
  surfaceLive: 10,
  surfaceError: 11,
  surfaceTimeout: 12,
});

function values(value) {
  return Array.isArray(value) ? value : [value];
}

function eventElement(event, boundary, attribute) {
  const path = typeof event.composedPath === "function" ? event.composedPath() : [];
  const boundaryIndex = path.indexOf(boundary);
  if (boundaryIndex >= 0) {
    return path.slice(0, boundaryIndex).find(value =>
      value instanceof Element && value.hasAttribute(attribute)) ?? null;
  }
  const element = event.target instanceof Element
    ? event.target.closest(`[${attribute}]`)
    : null;
  return element && boundary.contains(element) ? element : null;
}

function eventBelongsToComponent(event, boundary) {
  const path = typeof event.composedPath === "function" ? event.composedPath() : [];
  const boundaryIndex = path.indexOf(boundary);
  if (boundaryIndex >= 0) {
    return !path.slice(0, boundaryIndex).some(value =>
      value instanceof Element && value.tagName === "FE-COMPONENT");
  }
  const owner = event.target instanceof Element
    ? event.target.closest("fe-component")
    : null;
  return owner === boundary;
}

function componentElements(boundary, scope, selector) {
  // Keyed repeat rows are projected while detached, before reconciliation
  // inserts them into their actor's container. In that one scoped case there
  // is no closest component yet; descendants are still owned unless they sit
  // beneath a nested component inside the row.
  const detachedOwnedScope = scope !== boundary && scope.closest("fe-component") === null;
  return Array.from(scope.querySelectorAll(selector)).filter(element =>
    element.closest("fe-component") === boundary ||
      (detachedOwnedScope && element.closest("fe-component") === null));
}

function componentElement(boundary, scope, selector) {
  return componentElements(boundary, scope, selector)[0] ?? null;
}

function eventTargetId(event, boundary) {
  const element = eventElement(event, boundary, "data-fe-action");
  if (!element) return 0;
  const target = Number(element.getAttribute("data-fe-action"));
  return Number.isInteger(target) && target >= 0 ? target >>> 0 : 0;
}

function eventKey(event, boundary) {
  const element = eventElement(event, boundary, "data-fe-key");
  if (!element) return 0;
  const key = Number(element.getAttribute("data-fe-key"));
  return Number.isInteger(key) && key >= 0 ? key >>> 0 : 0;
}

function numericEventValue(event, boundary) {
  const element = eventElement(event, boundary, "data-fe-action");
  const value = Number(element?.value);
  return Number.isFinite(value) ? Math.fround(value) : 0;
}

function textualEventValue(event, boundary) {
  const element = eventElement(event, boundary, "data-fe-action");
  if (typeof element?.value === "string") return element.value;
  return element instanceof HTMLAnchorElement ? element.href : "";
}

const COMPONENT_INPUT_CAPACITY = 4096;
const COMPONENT_COMMAND_LIMIT = 1024 * 1024;
const COMPONENT_RESOURCE_LIMIT = COMPONENT_COMMAND_LIMIT - 4096;
const componentTextEncoder = new TextEncoder();
const componentTextDecoder = new TextDecoder("utf-8", { fatal: true });

function boundedUtf8(value, capacity) {
  const encoded = componentTextEncoder.encode(value);
  if (encoded.byteLength <= capacity) return encoded;
  let end = capacity;
  while (end > 0 && (encoded[end] & 0xc0) === 0x80) end -= 1;
  return encoded.slice(0, end);
}

export function decodeComponentCommands(bytes) {
  const operations = [];
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  let cursor = 0;
  let repeatDepth = 0;
  const need = count => {
    if (cursor + count > bytes.byteLength) {
      throw new Error("fe-component command stream is truncated");
    }
  };
  const byte = () => {
    need(1);
    return view.getUint8(cursor++);
  };
  const word = () => {
    need(4);
    const value = view.getUint32(cursor, true);
    cursor += 4;
    return value;
  };
  const text = () => {
    const length = word();
    need(length);
    const value = componentTextDecoder.decode(bytes.subarray(cursor, cursor + length));
    cursor += length;
    return value;
  };
  while (cursor < bytes.byteLength) {
    const opcode = byte();
    switch (opcode) {
      case 1:
        if (repeatDepth !== 0) throw new Error("fe-component repeats cannot nest in v1");
        repeatDepth = 1;
        operations.push({ opcode, container: word(), template: word() });
        break;
      case 2: {
        if (repeatDepth !== 1) throw new Error("fe-component repeat item is outside a repeat");
        const key = word();
        if (key === 0) throw new Error("fe-component repeat key zero is reserved");
        operations.push({ opcode, key });
        break;
      }
      case 3:
        if (repeatDepth !== 1) throw new Error("fe-component repeat end is unmatched");
        repeatDepth = 0;
        operations.push({ opcode });
        break;
      case 4:
      case 5: {
        const target = word();
        operations.push({ opcode, target, value: text() });
        break;
      }
      case 11: {
        const target = word();
        operations.push({ opcode, target, value: text() });
        break;
      }
      case 12:
      case 13: {
        const request = word();
        if (request === 0) throw new Error("fe-component resource request zero is reserved");
        operations.push({ opcode, request, value: text() });
        break;
      }
      case 6:
      case 7:
      case 10: {
        const target = word();
        const value = byte();
        if (value > 1) throw new Error("fe-component boolean command must contain zero or one");
        operations.push({ opcode, target, value: value === 1 });
        break;
      }
      case 8: {
        const target = word();
        const token = word();
        const value = byte();
        if (value > 1) throw new Error("fe-component class command must contain zero or one");
        operations.push({ opcode, target, token, value: value === 1 });
        break;
      }
      case 9:
        operations.push({ opcode, target: word() });
        break;
      case 14:
        operations.push({ opcode, sequence: word(), timeout: word() });
        break;
      default:
        throw new Error(`fe-component received unknown command opcode ${opcode}`);
    }
  }
  if (repeatDepth !== 0) throw new Error("fe-component command stream has an unclosed repeat");
  return operations;
}

/**
 * Fixed, demo-blind host for one resident Fe component actor. JavaScript owns
 * only standards transport and applies the fixed `ComponentPatch` ABI; Fe owns
 * initialization, lifecycle/input interpretation, state, visibility choice,
 * focus choice, and prevent-default policy.
 */
const FeHTMLElement = globalThis.HTMLElement ?? class {};
class FeComponentElement extends FeHTMLElement {
  constructor() {
    super();
    this._instance = null;
    this._initialized = false;
    this._active = false;
    this._listeners = null;
    this._inputScratch = 0;
    this._inputCapacity = 0;
    this._resourceLoads = new Set();
    this._keyedRows = new WeakMap();
    this._surfaceObserver = null;
    this._discoveredSurfaces = new Set();
    this._surfaceActivations = new Map();
    this._readyPromise = new Promise((resolve, reject) => {
      this._resolveReady = resolve;
      this._rejectReady = reject;
    });
  }

  connectedCallback() {
    this._connect().catch(error => this._fail(error));
  }

  disconnectedCallback() {
    if (this._active) {
      try {
        this._send(COMPONENT_EVENT.disconnected, 0, 0, 0, 0, 0, performance.now());
      } catch (error) {
        this._fail(error);
      }
    }
    this._active = false;
    this._listeners?.abort();
    this._listeners = null;
    this._surfaceObserver?.disconnect();
    this._surfaceObserver = null;
    this._discoveredSurfaces.clear();
    for (const activation of this._surfaceActivations.values()) {
      clearTimeout(activation.timer);
    }
    this._surfaceActivations.clear();
    for (const controller of this._resourceLoads) controller.abort();
    this._resourceLoads.clear();
  }

  adoptedCallback() {
    if (!this._active) return;
    try {
      this._send(COMPONENT_EVENT.adopted, 0, 0, 0, 0, 0, performance.now());
    } catch (error) {
      this._fail(error);
    }
  }

  attachFeInstance(instance) {
    if (this._instance && this._instance !== instance) {
      throw new Error("fe-component already owns a different Wasm instance");
    }
    const exports = instance?.exports;
    for (const name of [ACTOR_INITIALIZE, ACTOR_TRANSITION, ACTOR_PROJECT]) {
      if (typeof exports?.[name] !== "function") {
        throw new Error(`fe-component module has no fixed export \`${name}\``);
      }
    }
    this._instance = instance;
    if (this.isConnected) this._connect().catch(error => this._fail(error));
    return this._readyPromise;
  }

  async _connect() {
    if (!this._instance || this._active) return;
    if (!this._initialized) {
      this._state = values(this._instance.exports[ACTOR_INITIALIZE]());
      this._initialized = true;
    }
    this._active = true;
    this._installListeners();
    this._send(COMPONENT_EVENT.connected, 0, 0, 0, 0, 0, performance.now());
    this._discoverSurfaces();
    this._resolveReady(this);
    this.dispatchEvent(new CustomEvent("fe-ready", { detail: this }));
  }

  _installListeners() {
    this._listeners?.abort();
    const controller = new AbortController();
    this._listeners = controller;
    const on = (type, kind) => this.addEventListener(type, event => {
      try {
        if (!eventBelongsToComponent(event, this)) return;
        const patch = this._send(
          kind,
          eventTargetId(event, this),
          0,
          eventKey(event, this),
          type === "keydown" ? (event.keyCode >>> 0) : (event.detail >>> 0),
          numericEventValue(event, this),
          event.timeStamp,
          textualEventValue(event, this),
        );
        if ((patch[2] & 1) !== 0) event.preventDefault();
      } catch (error) {
        this._fail(error);
      }
    }, { signal: controller.signal });
    on("click", COMPONENT_EVENT.activate);
    on("input", COMPONENT_EVENT.input);
    on("change", COMPONENT_EVENT.change);
    on("submit", COMPONENT_EVENT.submit);
    on("keydown", COMPONENT_EVENT.keyDown);
    const onSurface = (type, kind) => this.addEventListener(type, event => {
      try {
        if (!eventBelongsToComponent(event, this)) return;
        const sequence = this._surfaceSequence(event.target);
        if (sequence === null || !this._surfaceActivations.has(sequence)) return;
        const activation = this._surfaceActivations.get(sequence);
        clearTimeout(activation.timer);
        this._surfaceActivations.delete(sequence);
        this._send(kind, 0, sequence + 1, 0, 0, 0, event.timeStamp);
      } catch (error) {
        this._fail(error);
      }
    }, { signal: controller.signal });
    onSurface("fe-live", COMPONENT_EVENT.surfaceLive);
    onSurface("fe-error", COMPONENT_EVENT.surfaceError);
    if (typeof MutationObserver !== "undefined") {
      this._surfaceObserver = new MutationObserver(() => this._discoverSurfaces());
      this._surfaceObserver.observe(this, { childList: true, subtree: true });
    }
  }

  _surfaceSequence(surface) {
    if (!(surface instanceof Element) || surface.tagName !== "FE-SURFACE") return null;
    const value = Number(surface.getAttribute("data-fe-sequence"));
    return Number.isInteger(value) && value >= 0 && value < 32 ? value : null;
  }

  _discoverSurfaces() {
    if (!this._active) return;
    for (const surface of componentElements(this, this, "fe-surface[data-fe-sequence]")) {
      const sequence = this._surfaceSequence(surface);
      if (sequence === null || this._discoveredSurfaces.has(sequence)) continue;
      this._discoveredSurfaces.add(sequence);
      this._send(
        COMPONENT_EVENT.surfaceDiscovered,
        0,
        sequence + 1,
        0,
        0,
        0,
        performance.now(),
      );
    }
  }

  _writeEventText(value, limit = COMPONENT_INPUT_CAPACITY) {
    const bytes = boundedUtf8(value, limit);
    if (bytes.byteLength === 0) return [0, 0];
    const exports = this._instance?.exports;
    if (!(exports?.memory instanceof WebAssembly.Memory) ||
        typeof exports?.fe_cabi_alloc !== "function") {
      throw new Error("fe-component rich input requires Fe canonical Wasm memory");
    }
    if (bytes.byteLength > this._inputCapacity) {
      let capacity = COMPONENT_INPUT_CAPACITY;
      while (capacity < bytes.byteLength) capacity *= 2;
      if (capacity > COMPONENT_RESOURCE_LIMIT) capacity = COMPONENT_RESOURCE_LIMIT;
      this._inputScratch = exports.fe_cabi_alloc(capacity, 1) >>> 0;
      this._inputCapacity = capacity;
    }
    const end = this._inputScratch + bytes.byteLength;
    if (end > exports.memory.buffer.byteLength || end < this._inputScratch) {
      throw new Error("fe-component input scratch is outside Wasm memory");
    }
    new Uint8Array(exports.memory.buffer, this._inputScratch, bytes.byteLength).set(bytes);
    return [this._inputScratch, bytes.byteLength];
  }

  _send(kind, target, request, key, detail, value, timestamp, text = "", textLimit = COMPONENT_INPUT_CAPACITY) {
    if (!this._instance || !this._initialized) {
      throw new Error("fe-component received an event before Fe initialization");
    }
    const [textPointer, textLength] = this._writeEventText(text, textLimit);
    this._state = values(this._instance.exports[ACTOR_TRANSITION](
      kind >>> 0,
      target >>> 0,
      request >>> 0,
      key >>> 0,
      detail >>> 0,
      Math.fround(value),
      Math.fround(timestamp),
      textPointer,
      textLength,
    ));
    const patch = values(this._instance.exports[ACTOR_PROJECT]());
    if (patch.length !== 5 || patch.some(value => (value >>> 0) !== value)) {
      throw new Error(
        "fe-component projection must return ComponentPatch(u32, u32, u32, bytes)",
      );
    }
    this._applyPatch(patch.map(value => value >>> 0));
    return patch;
  }

  _applyPatch(patch) {
    const [visibleMask, focusTarget, flags, commandPointer, commandLength] = patch;
    if ((flags & ~1) !== 0) {
      throw new Error(`fe-component received unknown ComponentPatch flags ${flags}`);
    }
    for (const view of componentElements(this, this, "[data-fe-view]")) {
      const index = Number(view.getAttribute("data-fe-view"));
      if (!Number.isInteger(index) || index < 0 || index >= 32) {
        throw new Error("data-fe-view must be an integer in [0, 31]");
      }
      view.hidden = (visibleMask & (1 << index)) === 0;
    }
    if (focusTarget !== 0) {
      const candidate = componentElements(this, this, "[data-fe-action]")
        .find(element => Number(element.getAttribute("data-fe-action")) === focusTarget);
      if (candidate) this._focusAfterDispatch(candidate);
    }
    if (commandLength !== 0) {
      const memory = this._instance?.exports?.memory;
      if (!(memory instanceof WebAssembly.Memory) || commandLength > COMPONENT_COMMAND_LIMIT) {
        throw new Error("fe-component command stream requires bounded Wasm memory");
      }
      const end = commandPointer + commandLength;
      if (end > memory.buffer.byteLength || end < commandPointer) {
        throw new Error("fe-component command stream is outside Wasm memory");
      }
      const bytes = new Uint8Array(memory.buffer, commandPointer, commandLength).slice();
      this._applyCommands(decodeComponentCommands(bytes));
    }
    this.dispatchEvent(new CustomEvent("fe-state", {
      bubbles: true,
      composed: true,
      detail: { state: this._state.slice(), patch: patch.slice() },
    }));
  }

  _node(scope, target) {
    if (target === 0 && scope !== this) return scope;
    const node = componentElement(this, scope, `[data-fe-node="${target}"]`);
    if (!node) throw new Error(`fe-component has no data-fe-node ${target} in command scope`);
    return node;
  }

  _focusAfterDispatch(target) {
    // A click's browser-default focus step runs after its event listeners.
    // Defer the effect by one microtask so the focus Fe projected is the final
    // state, while still applying it within the same browser task.
    queueMicrotask(() => {
      if (this._active && target.isConnected) target.focus();
    });
  }

  _applyCommands(operations) {
    let scope = this;
    let repeat = null;
    for (const operation of operations) {
      if (operation.opcode === 1) {
        const container = this._node(this, operation.container);
        const template = componentElement(
          this,
          this,
          `[data-fe-template="${operation.template}"]`,
        );
        if (!(template instanceof HTMLTemplateElement)) {
          throw new Error(`fe-component has no template ${operation.template}`);
        }
        let rows = this._keyedRows.get(container);
        if (!rows) {
          rows = new Map();
          for (const child of container.children) {
            const key = Number(child.getAttribute("data-fe-key"));
            if (!Number.isInteger(key) || key <= 0 || key > 0xffff_ffff || rows.has(key)) {
              throw new Error("fe-component existing repeat rows need unique nonzero u32 keys");
            }
            rows.set(key >>> 0, child);
          }
          this._keyedRows.set(container, rows);
        }
        repeat = { container, template, rows, desired: [], keys: new Set() };
        scope = this;
        continue;
      }
      if (operation.opcode === 2) {
        if (!repeat || repeat.keys.has(operation.key)) {
          throw new Error(`fe-component repeat key ${operation.key} is invalid or duplicated`);
        }
        let row = repeat.rows.get(operation.key);
        if (!row) {
          row = repeat.template.content.firstElementChild?.cloneNode(true);
          if (!(row instanceof Element) || repeat.template.content.children.length !== 1) {
            throw new Error("fe-component repeat template needs one root element");
          }
          row.setAttribute("data-fe-key", String(operation.key));
          repeat.rows.set(operation.key, row);
        }
        repeat.keys.add(operation.key);
        repeat.desired.push(row);
        scope = row;
        continue;
      }
      if (operation.opcode === 3) {
        for (const [key, row] of repeat.rows) {
          if (!repeat.keys.has(key)) {
            row.remove();
            repeat.rows.delete(key);
          }
        }
        // Move only rows whose keyed order actually changed. Re-appending an
        // already-correct row transiently detaches its focused descendant in
        // Chromium, breaking controlled Fe text input after the first key.
        let cursor = repeat.container.firstElementChild;
        for (const row of repeat.desired) {
          if (row === cursor) {
            cursor = cursor.nextElementSibling;
          } else {
            repeat.container.insertBefore(row, cursor);
          }
        }
        repeat = null;
        scope = this;
        continue;
      }
      if (operation.opcode === 12 || operation.opcode === 13) {
        this._loadResource(operation, operation.opcode === 12);
        continue;
      }
      if (operation.opcode === 14) {
        this._activateSurface(operation);
        continue;
      }
      const target = this._node(scope, operation.target);
      switch (operation.opcode) {
        case 4:
          target.textContent = operation.value;
          break;
        case 5:
          if (!("value" in target)) throw new Error("set-value target has no value property");
          // Preserve the browser's selection/caret when Fe projects the value
          // already present in a live text control. Assigning the same string
          // still resets selection in some engines, which makes a fully
          // controlled Fe input feel broken while typing.
          if (target.value !== operation.value) target.value = operation.value;
          break;
        case 6:
          if (!("checked" in target)) throw new Error("set-checked target has no checked property");
          target.checked = operation.value;
          break;
        case 7:
          target.hidden = operation.value;
          break;
        case 8: {
          const className = target.getAttribute(`data-fe-class-${operation.token}`);
          if (!className) {
            throw new Error(`class token ${operation.token} is not declared on target`);
          }
          target.classList.toggle(className, operation.value);
          break;
        }
        case 9:
          this._focusAfterDispatch(target);
          break;
        case 10:
          if (!("disabled" in target)) {
            throw new Error("set-disabled target has no disabled property");
          }
          target.disabled = operation.value;
          break;
        case 11: {
          if (!(target instanceof HTMLAnchorElement)) {
            throw new Error("set-href target is not an anchor");
          }
          const url = new URL(operation.value, this.baseURI);
          if (url.origin !== new URL(this.baseURI).origin) {
            throw new Error("fe-component set-href requires a same-origin URL");
          }
          target.href = url.href;
          break;
        }
        default:
          throw new Error(`unapplied fe-component command ${operation.opcode}`);
      }
    }
  }

  _activateSurface(operation) {
    if (this._surfaceActivations.has(operation.sequence)) return;
    const surface = componentElement(
      this,
      this,
      `fe-surface[data-fe-sequence="${operation.sequence}"]`,
    );
    if (!surface || typeof surface.live !== "function") {
      this._send(
        COMPONENT_EVENT.surfaceError,
        0,
        operation.sequence + 1,
        0,
        0,
        0,
        performance.now(),
      );
      return;
    }
    const timer = operation.timeout === 0 ? null : setTimeout(() => {
      if (!this._active || !this._surfaceActivations.has(operation.sequence)) return;
      this._surfaceActivations.delete(operation.sequence);
      this._send(
        COMPONENT_EVENT.surfaceTimeout,
        0,
        operation.sequence + 1,
        0,
        0,
        0,
        performance.now(),
      );
    }, operation.timeout);
    this._surfaceActivations.set(operation.sequence, { surface, timer });
    Promise.resolve(surface.live()).then(() => {
      // `fe-live` normally reports completion synchronously. The resolved
      // effect also covers idempotently activating an already-live surface
      // after its owning component reconnects.
      if (this._active && this._surfaceActivations.has(operation.sequence)) {
        clearTimeout(timer);
        this._surfaceActivations.delete(operation.sequence);
        this._send(
          COMPONENT_EVENT.surfaceLive,
          0,
          operation.sequence + 1,
          0,
          0,
          0,
          performance.now(),
        );
      }
    }).catch(error => {
      // An emitted `fe-error` normally wins this race; this rejection path
      // guarantees the fixed effect still has exactly one completion fact.
      if (this._surfaceActivations.has(operation.sequence)) {
        clearTimeout(timer);
        this._surfaceActivations.delete(operation.sequence);
        this._send(
          COMPONENT_EVENT.surfaceError,
          0,
          operation.sequence + 1,
          0,
          0,
          0,
          performance.now(),
          String(error),
        );
      }
    });
  }

  _loadResource(operation, asText) {
    let url;
    try {
      url = new URL(operation.value, this.baseURI);
      if (url.origin !== new URL(this.baseURI).origin) {
        throw new Error("resource effects require a same-origin URL");
      }
    } catch (error) {
      this._deliverResource(operation.request, 0, 0, String(error));
      return;
    }
    const controller = new AbortController();
    this._resourceLoads.add(controller);
    (async () => {
      try {
        const response = await fetch(url, {
          mode: "same-origin",
          credentials: "same-origin",
          signal: controller.signal,
        });
        if (new URL(response.url).origin !== url.origin) {
          throw new Error("resource effect redirected across origins");
        }
        const bytes = new Uint8Array(await response.arrayBuffer());
        if (bytes.byteLength > COMPONENT_RESOURCE_LIMIT) {
          throw new Error(`resource exceeds ${COMPONENT_RESOURCE_LIMIT} byte component limit`);
        }
        let body = "";
        if (asText) body = componentTextDecoder.decode(bytes);
        this._deliverResource(
          operation.request,
          response.status >>> 0,
          bytes.byteLength >>> 0,
          body,
        );
      } catch (error) {
        if (error?.name !== "AbortError") {
          this._deliverResource(operation.request, 0, 0, String(error));
        }
      } finally {
        this._resourceLoads.delete(controller);
      }
    })();
  }

  _deliverResource(request, status, byteLength, text) {
    if (!this._active) return;
    try {
      this._send(
        COMPONENT_EVENT.resourceLoaded,
        0,
        request,
        status,
        byteLength,
        0,
        performance.now(),
        text,
        COMPONENT_RESOURCE_LIMIT,
      );
    } catch (error) {
      this._fail(error);
    }
  }

  _fail(error) {
    this._rejectReady(error);
    this.dispatchEvent(new CustomEvent("fe-error", { detail: error }));
  }
}

if (typeof customElements !== "undefined" && !customElements.get("fe-component")) {
  customElements.define("fe-component", FeComponentElement);
}

async function runFeComponent(script, instance) {
  const selector = script.dataset.feMount;
  let component = selector ? document.querySelector(selector) : null;
  if (component && !(component instanceof FeComponentElement)) {
    throw new Error("data-fe-mount must select a <fe-component>");
  }
  if (!component) {
    component = document.createElement("fe-component");
    script.insertAdjacentElement("afterend", component);
  }
  await component.attachFeInstance(instance);
  return component;
}

/**
 * `data-fe-render` handoff: a render bundle's manifest/wasm/wgsl fetching,
 * pipeline setup, lifecycle, and per-pixel wasm fallback all live in the ONE
 * shipped `fe-render-runtime.js` module (the same module the legacy `fe web
 * build --mode render` bundle's emitted index.html imports), not here. This
 * function locates that published module (importing it defines the
 * `<fe-surface>` custom element as a side effect) and inserts a
 * `<fe-surface manifest=...>` element after the script, letting the element
 * mount and drive its own lifecycle rather than calling `mountRenderSurface`
 * imperatively (FE_WEB_V5_ORCHESTRATION_DESIGN.md 3.3). `fe:load` resolves
 * once the element reaches `ready` (manifest fetched, poster rendered); it no
 * longer waits for the surface to go live, since v5 tiles are poster-first by
 * design.
 */
async function runRenderSurface(element, manifestUrl) {
  const runtimeReference = element.dataset.feRenderRuntime;
  if (!runtimeReference) {
    throw new Error("data-fe-render requires data-fe-render-runtime");
  }
  const runtimeUrl = new URL(runtimeReference, element.baseURI);
  await import(runtimeUrl); // defines `<fe-surface>`; no other export is needed here.

  const surface = document.createElement("fe-surface");
  surface.setAttribute("manifest", manifestUrl.href);
  if (element.dataset.feSequence !== undefined) {
    surface.setAttribute("data-fe-sequence", element.dataset.feSequence);
    surface.setAttribute("boot", "manual");
  }
  if (element.dataset.feWidth) surface.setAttribute("width", element.dataset.feWidth);
  if (element.dataset.feHeight) surface.setAttribute("height", element.dataset.feHeight);
  for (const kind of ["wgsl", "wasm", "manifest"]) {
    const attribute = `data-fe-${kind}-action`;
    if (element.hasAttribute(attribute)) {
      surface.setAttribute(attribute, element.getAttribute(attribute));
    }
  }

  const canvasSelector = element.dataset.feCanvas;
  const adopted = canvasSelector ? document.querySelector(canvasSelector) : null;
  if (adopted) {
    // `data-fe-canvas` adoption keeps working: the element accepts an
    // adopted canvas instead of generating its own poster/live pair.
    surface.adoptCanvas(adopted);
    adopted.insertAdjacentElement("afterend", surface);
  } else {
    // With no `data-fe-canvas`, mount immediately after the script element
    // (page authors keep layout control via HTML when they DO adopt a
    // canvas by selector).
    element.insertAdjacentElement("afterend", surface);
  }

  await new Promise((resolve, reject) => {
    surface.addEventListener("fe-ready", () => resolve(), { once: true });
    surface.addEventListener("fe-error", (event) => reject(event.detail), { once: true });
  });
  return { manifest: surface.manifest, surface, instance: null, module: null, value: undefined };
}

async function run(element) {
  if (element.dataset.feState === "complete") return element.feResult;
  if (element.dataset.feState === "running") return element.fePromise;
  const promise = (async () => {
    element.dataset.feState = "running";
    try {
      const manifestUrl = new URL(element.dataset.feManifest, element.baseURI);
      if (element.dataset.feRender !== undefined) {
        const result = await runRenderSurface(element, manifestUrl);
        element.feResult = result;
        element.dataset.feState = "complete";
        element.dispatchEvent(new CustomEvent("fe:load", { detail: result }));
        return result;
      }
      const wasmUrl = new URL(element.dataset.feSrc, element.baseURI);
      const [manifestResponse, wasmResponse] = await Promise.all([
        fetch(manifestUrl, { mode: "cors", credentials: "same-origin" }),
        fetch(wasmUrl, {
          mode: "cors",
          credentials: "same-origin",
          integrity: element.dataset.feIntegrity || "",
        }),
      ]);
      if (!manifestResponse.ok || !wasmResponse.ok) {
        throw new Error("could not load precompiled Fe manifest or Wasm artifact");
      }
      const manifest = await manifestResponse.json();
      const artifact = manifest.artifacts?.find(value => value.kind === "wasm_module");
      if (!artifact) throw new Error("Fe manifest contains no Wasm module");
      const bytes = new Uint8Array(await wasmResponse.arrayBuffer());
      if (bytes.byteLength !== artifact.byte_len ||
          await sha256Hex(bytes) !== artifact.sha256) {
        throw new Error("Fe Wasm artifact failed manifest integrity verification");
      }
      const module = await WebAssembly.compile(bytes);
      const context = { element, manifest, module };
      let selectedImports;
      if (element.dataset.feAdapter) {
        const environmentFactory = globalThis.feAdapterEnvironment;
        if (!environmentFactory) {
          throw new Error("selected Fe adapter requires globalThis.feAdapterEnvironment");
        }
        const environment = typeof environmentFactory === "function"
          ? await environmentFactory(context)
          : environmentFactory;
        const adapterModule = await import(new URL(element.dataset.feAdapter, element.baseURI));
        if (typeof adapterModule.createFeHostAdapter !== "function") {
          throw new Error("selected Fe adapter exports no createFeHostAdapter");
        }
        selectedImports =
          adapterModule.createFeHostAdapter(environment.host, environment.runtime).imports;
      }
      const imports = await importsFor(context, selectedImports);
      for (const required of WebAssembly.Module.imports(module)) {
        if (!Object.hasOwn(imports, required.module) ||
            !Object.hasOwn(imports[required.module], required.name)) {
          throw new Error(`missing Wasm import: ${required.module}.${required.name}`);
        }
      }
      const instance = await WebAssembly.instantiate(module, imports);
      if (element.dataset.feComponent !== undefined) {
        const component = await runFeComponent(element, instance);
        const result = { instance, module, manifest, component, value: undefined };
        element.feResult = result;
        element.dataset.feState = "complete";
        element.dispatchEvent(new CustomEvent("fe:load", { detail: result }));
        return result;
      }
      const entryName = element.dataset.feEntry || manifest.entry || "main";
      const entry = instance.exports[entryName];
      if (typeof entry !== "function") {
        throw new Error(`Fe Wasm export \`${entryName}\` was not found`);
      }
      const value = element.dataset.feAutostart === "false" ? undefined : await entry();
      const result = { instance, module, manifest, value };
      element.feResult = result;
      element.dataset.feState = "complete";
      element.dispatchEvent(new CustomEvent("fe:load", { detail: result }));
      return result;
    } catch (error) {
      element.dataset.feState = "error";
      element.dispatchEvent(new CustomEvent("fe:error", { detail: error }));
      throw error;
    }
  })();
  element.fePromise = promise;
  try {
    return await promise;
  } finally {
    delete element.fePromise;
  }
}

export async function bootFeArtifacts(root = document) {
  const elements = Array.from(root.querySelectorAll('script[type="application/fe+wasm"]'));
  const settled = await Promise.allSettled(elements.map(element => run(element)));
  const failures = settled.filter(result => result.status === "rejected");
  if (failures.length === 1) {
    throw failures[0].reason;
  }
  if (failures.length > 1) {
    throw new AggregateError(
      failures.map(result => result.reason),
      `${failures.length} Fe artifacts failed to boot`,
    );
  }
  return settled.map(result => result.value);
}

if (typeof document !== "undefined") {
  bootFeArtifacts().catch(error => {
    globalThis.dispatchEvent?.(new CustomEvent("fe:bootstrap-error", { detail: error }));
  });
}
