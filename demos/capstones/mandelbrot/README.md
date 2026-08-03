# Canonical multi-backend Mandelbrot

`kernel.fe` is the single authored Fe source for the EVM/revm, Wasm,
browser-profile WebGPU, and Native/Cranelift capstone.

Its SHA-256 is
`dd9edf593b8477f2afeea3c2e4e51669d67a1a1e8f37782f2c43e1b124f8d871`.

The target-neutral entry is:

```text
mandel_pixel_q12(i32, i32) -> u32
```

Target envelopes are tooling concerns and must not duplicate or modify the
kernel body. The stable semantic reference for the 512×512 row-major result is
FNV-1a-32 `0x2d29649a`; `(0,0) = 1`, `(511,0) = 2`, `(0,511) = 1`,
`(511,511) = 2`, and `(256,256) = 100`.

Current executed coverage:

- EVM/revm: four corners plus the centre through a generated contract envelope.
- Wasm/wasmtime: every pixel.
- Native/Cranelift: every pixel through Fe's opt-in Native backend, with the
  same `0x2d29649a` frame hash.
- SPIR-V/WGSL: browser-profile WGSL validation is mandatory. The live WebGPU
  test compares every pixel with Wasm and the oracle when an adapter is
  available; a host without an adapter does not earn or report live execution.

The EVM leg intentionally uses a deterministic probe set because executing
262,144 contract calls is not a useful capstone cost. Wasm, Native, and live
WebGPU verification cover the entire frame.

## Reproducible artifacts and evidence

Run the existing browser artifact generator from the repository root:

```sh
cargo run -p fe-codegen --example gen_mandelbrot_demo
```

After its compile, interface, browser-profile, and exhaustive Wasm oracle gates
pass, it writes `evidence.json` here. The versioned evidence manifest records
the canonical source SHA-256, the `mandel_pixel_q12(i32, i32) -> u32`
interface snapshot, sorted imports and exports, emitted artifact byte counts
and SHA-256 hashes, runtime, and verification status for EVM, Native, Wasm, and
WebGPU. It has no timestamp, host path, adapter result, or git state, so equal
compiler outputs produce byte-identical evidence.

This one command emits portable artifacts for the Wasm and WebGPU lanes. The
EVM contract envelope is test-only and the Native backend is an in-process JIT,
so their artifact fields remain empty and their exact verification commands are
recorded as `not_run`; no portable bytes or successful execution are invented.
Likewise, browser-profile WGSL validation earns `validated`, never a live-GPU
`verified` claim. The live adapter-dependent test remains the separate gate
documented above.
