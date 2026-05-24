# Origin Architecture Bundle - 2026-05-24

## Contents

- `source-snapshot/` — Full worktree snapshot of the `origin-overhaul` branch (7 commits ahead of argot/master, ~37K lines added)
- `docs/` — Design docs, guidelines, reconciliation, and the SSOT Containment Directive PDF
- `session-logs/` — Codex instrumentation session (107MB main + continuations) and Claude oversight session
- `git-log.txt` — Commit log for the branch
- `git-diffstat.txt` — Diff stats vs master
- `analysis/ssot-inventory.md` — SSOT violation inventory (25 instances, ~2,850 lines of duplication)

## Current Status

The branch compiles and passes focused tests. A full `cargo test --workspace` baseline is being established. The Codex agent has been directed to produce an SSOT audit table per the architect's containment directive before any further code changes.

## Key Files in source-snapshot

- `crates/common/src/origin/` — Typed origin key infrastructure (OriginKey<Owner,Local>, export keys, macros)
- `crates/common/src/shape.rs` or `shape/` — ShapeGraph, multi-dimensional hashing, ShapeDescribe trait
- `crates/common/src/facts/` — Typed fact system (~5,800 lines, includes ~2,300 lines of redundant relation system)
- `crates/codegen/src/origin/` — Sonatina pre/post-opt origins, bytecode origins, coverage
- `crates/codegen/src/debug/` — Source maps, debug locations, line tables
- `crates/fe/src/analyze/` — CLI analyze command
- `crates/mir/src/origin/` — MIR runtime origin tracking
- `crates/hir/src/origin.rs` — HIR origin types
- `crates/shape-derive/` — ShapeDescribe derive macro

## Known Issues

- 25 SSOT violations identified (see analysis/ssot-inventory.md)
- Relation system (~2,300 lines) duplicates typed fact queries on string representations
- Over-split module tree from autonomous splitting pass
- Near-zero documentation on public types
- 5 coverage mirror types, 6 overlapping error enums, 3 source-location struct forms
