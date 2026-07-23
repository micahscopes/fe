# Bounded sparse multivector value API

The reusable source fragments
`crates/codegen/tests/fixtures/support_bladeset_api.fe` and
`sparse_multivector_api.fe` now cover both halves of a sparse algebra:
compile-time support planning and support-sized runtime values. They use
ordinary Fe only.

For supports selected from the 32 blades of a dimension-at-most-five algebra,
with up to five populated coefficients:

- `SparseStorage<support_cardinality(...)>` computes the logical storage
  shape. Accepted conversion witnesses prove the five- and four-cell manual
  runtime aliases equal those computed shapes.
- `sparse_rank` computes compact positions at CTFE. Sound ground-support
  countdown selectors preserve the requested blade until their base case,
  then turn presence and rank into domain `Found`/`Missing` types. Missing and
  out-of-range blades therefore fail closed.
- `sparse_coefficient` expresses absent blades as a statically selected zero.
  A concrete aggregate-domain trait call lowers through semantic MIR and Wasm.
- `sparse_present_coefficient` has no absent-blade implementation, so code
  which promises presence cannot produce executable Wasm.
- Domain facades can hide `SparseCell` completely. `sparse_cga_value.fe`
  demonstrates semantic `cga_point` and `cga_inversion_sphere` constructors.

The test proves the five-scalar point and four-scalar sphere cardinalities,
ground/computed type equality, compact ranks, and the sphere-e3
`CgaSphereMissing` selection at compile time. It then constructs an actual
four-cell semantic sphere and executes its missing-e3
`SparseCoefficient<CgaInversionSphere>` implementation through import-free
Wasm. The semantic probe calls that accessor rather than injecting an
unrelated zero lane. A separate gate proves present-only access to sphere e3
cannot produce executable Wasm.

This is intentionally not yet the final public algebra package. Fe fixtures do
not have imports, so Rust tests compose the fragments. More importantly, a
support-generic invariant const parameter in today's restricted recursive type
function leaves executable trait operations unresolved; the diagnostic's
suggested non-recursive `type fn` selector is not implemented on this branch.
Ground-support wrappers are therefore required. Aggregate runtime lowering
still requires ground aliases rather than computed `Storage<N>` aliases even
though their type equality is accepted. The executable accessor is therefore
grounded, while the computed missing selection remains a separate type
witness. The parser now explicitly consumes a continuation newline after a
type-alias `=`, so formatting that ground alias over multiple lines no longer
silently leaves its RHS invalid during later trait selection.
`Found<rank>` record projection is bounded to five populated coefficients.
Constructors spell the private nested cells once; derive/FCO can generate them
after multi-type record reflection is available. The root Cargo pin still
targets Sonatina `150d327e`, which lacks the float and layout APIs used by this
branch, so codegen gates require `/workspace/sonatina-sparse-api`.

The flagship aggregate `Sandwich::sandwich` implementation remains unchanged;
this spike records the reusable substrate and its exact compiler edges without
changing or regenerating browser artifacts.
