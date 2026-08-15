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
including a residual-zero noncanonical shift decomposition. This is constraint
evidence, not yet proof-field AIR or a succinct proof. Escaping witnesses also
derive a power-of-two proof shape with one terminal marker and deterministic
inactive padding; invalid and non-escaping claims cannot produce proof rows.
It checks semantics, not artifact bytes:

```console
cargo nextest run --release --locked -p fe-codegen --test mandelbrot_bounded_claim_oracle
```

The canonical claim, integer semantics, witness columns, commitment plan, and
succinctness gate are specified in
`docs/mb2/MANDELBROT_BOUNDED_PROOF_SPEC.md`.
