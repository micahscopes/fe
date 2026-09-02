# Riff-cat and Sonatina review

## Executive assessment

Riff-cat paid for itself quickly as an observational tool. It exposed a compiler
growth pattern that ordinary diagnostics and wall-clock timings hid: after
lowering, the number of physical blocks and instructions grew dramatically,
while the number of distinct structural shapes grew much less. That is strong
evidence for repeated lowered bodies and a useful target for compiler-side
interning. It is not, by itself, proof that two bodies are semantically
interchangeable.

The right role is therefore an independent measurement and regression oracle.
Riff-cat should describe and compare compiler artifacts, while the compiler
continues to own semantics, legality, and transformation decisions.

## What worked

The Sonatina adapter now ingests `.sona` snapshots and reports a structural
census. On the Mandelbrot proof snapshots, the prepared form had 343 functions,
1,222 blocks, and 5,862 instructions. The lowered form retained 343 functions
but grew to 3,409 blocks and 39,866 instructions. Distinct structural shapes
rose from 285 to 369, while repeated occurrences rose from 8,954 to 45,146.
The largest repeated class grew from 981 to 8,460 occurrences.

This gave us three practical wins:

1. It separated semantic growth from physical duplication.
2. It provided a compact corpus for before and after comparisons.
3. It made phase boundaries observable without changing the compiler's output.

Buffered corpus writes also mattered. The first implementation made hundreds of
thousands of small writes, while buffering kept the same record model and made
the ingestion path practical.

## What riff-cat should not do

Riff-cat should not become a second optimizer, a hidden name resolver, or a
semantic authority. Structural equality is not semantic equality. Constants,
types, effects, memory regions, call targets, and control-flow reachability all
need explicit treatment before a compiler may reuse a body. A byte match is
also only evidence of identical output, not correctness.

The compiler should consume reports or stable analysis keys, not depend on
riff-cat's internal storage format. This keeps the tool useful for historical
corpora and for other IR producers.

## Recommended facets

Keep the current five dimensions stable:

- Structure: blocks, instructions, topology, and repetition.
- Types: operand and result representation, ABI shape, and memory classes.
- Constants: literal values and generated tables.
- Names: source and symbol provenance.
- TraceEvents: phase and transformation provenance.

Useful views are `structure`, `structure+types`,
`structure+constants`, `structure+types+constants`, `all`, and
`names-blind`. Do not add a new stored dimension for every question. Prefer
derived reports such as call-preserving, control-shape, data-shape,
resource-effect, cost, and phase-delta.

## Near-term feature requests

1. Add a stable machine-readable summary for each corpus record, including
   counts, digest, parent record, compiler phase, and source revision.
2. Add a call-preserving view that distinguishes a repeated body with the same
   callees from one whose call edges differ.
3. Add a resource-effect view covering arena operations, storage classes,
   barriers, and external effects.
4. Add a phase-delta command that reports new shapes, duplicated shapes, and
   removed shapes between two records.
5. Add bounded top-k reports so large corpora remain inspectable without
   dumping every occurrence.
6. Add a corpus manifest only for the analysis tool itself. It must not become
   a runtime or web-build manifest requirement.
7. Add optional timing and peak-memory fields for each ingestion and query.

## Compiler integration ideas

The lowest-risk integration is read-only: emit normalized body fingerprints and
phase events from Sonatina, then let riff-cat verify trends in CI. A stronger
integration can use the same semantic digest as a cache key, but only after the
digest includes all type, effect, constant, call, and target information needed
for correctness.

The most promising optimization sequence is:

1. Measure repeated bodies at each lowering boundary.
2. Intern exact normalized bodies after substitution.
3. Reuse the interned representation for scalar and shader lowering.
4. Keep target-specific placement and resource allocation outside the semantic
   body identity.
5. Parallelize independent lowering only after deduplication and memory limits
   are measured.

This directly addresses shader bloat without turning riff-cat into a hidden
compiler pass.

## Speculative direction

Riff-cat could become a general compiler observatory for Fe's multi-interpreter
model. One Fe-authored DAG could be recorded through parsing, CTFE, normalized
MIR, Sonatina IR, Wasm lowering, SPIR-V lowering, and EVM lowering. Queries
could then compare preservation of structure, effects, resource contracts, and
cost across interpreters.

That would support a Salsa-backed future naturally: Salsa owns invalidation and
incremental computation, while riff-cat stores or inspects stable analysis
records. Direct riff-cat calls inside semantic compilation should remain
optional. Making the compiler depend on an observational database would risk
turning measurements into hidden semantics.

## Operational tips

- Use release builds for representative corpus collection and debug builds for
  fast local diagnosis.
- Keep corpora under `/workspace/scratch`, not `/tmp`.
- Capture pre and post snapshots from the same source revision and compiler
  configuration.
- Start with `structure+types+constants`; use `all` only when provenance is the
  question.
- Treat repeated-shape counts as leads for investigation, not automatic rewrite
  permissions.
- Commit each adapter or report change separately from compiler behavior
  changes.

## Bottom line

Riff-cat is already valuable because it made compiler duplication visible. The
next step is not to make it a larger optimizer. It is to make its observations
stable, effect-aware, and cheap enough to run at every important compiler
boundary, then use those measurements to guide exact body interning and shared
multi-backend lowering.
