# Browser compiler footprint

The browser compiler deliberately builds the workspace's Wasm-only codegen
profile. SPIR-V/Naga remains available through `fe-codegen/spirv-backend` and
is enabled explicitly by the legacy `fe` CLI; native JIT support remains behind
`fe-codegen/native-backend`.

Measured with `demos/fe-sandbox/build-browser-compiler.sh` (release
`wasm-pack`, including its `wasm-opt` pass):

| profile | optimized compiler Wasm |
| --- | ---: |
| shared Wasm + SPIR-V codegen features | 18,064,948 bytes |
| Wasm-only codegen features | 17,160,465 bytes |
| Wasm-only + HIR test utilities excluded | 17,174,164 bytes |

The reduction is 904,483 bytes (5.01%). Both measurements used the same smoke
test: the browser module compiled virtual Fe source to an 87-byte executable
Wasm module, produced zero diagnostics, and the result returned `42`.

Excluding HIR test utilities cleans the compiled dependency graph but did not
produce an additional payload reduction: the measured artifact changed by
+13,699 bytes (+0.08%). The test-only code was already unreachable to
link-time/`wasm-opt` elimination, and the shared worktree continued evolving
between these whole-workspace builds, so this small delta is not attributed as
a regression or saving.

`cargo tree -p fe-browser-compiler --target wasm32-unknown-unknown -e normal`
confirms that Naga, SPIR-V, the native Cranelift JIT, REVM, Axum, Tokio, and the
Fe CLI are absent. `cranelift-entity` remains as a target-independent arena
index/data-structure dependency; it is not the native backend.

The remaining large safe-split blocker is upstream/shared architecture:
Sonatina's Wasm backend and Fe's runtime lowering share Sonatina IR,
parser/verifier, and codegen modules with EVM-oriented lowering. Splitting those
without an upstream crate/feature boundary would duplicate or fork required
Wasm lowering, so this audit does not guess at source-level dead-code gates.
Fe HIR's test database is gated by its `testutils` feature. Direct HIR test
builds retain that feature by default, while workspace production dependencies
disable HIR defaults and test consumers such as MIR opt in from their
dev-dependencies. Browser builds therefore do not carry `fe-test-utils`.
