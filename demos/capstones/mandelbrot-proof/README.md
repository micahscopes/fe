# Bounded Mandelbrot proof capstone

This directory begins the proof capstone with one exact Fe-authored statement
and witness transition. It does not claim that the current witness is a
succinct proof.

`kernel.fe` defines `EscapesByQ12`, the least terminal trace row, and the
expanded integer columns needed by the first AIR. The independent Rust oracle
executes the compiled Fe Wasm, compares every row value and signed Q12
quotient/remainder against an i64 model, and rejects one-unit mutations in
every expanded column. Fe also emits the canonical sign-plus-magnitude row
encoding, evaluates widened integer polynomial residuals, and verifies an
alleged directed row pair. The gate mutates both rows and public claim values,
including a residual-zero noncanonical shift decomposition.

`field-air` is a separate Fe ingot that lifts nine local and nine transition
residuals into BN254 Fr through the reusable modulus-branded field API. Its
Wasm has no function imports. The independent gate checks directed residuals,
one-unit mutations, and sign-bit rejection. BN254 first/pair/last polynomials
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

`commitment` adds the first executable trace root. The canonical 17-column row
schema has 210 audited bits, which Fe packs injectively into one BN254 field
element. Fe derives typed row and node domains from `"MR01"` and `"MN01"`, then
computes a four-leaf Poseidon Merkle root in zero-import Wasm. The independent
oracle reconstructs the orbit, row packing, canonical Poseidon permutation,
and tree. It mutates every committed column and row order. General trace
lengths remain pending. For the fixed slice, a distinct `"MT01"` domain binds
the trace root to an injective 114-bit encoding of the public point, bound,
terminal step, semantic length, and padded length. Every public field is
independently mutated. General Fiat-Shamir challenges, composition, and FRI
remain pending.

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
