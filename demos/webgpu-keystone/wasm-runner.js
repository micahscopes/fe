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
