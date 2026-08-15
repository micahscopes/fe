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
The gate records the remaining soundness boundary honestly: current field
constraints still accept an out-of-range remainder decomposition, negative
zero, and a premature terminal marker until range, canonical-zero, and
threshold constraints land.

Escaping witnesses derive a power-of-two proof shape with one terminal marker
and deterministic inactive padding; invalid and non-escaping claims cannot
produce proof rows. Fe integer first/pair/last constraints make activity
monotone and the padding values a terminal-state fixed point. Their proof-field
form now executes, while low-degree composition remains pending. This is
executable constraint evidence, not yet a succinct proof. The gate checks
semantics, not artifact bytes:

```console
cargo nextest run --release --locked -p fe-codegen --test mandelbrot_bounded_claim_oracle
```

The canonical claim, integer semantics, witness columns, commitment plan, and
succinctness gate are specified in
`docs/mb2/MANDELBROT_BOUNDED_PROOF_SPEC.md`.
