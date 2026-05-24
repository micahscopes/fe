# Origin Overhaul Working Guidelines

This checklist captures the maintenance lessons from the current
`origin-overhaul` worktree review. Future work on this branch should satisfy
these principles before adding new surface area.

- [ ] Keep every data model in one canonical form.
  Example violation: typed facts are currently described independently by JSON
  serialization, raw deserialization, relation-row export, relation-schema
  catalog entries, validators, and indexes. These should be generated from one
  schema definition instead of maintained in parallel.

- [ ] Validate at durable boundaries, not on every intermediate view.
  Example violation: `fe analyze` report DTOs repeat validation that overlaps
  with common fact/source-map DTO validation. Intermediate report views should
  consume already-validated canonical data unless they introduce a genuinely new
  invariant.

- [ ] Treat public re-exports as product API, not as convenience plumbing.
  Example violation: `common::facts`, `mir::origin`, and `codegen::origin`
  re-export many implementation DTOs. Keep public surfaces small enough that
  downstream users cannot accidentally depend on temporary representation
  choices.

- [ ] Split modules only when the split creates a meaningful ownership boundary.
  Example violation: several facts/origin modules are tiny forwarding files that
  add navigation cost without hiding complexity. Prefer a cohesive file over a
  facade plus many leaf modules when the leaf modules only contain a helper or
  two.

- [ ] Keep wrappers only when they enforce an invariant at compile time.
  Example violation: owner/local/export string key wrappers are useful where
  they prevent cross-phase confusion, but the branch also contains multiple
  string forms, serde mirrors, traits, and macro variants around the same key
  concept. Collapse wrappers that only rename identical strings.

- [ ] Derive boilerplate from schema or macros once the pattern is proven.
  Example violation: the fact layer manually repeats per-variant dispatch in
  serializers, deserializers, relation export, schema catalogs, and tests. A
  derive or declarative table should own that repetition.

- [ ] Make CLI and debug reports views over compiler data, not alternate data
  stores.
  Example violation: source-map entries, bytecode-origin records, coverage
  summaries, and analyze reports currently risk becoming parallel truth sources.
  The canonical origin/fact data should be computed once and rendered many ways.

- [ ] Do not mix architectural foundation work with unrelated fixture or tooling
  expansion.
  Example violation: the current branch includes origin infrastructure alongside
  benchmark fixtures, deposit-contract tests, SSZ files, and `solc-runner`
  changes. Those may be useful, but they should not ride in the same branch
  unless the origin work directly requires them.

- [ ] Prefer per-level identity and hashing policies over one cross-level
  abstraction.
  Example violation: the shape/hash layer is aiming at local, tree, graph, and
  dimensional digests at once. HIR, MIR, Sonatina, and bytecode need their own
  digest policies; origin links should explain how those policies relate.

- [ ] Tests should protect invariants, not scaffolding.
  Example violation: many current tests assert row existence, JSON roundtrips,
  or intermediate DTO shape. Keep tests for owner/local separation, export-key
  stability, source-span joins, graph-edge hashing, and bytecode/post-opt
  coverage. Delete or generate tests that only mirror duplicate representations.

- [ ] Keep salsa queries pure and cacheable; emit facts only from returned data.
  Example violation to avoid: instrumentation sinks that mutate fact/debug/hash
  collectors from inside queries. Origin gathering may be salsa-cached, but
  exporting and rendering must stay outside query side effects.

- [ ] Make cleanup part of every phase, not a final phase.
  Example violation: module splits, temporary DTOs, and broad re-export facades
  were kept while later phases continued to add features. Each phase should
  remove or consolidate its scaffolding before adding the next layer.
