# Rollcall depth-4 browser fixture

These raw bytes are generated from the existing Fe `RollcallRegistry` source
and its native `revm` depth-4 acceptance test. They are inputs to the generic
browser execution engine, not a second verifier or an application-data format.

Regenerate them explicitly with:

```console
TMPDIR=/workspace/tmp CARGO_INCREMENTAL=0 SCCACHE_DIR=/workspace/.sccache \
  cargo test --release -p fe-codegen --test rollcall_e2e \
  regenerate_rollcall_depth4_browser_fixture -- --ignored --exact
```

The ordinary depth-4 native test derives the same runtime and calldata from Fe
and rejects any stale or hand-edited fixture byte.
