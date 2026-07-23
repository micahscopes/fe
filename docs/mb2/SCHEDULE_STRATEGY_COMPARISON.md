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
