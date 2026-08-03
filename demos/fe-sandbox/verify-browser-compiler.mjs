import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";

const packageDir = resolve(
  process.argv[2] || new URL("./gen/compiler", import.meta.url).pathname,
);
const glue = await import(
  pathToFileURL(resolve(packageDir, "fe_browser_compiler.js")).href
);
const compilerBytes = await readFile(
  resolve(packageDir, "fe_browser_compiler_bg.wasm"),
);
await glue.default({ module_or_path: compilerBytes });
glue.install_panic_hook();

const request = {
  protocol: { major: 1, minor: 1 },
  root: "fe-memory:///inline.fe",
  sources: [{
    url: "fe-memory:///inline.fe",
    text: "pub fn main() -> u32 { 42 }",
  }],
  target: "wasm",
  entries: ["main"],
  options: { optimization: "none", debug_info: false },
};
const result = JSON.parse(glue.compile_json(JSON.stringify(request)));
const artifact = result.artifacts.find(({ kind }) => kind === "wasm_module");
assert(artifact, "browser compiler must return a Wasm module");
assert.deepEqual(result.diagnostics, []);

const compiled = await WebAssembly.instantiate(
  Uint8Array.from(artifact.bytes),
  {},
);
assert.equal(compiled.instance.exports.main(), 42);

console.log(JSON.stringify({
  compiler_wasm_bytes: compilerBytes.length,
  produced_wasm_bytes: artifact.bytes.length,
  value: 42,
  diagnostics: 0,
}));
