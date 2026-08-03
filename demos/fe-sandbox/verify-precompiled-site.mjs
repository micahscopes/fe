import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";

import { createFeScriptLoader } from "./fe-script-loader.js";

if (!globalThis.CustomEvent) {
  globalThis.CustomEvent = class CustomEvent {
    constructor(type, init) {
      this.type = type;
      this.detail = init?.detail;
    }
  };
}

const siteDir = resolve(process.argv[2]);
const html = await readFile(resolve(siteDir, "index.html"), "utf8");
const tag = html.match(/<script\s+([^>]*type="application\/fe\+wasm"[^>]*)>/);
assert(tag, "built site must contain a precompiled Fe script");
const attributes = new Map();
for (const match of tag[1].matchAll(/([:\w-]+)="([^"]*)"/g)) {
  attributes.set(match[1], match[2]);
}
const element = {
  type: attributes.get("type"),
  dataset: {
    feEntry: attributes.get("data-fe-entry"),
  },
  get attributes() {
    return Array.from(attributes, ([name, value]) => ({ name, value }));
  },
  getAttribute(name) {
    return attributes.get(name) ?? null;
  },
  dispatchEvent(event) {
    this.lastEvent = event;
  },
};
const baseUrl = pathToFileURL(`${siteDir}/`).href;
const loader = createFeScriptLoader({
  baseUrl,
  fetch: async (url) => {
    const path = new URL(url);
    const bytes = await readFile(path);
    return {
      ok: true,
      json: async () => JSON.parse(bytes.toString("utf8")),
      arrayBuffer: async () =>
        bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength),
    };
  },
});
const result = await loader.run(element);
assert.equal(result.value, 42);
assert.equal(element.dataset.feState, "complete");
assert.equal(element.lastEvent.type, "fe:load");
console.log(JSON.stringify({
  mode: "precompiled",
  value: result.value,
  state: element.dataset.feState,
}));

