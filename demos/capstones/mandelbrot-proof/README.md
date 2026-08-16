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
Wasm values and exact subgroup orders. One generic Fe radix-2 transform now
provides forward NTT and inverse interpolation, rejects invalid domains through
const predicates, and matches direct bigint DFTs at 4, 8, and 16 points.
Generic Fe coset low-degree extension now interpolates, shifts coefficients,
zero-pads, and evaluates at a larger domain. Its typed validity bit rejects
zero or output-subgroup shifts, and independent direct bigint interpolation and
evaluation gate 4-to-16 and 8-to-16 extensions. The `composition` ingot derives
the exact four-row trace in Fe, evaluates all 17 main and 411 Fe-derived
auxiliary columns on a disjoint 16-point coset, and folds all 708 AIR
constraints under the post-auxiliary challenge. It commits those evaluations
under typed `"CR01"` and `"CN01"` domains, binds the root through `"CT01"`, and
derives the first FRI fold challenge through `"FC01"`. A zero-import Wasm gate
compares every evaluation, root, and transcript value with an independent
bigint direct-DFT and Poseidon model. The `fri` ingot uses that challenge to
fold each `(f(x), f(-x))` pair through the complete
16-to-8-to-4-to-2-to-1 chain. One const-generic Fe fold implements all four
rounds. Fe derives the `FR`, `FN`, `FT`, and next-round `FC` Poseidon domains
from each const round index, without a copied round table. The oracle checks
every value using both the pair formula and an independently interpolated
even/odd coefficient formula, then reconstructs every root, transcript, and
challenge. After `"FT04"`, typed `"FQ01"` selects one index in the positive
half of the 16-point domain. Fe opens each `(x, -x)` pair with compact
depth-3, depth-2, depth-1, and two-leaf Merkle paths, then independently
rebuilds the transcript and verifies every authentication path and fold
equation. The shared pure `poseidon_merkle` ingot adapts zk-kit binary-path
semantics to field values and typed capacity domains. The bigint oracle
independently derives `FQ01`, every selected evaluation, every sibling, and
all four roots. The full gate passes in zero-import Wasm. This authenticates
the FRI chain and its composition claim against the AIR. The prover also
commits all 16 field-valued main and auxiliary LDE rows under typed
`"MR02"`/`"MN02"` and `"AR02"`/`"AN02"` domains, then binds both roots through
`"AT02"` before deriving the composition challenge. Its query opens the four
rows containing the selected current/next pair. The Fe verifier checks both
quartet paths, rebuilds the ordered transcript, and recomputes the two alleged
composition evaluations through the same generic constraint interpreter. The
independent bigint oracle separately derives every opened field, row digest,
sibling, root, challenge, and recomputed numerator. Canonical proof encoding,
systematic malformed-proof rejection at that boundary, and measured verifier
cost remain pending. The typed verifier already rejects representative
mutations across authenticated AIR rows and paths, FRI values and paths,
transcript roots, the query index, and public metadata.

Escaping witnesses derive a power-of-two proof shape with one terminal marker
and deterministic inactive padding; invalid and non-escaping claims cannot
produce proof rows. Fe integer first/pair/last constraints make activity
monotone and the padding values a terminal-state fixed point. Their proof-field
form and the first low-degree composition now execute. This is executable
constraint evidence, not yet a succinct proof. The gate checks semantics, not
artifact bytes:

```console
cargo nextest run --release --locked -p fe-codegen --test mandelbrot_bounded_claim_oracle
cargo nextest run --release --locked -p fe-codegen --test mandelbrot_trace_commitment_oracle
cargo nextest run --release --locked -p fe-codegen --test mandelbrot_composition_oracle
```

The canonical claim, integer semantics, witness columns, commitment plan, and
succinctness gate are specified in
`docs/mb2/MANDELBROT_BOUNDED_PROOF_SPEC.md`.
