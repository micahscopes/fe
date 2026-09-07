# Generic control-net materialization

Quilting's generic Fe constructor returns `Option<[ProjectiveControl<A>;N]>`;
each control contains associated point and weight types. Source checking passed,
but Wasm lowering rejected materialization, then aggregate copy, as containing
non-scalar memory leaves.

Both operations were asking an aggregate-only admission query which lacked the
owning generic instantiation. The function lowerer now uses one local-aware helper
to pass the local's semantic type and `RuntimeBody::owner` to the existing
`aggregate_is_semantically_memory_lowerable_in` query. No scalar envelope, pointer
exception, layout, or copy semantics is broadened. Resolving associated fields
uses the same normalization machinery already used for scoped-arena parameters.

The focused regression in `wasm_aggregate_array_dynamic_index` returns an optional
array of generic controls with a nested associated point record, mutates each
element, checks every returned element and `None`, and verifies bounds trapping.
The full Quilting construction additionally passed comparison with an independent
f64 inversion/interpolation calculation for triangular and bilinear patches.

September 7 receipts:

- Focused associated-record regression passed.
- Full aggregate-array file: **10 passed, 1 failed**. The zero-length generic
  mutation fixture panics at `crates/mir/src/runtime/lower/body.rs:5085`, `cannot
  lower erased place root`, while building its runtime body. This is earlier than
  the changed portable lowering operations. No MIR files were changed in this
  slice. The suite is not claimed green, nor is an earlier baseline test run
  claimed; this separate failure remains to be resolved.
- Quilting's final geometry oracle passed in 16.89 seconds, maximum normalized
  coordinate/residual error `1.626367520657368e-6` against a `1e-5` gate.
- The Wasm-only consumer also caught a missing `spirv-backend` gate on exports
  introduced in the preceding direct-draw change; that was restored separately
  as `3f741f3c1`.

Logs under `/laboratory/quilting/scratch/`:
`pole-associated-record-final-20260907.log`, `pole-zero-length-isolated-20260907.log`,
and `pole-patch-final-oracle-20260907.log`.
