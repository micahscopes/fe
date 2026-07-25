# Canonical Cl(4,1) browser gate ladder

This is the ordered gate sequence for the recurrence-derived canonical-50 to
Schedule32 browser kernel. Do not skip directly to full generation after a
compiler or canonical-ingot change: the full HIR failure cycle is currently
about ten minutes and obscures which recursive rung failed.

The algebra under test is fixed:

- `canonical_cl41_schedule` obtains its coefficients from the public
  Fuchs–Théry `CliffordGp<Cl41Metric>` recurrence.
- The canonical 50 commutative monomials contain exactly 32 nonzero terms.
- `Canonical50Term` carries candidate, left, point, right, output, magnitude,
  and sign metadata.
- The runtime tree is four right-deep eight-term chunks, pairwise associated.
- The typed tree's reassociation is tolerance-checked against independent
  raw80 arithmetic; Wasm and WebGPU execute the same tree and are compared
  exactly.

## Current compiler reproducer

`canonical50_eval5_chunk0_n1_signed_field_reproducer` in
`crates/codegen/tests/fco_cga80_direct_lanes.rs` selects
`Canonical50TypedChunk0<1>`.

Observed on 2026-07-24:

- `Zero` passed application HIR and all-root runtime-package construction.
  Fe evaluation took about 42 seconds after the Rust generator was built.
- `Chunk0<1>` normalized candidate `10`, left `0`, point `0`, right `2`,
  output `8`, and magnitude `2`.
- Its seventh staged anonymous const became
  `invalid(ConstTyExpected(i32))`.
- Instrumentation identified `StepLimitExceeded` while evaluating that staged
  const. This is an evaluation-budget/cache defect, not a rejected type-system
  construct.
- The same recurrence-derived expression passes
  `static_assert(canonical50_projected_sign(10) == 0)` and direct
  `SignProbe<{canonical50_projected_sign(10)}> == SignProbe<0>` type
  materialization outside the recursive payload.
- The isolated exact 32-term type/equality/Wasm proof previously completed in
  about 290.5 seconds.
- A full browser HIR composition reached the same invalid signed payload after
  roughly eleven minutes.

The enhanced direct-sign plus N1 diagnostic run was intentionally interrupted
after about ten minutes once the direct checks had passed and the compiler
instrumentation had isolated `StepLimitExceeded`.

## Post-fix order

Always invoke the Cargo wrapper. Do not run an old
`target/fe-browser/debug/examples/gen_cga_schedule32_vec5_demo` binary
directly: the generator embeds Fe sources with `include_str!`, so a direct
binary can contain stale canonical or interpreter text.

From the repository root:

```sh
FE_CGA_EVAL_ROOT='Canonical50TypedChunk0<1>' \
FE_CGA_SCHEDULE32_HIR_ONLY=1 \
demos/with-browser-cargo.sh run -p fe-codegen \
  --example gen_cga_schedule32_vec5_demo
```

Then run the same command with these roots, in order:

```text
Canonical50TypedChunk0<2>
Canonical50TypedChunk0<8>
Canonical50TypedBalancedSchedule32
```

For the full root, omitting `FE_CGA_EVAL_ROOT` is the authoritative form:

```sh
FE_CGA_SCHEDULE32_HIR_ONLY=1 \
demos/with-browser-cargo.sh run -p fe-codegen \
  --example gen_cga_schedule32_vec5_demo
```

After all HIR rungs pass, build and audit the runtime package without producing
browser artifacts:

```sh
FE_CGA_SCHEDULE32_PACKAGE_ONLY=1 \
demos/with-browser-cargo.sh run -p fe-codegen \
  --example gen_cga_schedule32_vec5_demo
```

That gate must show that recurrence, planner, support, packed-coefficient, and
wide-integer helpers are absent from runtime MIR.

Next run full generation:

```sh
demos/with-browser-cargo.sh run -p fe-codegen \
  --example gen_cga_schedule32_vec5_demo
```

Full generation must pass, in order:

1. independent raw80 versus typed-tree algebra checks;
2. Wasm validation and pinned-frame oracle comparison;
3. browser-profile SPIR-V lowering and WGSL validation;
4. generated-source audits proving no planner, recurrence, candidate decoder,
   or unsupported wide integer leaked into WGSL;
5. Wasm/WebGPU exact agreement for the shared typed kernel.

Finally run the asset verifier and real-browser acceptance/continuous-render
gates:

```sh
python3 demos/webgpu-cga-inversion/verify-assets.py
python3 demos/webgpu-cga-inversion/test_cdp_acceptance.py
python3 demos/webgpu-cga-inversion/test_cdp_continuous_benchmark.py
CHROME_BIN=/path/to/chrome \
  demos/webgpu-cga-inversion/smoke-chrome.sh
CGA_SMOKE_VERIFY=off \
CGA_SMOKE_BENCHMARK=continuous \
CHROME_BIN=/path/to/chrome \
  demos/webgpu-cga-inversion/smoke-chrome.sh
```

The normal presentation path must submit WebGPU work directly to the canvas
without per-frame readback. Readback is permitted only in the explicit
verification path.
