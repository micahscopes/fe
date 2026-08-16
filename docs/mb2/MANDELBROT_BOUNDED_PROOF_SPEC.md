# Bounded Mandelbrot proof specification

Status: v0 numeric claim, witness contract, and initial BN254 constraints

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

`demos/capstones/mandelbrot-proof/kernel/src/lib.fe` is the single authored transition.
Its `EscapeWitness`, `EscapeTraceRow`, and `EscapeAirRow` values are the current
Fe witness surface. Scalar tuple exports exist only so independent Wasm and
native gates can inspect the nominal values without a JSON interface.

The Fe witness exposes the expanded integer row as `EscapeAirRow`, with
`rr`, `ii`, and canonical arithmetic-shift quotient/remainder pairs alongside
the semantic row. The independent Wasm gate checks every value, every directed
transition, and one-unit mutations in every column. Fe now also evaluates the
five row-local polynomial residuals over widened i64 values and verifies a
directed pair of alleged rows. The independent gate mutates all 11 columns on
both sides, the public point, and the bound. It also proves that residual-zero
but noncanonical quotient/remainder pairs reject at the integer boundary.

`demos/capstones/mandelbrot-proof/field-air` is the first proof-field lift. Its
Fe code evaluates nine row-local residuals and nine directed-transition
residuals in BN254 Fr. Signed integers are supplied as sign-plus-magnitude and
reconstructed as `(1 - 2s) * magnitude`; `s * (s - 1)` constrains each supplied
sign to a bit. The local equations constrain `rr = x^2`, `ii = y^2`, their
sum, and both Q12 shift equalities. The transition equations constrain the
step and next coordinates. The compiled Wasm has no function imports, and the
independent gate checks directed rows, mutations, and a non-bit sign.

The equality slice alone deliberately does not claim range soundness. The gate
exhibits two zero-residual counterexamples: replacing `(q, r)` with
`(q - 1, r + 4096)` still satisfies the shift equality, and sign-one
magnitude-zero still denotes field zero. Companion BN254 range polynomials now
close both cases. A generic `BitRangeWitness<N>` supplies boolean little-endian
bits and an inclusive prefix OR. Linear reconstruction binds the bits to the
field value. Quadratic OR relations expose nonzero without a high-degree
product, allowing sign-one zero to be rejected. The signed-32 profile also
enforces the unique i32 minimum: magnitude `2^31` requires negative sign and
all lower magnitude bits zero. Twelve-bit reconstruction bounds each Q12
remainder. The oracle accepts boundary values and rejects mutated bits, OR
columns, reconstruction, negative zero, and both directions of i32 overflow.
`RangedAirRow` wires the generic witnesses to every trace-row column with widths
encoded in Fe types: step 21 bits; each coordinate 15; each square 30;
magnitude 31; real and imaginary quotients 18 and 19; and each remainder 12.
One nominal Fe entry evaluates all ten column groups. The independent gate
accepts a real terminal row and checks that a combined malformed row reports
exactly its negative-zero coordinate, 4096 remainder, and premature terminal
groups. `escape_public_proof_domain_holds_q12` is the explicit cheap verifier
boundary. Without orbit replay, it checks the canonical point box and bound,
requires the alleged terminal step not to exceed that bound, binds semantic
length to `terminal_step + 1`, and derives the exact next-power-of-two domain.
The gate rejects mutations of every input category. The accepted point already
feeds the field transition constraints. Bound and shape still need
cryptographic binding through the pending transcript.

The integer constraint evaluator rejects any alleged row whose coordinate
magnitude exceeds 24576 before evaluating the doubled cross-product. This is
the conservative envelope induced by one transition from an in-radius row and
the canonical public-point domain. The gate includes `i32::MIN` adversarial
coordinates, so widened host arithmetic is not an implicit trust assumption.

For an escaping claim with least terminal step `k`, the Fe witness derives
`trace_length = k + 1` and the least power-of-two `padded_length` greater than
or equal to it. Rows `0..k` are active, exactly row `k` carries the proof
terminal marker, and later rows repeat row `k` while clearing both activity
and the proof terminal marker. Requests at or beyond `padded_length` fail
closed. Invalid and non-escaping claims have no proof shape. The independent
gate checks every row and encoding in directed traces, counts exactly one
terminal marker, and separately derives both lengths. These are canonical
witness-shape semantics.

The Fe integer constraint surface also checks the first row, every padded
pair, and the final row. Activity is monotone. An active nonterminal row must
take the Mandelbrot transition to another active row. The unique active
terminal row must transition to inactive padding when another row exists, and
inactive padding must remain an exact fixed point of the terminal AIR values.
The last row must be either that active terminal or inactive padding. The gate
rejects non-bit flags, premature inactivity, a nonterminal final row, and
one-unit mutations of the padding fixed point. These are executable integer
constraints and remain the independent semantic reference for the field lift.

The first/pair/last state machine is now also reduced to BN254 polynomials.
Nested nominal `EncodedAirRow` and `EncodedProofRow` values preserve the typed
Fe boundary while Wasm flattens them for the independent oracle. Per-row
constraints make activity, the unique terminal marker, and the semantic escape
flag boolean; relate the unique marker to an active escape row; and require
inactive rows to retain the semantic terminal flag. Pair constraints select
the Mandelbrot transition only for an active nonterminal row. Terminal and
inactive rows instead select exact equality of all 15 encoded AIR words and
force the successor to inactive nonterminal padding. First and last
constraints establish `z_0 = 0` and prohibit an active nonterminal final row.

The state polynomials alone do not relate the semantic terminal flag to
`magnitude >= 67108864`. The gate constructs a correct nonterminal transition,
changes its successor into a premature terminal row, pads from it, and records
that the state and transition residuals are still zero. The companion range
layer closes the gap. Magnitude is reconstructed from 31 bits, and the escape
threshold is exactly `2^26`, so a five-step quadratic prefix OR over bits 26
through 30 equals the terminal flag. The oracle accepts `2^26 - 1` as
nonterminal and `2^26` as terminal, rejects the premature marker, and rejects
non-bit terminal and malformed high-OR witnesses.

## Commitment and proof direction

The trace commitment is a domain-separated Poseidon Merkle tree over canonical
row encodings. Signed integers are encoded as a sign bit plus bounded
magnitude, not by silently treating a negative i32 as an unconstrained field
element. The first executable slice commits an exact four-row escaping trace.
Fe derives the row and node tags from the visible string literals `"MR01"` and
`"MN01"`; no numeric protocol identifiers or generated parameter tables appear
in the implementation. Claim, numeric-model version, padded trace length, and
terminal row are bound for this fixed slice by a third typed domain, `"MT01"`.
The low-degree range argument introduces bit-decomposition and prefix-OR
auxiliary trace columns. Their root is absorbed after the main root and before
the composition randomizer is sampled. Sampling from the main root alone would
let a prover adapt the auxiliary columns after seeing the challenge. The typed
transcript schedule is therefore:

1. bind the public claim and main-trace root;
2. derive and commit every auxiliary trace column used by composition;
3. absorb the ordered auxiliary root and derive the composition challenge;
4. absorb the composition commitment before deriving out-of-domain and FRI
   challenges.

Steps 1 through 3 now execute. The kernel derives the ten bit decompositions,
their inclusive prefix ORs, and the five-bit terminal-threshold prefix in Fe.
The resulting 411 bits pack into two 253-bit field elements, then typed
`"AR01"` leaves and `"AN01"` nodes form the auxiliary tree. Typed `"AT01"`
binds the main statement to that auxiliary root before `"MC01"` derives the
composition challenge. No provisional pre-auxiliary challenge is part of the
protocol. Step 4 remains open.

`escape_air_row_encoding_q12` now materializes that canonical row encoding in
Fe. It gives zero only the positive sign and emits a fixed 15-word order.
`escape_proof_row_encoding_q12` adds the activity and unique-terminal words to
that encoding. The 17 audited column widths total 210 bits, so every
range-valid row packs injectively into one BN254 field element. Fe performs
that transparent packing and computes the four leaves and two-level Merkle
tree. An independent bigint oracle reconstructs the orbit, packing, canonical
Poseidon permutation, and tree, then mutates every logical column and row
order. The same gate independently packs the public point, bound, terminal
step, semantic length, and padded length into 114 bits, binds them to the root,
and mutates every public field. This is a real fixed-size statement
commitment, not yet a general proof.

The reusable field substrate is the modulus-branded
`precision::field::FieldElement<L, M>` over array-native 13-bit limbs. It has
independently checked addition, subtraction, negation, multiplication, `pow5`,
signed/unsigned embedding, and Montgomery conversion on a second modulus,
while BN254 multiplication remains bit-identical to both prior kernels. It now
also derives BN254 Fr's maximal two-adic root from the prime and generator 5
inside Fe, converts it to Montgomery form at compile time, and exposes generic
square-and-multiply, Fermat inversion, and subgroup roots through `2^28`.
There is no generated root table. An independent bigint gate checks ordinary
powers, the full-width `p - 2` exponent, exact subgroup orders, and
unsupported-order rejection through compiled Fe Wasm. One generic Fe
Cooley-Tukey transform supplies forward NTT and inverse interpolation.
Compile-time predicates reject zero, non-power-of-two, and field-unsupported
domains; direct bigint DFT and round-trip gates exercise the same algorithm at
4, 8, and 16 points. Its generic coset low-degree extension interpolates base
evaluations, applies a multiplicative shift in coefficient form, zero-pads,
and evaluates on a larger subgroup. The typed result is invalid and all-zero
when the shift is zero or lies in the base subgroup, so composition cannot
silently evaluate on roots of the trace zerofier. Independent direct bigint
interpolation and evaluation gate 4-to-16 and 8-to-16 extensions. The
field-AIR ingot now consumes this API directly on Wasm. Canonical Poseidon
parameters derive from Grain inside Fe, and the concise Fe permutation now
executes the fixed four-row commitment in zero-import Wasm. Its honest SPIR-V
gate currently fails closed on the retained array-returning `mul_words` call,
so aggregate-return inlining or shader function-call lowering is required
before the same field implementation runs as an application GPU kernel.
The production power-of-two main and auxiliary trace streams, ordered
pre-composition transcript, and first Fiat-Shamir challenge now execute.
Composition, later transcript stages, and FRI remain open.

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
