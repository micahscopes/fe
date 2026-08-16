# Bounded Mandelbrot proof capstone

This directory begins the proof capstone with one exact Fe-authored statement
and witness transition. It does not claim that the current witness is a
succinct proof.

The `kernel` Fe ingot defines `EscapesByQ12`, the least terminal trace row, and
the expanded integer columns needed by the first AIR. It also owns the one
canonical encoded-row schema shared by the AIR and commitment ingots. The
independent Rust oracle
executes the compiled Fe Wasm, compares every row value and signed Q12
quotient/remainder against an i64 model, and rejects one-unit mutations in
every expanded column. Fe also emits the canonical sign-plus-magnitude row
encoding, evaluates widened integer polynomial residuals, and verifies an
alleged directed row pair. The gate mutates both rows and public claim values,
including a residual-zero noncanonical shift decomposition.

`field-air` is a separate Fe ingot that consumes that schema and lifts nine
local and nine transition residuals into BN254 Fr through the reusable
modulus-branded field API. Its Wasm has no function imports. The independent
gate checks directed residuals, one-unit mutations, and sign-bit rejection.
BN254 first/pair/last polynomials
also constrain activity, the unique terminal marker, selected Mandelbrot
transitions, and exact terminal-state padding through nested nominal Fe rows.
Generic low-degree range polynomials add bit reconstruction and a quadratic
prefix OR. Their signed profile enforces canonical positive zero and i32
bounds, their 12-bit profile bounds Q12 remainders, and a high-bit OR binds the
terminal flag to the exact `2^26` escape threshold. The gate first demonstrates
that equality/state constraints alone accept alternate remainders, negative
zero, and premature termination, then proves the companion range constraints
reject all three. `RangedAirRow` wires every trace column to a type-level Fe
width and checks the complete row through one nominal entry. A cheap Fe verifier
boundary validates the public point, bound, terminal step, semantic length, and
padded domain without replaying the orbit.

`commitment` adds the first executable trace roots. The canonical 17-column
row schema has 210 audited bits, which Fe packs injectively into one BN254
field element. Fe derives typed row and node domains from `"MR01"` and
`"MN01"`. A 22-slot Fe Merkle frontier accepts only CTFE-proved nonzero
power-of-two domains through `2^21`, so it retains O(log N) digests. The kernel
now exposes a stateful Fe trace stream that retains the current expanded row
and advances from its exact Q12 quotients. The production commitment consumes
that stream once, immediately folds each active row into the frontier, and
derives the terminal-state padding in Fe. No host-authored witness rows cross
that API. Four-row and eight-row compatibility boundaries remain as useful
mutation gates, while the production gate also crosses into a 16-row domain.
All execute in zero-import Wasm. The independent oracle reconstructs the
orbit, row packing, canonical Poseidon permutation, and tree. It mutates every
four-row column, row order, and both inactive padding positions of a six-active
row, eight-row domain. A distinct `"MT01"` domain binds each trace root to an
injective 114-bit encoding of the public point, bound, terminal step, semantic
length, and padded length. Every public field is independently mutated.
The kernel now derives every bit-decomposition and prefix-OR auxiliary column
in Fe. Their canonical 411-bit row encoding is split into two injective 253-bit
BN254 elements, committed under typed `"AR01"` leaves and `"AN01"` nodes, and
folded beside the main trace in the same pass. An `"AT01"` transcript stage
binds the auxiliary root after the public main-trace statement; only then does
typed `"MC01"` derive the field-native composition challenge. The independent
oracle reconstructs all auxiliary columns, packing, both trees, the ordered
transcript, and the challenge for all three streamed domains. No host-authored
range witness or pre-auxiliary challenge crosses the production API.
The shared precision ingot now derives BN254 Fr's maximal two-adic root from
the prime and generator 5 in Fe, converts it to Montgomery form without a
generated table, and provides field exponentiation, fail-closed inversion, and
roots through order `2^28`. An independent bigint gate checks the compiled Fe
Wasm values and exact subgroup orders. Radix-2 interpolation and low-degree
extension, composition, and FRI remain pending.

Escaping witnesses derive a power-of-two proof shape with one terminal marker
and deterministic inactive padding; invalid and non-escaping claims cannot
produce proof rows. Fe integer first/pair/last constraints make activity
monotone and the padding values a terminal-state fixed point. Their proof-field
form now executes, while low-degree composition remains pending. This is
executable constraint evidence, not yet a succinct proof. The gate checks
semantics, not artifact bytes:

```console
cargo nextest run --release --locked -p fe-codegen --test mandelbrot_bounded_claim_oracle
cargo nextest run --release --locked -p fe-codegen --test mandelbrot_trace_commitment_oracle
```

The canonical claim, integer semantics, witness columns, commitment plan, and
succinctness gate are specified in
`docs/mb2/MANDELBROT_BOUNDED_PROOF_SPEC.md`.
