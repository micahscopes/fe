# Bounded QCGA sparse-incidence planner

This slice is a genuine Fe-native planner for the grade-1 point/dual-quadric
contraction used by the rotated-quadric demo. It is deliberately not described
as general QCGA or as a dense Cl(9,6) multivector implementation.

The supported point has 12 populated basis vectors and the supported dual
quadric has 12. Ordinary Fe `const fn` semantics enumerate their 144 candidate
pairs, apply the paper-null metric, and retain exactly 12 terms. The recursive
`IncidencePlan<12>` type is derived from those survivors; no term table,
Python-generated Fe, 32,768-entry storage, or runtime blade loop exists.

`QcgaIncidenceProvider` consumes the same bounded slot model and publishes one
aggregate `incidence` implementation. The provider interpreter cannot currently
branch inside an ordinary const helper, so its three metric cases are visible
at the provider loop site while the typed plan uses `contraction_sign`.
This is a compiler phase boundary, not a second survivor table.

Evidence:

- the independent Python oracle constructs the R(9,6) execution basis from the
  paper-null basis, proves the metric, independently enumerates the same 144
  pairs, and pins the exact 12 signed survivors;
- Fe statically pins the survivor count/order and the recursive plan length;
- entry-rooted Wasm matches an independently authored raw candidate-order
  contraction bit-for-bit and a fused polynomial within f32 tolerance;
- the aggregate Wasm shape is 14 adds, 4 subs, 24 multiplies, one shared helper
  call, and zero loops;
- the parameterized render uses typed `CameraInputs` and `QuadricInputs`,
  executes the planned contraction at the hit, and reproduces all 16,384 pixels
  of the current frame exactly (FNV-1a-32 `2368784280`);
- browser-profile SPIR-V/Naga emits 11,877 bytes of WGSL with only the vertex
  and fragment functions. The FCO helper is specialized away; there is no WGSL
  runtime loop or dense Cl(9,6) storage.

The render keeps the established fused square/cross/linear grouping for ray
root selection and uses the planned sparse contraction as the independent hit
incidence. This preserves current f32/frame behavior while making the algebraic
planner execute on the render path.

## Sharing boundary

The exact retained product keys are:

| Sign | Point/quadric slot pairs |
| --- | --- |
| positive | `(0,0)`, `(1,1)`, `(2,2)` |
| negative | `(3,9)`, `(4,10)`, `(5,11)`, `(6,3)`, `(7,4)`, `(8,5)`, `(9,6)`, `(10,7)`, `(11,8)` |

All twelve keys are distinct and every term has coefficient magnitude one.
Consequently the direct unrolled provider is already the compact/shared-DAG
fixed point for this operator: there is neither within-term fanout nor a
cross-term product to reuse. The repeated `j / 3` value is shared while the
ordinary Fe caller constructs its operand record; it is not a repeated
plan-product node.

The Wasm planner gate independently enumerates this order, requires twelve
unique keys, and rejects both runtime candidate rediscovery and a misleading
`builder.share` annotation. Unlike the Cl(4,1) Schedule32 comparison, adding
three QCGA executors would produce no distinct execution graph and would only
duplicate the existing raw-expansion, full-frame, and browser-profile gates.
This is evidence about this bounded incidence operator, not proof that general
QCGA expressions lack useful common subexpressions.

The planner is now the live QCGA browser artifact. Its compiler-derived
`FrameRequest` carries generation plus all 15 camera/quadric scalars across the
render, verify, and oracle lanes. The generated WebGPU layout independently
pins those scalars to arguments 2 through 16 and byte offsets 0 through 56.

Observed browser acceptance on Chromium/SwiftShader:

- presentation-only mode fetched only layout, WGSL, and Fe source, created no
  Worker, loaded no Wasm, and performed zero readbacks;
- explicit verification used one generated Worker, one canonical full-frame
  Fe/Wasm oracle call, and one GPU readback;
- all 16,384 pixels (65,536 RGBA bytes) agreed at FNV-1a-32 `2368784280`
  (`0x8d30c798`); the browser Wasm oracle took 58.3 ms in that run.

This promotion makes QCGA a genuine second planner-backed browser application.
It does not broaden the supported algebra beyond the documented 12-by-12
paper-null incidence plan.
