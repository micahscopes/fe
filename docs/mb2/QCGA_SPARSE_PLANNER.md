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

The planner fixture is not yet the live QCGA browser artifact. Browser promotion
still requires extending the canonical actor request and UI/runtime values for
the 15 camera/quadric scalars, regenerating the standard bundle, and rerunning
the existing default-no-readback and explicit Wasm/WebGPU equality gates.
