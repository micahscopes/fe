# Bounded sparse multivector value API

The package-like ordinary-Fe prelude
`crates/codegen/tests/fixtures/sparse_clifford_api.fe` covers three reusable
pieces of a sparse algebra in one source unit: compile-time support planning,
the `Zero`/`Term`/`Add` operator-plan vocabulary, and support-sized runtime
values. Schedule32 and QCGA prepend this same source before their domain
planner and kernel.

For supports selected from the 32 blades of a dimension-at-most-five algebra:

- `SparseStorage<support_cardinality(...)>` computes the logical storage
  shape. Accepted conversion witnesses prove the five- and four-cell manual
  runtime aliases equal those computed shapes.
- `sparse_rank` computes compact positions at CTFE. `SparseIndex<rank>`
  recursively materializes a `Here/Next` access path for the full bounded
  support; there is no rank-specific `Found<0>` through `Found<4>` ladder.
  Ground-support countdown selectors preserve the requested blade until their
  base case and turn presence and rank into domain index/missing types.
  Missing and out-of-range blades therefore fail closed.
- `sparse_coefficient` expresses absent blades as a statically selected zero.
  A concrete aggregate-domain trait call lowers through semantic MIR and Wasm.
- `sparse_present_coefficient` has no absent-blade implementation, so code
  which promises presence cannot produce executable Wasm.
- Domain facades can hide `SparseCell` completely. `sparse_cga_value.fe`
  demonstrates semantic `cga_point` and `cga_inversion_sphere` constructors.

The tests prove the five-scalar point and four-scalar sphere cardinalities,
ground/computed type equality, compact ranks, and the sphere-e3
`CgaSphereMissing` selection at compile time. It then constructs an actual
four-cell semantic sphere and executes its missing-e3
`SparseCoefficient<CgaInversionSphere>` implementation through import-free
Wasm. A separate eight-cell `SparseStorage<8>` proof executes its recursively
selected final coefficient through Wasm and returns `8.0`, while the same
computed storage type defaults a missing coefficient to zero. The semantic
probe calls its accessor rather than injecting an unrelated zero lane. A
separate gate proves present-only access to sphere e3 cannot produce executable
Wasm.

This is intentionally not yet the final public algebra package. Fe fixtures do
not have imports, so Rust tests and generators compose the prelude as source.
Domain support wrappers
remain useful to bind a particular support and blade at a readable API
boundary, but rank depth and computed storage aliases are no longer compiler
limits: recursive `SparseIndex<7>` and `SparseStorage<8>` both execute. The
parser explicitly consumes a continuation newline after a
type-alias `=`, so formatting that ground alias over multiple lines no longer
silently leaves its RHS invalid during later trait selection.
Constructors spell the private nested cells once; derive/FCO can generate them
after multi-type record reflection is available. The root Cargo pin still
targets Sonatina `150d327e`, which lacks the float and layout APIs used by this
branch, so codegen gates require `/workspace/sonatina-sparse-api`.

The flagship aggregate `Sandwich::sandwich` implementation remains unchanged.
It and the QCGA showcase now consume the same prelude, while each still owns
its metric, candidate enumeration, coefficient provider, survivor ordering,
and operator evaluation.
