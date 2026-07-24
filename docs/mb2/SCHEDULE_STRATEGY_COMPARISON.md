# Schedule32 strategy comparison

The measurement harness is
`crates/codegen/tests/schedule32_strategy_measure.rs`. Run it explicitly:

```text
cargo test -p fe-codegen --test schedule32_strategy_measure -- --ignored --nocapture
```

Use the same Sonatina overrides required by the mb2 worktree. Timings are
observations, never pass/fail thresholds. The harness pins semantic equality by
executing every strategy at `x = 7` and requiring `224`.

## What the four rows mean

| Row | Publication | Runtime computation | Honest structural claim |
| --- | --- | --- | --- |
| `recursive_tree` | `Schedule<32>` recursive type function | recursive `Eval` over 32 `Add<Term, Tail>` nodes | typed tree |
| `compact_loop` | no type-level tree | one bounded loop over 32 logical terms | compact schedule interpreter |
| `fco_published_tree` | FCO associated type publishes that same `Schedule<32>` | the same recursive `Eval` tree | publication/abstraction only |
| `shared_dag` | ordinary named values | five doubling steps reuse prior results | genuine runtime DAG reuse |

FCO publication is not shared-DAG evaluation. `builder.emit_assoc_ty` publishes
the already-computed tree as an associated type; it does not intern equal
subexpressions into runtime nodes, introduce let-sharing, or change `Eval`'s
tree traversal. A genuine DAG needs explicit value sharing (the control row),
or a future plan representation/interpreter whose nodes have stable identities
and can be referenced more than once.

## Measurements

The harness reports:

- HIR/type normalization wall time;
- runtime-package construction wall time;
- cached end-to-end O0 Wasm backend wall time;
- formatted RMIR bytes and residual pre-inlining call sites;
- final Wasm bytes.

These reduced rows compare execution shapes under the same `32 * x` semantics.
They do not claim to benchmark CGA arithmetic. The canonical Cl(4,1) fixture
adds the separate cost of deriving the real 80-to-32 survivor payloads. Its
`Schedule<32>` is still the `recursive_tree` shape after normalization.

The existing FCO Cl(4,1) bridge proves that the provider can publish that
Fe-computed ground schedule and ordinary runtime trait dispatch can consume it.
That is an important modularity result, but its normalized plan remains the
same 32-node tree. The compact and DAG rows are therefore alternative execution
representations, not consequences of FCO.

## Sample run

One debug/O0 run on 2026-07-23 produced:

| Strategy | HIR ms | Package ms | Backend ms | RMIR bytes | RMIR calls | Wasm bytes |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| recursive tree | 18475.30 | 23766.46 | 205.12 | 99578 | 65 | 9964 |
| compact loop | 22143.77 | 72.51 | 12.85 | 2661 | 0 | 139 |
| FCO-published tree | 20649.34 | 30393.23 | 119.85 | 99578 | 65 | 10476 |
| explicit shared DAG | 15188.63 | 59.08 | 2.06 | 2142 | 0 | 123 |

Absolute HIR timings include cold per-database setup and are order/machine
sensitive. The robust structural result is that direct and FCO-published trees
have exactly the same formatted RMIR size and pre-inlining call count. FCO
changes publication, not the normalized evaluator graph. The compact loop and
explicit DAG instead remove the recursive runtime call graph and are two orders
of magnitude smaller at the RMIR boundary in this reduced workload.

## Real Cl(4,1) inversion comparison

The ignored test
`compare_real_cga_tree_compact_schedule32_and_fco_shared_dag` runs the actual
cyclide distance estimator through three executable representations:

- the generated recursive `MvTF<5>`/Cayley sandwich tree;
- the four-chunk typed `Schedule<32>` evaluator;
- the bounded FCO provider using `builder.share` to emit five direct lanes.

All three use the same seven-argument render ABI. The test requires identical
results at fixed probe pixels, validates WGSL with browser-default Naga
capabilities, requires exactly the single ray-march loop, and reports
Wasm/RMIR/WGSL shape. Timings remain observations; semantic and structural
properties are assertions.

One debug/O0 run on 2026-07-23 produced:

| Strategy | Analysis ms | Package ms | Wasm ms | SPIR-V ms | RMIR bytes | RMIR calls | Wasm bytes | f32 add | f32 mul | Wasm calls | WGSL bytes/lines |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| recursive Cayley tree | 48864.40 | 4442.78 | 340.70 | 293.34 | 154782 | 1 | 1607 | 93 | 155 | 0 | 4824 / 122 |
| typed Schedule32 | 303275.03 | 131373.81 | 181.64 | 524.58 | 686650 | 235 | 36550 | 387 | 91 | 196 | 6284 / 137 |
| FCO/shared direct lanes | 37793.03 | 1071.95 | 6.12 | 19.95 | 78077 | 5 | 1456 | 74 | 91 | 5 | 5545 / 132 |

The recursive Cayley tree has compact final artifacts, but performs 155 float
multiplications because its decomposition does not exploit the 80-to-32
commutative/cancellation specialization. The typed Schedule32 proves the
compile-time plan and reaches 91 multiplications, but interpreting that plan
through recursive traits is by far the largest and slowest compiler path: its
runtime package retains 235 call sites before backend inlining.

The FCO/shared-DAG route preserves Schedule32 as an inspectable typed witness
while emitting specialized executable lanes from the same canonical Fe
helpers. It reaches the same 91-multiply arithmetic shape with the smallest
RMIR and Wasm in this comparison, and no runtime algebra loop. It should
therefore remain the canonical executable path. The recursive tree remains a
semantic/reference implementation; compact Schedule32 remains a type-level
proof and regression fixture, not the default runtime evaluator.

## One reflected plan: honest term sharing

The non-ignored
`one_reflected_plan_drives_tree_compact_terms_and_honest_shared_dag` gate in
`crates/codegen/tests/fco_cga80_direct_lanes.rs` removes an ambiguity in the
older table. All three rows are derived from the same normalized
`SparsePlan<...,80,32>` traversal and never rescan `0..80`:

- `UnsharedTree` embeds every reused expression at each edge;
- `CompactTerms` materializes a product only when its magnitude-two term
  references that product twice;
- `SharedDag` materializes every product as a potential shared node.

This is not the reduced harness's runtime compact loop. `CompactTerms` is an
unrolled finite term schedule, named separately so the two representations are
not conflated.

Schedule32 contains 32 distinct operand-product keys. Its twelve off-diagonal
terms each have one genuine repeated edge because their magnitude is two;
there is no cross-term product reuse. The measured O0 five-lane Wasm shapes
therefore are:

| Execution | f32 multiplies | runtime loops | Wasm bytes |
| --- | ---: | ---: | ---: |
| unshared tree | 440 | 0 | 4031 |
| compact terms | 320 | 0 | 3991 |
| shared DAG | 320 | 0 | 3991 |

Eight deterministic coefficient cases across all five lanes agree with the
independent raw-80 expansion and with one another. A second non-ignored gate
executes the complete pinned 128x128 frame for compact terms and shared DAG.
Both produce the independently generated reference FNV `3470936828`; both
validate with browser-default Naga capabilities as two-function WGSL with only
the raymarch loop. Compact-term WGSL is 5541 bytes/132 lines and full-share
WGSL is 5566 bytes/132 lines. The extra sharing annotations change local
materialization but discover no additional arithmetic reuse.

That is the bounded, Cl(4,1)-specific result: explicit sharing is useful for the
twelve known repeated edges, while general hash-consing has no further win for
this canonical plan. It is not evidence for an automatic general GA DAG
planner.
