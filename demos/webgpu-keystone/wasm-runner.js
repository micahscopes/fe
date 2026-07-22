// wasm-runner.js - the Fe -> wasm lane, run in-page.
//
// Loads the Fe-compiled `kernel.wasm` (zero imports) and calls the export named
// in layout.json. Kernel-blind: the export name comes from the metadata, not a
// literal here. Fe `u32` returns as wasm `i32`; `>>> 0` restores the unsigned
// value (the > 2^31 pin comes back as a negative i32 without it).

export async function runWasm(wasmUrl, layout) {
  const resp = await fetch(wasmUrl);
  if (!resp.ok) throw new Error(`fetch ${wasmUrl} -> HTTP ${resp.status}`);
  const bytes = await resp.arrayBuffer();

  // No imports: the kernel is a pure straight-line function.
  const { instance } = await WebAssembly.instantiate(bytes, {});

  const exportName = layout.wasm_export;
  const fn = instance.exports[exportName];
  if (typeof fn !== "function") {
    throw new Error(`wasm export \`${exportName}\` (from layout.json) not found`);
  }

  const raw = fn();
  // Normalize i32 -> u32.
  return raw >>> 0;
}

// runWasmGrid - the Fe -> wasm lane for a GRID kernel, run in-page.
//
// Instantiates the SAME `kernel.wasm` and calls the export per (px, py),
// row-major, returning a `Uint32Array(width * height)`. Kernel-blind beyond the
// export name (which the page reads from layout.json). Fe `u32` returns as wasm
// `i32`; `>>> 0` normalizes each pixel to unsigned. This is the browser's
// cross-backend oracle the WebGPU grid is compared against pixel-for-pixel.
export async function runWasmGrid(exportName, width, height) {
  const wasmUrl = "./gen/kernel.wasm";
  const resp = await fetch(wasmUrl);
  if (!resp.ok) throw new Error(`fetch ${wasmUrl} -> HTTP ${resp.status}`);
  const bytes = await resp.arrayBuffer();

  // No imports: the kernel is a pure straight-line function.
  const { instance } = await WebAssembly.instantiate(bytes, {});

  const fn = instance.exports[exportName];
  if (typeof fn !== "function") {
    throw new Error(`wasm export \`${exportName}\` (from layout.json) not found`);
  }

  const grid = new Uint32Array(width * height);
  for (let y = 0; y < height; y++) {
    for (let x = 0; x < width; x++) {
      grid[y * width + x] = fn(x, y) >>> 0;
    }
  }
  return grid;
}

// instantiateWasm(bytesOrUrl) - load a zero-import Fe wasm module, from a URL
// string (fetch) or an ArrayBuffer/Uint8Array (the standalone inlines base64).
// Returns the instance's exports.
export async function instantiateWasm(bytesOrUrl) {
  let bytes;
  if (typeof bytesOrUrl === "string") {
    const resp = await fetch(bytesOrUrl);
    if (!resp.ok) throw new Error(`fetch ${bytesOrUrl} -> HTTP ${resp.status}`);
    bytes = await resp.arrayBuffer();
  } else {
    bytes = bytesOrUrl;
  }
  const { instance } = await WebAssembly.instantiate(bytes, {});
  return instance.exports;
}

// renderFragmentGrid(fragExports, exportName, view, width, height) - the AMBER
// (no-WebGPU) render leg: call the Fe render FRAGMENT per pixel in V8, exactly the
// same (px, py, center_re, center_im, scale_q) -> packed-RGBA function the GPU runs
// as its fragment stage. Returns a Uint32Array(width*height) of packed RGBA words
// (LE bytes = [R,G,B,A]); the fragment owns the palette, so JS colors NOTHING.
// This is "your browser computed every pixel with Fe" without a GPU.
export function renderFragmentGrid(fragExports, exportName, view, width, height) {
  const f = fragExports[exportName];
  if (typeof f !== "function") {
    throw new Error(`wasm export \`${exportName}\` (from layout.json) not found`);
  }
  // Kernel-blind about param arity: spread the broadcast params (mandel's view
  // triple, clifford's rotor quad, ...) after (x, y). A 3-array expands to the
  // same f(x,y,cr,ci,sq) call the mandel page always made.
  // Preserve JS Numbers. WebAssembly performs the declared parameter coercion:
  // integer exports receive i32, while f32 exports retain their fractional
  // values. Pre-coercing with `| 0` silently destroyed typed f32 broadcasts.
  const params = [...view];
  const out = new Uint32Array(width * height);
  for (let y = 0; y < height; y++) {
    const row = y * width;
    for (let x = 0; x < width; x++) {
      out[row + x] = f(x, y, ...params) >>> 0;
    }
  }
  return out;
}
