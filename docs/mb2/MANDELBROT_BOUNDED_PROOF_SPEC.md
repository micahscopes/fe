# Bounded Mandelbrot proof specification

Status: v0 numeric claim and witness contract

This specification deliberately proves a bounded execution claim, not general
Mandelbrot membership and not vague convergence.

## Public claim

The first claim type is `EscapesByQ12`:

```text
EscapesByQ12 {
    c_re_q12: i32,
    c_im_q12: i32,
    bound: u32,
}
```

It is true exactly when the canonical orbit has a least index `k` with
`0 <= k <= bound` and squared magnitude at least four. The public output is
that least `k`. Failure to escape by the bound is not evidence of membership.

The v0 domain is the canonical Q12 view:

```text
-8192 <= c_re_q12 < 4096
-6144 <= c_im_q12 < 6144
bound <= 1048576
```

The orbit begins at `z_0 = (0, 0)`. Given `z_k = (x_k, y_k)`, define:

```text
rr_k = x_k * x_k
ii_k = y_k * y_k
m_k  = rr_k + ii_k

x_(k+1) = floor((rr_k - ii_k) / 4096) + c_re_q12
y_(k+1) = floor((2 * x_k * y_k) / 4096) + c_im_q12
```

Signed right shift is arithmetic, so the divisions above round toward negative
infinity. `m_k` is Q24 and the terminal threshold is `4 * 4096 * 4096`, or
`67108864`.

The transition is evaluated only when `m_k < 67108864`. In that region,
`x_k*x_k`, `y_k*y_k`, `2*x_k*y_k`, and both next coordinates fit signed i32.
The canonical `c` domain also keeps the next row's two squares and their sum in
signed i32 before the next terminal check. The independent oracle must retain
i64 intermediates and assert these bounds rather than inheriting i32 wrapping.

## Witness

For an escaping claim, the semantic witness is the ordered sequence
`row_0 .. row_k`. The proof-system trace expands each semantic row to:

```text
(step, x, y, rr, ii, magnitude, q_re, r_re, q_im, r_im, terminal)
```

For every nonterminal row:

```text
rr - ii       = 4096 * q_re + r_re
2 * x * y     = 4096 * q_im + r_im
0 <= r_re < 4096
0 <= r_im < 4096
next.x        = q_re + c_re_q12
next.y        = q_im + c_im_q12
```

The terminal bit is false before row `k` and true at row `k`. This proves that
`k` is the least escape index. A non-escaping trace may be useful prover input,
but it cannot construct an `EscapesByQ12` proof.

`demos/capstones/mandelbrot-proof/kernel.fe` is the single authored transition.
Its `EscapeWitness`, `EscapeTraceRow`, and `EscapeAirRow` values are the current
Fe witness surface. Scalar tuple exports exist only so independent Wasm and
native gates can inspect the nominal values without a JSON interface.

The Fe witness now exposes the expanded integer row as `EscapeAirRow`, with
`rr`, `ii`, and canonical arithmetic-shift quotient/remainder pairs alongside
the semantic row. The independent Wasm gate checks every value, every directed
transition, and one-unit mutations in every column. Converting these integer
relations into field-polynomial constraints, including signed range proofs,
activity, terminal uniqueness, and padding, remains part of the pending AIR
layer.

## Commitment and proof direction

The trace commitment will be a domain-separated Poseidon Merkle tree over
canonical row encodings. Signed integers are encoded as a sign bit plus bounded
magnitude, not by silently treating a negative i32 as an unconstrained field
element. Claim, numeric-model version, padded trace length, and terminal row
are transcript inputs.

The intended succinct construction is a transparent AIR plus FRI over a field
with an audited two-adic domain. The first implementation should reuse the
Fe field, Poseidon, and Merkle work already exercised by Rollcall. It must add:

- bit decomposition and range constraints for every signed integer column;
- quotient and remainder constraints for arithmetic-shift rounding;
- boolean activity and terminal columns, with first-terminal uniqueness;
- padding constraints after the terminal row;
- Fiat-Shamir transcript domain separation;
- trace and composition commitments;
- FRI folding, openings, and malformed-opening rejection.

The proof is not called succinct until both conditions hold:

1. proof size and verifier work grow polylogarithmically in the padded bound;
2. measured Fe verifier work is lower than replaying the same orbit at the
   demonstrated bound.

A Merkle root plus sampled transition rows is not sufficient. Without the AIR
composition and low-degree argument, it remains only a probabilistic spot check
of a committed trace.

## Required gates

- Fe Wasm witness rows equal an independent i64 replay across directed,
  boundary, random, escaping, and non-escaping cases.
- The least terminal index is stable, and a requested row after termination
  clamps to the terminal row.
- Invalid points and bounds fail closed.
- Altered point, bound, terminal index, row, commitment opening, transcript,
  and FRI value are rejected.
- Fe Wasm and native verifiers agree on accepted and rejected proofs.
- The verifier reports proof bytes, field operations, hashes, and wall time
  beside full replay cost.
- Browser proof production may use WebGPU, but the verifier must accept only
  the canonical proof value and must not trust dispatch completion.

`EntersAttractor` is a separate future claim. It requires a certified attracting
cycle enclosure and explicit contraction and error bounds. It cannot reuse
`EscapesByQ12` as a membership claim.
