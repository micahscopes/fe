# Bounded Mandelbrot proof capstone

This directory begins the proof capstone with one exact Fe-authored statement
and witness transition. It does not claim that the current witness is a
succinct proof.

`kernel.fe` defines `EscapesByQ12`, the least terminal trace row, and the
expanded integer columns needed by the first AIR. The independent Rust oracle
executes the compiled Fe Wasm, compares every row value and signed Q12
quotient/remainder against an i64 model, and rejects one-unit mutations in
every expanded column. It checks semantics, not artifact bytes:

```console
cargo nextest run --release --locked -p fe-codegen --test mandelbrot_bounded_claim_oracle
```

The canonical claim, integer semantics, witness columns, commitment plan, and
succinctness gate are specified in
`docs/mb2/MANDELBROT_BOUNDED_PROOF_SPEC.md`.
