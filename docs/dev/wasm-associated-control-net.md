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

## Optional control arrays passed to helpers

Factoring the evaluator into a helper exposed additional missing paths. The
regression now passes the optional array into `evaluate` before matching and
indexing it. This is ordinary Fe composition, not a demo-specific ABI.

- Private reference admission, enum payload recursion, copy planning and place
  access retain the owning type instantiation.
- An aggregate-value carrier can retain a source `ref T` label after copying a
  pattern payload. Admission uses T for that value carrier only; this does not
  legalize arbitrary reference carriers.
- Canonical-arena aggregate projections use the existing enum tag/payload
  offsets for variant fields.
- Nonempty Wasm aggregate slots read through projected places receive arena
  storage. Empty aggregates keep their erased representation. Copying from
  flattened values into those slots is an explicit `Materialize` storage-plan
  operation, counted by the existing allocation/emission consistency check.

This does not change the public pointer ABI or disable bounds, escape, copy,
or allocation checks. Shader typed-private storage is not selected by the new
Wasm slot rule.

Final release receipts for this extension:

- Aggregate arrays: **10 passed, 1 failed** (the same earlier zero-length MIR
  mutation failure); the helper-argument regression now passes.
- Canonical arena: **5 passed**; nested record view retag: **1 passed**.
- Quilting's `patch_` selection: **12 passed**, including the edge-authored
  triangle/quad boundary oracle and existing dense-Clifford/QB comparisons.

Logs: `associated-helper-planned-20260907.log`,
`associated-helper-boundaries-final-20260907.log`, and
`edge-patch-planned-20260907.log` in `/laboratory/quilting/scratch/`.

## Associated fields at the web state boundary

The integrated viewer exposed a separate canonical-interface omission:
instantiating `Control<Model>` substituted its argument but left a closed
associated projection in a field type. Canonical state derivation then rejected
the field instead of resolving it. Record and variant field derivation now use
the existing semantic normalizer before selecting the canonical layout. This
does not add a new transport or flatten application state by hand.

Release canonical-interface tests: **18 passed**, including nested associated
record/variant layout and continued rejection of an associated `u256` field.
Receipt: `/laboratory/quilting/scratch/canonical-associated-fields-20260907.log`.
The real cross-ingot viewer build and browser execution remain separate gates;
these unit results alone do not establish either.
