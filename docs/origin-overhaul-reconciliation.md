# Origin Overhaul Reconciliation And Prototype Lessons

Status: draft
Date: 2026-05-22

## Purpose

This note reconciles the independent instrumentation review with the Claude
sessions that created the current prototype branch. It is a decision record for
Phase 1 of the origin overhaul.

The current conclusion is: keep the origin-graph spine, treat the existing
instrumentation branch as prototype evidence, and start implementation from
typed owner-aware origin keys rather than patching raw IDs in place.

## Evidence Reviewed

- Current prototype branch: `experimental-compilation-analysis`.
- Current overhaul worktree: `fe-worktrees/origin-overhaul`.
- Coordinator session: `197dd6c2-65ba-4c20-9a70-cb033ca0575d`.
- Implementor session: `f0019886-5280-4b0d-937f-66cf1c9a787f`.
- Prior-session discovery across debug-info, derive/desugaring, Salsa exporter,
  and Sonatina observability sessions.

## Decisions Confirmed

- Use `origin` terminology for compiler identity and lineage.
- Model origins as a typed many-to-many graph across compiler artifacts.
- Preserve LazySpan/HIR origin as the final source-span resolver where possible.
- Keep Salsa queries pure: queries return immutable origin/hash data, exporters
  consume it at driver boundaries.
- Prove type boundaries with compile-fail tests where nominal wrappers carry
  correctness invariants, starting with HIR expr-vs-stmt origins.
- Capture only irrecoverable origin relationships online.
- Derive hashes, facts, debug records, and reports post-hoc from typed data.
- Split tree/content hashing from graph hashing.
- Treat Datalog/Cozo/Souffle/JSON as export/query backends, not compiler state.
- Make `IrConsumer`-style callbacks transitional, not the source of truth.

## Refinements Added

- Add correctness-oriented success criteria: exact source-to-bytecode tracing,
  optimized-instruction classification, dimensional hash explanation, exact
  fact-query oracle tests, and at least one realistic bug explanation.
- Avoid generic `effect` terminology for trace metadata because Fe already has
  language-level effect concepts.
- Make the online-capture vs post-hoc-derivation boundary explicit.
- Treat Sonatina as imperative pass infrastructure, not Salsa machinery.
- Require Sonatina origin support for creation, replacement, aliasing, erasure,
  split, merge, deletion, and layout-only movement.
- Track LoC reduction as an architectural goal by deleting or quarantining old
  visitors, consumers, and side channels as phases land.
- Require a real IR-family derive conversion before claiming the macro strategy
  is sustainable.
- Keep shape hash policy reviewable in its own module:
  `crates/common/src/shape/hash.rs` owns deterministic digesting and dimension
  projection, while `common::shape` remains the graph/builder/derive facade.
- Keep shape construction policy reviewable in focused modules:
  `shape/graph.rs` owns graph identity/types, `shape/describe.rs` owns builder
  and `ShapeDescribe` APIs, and `shape/field_value.rs` owns field-value text
  conversion while `shape.rs` remains the stable public facade.

## Prototype Lessons

| Prototype Area | Lesson | Plan Impact |
| --- | --- | --- |
| `SourceOrd` | Useful source-location tagging, but not a full origin model. It flattens identity too early. | Keep as historical evidence only; use typed owner-aware origin nodes. |
| `ProvenanceNodeId` | `(level, u32)` IDs collide across bodies and kinds. | Replace with typed keys that include owner context. |
| MIR source resolution | Resolvers that try every body can mask missing identity. | Make body/function ownership mandatory and test multi-body failures. |
| Sonatina PC maps | Joining optimized PC maps to pre-opt instruction IDs is unsound. | Add explicit pre-opt to post-opt origin links and unmapped reasons. |
| Bytecode PC identity | Sonatina observability reports section-local PC offsets. Object-only PC ownership collides across init/runtime/test sections. | Bytecode PC origins must include object and section ownership. |
| Sonatina observability IR IDs | The `ir_inst` in PC maps is the post-opt or backend-prepared instruction reference used by machine lowering, not a raw pre-opt lowering ID. | Bytecode origins should link from post-opt or backend-prepared nodes, with pre-opt origins connected through an explicit same-ID snapshot alias or future pass-lineage edge. |
| Hash consumer | Graph edges must not globally disable child/tree hashing. | Build `ShapeGraph`, then compute tree and graph digests separately. |
| Edge labels | Transcript claims can diverge from code; full label hashing must be verified. | Add invariant tests for full edge-label contribution. |
| Hash dimensions | Exporters must not silently drop constants/types or other computed dimensions, and graph edges must not make content-only endpoint changes look structural. | Keep exported fact/report dimensions aligned with core hash dimensions and test edge endpoint dimension separation. |
| `FactConsumer` | Pending `origin()`/`source_span()` state creates a fragile temporal protocol. | Export facts from typed data, not from callback ordering. |
| Security queries | Pipeline-existence tests do not prove query correctness. | Add small exact-answer fact fixtures. |
| Derive macro | The macro idea was right, but production migration was unfinished. | Make derive followthrough a phase gate with deletion of replaced boilerplate. |
| `fe analyze` | Temp-file analysis loses real workspace/import/config context. | Rebuild analysis on normal target/workspace resolution. |
| Gas/deposit profiling | Useful exploration, but not core architecture. | Keep out of the core overhaul until origin/fact foundations are solid. |

## Superseded Ideas

- A single cross-level hash that directly represents source-to-bytecode lineage.
- Raw traversal-order IDs as durable compiler identity.
- `SourceOrd` or source spans as the primary identity model.
- Mutable fact/debug sinks inside Salsa queries.
- Debug/source resolvers that recover identity by scanning all possible owners.
- Treating Cozo or any specific query engine as part of the compiler core.
- Treating prototype tests as sufficient evidence when they assert only that
  reports contain rows.

## Open Questions

- Which crate should own the durable origin types: `common`, `mir`, a new crate,
  or a split between internal keys and export keys?
- Which origin keys need stable serialized forms, and which should stay
  compiler-internal?
- How much Sonatina optimizer support needs upstream pass-framework changes?
- Should the first derive conversion target HIR definitions, MIR runtime IR, or
  a smaller self-contained IR family?
- What is the smallest realistic bug or mapping failure to use as the first
  end-to-end correctness demonstration?

## Current Overhaul Checkpoint

The `origin-overhaul` worktree now has an initial bytecode-to-source resolver
that follows the architectural spine rather than the prototype scanning model:

- Runtime origin lookup is owner-aware through `RuntimePackageOrigins`.
- Optimized-module Sonatina origin lookup is explicit through
  `SonatinaPostOptPackageOrigins`.
- Pre-opt Sonatina function-origin bundles construct owner/stage-aware
  instruction origins internally, and pre/post record constructors fail fast on
  wrong-stage, wrong-function, or false same-`InstId` data.
- Bytecode PC records join through post-opt or backend-prepared Sonatina
  instruction origins before reaching pre-opt Sonatina/runtime origins.
- Bytecode object selection for origin/fact graph projection uses
  `BytecodeObjectKey` instead of raw object-name strings. Bytecode section
  identity similarly goes through `BytecodeSectionNameKey` before forming a
  `BytecodeSectionKey`, while serialized artifact maps remain string-keyed
  boundary data.
- Bytecode PC records for backend-prepared/codegen-only instruction IDs that
  are not present in the optimized snapshot use a distinct `backend_prepared`
  instruction stage linked from `post_preopt_snapshot_gap`, not fake `post_opt`
  nodes or direct pre-opt joins.
- Bytecode origin records are sorted by object, section, and PC range after
  consuming Sonatina artifacts, so downstream source-map and fact consumers do
  not depend on object/section insertion order.
- The bytecode origin package constructor now rejects overlapping PC ranges
  inside one object section while still accepting adjacent half-open ranges,
  preventing ambiguous source classifications for the same bytecode byte.
- Bytecode-origin artifact ingestion now has a fallible
  `BytecodePackageOrigins::try_from_artifacts` path with typed errors for
  malformed or overlapping PC-map ranges. Origin-backed Sonatina compilation
  reports those validation failures as `LowerError`s instead of relying on a
  panic-only constructor after object emission.
- Runtime body/package origin bundles and post-opt Sonatina function bundles
  now reject duplicate identities, including runtime body symbols used for
  fact-owner derivation, so origin lookup APIs and exported owner namespaces do
  not mask ambiguous records with first-match behavior.
- Runtime bytecode outputs, test metadata, and `fe analyze --source-maps` now
  expose bytecode-origin coverage counts for post-opt, backend-prepared, and
  unmapped PC records, so this distinction is visible at the debug boundary.
- `BytecodeOriginCoverage` now derives its total from classification counts at
  construction time and exposes private fields through getters, making the
  partition invariant harder to violate at report boundaries.
- Sonatina pre-opt and post-opt coverage now use the same private-field,
  constructor-derived total pattern, including helpers for post-opt partition
  checks and observed pre-opt survivor/loss totals.
- Runtime bytecode outputs, test metadata, and `fe analyze --source-maps` now
  also expose object/section-filtered post-opt snapshot coverage. The counts make
  same-ID aliases, post-preopt created or unmatched instructions, and pre-opt
  snapshot losses visible without promoting them to precise optimizer pass
  lineage.
- Build and test report `source_map.json` artifacts now carry the same optional
  bytecode-origin and post-opt snapshot coverage, keeping report artifacts
  aligned with analyze summaries without parsing rendered PC rows or raw fact
  exports. The source-map artifact schema is now `schema_version: 3`.
- Build and Fe test report `origin_facts.json` and
  `snapshot_origin_facts.json` regressions now parse emitted artifacts with
  `OwnedTypedFactSetExport`, so fact artifacts are tested through the same
  fail-closed schema decoder that query/debug consumers will use.
- `fe analyze` JSON regressions now parse embedded origin/shape fact payloads
  with `OwnedTypedFactSetExport` and embedded relation-table payloads with
  `TypedFactRelationSet`, so the CLI report boundary is tested through typed
  decoders instead of raw nested JSON probes.
- Analyze origin-fact report DTOs now validate aggregate counts against the
  embedded typed fact payload. Total, origin-node, origin-link, source-span,
  source-span file summaries, relation counts, and relation-table rows must
  agree, and duplicate source-span file summaries, duplicate relation summaries,
  empty identity fields, non-origin relation summaries, or populated non-origin
  relation tables are rejected in origin-fact reports.
- Analyze origin-fact regressions now parse reachability summaries and
  path-witness payloads with `OriginReachabilitySummary` and
  `OriginPathWitnessExport`, and source-path witnesses with
  `OriginSourcePathWitnessExport`. The embedded source span is decoded through
  `SourceSpanExport`, so the source-path report test no longer needs raw
  source-span JSON probes.
- `fe analyze --source-maps` JSON regressions now parse embedded source-map
  entries with `BytecodeSourceMapEntry` and coverage payloads with the typed
  coverage DTOs, so CLI source-map reports are checked through codegen's
  source-map schema validators rather than raw `kind` and count fields.
- Analyze source-map report DTOs now also validate summary consistency on
  decode: total, source, non-source, per-kind classifications, debug-location
  counts, line-table counts, bytecode-origin coverage totals, and present entry
  rows must agree. Source-map scope, label, object, section, and optional test
  names must be non-empty. When full entry rows are present they must match the
  report object and must match the report section unless the report uses the
  explicit `<all>` aggregate section sentinel. Full entry rows remain optional
  so compact reports still deserialize without requiring every PC row.
- Analyze shape report DTOs now validate decoded graph hash dimensions and
  digest text, check embedded shape facts against report counts and graph hash
  rows, and verify relation-count/table row counts against the typed shape
  summary. Compact shape reports may still omit full fact rows.
- Origin-fact and shape report DTOs now share the same relation-summary
  validation helper, with per-report expected relation-count policies. This
  keeps duplicate-count, unexpected-family, and table-count drift handling from
  forking between origin and shape reports.
- `fe analyze` JSON regressions now also deserialize the full report through
  the internal `AnalyzeReport` DTO, which rejects unknown fields and exposes
  typed source-map, fact, shape, and relation-count children to the assertions.
  This keeps report-level schema drift visible instead of preserving
  permissive top-level JSON probes.
- The same `AnalyzeReport` DTO now rejects unsupported schema versions and
  target `runtime_bodies`, `runtime_statements`, or `runtime_terminators` counts
  that do not match emitted body rows, and report producers validate that
  boundary before rendering JSON or text output. Profiles must be non-empty,
  package kinds are closed to runtime/tests, target labels are unique and
  non-empty, body symbols are non-empty at the body DTO boundary, and duplicate
  body symbols are rejected per target.
- The analyze report DTOs and shared report-boundary validators now live behind
  the `crates/fe/src/analyze/report.rs` facade, with focused submodules for
  source-map reports, origin-fact reports, shape reports, origin counts, and
  shared validation. Text and JSON report rendering helpers now live in
  `crates/fe/src/analyze/render.rs`. Sonatina/codegen source-map and
  origin-fact report assembly now lives in
  `crates/fe/src/analyze/codegen_reports.rs`, leaving `analyze.rs` focused on
  CLI target resolution, compiler queries, shape/runtime-origin summaries, and
  top-level report assembly.
- The analyze report regression tests now live under focused modules in
  `crates/fe/src/analyze/tests/`, leaving `crates/fe/src/analyze/tests.rs` as
  a shared-helper facade and `analyze.rs` as the orchestration facade instead of
  embedding the large report-boundary fixture suite inline.
- Analyze source-map report construction now returns validation errors instead
  of panicking when codegen summaries, coverage totals, or emitted entry
  identities drift from the report boundary.
- Analyze source-map full-entry validation now derives entry classification and
  debug line-table counts from `codegen::debug::bytecode_source_map_entries_summary`,
  removing the duplicate CLI-side entry-kind classifier.
- Analyze origin-fact and shape relation tables now have to match the report's
  typed facts exactly after normalization, not merely carry the same row counts.
  Shape relation tables require the corresponding typed fact payload.
- Analyze relation-count summaries stay sparse but complete: facts-bearing
  reports reject missing counts for any non-empty relation, while zero-row
  relations may still be omitted.
- Shape-hash node/scope states fail at `ShapeHashFactKey`/`ShapeHashFact`
  construction now, instead of being accepted into typed facts and discovered
  only while building `ShapeFactIndex`.
- Origin owner/local/string key macros now provide fallible `try_new`
  constructors with shared `OriginKeyTextError`, so validation tests and future
  import boundaries can check key-text failures directly instead of relying on
  panic-only constructors.
- Analyze runtime origin count rows now reject totals that do not equal the
  semantic plus synthetic partitions, preventing the compact runtime
  statement/terminator summaries from drifting inside otherwise valid report
  JSON.
- The Sonatina public test-metadata regression now parses generated source-map
  JSON with `OwnedBytecodeSourceMapExport`, keeping test metadata on the same
  source-map schema boundary as report artifacts and analyze source-map views.
- Build-report and Fe test-report regressions deserialize emitted
  `source_map.json` artifacts through `OwnedBytecodeSourceMapExport`, so report
  writers are checked against the same fail-closed source-map schema boundary as
  downstream consumers.
- Source resolution delegates from semantic origins to LazySpan.
- Every bytecode record gets a classified resolution result; synthetic,
  unmapped, post-preopt snapshot gap, missing runtime origin, and missing span
  cases are explicit data.
- Bytecode source-resolution DTOs and helper functions now live in
  `crates/codegen/src/origin/source_resolution.rs`, starting the cleanup of the
  large codegen origin module without changing the public
  `codegen::origin::{BytecodeSourceResolution, BytecodeSourceResolutionResult}`
  API.
- Test report generation now has a typed source-map JSON boundary over those
  bytecode source resolutions. The artifact keeps object, section, PC range,
  source-span data when available, and explicit non-source reasons otherwise.
- `fe analyze` now has a minimal typed-origin view over runtime package
  origins. It uses normal CLI target/workspace resolution, supports ingot member
  selection, respects profile/recovery settings, and emits text or versioned
  JSON from cached compiler data instead of compiling temp-file source strings.
- `fe analyze --tests` extends that view to test runtime packages, preserving
  the test runner's ingot/module traversal so test-only ingots produce useful
  origin summaries.
- `fe analyze --tests --source-maps` now reports typed bytecode source-map
  summary counts, including explicit non-source classifications, without
  treating rendered JSON as compiler input.
- `fe analyze --tests --source-maps --source-map-entries` exposes the same
  typed bytecode source-map rows on demand, preserving compact summaries as the
  default and keeping report JSON as a boundary view over typed data.
- Text-mode source-map analysis now prints the complete typed classification
  breakdown and, when `--source-map-entries` is requested, renders each typed PC
  row with object, section, PC range, source details or explicit non-source
  reason. This keeps the CLI debug path useful without requiring JSON parsing.
- Source-map entry serialization is owned by `BytecodeSourceMapEntry`, so test
  artifacts and analyze reports no longer maintain separate JSON/DTO schemas.
- Source-map artifact JSON now has a matching owned
  `OwnedBytecodeSourceMapExport` schema with deserialization and round-trip
  tests. Unknown schema versions and unknown export, entry, coverage, or source
  fields are rejected during deserialization. This brings debug/source-map
  exports in line with the typed fact JSON boundary instead of leaving them
  write-only or silently permissive.
- Source-map artifact coverage decoding now rejects mismatched partitions and
  entry-count mismatches, so malformed coverage cannot round-trip through the
  owned export schema.
- `BytecodeOriginCoverageExport` and `SonatinaPostOptOriginCoverageExport` now
  have downstream compile-fail coverage proving public callers cannot bypass the
  typed conversion or fail-closed JSON decoder with struct literals.
- `BytecodeSourceMapEntry` now has private fields and constructor-backed PC
  range validation. JSON decoding rejects empty ranges, and a downstream
  compile-fail test blocks direct struct literal construction.
- `BytecodeSourceMapEntry` now exposes fallible
  `BytecodeSourceMapEntry::try_from_origin` construction with structured
  validation errors, while `BytecodeSourceMapEntry::from_origin` remains the
  convenience wrapper for invariant-preserving producers. Empty source
  files/snippets and inverted source byte/line-column ranges cannot be produced
  by Fe-owned source-map row constructors, and JSON decoding/export construction
  share the same kind validator.
- Source-map source entries now include a required derived snippet. The
  source-map artifact schema version 2 made snippets required; deserialization
  rejects missing or empty snippets and empty source file labels, and
  analyze/source-map-entry tests require non-empty snippets for real runtime
  and test bytecode mappings.
- Codegen now derives `debug_locations.json` from validated source-map rows.
  The export includes only real source PC ranges and intentionally omits
  synthetic/unmapped/non-source classifications, so future DWARF/ethdebug
  emitters have a typed `PcRange -> SourceSpan` artifact without pretending
  every bytecode range has a source location.
- The debug-location artifact now has a matching owned decoder. It rejects
  unsupported schema versions, unknown fields, empty payloads, metadata
  mismatches, invalid or overlapping PC ranges, and invalid source file/snippet
  or byte/line-column ranges, so the compact debug boundary is fail-closed
  before format-specific DWARF/ethdebug work begins.
- Build-report and Fe test-report regressions now deserialize emitted
  `debug_locations.json` artifacts through that owned decoder, so report
  writers are checked against the same compact schema boundary that later debug
  consumers will use.
- `BytecodeDebugLocationEntry` also has downstream compile-fail coverage
  proving public callers cannot construct unchecked debug-location rows with
  struct literals.
- Build-report and Fe test-report regressions now cover
  `debug_locations.json` write failures after source-map emission succeeds,
  keeping compact debug artifact I/O fail-closed instead of best-effort.
- `fe analyze --source-maps` now reports a `debug_locations` count derived from
  `BytecodeSourceMapSummary::debug_locations()`, so CLI source-map summaries
  expose how many source PC ranges are eligible for the compact debug-location
  artifact without reading rendered JSON back into the compiler.
- Codegen now owns `BytecodeDebugArtifactsExport`, a typed source-map and
  debug-location export bundle, with `BytecodeDebugArtifactsJson` layered above
  it for report JSON. Build and Fe test reports consume the JSON bundle instead
  of duplicating render-order and option policy, while future DWARF/ethdebug
  emitters can consume the typed exports directly and retain separate
  path-specific artifact writes.
- `BytecodeDebugArtifactsExport` now rejects mismatched source-map and
  debug-location metadata, preventing one bundled artifact set from mixing
  object-scoped source maps with section-scoped compact debug records.
- Codegen now also owns the debug artifact filename policy through
  `BytecodeDebugArtifactKind` and `BytecodeDebugArtifactsJson::artifacts()`.
  Build and Fe test report writers iterate typed artifacts instead of each
  hard-coding `source_map.json`, `debug_locations.json`, and
  `debug_line_table.json`.
- Codegen now also derives a versioned `debug_line_table.json` artifact from
  the owned debug-location export. It interns source files and preserves
  validated PC/source rows as a format-neutral input, avoiding separate DWARF
  and ethdebug code paths reinterpreting the compact artifact independently.
  Build and Fe test reports emit it and cover path-specific write failures.
- Debug line-table source files, rows, and owned exports now have downstream
  compile-fail coverage rejecting struct literals, so public callers cannot
  bypass the debug-location-derived builder or fail-closed JSON decoder.
- Codegen now exposes `line_records()` over the in-memory and owned
  debug-line-table exports. The view resolves interned file indices into direct
  PC/source records, keeping future DWARF and ethdebug emitters from duplicating
  line-table schema joins.
- `fe analyze --source-maps` now exposes `debug_line_table_files` and
  `debug_line_table_rows` from `BytecodeSourceMapSummary`, so CLI summaries can
  show line-table coverage without parsing rendered debug artifacts back into
  compiler data.
- Codegen debug/source-map regressions now live behind the
  `crates/codegen/src/debug/tests.rs` facade, with focused
  `crates/codegen/src/debug/tests/` modules for source-map JSON, source-map
  export/entry construction, debug locations, debug artifacts, debug line
  tables, and source-map summaries. This leaves `crates/codegen/src/debug.rs`
  focused on the public facade and source-map/source-span orchestration.
- Source-map coverage export DTOs now live in
  `crates/codegen/src/debug/coverage.rs`. The bytecode-origin coverage
  partition checks and post-opt observed-pre-opt validation stay on the same
  public `codegen::debug` types but no longer add schema boilerplate to the
  source-map/debug artifact assembly module.
- Source-map option, filter, and export metadata DTOs now live in
  `crates/codegen/src/debug/source_map_options.rs`. The public
  `codegen::debug` exports remain stable, but optional source-map artifact
  policy is no longer embedded in the large source-map/debug assembly module.
- Source-map row schema now lives in
  `crates/codegen/src/debug/source_map_entry.rs`. `BytecodeSourceMapEntry`,
  typed entry kinds, invalid source-span reasons, and fail-closed row decoding
  stay together while public `codegen::debug` re-export paths remain unchanged.
- Source-map export schema now lives in
  `crates/codegen/src/debug/source_map_export.rs`. Owned source-map exports,
  export errors, JSON/export helpers, and shared PC-range/metadata validation
  stay together while debug-location, line-table, artifact, and summary modules
  keep consuming the same parent-module helper names.
- Source-span conversion now lives in
  `crates/codegen/src/debug/source_spans.rs`. Snippet validation, line/column
  indexing, and source-span fact row conversion stay together behind the parent
  debug API rather than expanding the source-map/debug assembly module.
- Source-map summary policy now lives in
  `crates/codegen/src/debug/source_map_summary.rs`. Public
  `codegen::debug` re-exports remain stable, but source/non-source and
  debug-line count aggregation no longer shares the source-map row/schema
  module.
- Debug line-table DTOs now live in
  `crates/codegen/src/debug/line_table.rs`. Source-file interning,
  line-record views, file-index validation, and owned line-table decoding stay
  together while public `codegen::debug` re-export paths remain unchanged.
- Debug-location DTOs now live in
  `crates/codegen/src/debug/location.rs`. Compact source-PC row construction,
  fail-closed decoding, and debug-location export validation stay together
  while public `codegen::debug` re-export paths remain unchanged.
- Debug artifact orchestration now lives in
  `crates/codegen/src/debug/artifacts.rs`. Artifact bundle DTOs, filename
  policy, metadata mismatch errors, and JSON rendering stay together while
  public `codegen::debug` re-export paths remain unchanged.
- Source-map entry construction now classifies invalid resolved source spans as
  `source_span_invalid` rows instead of panicking while slicing snippets. The
  closed reasons distinguish inverted byte ranges, invalid UTF-8/snippet
  ranges, and empty snippets, and source-map summaries/analyze reports expose
  the count as an explicit non-source classification. Source-span fact export
  uses the same validated source-span details, so invalid spans are not
  exported as valid `source_span` fact rows.
- Source-map artifact decoding now also rejects unknown source span kinds,
  inverted source ranges, empty source file/snippet strings, empty export
  object/section metadata, export object/section metadata that does not match
  entry rows, overlapping PC ranges within one object section, and unknown
  non-source reason strings for Sonatina synthetic, Sonatina unmapped, and
  bytecode unmapped rows.
- Source-map entry construction now carries closed span kinds and non-source
  reasons as typed enums while serializing the same strings at the JSON
  boundary. Compile-fail coverage rejects raw-string construction of those
  classifications.
- Public source-map row construction now requires `BytecodePcOrigin` instead of
  raw object/section/PC parts; raw tuple parsing is private to JSON
  deserialization and remains fail-closed.
- Source-map export construction now validates object/section metadata,
  per-section PC overlap, and coverage row counts before serialization, so
  writer-side artifacts cannot bypass the same invariants enforced by the owned
  decoder.
- Public Sonatina test metadata coverage now compares source-map JSON against
  the owned export schema constant instead of a stale copied schema literal.
- Analyze source-map reports now share the source-map artifact coverage DTOs and
  a single summary-to-report constructor, reducing the chance that test and
  runtime report paths diverge as more debug fields are added.
- Source-map summaries are now derived only from validated
  `BytecodeSourceMapEntry` rows. The resolution-only summary helper was removed
  because it could count an invalid resolved source span as a valid source row
  before snippet validation classified it.
- Source-map artifact rendering now uses `BytecodeSourceMapExportOptions`,
  replacing the parallel `*_with_origin_coverage` helper axis with one typed
  place for optional export metadata. Writer-side metadata is now
  `BytecodeSourceMapExportMetadata`, backed by `BytecodeObjectKey` for
  object-level reports or `BytecodeSectionKey` for section-scoped artifacts.
- Source-map filtering is now keyed by `BytecodeSectionKey`, preserving the
  object/section owner pair internally. Raw `String` labels remain only in
  serialized source-map rows, JSON deserialization, and CLI/report DTOs.
- `BytecodeSectionKey` now requires typed object and section-name keys. Downstream
  compile-fail coverage rejects raw strings for both parts, and source-map
  artifact decoding applies the same non-empty check to serialized entry rows
  and optional export metadata before accepting an artifact.
- Test metadata no longer stores rendered source-map JSON alongside typed data;
  report generation renders source-map artifacts from typed entries.
- A regression test covers that report boundary directly by verifying
  `source_map.json` is emitted from typed entries.
- Analyze traversal now passes a single `AnalyzeOptions` value through target
  helpers, reducing the option-plumbing boilerplate before more analysis views
  are added.
- Codegen-internal `frontend_provenance` names have been renamed to
  `frontend_origin_labels`; the old spelling remains only for Sonatina's
  external observability API/JSON field. Fe-owned APIs now wrap the Sonatina map
  type in `FrontendOriginLabelMap`.
- `fe build --report` now emits non-test contract/runtime source-map artifacts
  from typed bytecode source-map entries. Ordinary non-report bytecode builds
  keep the bytecode-only path; report generation opts into origin-backed
  Sonatina compilation to produce the source-map entries.
- `fe build --report` also emits non-test contract/runtime origin-fact
  artifacts from typed `BytecodePackageOrigins` graphs. These facts use stable
  backend export keys for Sonatina instructions, Sonatina synthetic nodes,
  bytecode unmapped reasons, and bytecode PC ranges, wrapped in a
  `schema_version: 1` typed-fact JSON boundary whose decoder rejects unsupported
  versions and unknown export, row, or nested origin-key fields.
- Fe test Sonatina metadata now carries typed origin facts as returned data.
  Test reports render `artifacts/tests/<test>/sonatina/origin_facts.json`, and
  `fe analyze --tests --origin-facts` exposes the same fact export path through
  the analyze CLI.
- `fe analyze --origin-facts` also exports regular runtime semantic-to-MIR
  origin facts, so typed fact export is no longer limited to test bytecode or
  build-report artifacts.
- `fe analyze --shape-hashes --shape-facts` exposes runtime const-region
  `ShapeDescribe` graph hashes and versioned typed shape facts, giving the
  derive/schema path a user-facing analysis boundary. Text reports now include
  every graph hash dimension, so the default human-readable view stays aligned
  with the JSON shape/hash data.
- `ShapeDescribe` now has downstream `trybuild` compile-fail coverage for
  missing field policies, empty skip reasons, multiple field policies, and
  unknown item/field attributes. Unknown dimensions are rejected by the macro
  parser with a direct diagnostic, and runtime tests verify declared
  stable-key policies for structs and enum variants reach generated
  `ShapeGraph` nodes.
- The same derive boundary now rejects empty shape kinds and labels at macro
  parse time, so generated shape code cannot defer invalid node identity or
  child/field label data to a later `ShapeGraph` panic or relation export
  failure.
- Focused shape/hash tests now cover the highest-risk graph digest invariants:
  graph edges do not suppress child content, full edge labels affect structure,
  endpoint content-dimension changes stay out of the structure projection, and
  derived child content reaches parent tree digests. Local shape fields are
  hashed as unordered dimension/name/value metadata, while ordered content must
  flow through child edges; tests cover both sides of that distinction.
- Runtime semantic-to-MIR origin fact projection is now owned by `mir::origin`;
  analyze supplies stable owner labels but does not construct MIR fact graphs
  itself.
- MIR fact graph nodes now distinguish semantic owner keys, runtime owner keys,
  and synthetic local keys with nominal wrappers; downstream compile-fail tests
  cover owner-namespace mixups.
- MIR runtime fact export callbacks now return a typed
  `RuntimeOriginFactOwnerKeys` bundle. MIR derives that bundle from
  `RuntimeOriginFactTargetKey` plus each `RuntimePackageBodySymbol`, removing
  raw target/body formatting from analyze and letting compile-fail coverage
  reject swapped owner namespaces, raw target labels, raw body symbols, and raw
  string callbacks.
- MIR runtime package origin bodies now require `RuntimePackageBodySymbol` at
  construction. That keeps runtime package summaries and fact-owner derivation
  from accepting raw or empty runtime symbol labels at the public API boundary.
  Runtime package origin construction also sorts bodies by symbol and rejects
  duplicate symbols before those labels can collide as exported fact owner
  namespaces.
- MIR runtime terminator origins now use `RuntimeTerminatorSite` as the typed
  local key. Public construction from a raw `RBlockId` fails in trybuild, and
  codegen's end-to-end synthetic terminator keys derive their labels through
  that typed site instead of depending on a public MIR string helper.
- MIR runtime-origin regression coverage now lives in
  `crates/mir/src/origin/tests.rs`. The production MIR origin implementation is
  split by responsibility: `runtime_identity.rs` owns statement, terminator,
  and code-region identity plus export-key helpers; `package.rs` owns runtime
  body/package origin records and the cached package-origin query; and
  `fact_graph.rs` owns runtime semantic-to-MIR fact graph construction plus
  typed fact-owner policy. The parent `mir::origin` module remains the compact
  re-export facade and preserves existing `origin::tests::*` paths.
- Closed string enum handling now uses `define_closed_string_enum!` for origin
  kinds, link kinds, fact namespaces, shape dimensions/scopes, source-span
  kinds, and codegen debug/origin reason enums. This preserves the exported
  strings and Serde failure behavior while removing repeated hand-written
  parser/serializer blocks.
- HIR expr/stmt export now requires a typed `HirOriginBodyOwnerKey`, semantic
  export is gated by a semantic owner-key marker trait, and runtime
  statement/terminator export helpers are gated by a runtime owner-key marker
  trait. This closes the helper-level raw-string escape hatch after graph nodes
  became nominal.
- Codegen end-to-end fact graph nodes now make the same distinction before
  combining semantic, runtime, Sonatina, and bytecode origins into one fact-ID
  allocation. Their owner-key pair is derived by
  `EndToEndOriginOwnerKeys::for_function` from a typed
  `SonatinaFunctionExportKey`, with compile-fail coverage rejecting raw function
  labels.
- Codegen stable function-key collection now uses one internal typed map for
  codegen-only and end-to-end graph fact export. That keeps deduplication and
  missing-key errors out of duplicated `BTreeMap<FuncRef, _>` plumbing, with a
  regression proving repeated Sonatina nodes resolve the stable function key
  once. The stable `SonatinaFunctionExportKey`, map, collector, and
  `MissingSonatinaFunctionKey` error now live in
  `crates/codegen/src/origin/function_keys.rs`; `codegen::origin` re-exports the
  public types.
- Codegen graph/fact export code has been split by graph flavor:
  `crates/codegen/src/origin/codegen_graph.rs` owns codegen-only nodes/facts, and
  `crates/codegen/src/origin/end_to_end_graph.rs` owns semantic/runtime/Sonatina/
  bytecode graph stitching. Public APIs remain re-exported from
  `codegen::origin`.
- Bytecode identity/export-key types have been split into
  `crates/codegen/src/origin/bytecode_keys.rs`, keeping object/section/PC-range
  invariants separate from bytecode package construction and source-map
  resolution.
- Codegen origin regressions now live behind the
  `crates/codegen/src/origin/tests.rs` facade, with focused
  `crates/codegen/src/origin/tests/` modules for coverage, Sonatina records,
  frontend labels, bytecode origins, export keys, fact export, backend-prepared
  fallback, post-opt snapshot lineage, and graph shape. This preserves the
  existing `origin::tests::*` test paths while removing the large fixture block
  from the implementation facade.
- Codegen Sonatina and bytecode origin implementation is now split by
  responsibility. `sonatina_pre_opt.rs` owns pre-opt lowering records,
  `sonatina_post_opt.rs` owns optimized-snapshot, backend-prepared, and
  snapshot-loss records. `bytecode_origins.rs` owns PC-map ingestion,
  source-resolution entry points, object/section filtering, and package
  orchestration; `bytecode_coverage.rs` owns bytecode-origin coverage counting;
  `bytecode_graph.rs` owns bytecode fact graph projection; and
  `frontend_labels.rs` owns frontend-origin labels plus pre-opt source label
  classification. `codegen/src/origin.rs` is now a compact public facade over
  these modules.
- Sonatina function export keys are nominal wrappers instead of raw strings in
  codegen origin/fact/frontend-provenance callbacks, with downstream
  compile-fail coverage rejecting raw string keys.
- `FrontendOriginLabelMap` is now a Fe-owned wrapper rather than a public alias
  to Sonatina's raw `FrontendProvenanceMap`; the raw map is exposed only through
  an explicit adapter at the Sonatina observability boundary. Inserted labels
  must be nominal `FrontendOriginLabel` values derived from typed export keys,
  with compile-fail coverage rejecting raw string insertion. The label wrapper
  and map now live in `crates/codegen/src/origin/frontend_labels.rs`,
  preserving `codegen::origin` re-export paths while keeping
  dependency-boundary labels separate from package assembly.
- Frontend origin-label derivation now reports missing stable Sonatina function
  keys through `MissingSonatinaFunctionKey` when a runtime-origin label should
  be emitted, instead of silently omitting that label from Sonatina
  observability metadata. Synthetic and unmapped same-ID records remain
  explicit non-label sources. Origin-backed compilation maps missing runtime
  label keys into its normal `LowerError` path.
- The repeated nominal key-wrapper shapes are generated by
  `define_origin_string_key!` and `define_origin_owner_key!`, preserving the
  type barriers while reducing maintenance cost.
- The regular private `OriginKey<Owner, Local>` wrapper shape is generated by
  `define_origin_key_type!` for HIR expr/stmt/semantic origins and MIR runtime
  stmt/terminator origins. Custom Sonatina and bytecode origins stay manual
  because their constructors enforce stage or PC-range invariants.
- Common origin-core regression coverage now lives in
  `crates/common/src/origin/tests.rs`. The production common origin core is
  split by responsibility: `origin/key.rs` owns `OriginKey`,
  `origin/export_key.rs` owns export kinds, stable key validation, and typed
  owner/local traits, `origin/graph.rs` owns link kinds plus graph containers,
  and `origin/macros.rs` owns the exported helper macros. The parent
  `origin.rs` stays as the module/re-export facade, preserving the
  `common::origin` API and existing test paths.
- Generated nominal key wrappers derive `salsa::Update`. The generic
  origin key/link/graph containers still use manual `salsa::Update` impls
  because Salsa 0.20's derive path requires intrusive generic bounds there, but
  those impls now carry explicit safety notes and behavior coverage for
  unchanged and changed fieldwise updates.
- Public HIR, MIR, codegen, and end-to-end origin graph APIs now use nominal
  wrappers generated by `define_origin_graph_type!` instead of raw
  `OriginGraph<Node>` aliases. A downstream codegen compile-fail test verifies
  fact export does not accept a raw graph where the nominal codegen graph is
  required.
- Public HIR, MIR, and codegen origin APIs no longer expose raw
  `OriginKey<Owner, Local>` values through `.key()` accessors. HIR, MIR, and
  codegen compile-fail coverage now verifies expr/stmt/semantic, runtime,
  Sonatina instruction, and bytecode PC origins cannot be deconstructed into raw
  origin keys, keeping external consumers on nominal origin wrappers plus stable
  export-key helpers.
- Public HIR, MIR, and codegen origin APIs also avoid inherent raw
  export-local-key string helpers for semantic origins, runtime
  statement/terminator sites, and bytecode PC ranges. Local-key formatting stays
  behind typed wrapper construction or the shared `OriginExportLocalKey` trait
  at internal adapter points, with downstream compile-fail coverage for each
  former escape hatch.
- `BytecodePackageOrigins` now rejects malformed Sonatina PC-map rows with empty
  or inverted PC ranges instead of silently skipping them. That makes the
  bytecode-origin coverage partition a statement about all valid emitted PC-map
  rows, not just the subset that survived an implicit filter before fact export.
- `OriginExportKey` now validates owner/local parts and owns both canonical
  storage formatting and display labels. This removes duplicated formatting
  helpers in facts/codegen and keeps malformed keys from reaching fact
  allocation or JSON artifact boundaries.
- `OriginExportKey::new` and `try_new` now require typed owner and local-key
  inputs instead of raw strings. Raw owner/local text is still accepted at the
  explicit `try_from_raw_parts` serde/import boundary, and compile-fail coverage
  in `fe-common` rejects raw-string construction through the typed path.
- Generated nominal string-key and owner-key wrappers now reject empty strings
  and the reserved canonical-storage separator at construction, closing the
  gap where malformed owner/object/function labels could be carried until a
  later export-key panic.
- Bytecode origin fact projection is now owned by `codegen::origin`, including
  checked stable Sonatina function-key export before fact IDs are allocated.
- `TypedFactSet` intentionally does not provide a concatenating merge API:
  independently exported fact sets have allocation-local IDs, so combined
  origin views must be built as one typed graph and exported once.
- Fact ID namespace and allocator infrastructure has been split into
  `crates/common/src/facts/ids.rs`, preserving the `common::facts::*` public
  re-export path while reducing the large fact module around allocation-local
  identity first.
- Origin node/link fact DTOs and namespace validation have been split into
  `crates/common/src/facts/origin_fact.rs`, preserving the
  `common::facts::*` public re-export path while keeping constructor checks out
  of graph export, reachability indexing, and relation validation.
- Origin reachability summary/path/witness DTOs have been split into
  `crates/common/src/facts/origin_path.rs`, preserving the `common::facts::*`
  public re-export path while keeping query result validation separate from
  index traversal and relation-table queries.
- Origin path DTO code is now split below that facade:
  `origin_path/reachability.rs` owns `OriginReachabilitySummary` and per-kind
  aggregate validation; `origin_path/path.rs` owns internal fact-ID paths and
  kind-pair witnesses; `origin_path/witness.rs` owns stable export-key path
  witnesses; and `origin_path/source_witness.rs` owns source-span-attached path
  witnesses. Public `common::facts::*` re-exports remain stable.
- Origin path witness code is now split below `origin_path/witness.rs`:
  `witness/error.rs` owns validation errors, `witness/record.rs` owns
  `OriginPathWitnessExport`, and `witness/deserialize.rs` owns fail-closed JSON
  reconstruction. Query/export callers still use the same public DTO.
- Origin reachability DTO code is now split below `origin_path/reachability.rs`.
  `reachability/summary.rs` owns `OriginReachabilitySummary` and fail-closed
  serde reconstruction; `reachability/pair.rs` owns per-kind pair DTOs;
  `reachability/validation.rs` owns duplicate/total checks; and
  `reachability/error.rs` owns user-facing validation errors.
- Typed fact export code is now split below that facade.
  `typed_fact/export.rs` owns `OwnedTypedFactSetExport`, `TypedFactSetExport`,
  schema-version validation, and origin/shape index validation for imported
  exports; `typed_fact/fact.rs` owns the `TypedFact` enum plus per-variant
  serde mapping. Public `common::facts::*` re-exports remain stable.
- `TypedFact` is now split below `typed_fact/fact.rs`. The parent module owns
  only the enum; `typed_fact/fact/serialize.rs` owns stable per-variant JSON
  encoding; and `typed_fact/fact/deserialize.rs` owns the fail-closed tagged
  decoder and constructor validation. The wire schema and public re-exports
  remain stable.
- `TypedFact` decode internals are now split below
  `typed_fact/fact/deserialize.rs`: `deserialize/raw.rs` owns the tagged wire
  enum and `deserialize/construct.rs` owns conversion into validated typed fact
  variants. This keeps wire-shape drift separate from constructor invariant
  checks.
- Typed relation schema metadata has been split into
  `crates/common/src/facts/relation_schema.rs`, covering relation names, column
  names, schema descriptors, and column matching.
- Relation schema code is now split below that facade.
  `relation_schema/name.rs` owns the closed relation name enum plus origin/shape
  relation classification; `relation_schema/column.rs` owns the closed column
  enum; and `relation_schema/schema.rs` owns schema descriptors, raw-name
  lookup, column matching, and column indexing. Public `common::facts::*`
  re-exports remain stable.
- Relation schema descriptor/catalog code is now split below
  `relation_schema/schema.rs`: `schema/descriptor.rs` owns
  `TypedFactRelationSchema` and relation-name schema/column-index APIs, while
  `schema/catalog.rs` owns the fixed catalog, raw-name lookup, and column
  matching. This keeps relation metadata extensibility separate from table
  import/export validation.
- Typed relation table DTOs, relation-count DTOs, relation-row views, and
  relation JSON validation errors have been split into
  `crates/common/src/facts/relation.rs`, preserving the `common::facts::*`
  public re-export path while leaving semantic validation/query indexing in the
  parent facts module.
- Relation table code is now split below that facade. `relation/set.rs` owns
  `TypedFactRelationSet`; `relation/table.rs` owns `TypedFactRelation`;
  `relation/count.rs` owns `TypedFactRelationCount`; `relation/row.rs` owns
  relation row views; `relation/error.rs` owns relation diagnostics; and
  `relation/validation.rs` owns schema-version, column, and row-width
  validation. Public `common::facts::*` re-exports remain stable.
- Relation diagnostics are now split below `relation/error.rs`. The public
  `TypedFactRelationError` enum remains at the stable re-export path, while
  `relation/error/display.rs` owns display text for relation import,
  validation, source-span, and shape-hash diagnostics.
- Shape-hash DTOs and validation have been split into
  `crates/common/src/facts/shape_hash.rs`, preserving the `common::facts::*`
  public re-export path while keeping digest canonicalization and node/scope
  checks beside the constructors.
- Shape-hash code is now split below that facade.
  `shape_hash/scope.rs` owns the closed string scope enum; `shape_hash/key.rs`
  owns lookup keys plus node/scope invariants; `shape_hash/digest.rs` owns
  canonical digest validation; and `shape_hash/fact.rs` owns fact construction
  and serde validation. Public `common::facts::*` re-exports remain stable.
- Source-span DTOs and validation have been split into
  `crates/common/src/facts/source_span.rs`, preserving the `common::facts::*`
  public re-export path while keeping range/file checks beside the source-span
  constructors.
- Source-span fact code is now split below that facade:
  `source_span/export.rs` owns `SourceSpanKind`, `SourceSpanExport`, and shared
  range/file validation; `source_span/fact.rs` owns allocated
  `SourceSpanFact` rows and namespace validation; and
  `source_span/file_count.rs` owns compact per-file summary DTOs. Public
  `common::facts::*` re-exports remain stable.
- Source-span export code is now split below that facade too:
  `source_span/export/kind.rs` owns the closed span-kind enum;
  `export/error.rs` owns validation errors; `export/validation.rs` owns shared
  file/range checks; and `export/record.rs` owns `SourceSpanExport`,
  fail-closed serde construction, and deterministic sort keys.
- Source-span fact code is now split below `source_span/fact.rs`.
  `source_span/fact/error.rs` owns origin-namespace/span validation error
  conversion and display text; `source_span/fact/record.rs` owns
  `SourceSpanFact`, namespace-checked construction, source-span export
  attachment, and fail-closed serde reconstruction.
- Source-span record serde is now split below the record modules.
  `source_span/export/record/deserialize.rs` and
  `source_span/fact/record/deserialize.rs` own raw fail-closed JSON decoding,
  while `source_span/export/record/sort_key.rs` owns deterministic export
  ordering. This keeps DTO construction separate from boundary decoding.
- Shape node/field/child/edge, trace-event, and data-flow DTOs have been split
  into `crates/common/src/facts/shape_fact.rs`, preserving the
  `common::facts::*` public re-export path while keeping shape-node namespace
  checks and non-empty text validation beside the constructors.
- Shape fact code is now split below that facade. `shape_fact/text.rs` owns
  shared shape-node namespace and non-empty text validation;
  `shape_fact/node.rs` owns `ShapeNodeFact`; `shape_fact/field.rs` owns
  `ShapeFieldFact`; `shape_fact/edge.rs` owns child/edge facts;
  `shape_fact/trace_event.rs` owns trace-event facts; and
  `shape_fact/data_flow.rs` owns data-flow facts. Public `common::facts::*`
  re-exports remain stable.
- Origin and shape graph fact export builders have been split into
  `crates/common/src/facts/graph_export.rs`, preserving the
  `common::facts::*` public re-export path while keeping graph-to-fact
  projection separate from relation validation and query indexing.
- Graph export is now split below that facade. `graph_export/origin.rs` owns
  origin graph key/link deduplication and fact ID allocation;
  `graph_export/shape.rs` owns shape graph node/field/edge/hash/trace/data-flow
  projection. Public `common::facts::*` re-exports remain stable.
- Typed fact relation export projection has been split into
  `crates/common/src/facts/relation_export.rs`, preserving behavior while
  separating fact-to-row projection from relation validation and query indexing.
- Relation export is now split below that facade. `relation_export/cell.rs`
  owns fact-ID and graph-scope cell formatting; `relation_export/row.rs` owns
  per-variant typed fact row projection with schema-width assertions; and
  `relation_export/set.rs` owns deterministic row sorting and relation-set
  construction. Public `common::facts::*` re-exports remain stable.
- The `TypedFactSet` container and iterator/source-span attachment facade have
  been split into `crates/common/src/facts/typed_fact_set.rs`, preserving the
  `common::facts::*` public re-export path while keeping fact-set storage
  separate from relation-table validation and query indexing.
- `TypedFactSet` code is now split below that facade. The parent module owns
  storage plus export/relation-export adapters; `typed_fact_set/iterators.rs`
  owns typed per-variant iterators generated from one local macro; and
  `typed_fact_set/source_spans.rs` owns deterministic source-span attachment.
  Public `common::facts::*` re-exports remain stable.
- Shared fact-index and source-span attachment error types have been split into
  `crates/common/src/facts/index_error.rs`, preserving the
  `common::facts::*` public re-export path while keeping namespace/text guard
  helpers and error formatting out of the parent index implementations.
- Index diagnostics are now split below that facade.
  `index_error/fact_index.rs` owns `FactIndexError` and its display text;
  `index_error/source_span.rs` owns `SourceSpanFactError`; and
  `index_error/helpers.rs` owns namespace/text guard helpers consumed by origin
  and shape indexes. Public `common::facts::*` re-exports remain stable.
- Fact-index diagnostics are now split below `index_error/fact_index.rs`. The
  public `FactIndexError` enum remains at the stable re-export path, while
  `index_error/fact_index/display.rs` owns display text for origin,
  source-span, shape, and shape-hash index diagnostics.
- `OriginFactIndex` has been split into
  `crates/common/src/facts/origin_index.rs`, preserving the
  `common::facts::*` public re-export path while keeping typed-fact graph
  traversal, endpoint/source-span validation, reachability summaries, and path
  witnesses separate from relation-table query indexing.
- `OriginFactIndex` is now a compact facade over focused implementation
  modules. `origin_index/build.rs` owns typed-fact index construction and
  endpoint/source-span validation; `origin_index/source_spans.rs` owns
  source-span lookups; `origin_index/reachability.rs` owns reachability sets
  and summaries; and `origin_index/paths.rs` owns shortest paths plus
  path-witness exports. Public `common::facts::*` re-exports remain stable.
- Origin-index path query code is now split below `origin_index/paths.rs`:
  `paths/search.rs` owns shortest-path BFS and stable-key path lookup;
  `paths/representative.rs` owns representative kind-pair witness selection;
  and `paths/exports.rs` owns stable export-key witness projection plus
  priority-ordered export selection.
- `ShapeFactIndex` has been split into
  `crates/common/src/facts/shape_index.rs`, preserving the
  `common::facts::*` public re-export path while keeping shape-node/hash lookup
  and completeness validation separate from relation-table query indexing.
- `ShapeFactIndex` is now a compact facade over focused modules.
  `shape_index/build.rs` owns typed fact indexing, namespace/text/reference
  validation, and required hash coverage checks; `shape_index/lookup.rs` owns
  source-id/stable-key/node/hash lookup APIs. Public `common::facts::*`
  re-exports remain stable.
- `TypedFactRelationIndex` has been split into
  `crates/common/src/facts/relation_index.rs`, preserving the
  `common::facts::*` public re-export path while keeping relation-table
  semantic validation, relation-backed reachability/path queries, source-path
  witnesses, and shape-hash relation completeness checks separate from the
  parent facts facade.
- Relation-index validation helpers are now split below
  `relation_index/validation/helpers.rs`. `helpers/ids.rs` owns relation
  fact-ID collection and namespace checks; `helpers/uniqueness.rs` owns
  duplicate-key checks; `helpers/references.rs` owns cross-relation reference
  checks; and `helpers/cells.rs` owns non-empty, closed-value, and numeric cell
  validation.
- Relation-backed origin queries are now split below the
  `relation_index/origin_paths.rs` facade. `origin_paths/reachability.rs` owns
  reachability summaries; `origin_paths/paths.rs` owns plain path witness
  queries; `origin_paths/source_paths.rs` owns source-span-attached path
  witnesses; and `origin_paths/source_counts.rs` owns source-span file counts.
  Public `TypedFactRelationIndex` query APIs remain stable.
- Relation-backed graph decoding is now split below `origin_paths/graph.rs`.
  `graph/nodes.rs` owns origin-node row decoding and export-key reconstruction;
  `graph/links.rs` owns origin-link row decoding and deterministic
  outgoing-edge ordering; and `graph/ordinals.rs` owns `origin_node:` fact-ID
  parsing shared with source-span relation joins.
- The common facts unit tests have been split into
  focused modules under `crates/common/src/facts/tests/`, leaving
  `crates/common/src/facts/tests.rs` as a shared-helper facade and
  `crates/common/src/facts.rs` as a compact public facade over focused modules
  and re-exports.
- Build-report and test-bytecode origin fact artifacts now use that combined
  graph approach: each bytecode object/test gets one end-to-end fact set from
  semantic/runtime origins through MIR, pre-opt Sonatina, post-opt Sonatina, and
  bytecode PC ranges.
- `OriginFactIndex` adds an engine-agnostic typed query layer over origin facts,
  with exact-answer reachability fixtures and rejection of malformed links. This
  keeps query correctness testable before any specific Datalog backend is wired
  back in.
- The test-bytecode metadata regression now uses `OriginFactIndex` against real
  emitted facts and asserts that at least one runtime origin reaches a bytecode
  PC, so the end-to-end graph is tested as a queryable relation, not only as
  serialized rows.
- `OriginFactIndex` also produces a reachability summary grouped by origin
  kind. `fe analyze --origin-facts` reports that derived query result, including
  semantic-to-runtime and runtime-to-bytecode path coverage in focused tests,
  while keeping fact export as pure returned data.
- `TypedFactRelationIndex` now independently computes the same reachability
  summary from relation-table exports, and analyze uses that path for grouped
  reachable kind-pair counts.
- Reachability summaries now validate as DTOs: per-kind rows must be positive
  and unique, unknown fields are rejected, and the exported total must equal the
  sum of grouped rows. This prevents report JSON from drifting into an
  internally inconsistent coverage summary.
- The same index now returns deterministic shortest-path witnesses over fact
  IDs. The exact oracle test verifies both node sequence and link kinds, so path
  explanation can become shared infrastructure instead of duplicated graph
  traversal in every exporter.
- `fe analyze --origin-facts` now exposes representative path witnesses derived
  from `TypedFactRelationIndex`. JSON reports use the shared
  `common::facts::OriginPathWitnessExport` shape with stable origin export keys
  plus link kinds, and focused tests compare relation-table path witnesses
  against the `OriginFactIndex` typed-fact oracle. Text reports also render
  compact witness chains from the same data, so the default human-readable view
  now includes an actual origin explanation path instead of only a witness
  count.
- `TypedFactRelationIndex` now also joins those paths to terminal `source_span`
  relation rows and exports representative `source_path_witnesses`. Analyze
  renders them in JSON and text, giving a concrete source-to-bytecode explanation
  view from cached relation data rather than introducing mutable debug/fact sinks
  inside salsa queries.
- Origin path witness export now supports direct typed kind-pair queries and
  priority-driven witness export through the relation-table query path. The
  analyze boundary uses that API so high-value semantic-to-bytecode and
  runtime-to-bytecode joins cannot disappear merely because generic
  representative paths hit the display limit first.
- `TypedFactRelationIndex` also mirrors the stable-key path helper from
  `OriginFactIndex`, returning `OriginPathWitnessExport` for two
  `OriginExportKey`s without exposing allocation-local fact IDs to future query
  or debug adapters.
- Path witness DTOs now validate their own report-boundary invariants:
  `OriginPath`, `OriginPathWitnessExport`, and
  `OriginSourcePathWitnessExport` reject empty paths, node/link count
  mismatches, non-`origin_node` fact IDs, first/last origin-kind mismatches, and
  source spans attached to a different terminal origin key. This keeps
  source-to-bytecode explanation rows fail-closed even when consumed outside the
  in-memory origin index.
- Text origin-fact reports now print grouped reachable kind-pair counts as well
  as concrete path witnesses, so the compact human report exposes both graph
  coverage breadth and one deterministic explanation path.
- The same index now has stable-key path helpers, so future query adapters can
  work from `OriginExportKey` pairs instead of exposing allocation-local fact
  IDs outside the indexed fact set.
- Versioned typed fact JSON now deserializes back into `OwnedTypedFactSetExport`
  with explicit parsers for the string-tagged enums. A round-trip regression
  re-indexes decoded origin facts and covers shape fact tags, so future query
  adapters can consume the schema without reimplementing JSON parsing rules.
  Unknown typed fact schema versions are rejected during deserialization.
- The owned typed fact export decoder now also validates origin fact graph
  integrity with `OriginFactIndex`, rejecting malformed links and duplicate
  origin identities before the data reaches analyze or report consumers.
- Origin node, origin link, and source-span fact DTOs now reject non-origin
  fact ID namespaces at construction and serde boundaries. The graph-level
  checks for missing endpoints, missing source-span origins, duplicate IDs, and
  duplicate origin keys remain in `OriginFactIndex`.
- The decoder now validates shape fact integrity with `ShapeFactIndex` as well,
  so malformed shape fields, child/edge endpoints, scoped hash rows, and
  duplicate shape identities fail at the same versioned JSON boundary. Shape
  graph construction and decoded fact artifacts now reject empty stable keys,
  node kinds, field names, child labels, edge labels, trace event kinds, and
  data-flow kinds; the DTO constructors and serde boundaries own those local
  text checks before indexing. Shape fact DTOs also reject non-`shape_node` fact
  ID namespaces locally; missing node references and duplicate identity checks
  remain in `ShapeFactIndex`. Shape hash rows now require canonical lowercase
  16-character hex digests, unique `(node, scope, dimension)` keys, and complete
  graph plus per-node local/tree coverage across every shape dimension. The same
  index now provides typed graph/local/tree hash lookup
  helpers, reducing the incentive for future consumers to scan raw fact rows by
  hand.
- Shape fact exports now include typed `trace_event` and `data_flow` rows
  derived from trace-event fields and graph edges, with endpoint validation and
  analyze summary counts. Generic shape field/edge rows remain available for
  compatibility.
- Shape analyze reports now expose compact per-relation row counts from the
  same relation-index query path as origin fact reports. Text
  `--shape-hashes --shape-facts` reports show populated shape tables such as
  `shape_node`, `shape_field`, `shape_child`, `trace_event`, and `shape_hash`
  without requiring full JSON table output.
- Public relation-table query APIs now take `TypedFactRelationName`, a closed
  relation-name enum, instead of arbitrary relation-name strings. The exported
  JSON relation names remain the same, while internal/query/report callers get
  compile-time protection against relation-name typos.
- Public relation-table query APIs now also take `TypedFactRelationColumnName`
  for column lookup, row filtering, and relation-row cell access. The exported
  JSON column strings remain unchanged, while compile-fail coverage rejects raw
  string column queries at the `fe-common` API boundary.
- Public relation-table construction is now schema-driven as well:
  `TypedFactRelation::new` derives columns from `TypedFactRelationName`, and
  `TypedFactRelationSet::new` validates required relations immediately. This
  keeps raw table names and raw column lists out of normal construction paths;
  they remain only in serde/import validation.
- Relation schema metadata now flows through `TypedFactRelationSchema`, exposed
  by `TypedFactRelationName::schema` and `typed_fact_relation_schemas()`. This
  gives export, validation, query counts, and schema tests one shared typed
  descriptor instead of parallel tuple unpacking.
- Relation export now uses a private schema-initialized row collector keyed by
  `TypedFactRelationName`, removing the remaining parallel row vectors and
  handwritten relation assembly. Individual `TypedFact` values own their
  fact-to-row projection, and row width is checked against the declared schema
  before rows are sorted and emitted in schema order.
- Relation-count summaries now retain `TypedFactRelationName` until the
  analyze report DTO boundary. Public construction rejects raw relation-name
  strings, closing another small path for query/report code to drift from the
  declared relation schema.
- Relation-row query results now also retain `TypedFactRelationName`, and
  `TypedFactRelationRow::cell` resolves columns through the typed schema
  descriptor instead of scanning exported column strings. Deserialized table
  JSON still validates its strings at the boundary, but internal query
  consumers stay on typed schema metadata.
- Relation-table storage now follows the same rule: `TypedFactRelation` stores
  typed relation names and typed column IDs after validation, preserving raw
  strings only as JSON input and compatibility output. Query indexing no longer
  needs to treat decoded table names and columns as arbitrary strings.
- The relation-table index now keys its internal lookup maps by closed relation
  and column enums. The remaining raw-string validation helpers are adapters for
  decoded/imported artifacts and parse into typed schema keys before lookup.
- Relation semantic validation now uses the same closed relation and column
  enums in helper signatures. ID, uniqueness, reference, numeric, non-empty,
  and closed-value checks no longer pass raw relation/column names through the
  query path except when constructing error messages.
- Typed fact exports now have a fixed relation-table projection through
  `TypedFactSet::relation_export`, covering origin, source-span, shape,
  trace-event, data-flow, and shape-hash rows. This gives future Datalog/query
  adapters a stable table boundary while keeping query engines out of compiler
  core state.
- `fe analyze --fact-relation-tables --format json` now emits those full
  relation tables for origin and shape fact reports when the corresponding
  typed facts are requested. The flag is validation-gated by `--origin-facts` or
  `--shape-facts`, so table-shaped query artifacts are available without making
  them part of normal compact summaries.
- `fe analyze --origin-facts` text output now renders per-relation row counts
  from `TypedFactRelationIndex` over the same relation export, so
  human-readable reports show which engine-agnostic tables were populated
  without requiring full JSON relation rows or a CLI-local relation scan. Text
  and JSON reports reuse common's validating `TypedFactRelationCount` DTO, so
  relation-count summaries keep closed relation names and non-zero rows at the
  report boundary.
- Relation-table JSON is now fail-closed on decode: schema version, relation
  names, required table set, duplicate tables, fixed columns, row widths, and
  unknown fields are validated before a query adapter can consume the rows.
- Relation export now uses that same fixed schema table for column construction
  instead of repeating column lists beside every exported relation, with a
  schema-order test guarding the declared boundary.
- In-memory relation tables no longer store columns independently from their
  relation name. Accessors, validation, and serialization derive columns from
  the fixed schema descriptor, while the exported JSON shape still includes the
  same portable column list.
- Internal typed relation validation now mirrors that model by taking relation
  name plus rows only. Raw column validation is preserved at JSON/import
  boundaries, where arbitrary caller-provided column strings can still appear.
- Relation-set completeness checks now track seen relations as
  `TypedFactRelationName`, so duplicate/missing relation validation stays typed
  internally and stringifies only for diagnostics.
- Relation queries now derive column positions from that same schema descriptor
  after relation lookup, removing the index's separate relation-to-column cache
  and making the fixed schema the only source of column order truth.
- `TypedFactRelationIndex` is now a small facade over focused implementation
  modules. `relation_index/origin_paths.rs` owns relation-backed reachability,
  path witness, source-path witness, and source-span summary queries, while
  `relation_index/validation.rs` owns semantic row/reference validation,
  origin-key validation, source-span range checks, and shape-hash relation
  completeness.
- Relation-backed origin query code is now split below that facade:
  `relation_index/origin_paths/graph.rs` owns origin-node/link relation
  decoding and deterministic reachability ordering; `path_search.rs` owns
  shortest-path reconstruction; and `source_spans.rs` owns source-span relation
  projection plus per-file summaries. The public
  `TypedFactRelationIndex` query API remains unchanged.
- Plain relation-backed path queries are now split below
  `origin_paths/paths.rs`: `paths/between_keys.rs` owns exact stable-key path
  lookup, `paths/representative.rs` owns kind-pair representative lookup,
  `paths/priority.rs` owns priority-ordered witness selection, and
  `paths/export.rs` owns shortest-path witness construction shared with
  source-path queries.
- Relation-backed source-span queries are now split below
  `origin_paths/source_spans.rs`: `source_spans/columns.rs` owns source-span
  relation column lookup, `source_spans/decode.rs` owns conversion from
  relation rows into typed `SourceSpanExport` values, and source-span file-count
  aggregation lives in `origin_paths/source_counts.rs`.
- Relation semantic validation is now split below the same facade:
  `validation/helpers.rs` owns shared ID/reference/cardinality and cell-shape
  checks; `validation/origin_keys.rs` owns stable origin-key validation;
  `validation/source_spans.rs` owns source-span row validation; and
  `validation/shape_hashes.rs` owns shape-hash node/scope/dimension/digest
  completeness checks.
- Typed column-position lookup is now centralized on `TypedFactRelationName`.
  Row cell access and relation-index filtering share the same closed-schema
  mismatch diagnostics instead of open-coding the same schema scan.
- Relation-table export now sorts rows per relation, so backend/query artifacts
  are deterministic across equivalent typed fact sets even if fact insertion
  order changes.
- `TypedFactRelationIndex` now gives those decoded relation tables an exact,
  engine-agnostic query surface. New oracle tests consume the serialized table
  artifact, filter rows by column, join origin nodes/links, and verify
  trace-event, data-flow, source-span, and graph-hash answers without making a
  query backend part of compiler core state. The same index rejects missing
  origin/shape references, invalid closed string values, duplicate origin and
  shape keys, malformed origin key cells, duplicate origin links, wrong relation
  ID namespaces, malformed numeric cells, inverted source ranges, empty shape
  identity/label cells, empty source-span file cells, duplicate shape-hash keys,
  and incomplete shape-hash coverage before query adapters run. The typed fact
  JSON boundary now rejects duplicate origin links through `OriginFactIndex` as
  well.
- Shape-hash relation validation now keeps parsed `ShapeHashScope` and
  `ShapeDimension` values in its duplicate/completeness keys, so graph/local/tree
  hash checks stay typed internally and only stringify for diagnostics.
- Shape analyze reports now keep graph hash dimensions as `ShapeDimension`
  internally while preserving the existing JSON strings, so decoded report rows
  reject unknown dimensions instead of accepting arbitrary hash-dimension text.
- Shape analyze report decoding also checks that graph hash dimensions are
  complete and unique, report hash digests are canonical, embedded shape fact
  counts and graph hash rows match the summary, and populated relation
  summaries only describe shape tables. Duplicate relation-count summaries are
  rejected at the report boundary, and shape report scope/label fields must be
  non-empty.
- `ShapeHashFact` now rejects malformed digest text at construction and serde
  boundaries, so canonical lowercase hex digest validation no longer waits for a
  later shape fact index pass. Node/scope consistency and full hash coverage
  still belong to the index.
- Canonical digest text now flows through a `ShapeHashDigest` newtype in
  `common::facts`; analyze shape-hash reports reuse that boundary instead of
  carrying a second CLI-only lowercase/length rule. Shape report validation
  compares typed digest values and stringifies only for diagnostics. Public
  `ShapeHashFact` construction now requires the typed digest, with raw digest
  text confined to explicit JSON/import helpers and the raw canonicality
  predicate kept private. Digest-format errors are owned by
  `ShapeHashDigestError` and delegated through fact import errors. Shape fact
  indexes validate hash scope and completeness, not digest text again. Raw
  relation-table imports still reject malformed `digest_hex` by constructing
  `ShapeHashDigest`.
- End-to-end bytecode fact exports now include `source_span` rows for resolved
  bytecode PC source mappings. These rows are derived from typed
  `BytecodeSourceResolution` data at export/report boundaries and are validated
  through `OriginFactIndex`, keeping source locations queryable without
  re-parsing source-map artifacts.
- Source-map summaries now take typed `BytecodeSourceMapExportMetadata` for
  optional object/section filtering, matching source-map export metadata and
  preventing a second raw-string filter path from drifting from artifact
  scoping.
- `fe analyze --origin-facts` now summarizes those exported source-span facts by
  source file in text and JSON reports. The summary is derived through
  `TypedFactRelationIndex` and serialized with common's validating
  `SourceSpanFileCount` DTO, giving quick visibility into bytecode/source
  coverage while proving that the relation-table export can answer the same
  query without compiler-side mutable sinks or a permissive CLI-only summary
  schema.
- Source-span fact rows now use a closed span-kind enum and reject inverted byte
  or line/column positions during typed fact JSON deserialization. Empty source
  file labels are rejected in `SourceSpanFact` construction/serde, typed fact
  JSON, and relation-table artifacts, so decoded query inputs cannot silently
  widen the source-location schema. Origin existence still belongs to
  `OriginFactIndex`.
- The `SourceSpanExport` constructor now enforces non-empty file labels and
  ordered byte/line-column ranges before internal exporters produce fact rows,
  matching the JSON and relation-table boundary checks. `SourceSpanExport` also
  has a fail-closed decoder, so standalone source-span exports and source-path
  witness exports reject malformed source-span JSON at the DTO boundary.
- The implementation is pure returned data and does not emit fact/debug/source
  side effects from Salsa queries.

This does not finish optimization lineage. Same-`InstId` pre/post joins are
currently snapshot `alias` edges, not proof of specific pass transforms. The
post-opt package bundle now also records conservative pre-opt snapshot losses
for pre-opt instructions without same-`InstId` post-opt matches, and can export
them as typed origin facts through a `pre_opt_snapshot_loss` synthetic node.
Bytecode PC records that refer to optimized-snapshot-missing instruction IDs use
the `backend_prepared` stage instead of impersonating `post_opt` origins. Build
reports, test reports, and `fe analyze --tests --origin-facts` expose these
snapshot-diff facts separately from end-to-end bytecode facts. They are still
only snapshot facts. `fe analyze --source-maps` now also surfaces the same
distinction as filtered coverage counts so users do not need to parse raw fact
rows just to see snapshot gaps. Precise split, merge, replacement, deletion, and
alias tracking still requires Sonatina pass hooks or an equivalent
prepared-module origin bundle that is independent of observability PC maps.
`SonatinaPostOptOriginRecord` now enforces the same-ID snapshot alias claim: a
`SameInstId` source must reference a pre-opt record with the same function and
instruction ID as the post-opt origin. This closes an API hole where a manually
constructed post-opt record could have represented a cross-function or
cross-instruction relationship as a conservative snapshot alias.
The stage and snapshot-loss reason strings now use the shared closed-string enum
helper, including its `Display` implementation, so conservative optimizer
classification labels do not maintain another bespoke string-rendering path.

## Phase 2 Entry Point

Phase 2 should start with typed owner-aware origin keys and invariant tests.
The first code slice should not depend on Datalog, DWARF, ethdebug, or Sonatina
optimization support.

Minimum Phase 2 deliverables:

- Define typed keys for HIR expressions, HIR statements, MIR statements, MIR
  terminators, Sonatina instructions, and bytecode PC ranges.
- Make it impossible to construct a HIR expr/stmt origin without body context.
- Make it impossible to construct a MIR statement origin without runtime
  instance, block, and statement index.
- Add tests proving same local IDs in different owners do not collide.
- Add tests proving HIR expr and HIR stmt IDs cannot be confused.
- Add tests proving exported owner keys cannot cross helper boundaries as raw
  strings once a phase exposes stable origin keys.
- Add compatibility adapters only where needed to keep old prototype paths
  compiling during migration.

Non-goals for the first Phase 2 slice:

- Do not port the Datalog fact schema yet.
- Do not rebuild DWARF or ethdebug yet.
- Do not solve Sonatina optimization lineage yet.
- Do not expand the derive macro before the typed identity model exists.

## Sessions Worth Deeper Review

- `b7af97db-676a-4a1e-987b-f5251554d1cd`: debug-info prototype, `SourceOrd`,
  DWARF, ethdebug, Datalog facts, Sonatina PC maps.
- `b1ea79d7-e7c1-4c21-9a18-8a2e7c17816d`: proof/debug intent and the boundary
  between compiler metadata and native Fe verification features.
- `06abe0e6-e08a-4556-84b8-c5f7a242cc2a`: Salsa/query-driven exporter
  precedent.
- `f74d55cd-26b4-4d91-8e33-75f238a8861d`: derive, `HirBuilder`, and
  desugaring lessons.
- `d8352797-0e72-45ce-b63a-6070634e6f22`: Sonatina observability planning.
- `8c1f6bad-5c1d-4278-9bb5-aa84535a3fcc` and
  `4774b549-6737-49ce-91f7-acc645d38189`: multi-backend/Sonatina context.
