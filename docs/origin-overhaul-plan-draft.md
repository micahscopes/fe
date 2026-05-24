# Origin Graph Instrumentation Overhaul Plan

Status: draft
Date: 2026-05-22

## Objective

Replace the current instrumentation prototype with a maintainable, typed,
Salsa-compatible origin architecture that supports source mapping, hashing,
Datalog facts, debug output, and CLI analysis without ad hoc side channels.

## Original Intent And Success Criteria

The reviewed creation sessions confirm that the goal was not just "emit more
analysis data." The goal was a trustworthy, queryable compilation record that
can support debug views, source-to-bytecode explanation, security queries,
structural comparison, and later verification work.

Success criteria:

- Source-to-bytecode tracing works without guessing across bodies.
- Optimized instructions are linked, synthetic, or explicitly unmapped.
- Hash reports explain which dimension changed and why.
- Security/query facts return exact expected answers on small oracle fixtures.
- At least one real or realistic bug can be explained by the origin/shape data.

## Principles

- Use "origin" terminology.
- Keep compiler queries pure. Queries return origin/hash data; exporters consume
  it later.
- Ban raw cross-phase IDs.
- Make phase ownership explicit.
- Generate boilerplate with strict derives where possible.
- Prefer invariant tests over existence tests.
- Treat the current instrumentation branch as a prototype, not the final
  architecture.
- Capture only irrecoverable relationships online; derive reports post-hoc.
- Reduce steady-state LoC by replacing duplicated visitors and side channels
  with one typed origin/shape core.

## Phase Model

The work should be split into phases, not framed around pull requests. Each
phase should be relatively complete in its own right:

- It introduces one durable concept or removes one prototype concept.
- It has a narrow gate that can be reviewed without understanding the whole
  overhaul.
- It leaves the compiler in a coherent state even if later phases are delayed.
- It deletes or quarantines obsolete boilerplate where practical.

## Implementation Checkpoint: 2026-05-22

The `origin-overhaul` worktree has landed the foundation through the first
bytecode-to-source resolution slice:

- Phase 2: owner-aware typed origin keys for HIR, MIR, Sonatina, and bytecode
  export keys. HIR expr/stmt origins now have downstream compile-fail coverage
  proving the nominal wrappers cannot cross API boundaries. HIR export helpers
  now require typed owner keys instead of raw strings, and semantic-origin
  export is gated by an explicit semantic owner-key marker trait.
- Phase 3: cached typed MIR runtime body/package origins. Runtime package body
  labels now enter through `RuntimePackageBodySymbol`, so package-origin
  construction cannot accept raw or empty runtime symbols at the public API
  boundary. `RuntimePackageOrigins::new` now owns deterministic body ordering
  and rejects duplicate runtime body symbols as well as duplicate runtime
  instances, keeping symbol-derived fact owner namespaces collision-free.
  Runtime terminator origins now carry a typed `RuntimeTerminatorSite` local
  key; the primary constructor no longer accepts a raw `RBlockId`, and codegen
  synthetic-local keys derive terminator labels through that typed site rather
  than through a public raw-string helper.
- Phase 4: `ShapeGraph` with separate local/tree/graph digests and dimension
  projections. Edge labels/topology affect structure, while endpoint
  content-dimension changes affect exact and their own dimensions without
  polluting the structure projection. Focused shape/hash tests now gate the
  child-content, full-edge-label, field-order, child-order,
  dimension-projection, and derive fail-closed invariants directly.
- Phase 5: fail-closed `ShapeDescribe` derive plus a real MIR const IR
  conversion. The derive path now has trybuild compile-fail coverage for
  missing field policies, empty skip reasons, multiple field policies, and
  unknown item/field attributes, so the fail-closed behavior is enforced at a
  downstream crate boundary.
- Phase 6: typed origin/shape/hash fact export with namespaced IDs.
  Runtime fact graph nodes now use nominal semantic-owner, runtime-owner, and
  synthetic-local key wrappers internally, with compile-fail coverage for owner
  namespace mixups. MIR runtime fact export callbacks now return a typed
  semantic/runtime owner-key bundle instead of raw owner strings. Owner-key
  derivation now lives in MIR through `RuntimeOriginFactTargetKey` plus
  `RuntimePackageBodySymbol`, so callers cannot format or swap namespaces before
  MIR builds the graph. Codegen end-to-end fact
  graph nodes use the same kind of nominal wrappers for semantic owners,
  runtime owners, and runtime synthetic locals, so cross-IR fact construction
  cannot swap those labels internally. Their semantic/runtime owner-key bundle
  is derived from a typed `SonatinaFunctionExportKey`, so graph builders do not
  repeatedly format owner namespaces from raw strings. Codegen fact export now
  centralizes stable Sonatina function-key collection behind one internal map
  used by both codegen-only and end-to-end graphs, with focused coverage that a
  repeated function node resolves its stable key once.
  Sonatina stable function export keys are also nominal now, so backend fact
  and frontend-origin-label callbacks cannot hand raw strings to instruction
  export-key construction. `FrontendOriginLabelMap` is a Fe-owned wrapper over
  Sonatina's external frontend-provenance map rather than a public type alias,
  so the dependency-boundary "provenance" shape cannot leak through origin APIs.
  Frontend labels inserted into that map are nominal `FrontendOriginLabel`
  values derived from typed origin export keys, not arbitrary raw strings.
  Runtime statement/terminator export helpers are similarly gated by a runtime
  owner-key marker trait, so raw owner strings cannot bypass the graph-node
  wrappers.
  The repeated string-key and owner-key wrapper shapes are now generated
  through shared `define_origin_string_key!` and `define_origin_owner_key!`
  helpers to keep the nominal-type pattern sustainable. Generated wrappers
  reject empty strings and the reserved origin storage separator before those
  labels reach export-key construction. Generated wrappers derive
  `salsa::Update`. Closed string enums for origin kinds, link kinds, fact
  namespaces, shape dimensions/scopes, and codegen debug reasons now use a
  shared `define_closed_string_enum!` helper, replacing repeated
  `STRINGS`/`as_str`/`from_str`/Serde implementations while preserving the
  existing wire strings and unknown-variant failures. The
  remaining manual `salsa::Update` impls for generic origin key/link/graph
  containers have explicit safety notes plus regression coverage that they
  satisfy the Salsa update contract, including no-op precision and changed
  fieldwise updates.
  Public HIR, MIR, and codegen origin graph types now use nominal graph wrappers
  generated by a shared `define_origin_graph_type!` helper instead of exposing
  raw `OriginGraph<Node>` aliases. Codegen has downstream compile-fail coverage
  proving fact export rejects a raw graph at the public API boundary.
- Phase 2/5 type-boundary cleanup slice: public HIR, MIR, and codegen origin
  wrappers no longer expose raw `OriginKey<Owner, Local>` values through
  `.key()` methods. Export and debug consumers must stay on nominal origin
  wrappers plus typed stable export-key helpers. HIR, MIR, and codegen now have
  downstream compile-fail coverage that origin wrappers cannot be deconstructed
  into raw origin keys. MIR also has compile-fail coverage that a runtime
  terminator origin cannot be constructed from a raw block ID without first
  forming a `RuntimeTerminatorSite`. A follow-up cleanup removed public
  inherent raw-local-key string helpers from semantic origins, runtime sites,
  and bytecode PC ranges; downstream compile-fail coverage now keeps those
  APIs on typed origin/local-key boundaries.
- Phase 5/9 bytecode observability invariant slice: `BytecodePackageOrigins`
  now rejects malformed Sonatina PC-map rows with empty or inverted PC ranges
  instead of silently dropping them before coverage, source-map, and fact export.
  This keeps bytecode-origin coverage aligned with emitted observability rows:
  each valid PC-map row becomes a typed PC origin classified as post-opt,
  backend-prepared, or explicitly bytecode-unmapped.
- Phase 2/6 export-key boundary slice: `OriginExportKey` now owns its
  validation and string projections. Owner/local parts must be non-empty and
  cannot contain the reserved storage separator; deserialization enforces the
  same rules. Fact ID allocation uses the canonical storage key, while frontend
  provenance labels use the display label, so formatting policy is no longer
  duplicated in facts and codegen. Its public `new`/`try_new` constructors now
  require typed owner-key and local-key traits, with raw strings limited to the
  explicit `try_from_raw_parts` import/serde boundary. A shared
  `define_origin_local_key!` helper keeps local-key wrappers as cheap to define
  as owner-key wrappers, and `fe-common` compile-fail coverage blocks raw-string
  construction through the typed constructor.
- Phase 7: pre-opt MIR-to-Sonatina instruction origins, including prologue,
  statements, terminators, and explicit unmapped classification. Pre-opt
  Sonatina function-origin bundles now construct their own owner/stage-aware
  instruction origins instead of accepting arbitrary prebuilt instruction keys.
- Phase 8 first slice: phase-aware Sonatina instruction origins and
  observability-backed bytecode PC origins. Bytecode ranges now join from
  post-opt/codegen-prepared Sonatina instruction references, not directly from
  pre-opt IDs, and PC identity includes object plus section. Bytecode object
  selection now uses a nominal `BytecodeObjectKey`, and bytecode section
  identity uses `BytecodeSectionNameKey`, so internal origin/fact graph filters
  cannot accept loose object or section-name strings.
- Phase 8 second slice: a pure `BytecodePackageOrigins` source resolver joins
  bytecode PC origins through post-opt Sonatina, pre-opt Sonatina, MIR runtime
  origins, semantic origins, and LazySpan source spans. It returns a classified
  result per bytecode record, preserving non-source cases such as synthetic,
  unmapped, post-preopt snapshot gap, missing-runtime-origin, and
  missing-semantic-span cases instead of silently dropping them. The public
  source-resolution DTOs and resolver helpers now live in
  `crates/codegen/src/origin/source_resolution.rs`, separating source-span
  resolution policy from origin identity, graph construction, and fact export.
- Phase 7/8 codegen-origin module cleanup slice: the Sonatina and bytecode
  origin implementation is now split by responsibility. Pre-opt lowering
  records live in `crates/codegen/src/origin/sonatina_pre_opt.rs`; optimized
  snapshot, backend-prepared, and pre-opt snapshot-loss records live in
  `crates/codegen/src/origin/sonatina_post_opt.rs`;
  `bytecode_origins.rs` owns PC-map ingestion, source-resolution entry points,
  object/section filtering, and package orchestration;
  `bytecode_coverage.rs` owns bytecode-origin coverage counting;
  `bytecode_graph.rs` owns bytecode fact graph projection; and
  `frontend_labels.rs` owns frontend-origin labels plus pre-opt source label
  classification. The parent `crates/codegen/src/origin.rs` is now a compact
  facade that preserves the public `codegen::origin` API surface.
- Resolver tests include a multi-test-body fixture that requires mappings from
  two similar bodies to resolve to their own source snippets.
- Phase 8 third slice: `SonatinaPostOptPackageOrigins` classifies every
  instruction still present in the optimized Sonatina module before bytecode
  emission. Bytecode PC origins now consume that post-opt bundle; PC entries for
  backend-prepared/codegen-only instruction IDs that are not in the optimized
  snapshot are explicitly classified `post_preopt_snapshot_gap`. Same-`InstId`
  pre/post joins are snapshot `alias` edges, not pass-lineage transform events.
- Phase 8 backend-prepared cleanup slice: bytecode PC-map entries that reference
  instruction IDs missing from the optimized Sonatina snapshot no longer create
  fake `post_opt` instruction origins. They now use a distinct
  `backend_prepared` Sonatina instruction stage linked from the
  `post_preopt_snapshot_gap` synthetic node before lowering to bytecode PC
  ranges. `BytecodePackageOrigins::coverage` counts post-opt,
  backend-prepared, and unmapped PC classifications so callers can check that
  every PC origin record has exactly one source classification.
- Phase 8 bytecode-origin determinism slice: `BytecodePackageOrigins` now sorts
  records by object, section, and PC range after consuming Sonatina artifacts,
  so source maps, facts, and coverage consumers do not inherit artifact or
  `IndexMap` traversal order.
- Phase 8 bytecode-PC integrity slice: the same constructor now rejects
  overlapping bytecode PC ranges within a single object section while allowing
  adjacent half-open ranges. This keeps source maps and bytecode-origin facts
  from carrying two competing source classifications for the same byte.
- Phase 8/11 bytecode-origin error-boundary slice:
  `BytecodePackageOrigins::try_from_artifacts` now returns typed validation
  errors for empty, inverted, or overlapping PC-map ranges. Origin-backed
  Sonatina compilation propagates those errors through `LowerError` instead of
  panicking after object emission, while `from_artifacts` remains the trusted
  invariant-preserving convenience wrapper.
- Phase 8/10 coverage-report slice: codegen now carries bytecode-origin
  coverage through runtime bytecode outputs and test metadata, and
  `fe analyze --source-maps` reports that coverage beside source-map summaries.
  Runtime and test analyze regressions require the coverage to partition PC
  origin records across post-opt, backend-prepared, and unmapped sources.
- Phase 8/9/10 coverage-artifact slice: source-map JSON artifacts now carry
  optional `bytecode_origin_coverage` beside their typed PC rows. Build and test
  report regressions verify the field is emitted from typed coverage data rather
  than recomputed from rendered JSON.
- Phase 8 coverage-invariant cleanup slice: `BytecodeOriginCoverage` is now
  constructor-backed in `crates/codegen/src/origin/bytecode_coverage.rs` with
  private count fields and partition getters, so callers cannot create
  mismatched totals with public struct literals.
- Phase 8 fourth slice: the post-opt package origin bundle now also records
  conservative pre-opt snapshot losses for pre-opt instructions that no longer
  have a same-`InstId` post-opt match. This does not infer deletion vs
  replacement vs merge/split intent, but it makes the missing half of the
  pre/post snapshot diff explicit and testable.
- Phase 8/6 snapshot-fact slice: `SonatinaPostOptPackageOrigins` now projects
  the snapshot diff into a typed `CodegenOriginGraph` and versioned fact export.
  Same-ID survivors are `alias` links; pre-opt losses link to an explicit
  `pre_opt_snapshot_loss` synthetic node with `synthetic`, not `transformed`,
  edges so consumers cannot mistake conservative snapshot loss for precise pass
  lineage.
- Phase 8 same-ID invariant slice: `SonatinaPostOptOriginRecord` now rejects
  `SameInstId` sources whose pre-opt record has a different function or
  instruction ID than the post-opt origin. This keeps snapshot alias edges from
  accidentally becoming cross-function or cross-instruction pass-lineage claims.
- Phase 8/9/10 snapshot-export slice: build reports and test reports now keep
  snapshot-diff facts separate from end-to-end bytecode facts. Build reports
  emit `<contract>.snapshot_origin_facts.json`; test reports emit
  `artifacts/tests/<test>/sonatina/snapshot_origin_facts.json`; `fe analyze
  --tests --origin-facts` exposes them as `test_sonatina_snapshot` reports.
- Phase 8/10 post-opt coverage slice: codegen metadata and
  `fe analyze --source-maps` now expose object/section-filtered
  `SonatinaPostOptOriginCoverage` beside bytecode-origin coverage. The report
  distinguishes optimized-snapshot same-ID aliases, post-preopt created or
  unmatched instructions, and conservative pre-opt snapshot losses without
  claiming precise pass lineage.
- Phase 8/9/10 post-opt coverage artifact slice: source-map JSON artifacts now
  carry optional `post_opt_origin_coverage` beside `bytecode_origin_coverage`,
  using the same export DTO that analyze reports use. The source-map artifact
  schema is bumped to `schema_version: 3`, and decoding rejects unknown or
  inconsistent post-opt coverage fields.
- Phase 9 first slice: `bytecode_source_map_json` renders the typed bytecode
  source-resolution data as a boundary artifact for test reports. It includes
  object and section ownership for every PC range, emits source spans when
  available, and preserves explicit non-source classifications instead of
  inventing source locations.
- Phase 9 source-map schema slice: source-map JSON now has an owned
  `OwnedBytecodeSourceMapExport` boundary DTO with `Deserialize` support and
  round-trip coverage. Deserialization rejects unsupported schema versions, so
  unknown schema versions, export fields, entry/source fields, and nested
  coverage fields now fail closed instead of leaving artifacts as write-only or
  permissive JSON.
- Phase 9 coverage-schema invariant slice: source-map JSON decoding now rejects
  `bytecode_origin_coverage` whose classification counts do not sum to `total`,
  or whose `total` does not match the number of exported PC rows.
- Phase 9 coverage-export boundary slice: downstream compile-fail coverage now
  proves `BytecodeOriginCoverageExport` and
  `SonatinaPostOptOriginCoverageExport` cannot be constructed with struct
  literals, so source-map coverage DTOs stay tied to typed coverage conversion
  or fail-closed JSON decoding.
- Phase 7/9 coverage API cleanup slice: Sonatina pre-opt and post-opt coverage
  counters now mirror bytecode coverage with private fields, constructor-derived
  totals, partition helpers, and getter-only callers. This removes another
  public-counter path where report totals could drift from classifications.
- Phase 9 source-map entry invariant slice: `BytecodeSourceMapEntry` is now
  constructor-backed with private fields. JSON deserialization rejects empty or
  inverted PC ranges, and downstream compile-fail coverage rejects struct
  literals that bypass the constructor.
- Phase 9 source-map constructor invariant slice: public
  source-map row construction now has a fallible
  `BytecodeSourceMapEntry::try_from_origin` path for callers that need
  structured errors and an infallible `BytecodeSourceMapEntry::from_origin`
  convenience wrapper for invariant-preserving producers. Both validate
  source-row semantic fields at construction time, including non-empty source
  file/snippet strings and ordered source byte/line-column ranges. JSON decoding
  and export construction use the same shared kind validator, removing the
  duplicated source-row validation path.
- Phase 9 debug-snippet slice: source-map source entries now carry a required
  snippet derived from the same typed source span as the file/range fields.
  Source-map artifact schema version 2 made snippets required; decoding rejects
  missing or empty snippets and empty source file labels, and
  `fe analyze --source-map-entries` tests require non-empty snippets for real
  runtime and test bytecode mappings.
- Phase 9 debug-location slice: codegen now exports a typed
  `debug_locations.json` boundary derived from validated
  `BytecodeSourceMapEntry` rows. The artifact contains only real source
  PC-range mappings and skips synthetic/unmapped/non-source classifications
  instead of inventing source spans. Build reports and Fe test reports write it
  beside `source_map.json`, giving future DWARF/ethdebug exporters a typed
  `PcRange -> SourceSpan` input before those format-specific emitters are
  rebuilt.
- Phase 9 debug-location schema slice: `debug_locations.json` now has an owned
  validating decoder. Deserialization rejects unsupported schema versions,
  unknown export/location fields, empty location lists, empty object/section
  metadata, metadata mismatches, empty or overlapping PC ranges, and invalid
  source file/snippet or byte/line-column ranges. This keeps the compact
  `PcRange -> SourceSpan` artifact fail-closed before DWARF/ethdebug emitters
  consume it.
- Phase 9/10 debug-location report-boundary slice: build-report and Fe
  test-report regressions now deserialize emitted `debug_locations.json`
  artifacts through `OwnedBytecodeDebugLocationExport`, so the generated
  artifacts are checked against the same fail-closed schema boundary that later
  debug emitters will consume.
- Phase 9 debug-location type-boundary slice: downstream compile-fail coverage
  now proves `BytecodeDebugLocationEntry` cannot be constructed with public
  struct literals. Producers must derive it from validated source-map rows or
  go through the owned JSON decoder.
- Phase 9/10 debug-location error-boundary slice: build-report and Fe
  test-report regressions now cover `debug_locations.json` write failures
  after source-map emission succeeds, so compact debug artifact I/O errors are
  surfaced with the failing artifact path instead of being silently skipped.
- Phase 9/10 analyze debug-location summary slice: `fe analyze --source-maps`
  now exposes `debug_locations` beside source/non-source bytecode PC counts.
  The value is derived from `BytecodeSourceMapSummary::debug_locations()`, which
  is the number of real source PC ranges that can become compact
  `debug_locations.json` rows for DWARF/ethdebug consumers.
- Phase 9/10/11 debug-artifact cleanup slice: codegen now owns a typed
  `BytecodeDebugArtifactsExport` bundle plus a `BytecodeDebugArtifactsJson`
  renderer layered above it. Future DWARF/ethdebug emitters can consume the
  typed source-map/debug-location exports directly, while build and Fe test
  report writers consume the JSON bundle instead of duplicating render-order
  and option policy. They still write each artifact separately so path-specific
  I/O failures remain visible.
- Phase 9/10/11 debug-artifact metadata invariant slice: bundled debug
  artifacts now reject mismatched source-map and debug-location metadata, so a
  single artifact set cannot silently mix object-scoped source maps with
  section-scoped compact debug records.
- Phase 9/10/11 debug-artifact filename cleanup slice: codegen now owns
  `BytecodeDebugArtifactKind` and `BytecodeDebugArtifactsJson::artifacts()`,
  which define the source-map, debug-location, and debug-line-table artifact
  order plus filenames. Build and Fe test report writers iterate that typed
  artifact list instead of hard-coding each debug artifact name.
- Phase 9 debug-line-table slice: codegen now derives a
  versioned `debug_line_table.json` artifact from the owned debug-location
  export. The line table interns source files and exposes validated PC/source
  rows without choosing a DWARF or ethdebug encoding. Build and Fe test reports
  emit it beside `source_map.json` and `debug_locations.json`, and focused
  regressions parse it with `OwnedBytecodeDebugLineTableExport` plus cover
  path-specific write failures.
- Phase 9 debug-line-table type-boundary slice: downstream compile-fail
  coverage now proves public callers cannot construct unchecked source-file,
  line-row, or owned line-table export structs with literals. Line-table data
  must come from the debug-location-derived builder or fail-closed decoder.
- Phase 9 debug-line-record view slice: codegen now exposes `line_records()`
  over both in-memory and owned debug-line-table exports. The iterator resolves
  interned file indices into direct `PC range -> source file/range` records, so
  future DWARF/ethdebug emitters do not need to duplicate file-index joins or
  reinterpret the line-table schema.
- Phase 9/10 debug-line-table analyze slice: `BytecodeSourceMapSummary` now
  computes debug-line-table file and row counts from typed source-map rows, and
  `fe analyze --source-maps` exposes `debug_line_table_files` and
  `debug_line_table_rows` in JSON and text output. The CLI report stays derived
  from compiler-owned typed data and does not parse `debug_line_table.json`.
- Phase 9 debug-export test-facade cleanup slice: codegen debug/source-map
  regressions now live behind the `crates/codegen/src/debug/tests.rs` facade,
  with focused `crates/codegen/src/debug/tests/` modules for source-map JSON,
  source-map export/entry construction, debug locations, debug artifacts,
  debug line tables, and source-map summaries. This leaves the parent
  `debug.rs` focused on the public facade and source-map/source-span
  orchestration rather than embedding thousands of lines of fixtures.
- Phase 9 coverage DTO cleanup slice: `BytecodeOriginCoverageExport` and
  `SonatinaPostOptOriginCoverageExport` now live in
  `crates/codegen/src/debug/coverage.rs`, keeping coverage partition and
  observed-pre-opt validation separate from source-map/debug artifact assembly
  while preserving public `codegen::debug` re-export paths.
- Phase 9 source-map options cleanup slice: `BytecodeSourceMapFilter`,
  `BytecodeSourceMapExportMetadata`, and `BytecodeSourceMapExportOptions` now
  live in `crates/codegen/src/debug/source_map_options.rs`, keeping optional
  export policy out of the main debug/source-map DTO module while preserving
  public `codegen::debug` re-export paths.
- Phase 9 source-map entry cleanup slice: `BytecodeSourceMapEntry`,
  `BytecodeSourceMapEntryKind`, `SourceSpanInvalidReason`, and row-level JSON
  decoding/validation now live in
  `crates/codegen/src/debug/source_map_entry.rs`. The parent debug module keeps
  the existing public re-exports, while row schema and closed reason handling
  are isolated from artifact/export assembly.
- Phase 9 source-map export cleanup slice: `OwnedBytecodeSourceMapExport`,
  source-map export errors, source-map JSON/export helpers, and shared
  PC-range/metadata validation now live in
  `crates/codegen/src/debug/source_map_export.rs`. Debug-location, line-table,
  artifact, and summary modules keep using the same parent-module helper names.
- Phase 9 source-span conversion cleanup slice: source-map row/source-span fact
  conversion now lives in `crates/codegen/src/debug/source_spans.rs`, keeping
  snippet validation and line/column indexing separate from schema DTOs and
  artifact assembly while preserving the existing public debug API.
- Phase 9 source-map summary cleanup slice: `BytecodeSourceMapSummary` and
  `bytecode_source_map_entries_summary` now live in
  `crates/codegen/src/debug/source_map_summary.rs`. The parent debug module
  keeps public re-exports while summary/counting policy is isolated from
  source-map row/schema validation and artifact assembly.
- Phase 9 debug-line-table cleanup slice: `BytecodeDebugSourceFile`,
  `BytecodeDebugLineRow`, `BytecodeDebugLineRecord`,
  `BytecodeDebugLineTable`, and `OwnedBytecodeDebugLineTableExport` now live in
  `crates/codegen/src/debug/line_table.rs`. The module owns line-table
  interning, fail-closed decoding, and file-index validation while the parent
  debug module preserves public re-export paths.
- Phase 9 debug-location cleanup slice: `BytecodeDebugLocationEntry` and
  `OwnedBytecodeDebugLocationExport` now live in
  `crates/codegen/src/debug/location.rs`. The module owns compact debug-location
  row construction, fail-closed decoding, and export validation while the
  parent debug module preserves public re-export paths.
- Phase 9 debug-artifact orchestration cleanup slice: debug artifact bundle
  DTOs, artifact filename policy, metadata mismatch errors, and JSON rendering
  now live in `crates/codegen/src/debug/artifacts.rs`. Build/test writers keep
  using stable `codegen::debug` re-exports, but artifact assembly is no longer
  embedded in the source-map row/schema module.
- Phase 9 invalid source-span slice: source-map entry construction no longer
  panics when a resolved source span cannot produce a valid Rust string slice.
  Invalid byte ranges, invalid UTF-8/snippet ranges, and empty snippets are
  classified as `source_span_invalid` rows with closed reason strings, and
  summaries/analyze reports count them as explicit non-source entries. The
  `source_span` fact exporter shares the same validation so invalid spans do
  not appear as valid source-span fact rows.
- Phase 9 source-map semantic-boundary slice: decoded source-map JSON now
  rejects unknown source span kinds, inverted source byte/line-column ranges,
  empty source file/snippet strings, export object/section metadata that does
  not match entry rows, and overlapping PC ranges within one object section.
  This mirrors the typed source-span and bytecode-origin invariants at the
  serialized debug boundary.
- Phase 9 source-map reason-boundary slice: decoded source-map JSON now rejects
  unknown non-source reason strings for Sonatina synthetic, Sonatina unmapped,
  and bytecode unmapped rows, keeping exported debug classifications aligned
  with the closed origin enums.
- Phase 9 source-map type-boundary slice: `BytecodeSourceMapEntryKind` now uses
  typed `SourceSpanKind`, `SonatinaSyntheticOrigin`, `SonatinaUnmappedReason`,
  and `BytecodeUnmappedReason` fields internally while preserving the string
  JSON schema. A compile-fail test blocks raw-string construction of these
  closed classifications.
- Phase 9 source-map entry-construction slice: public source-map entry
  construction now goes through `BytecodePcOrigin` via
  `BytecodeSourceMapEntry::try_from_origin` or
  `BytecodeSourceMapEntry::from_origin`; raw object/section/PC tuple
  construction is kept only inside JSON deserialization and is covered by a
  compile-fail regression. Malformed source rows are rejected by the constructor
  before report/export serialization can see them, and non-invariant callers can
  keep those failures as typed errors instead of panics.
- Phase 9 source-map export-construction slice: source-map export builders now
  validate object/section metadata, per-section PC range overlap, and coverage
  entry counts before serialization. Invalid artifacts fail on the writer side
  instead of relying on a later JSON decode to catch them.
- Phase 9 schema-test cleanup slice: public Sonatina test metadata coverage now
  checks source-map JSON against `OwnedBytecodeSourceMapExport::SCHEMA_VERSION`
  instead of a stale literal, so test coverage tracks intentional schema bumps.
- Phase 9/10 report source-map decode cleanup slice: build-report and Fe
  test-report regressions now deserialize emitted `source_map.json` artifacts
  through `OwnedBytecodeSourceMapExport`, so report writers are checked against
  the same fail-closed schema boundary as downstream source-map consumers.
- Phase 10 first slice: `fe analyze` is restored as a small typed-origin
  boundary over cached runtime-origin data. It uses normal CLI target
  resolution, supports ingot/workspace member selection, respects compilation
  profile and recovery options, and emits text or JSON summaries without the
  prototype temp-file source path.
- Phase 10 second slice: `fe analyze --tests` summarizes Fe test runtime
  packages as the same typed origin data, so test-only ingots no longer appear
  empty when analyzing runtime origins.
- Phase 10 third slice: `fe analyze --tests --source-maps` consumes typed
  bytecode source-map summaries from codegen and reports source/non-source PC
  classification counts without parsing the JSON boundary artifact.
- Phase 10 fourth slice: `fe analyze --tests --source-maps
  --source-map-entries` exposes the typed bytecode PC rows from codegen on
  demand. Summaries stay compact by default; full rows include object, section,
  PC range, source span fields when present, and explicit non-source reasons.
- Phase 10 runtime debug slice: `fe analyze --source-maps` no longer requires
  `--tests`. For regular runtime packages, analyze now reuses the typed
  Sonatina bytecode path to report `runtime_bytecode` source-map summaries and
  opt-in full PC rows. When combined with `--origin-facts`, the same runtime
  codegen analysis adds `runtime_bytecode` end-to-end origin facts and
  `runtime_sonatina_snapshot` facts alongside the existing semantic-to-MIR
  runtime facts.
- Phase 10 cleanup slice: source-map JSON artifacts and analyze full-entry
  reports now serialize the same typed `BytecodeSourceMapEntry` rows, removing
  a hand-written JSON encoder and a duplicate analyze DTO. The JSON artifact
  is now also decoded by the owned source-map export schema in focused codegen
  tests.
- Phase 9/10 source-map summary cleanup slice: typed source-map summary
  counting for owned `BytecodeSourceMapEntry` rows is centralized in
  `codegen::debug::bytecode_source_map_entries_summary`. Runtime analyze
  reports now consume the shared codegen summary policy instead of matching
  every source-map entry kind in the CLI layer. The old resolution-only summary
  path is removed so summaries cannot count an invalid source-span resolution
  as a valid source row before entry validation runs.
- Phase 9/10 source-map summary metadata cleanup slice: the public source-map
  summary API now accepts typed `BytecodeSourceMapExportMetadata` instead of
  loose object/section string filters. Section-scoped Sonatina summaries pass
  the same metadata object used for source-map export, and downstream
  compile-fail coverage rejects raw filter strings.
- Phase 9/10 analyze source-map cleanup slice: analyze source-map reports now
  reuse the same `BytecodeOriginCoverageExport` and
  `SonatinaPostOptOriginCoverageExport` DTOs as source-map artifacts, and report
  construction funnels all summary count fields through one constructor instead
  of duplicating the mapping for test and runtime bytecode.
- Phase 9/10 source-map text-view slice: `fe analyze --source-maps` text output
  now prints the full typed non-source classification breakdown. When
  `--source-map-entries` is requested, text output also renders the typed PC
  rows with object, section, PC range, entry kind, source span fields, compact
  snippets, and explicit non-source reasons, so the flag is meaningful outside
  JSON mode.
- Phase 9/10 source-map export-options cleanup slice: source-map JSON/export
  rendering now takes `BytecodeSourceMapExportOptions` instead of growing
  parallel `*_with_<field>` helper functions whenever the boundary schema gains
  a field. Writer-side export metadata is typed through
  `BytecodeSourceMapExportMetadata`, which accepts either a
  `BytecodeObjectKey` for object-level reports or a `BytecodeSectionKey` for
  section-scoped source maps. `BytecodeSectionKey` itself is built from
  `BytecodeObjectKey` plus `BytecodeSectionNameKey`; raw object/section strings
  are confined to JSON deserialization and CLI/report DTO boundaries.
- Phase 9/10 cleanup slice: source-map filtering now takes a typed
  `BytecodeSectionKey` instead of loose object/section strings. A downstream
  compile-fail test rejects raw-string construction of the public filter and
  export metadata APIs.
  The remaining `String` fields in source-map entries and analyze/build reports
  are serialization and CLI artifact boundaries, not internal identity joins.
- Phase 8/9 bytecode section-key invariant slice: `BytecodeSectionKey` now
  requires typed object and section-name keys, with downstream compile-fail
  coverage rejecting raw strings for both parts. Source-map JSON decoding mirrors
  the non-empty invariant for serialized entry rows and optional export metadata
  `object`/`section` fields.
- Phase 10 cleanup slice: analyze option plumbing is centralized in
  `AnalyzeOptions` so future shape/fact/hash views do not add more parallel
  boolean/opt-level parameter chains through every target helper.
- Phase 10 cleanup slice: test metadata now carries typed source-map entries
  and summaries only; report JSON is rendered at the artifact boundary instead
  of being stored beside the typed data.
- A focused report-boundary regression test now verifies that
  `artifacts/tests/<test>/sonatina/source_map.json` is rendered from typed
  entries.
- Phase 11 terminology cleanup slice: codegen internals now call the labels
  attached to Sonatina observability data `frontend_origin_labels`. The
  `frontend_provenance` spelling remains only at the Sonatina external API/JSON
  boundary, and Fe-owned code wraps Sonatina's map type in
  `FrontendOriginLabelMap`.
- Phase 11 closed-enum cleanup slice: the shared closed-string enum helper now
  also owns `Display`. Analyze package kinds, Sonatina instruction stages, and
  conservative pre-opt snapshot-loss reasons use that helper, reducing bespoke
  `as_str`/serde/display boilerplate while keeping wire strings unchanged.
- Phase 9/10 runtime export slice: `fe build --report` now emits
  `artifacts/<scope>/<contract>.source_map.json` for non-test contract/runtime
  bytecode. The report path uses origin-backed Sonatina bytecode compilation and
  typed `BytecodeSourceMapEntry` rows; normal non-report bytecode builds keep
  the cheaper bytecode-only path.
- Phase 9/10/11 report-boundary cleanup slice: build report copies for ABI,
  IR, bytecode, source maps, end-to-end origin facts, and snapshot-diff origin
  facts now surface write failures instead of silently dropping report
  artifacts. Test report Sonatina artifacts now do the same for initcode,
  runtime bytecode, observability JSON, source maps, end-to-end origin facts,
  and snapshot-diff origin facts. Suite-level runtime/Sonatina debug report
  artifacts now also return write failures instead of silently dropping package,
  IR, optimized IR, or validation outputs. Focused regressions block the
  source-map and suite debug report paths to ensure typed debug artifacts cannot
  fail invisibly at the boundary.
- Phase 9/10/11 report-boundary serialization cleanup slice: source-map JSON
  renderers now return serialization errors, and build/test report paths
  propagate source-map, end-to-end origin fact, and snapshot-diff fact JSON
  serialization failures through their existing artifact `Result` paths instead
  of panicking at the report boundary.
- Phase 6/9/10 fact export slice: backend origin graphs now have stable export
  keys for Sonatina instructions, Sonatina synthetic nodes, bytecode unmapped
  reasons, and bytecode PC ranges. `fe build --report` emits
  `artifacts/<scope>/<contract>.origin_facts.json` from the typed
  `BytecodePackageOrigins` graph, wrapped in a versioned typed-fact JSON schema.
  The artifact is derived at the report boundary and does not require Salsa
  side effects. The same report path also emits
  `artifacts/<scope>/<contract>.snapshot_origin_facts.json` for conservative
  Sonatina pre/post snapshot-diff facts.
- Phase 3/8 duplicate-origin invariant slice: runtime body/package origin
  bundles and post-opt Sonatina function origin bundles now reject duplicate
  statement, terminator, runtime instance, runtime body symbol, and instruction
  identities. This prevents first-match lookup APIs and symbol-derived owner
  namespaces from hiding ambiguous origin data.
- Phase 3/11 MIR origin test-facade cleanup slice: runtime-origin regressions
  now live in `crates/mir/src/origin/tests.rs`, leaving `mir::origin` focused
  on runtime statement/terminator identity, package origin bundles, and
  semantic-to-runtime fact projection while preserving the existing test paths.
- Phase 3/6/11 MIR origin module cleanup slice: the production MIR origin
  implementation is now split by responsibility. `runtime_identity.rs` owns
  statement/terminator/code-region identity and export-key helpers;
  `package.rs` owns `RuntimeBodyOrigins`, `RuntimePackageOrigins`, and the
  cached `runtime_package_origins` query; `fact_graph.rs` owns runtime
  semantic-to-MIR fact graph construction and typed owner-key policy. The parent
  `mir::origin` module remains a compact re-export facade with the public
  `RuntimeOriginNode` graph wrapper.
- Phase 6/9/10 test/analyze fact slice: Fe test Sonatina metadata now carries
  typed origin facts, test reports render
  `artifacts/tests/<test>/sonatina/origin_facts.json`, and
  `fe analyze --origin-facts` exposes versioned runtime origin facts for regular
  runtime packages. With `--tests`, analyze also includes test-bytecode origin
  facts in JSON plus compact counts in text.
- Phase 4/5/6/10 shape/hash slice: `fe analyze --shape-hashes` now reports
  graph hash dimensions for runtime const-region shapes, and
  `--shape-facts` includes the versioned typed shape facts. This intentionally
  starts with the IR family already migrated to `ShapeDescribe` policy rather
  than adding another manual traversal. Text reports now render every graph
  hash dimension instead of collapsing the human-readable view to structure.
- Phase 4 shape-hash module cleanup slice: deterministic shape hashing now
  lives in `crates/common/src/shape/hash.rs`. `common::shape` keeps the public
  re-export surface stable, while `shape.rs` is narrowed toward graph identity,
  builder/describer APIs, field-value conversion, and derive-facing tests.
- Phase 4 shape module cleanup slice: graph identity/types now live in
  `crates/common/src/shape/graph.rs`, builder and `ShapeDescribe` APIs in
  `shape/describe.rs`, and field-value formatting in `shape/field_value.rs`.
  `shape.rs` remains a compact facade with derive-facing tests and stable
  `common::shape` re-exports.
- Phase 5 derive cleanup slice: `ShapeDescribe` now fails closed on duplicate
  item/variant `kind`, duplicate `stable_key`, and duplicate field `label`
  attributes instead of silently letting later metadata win. Macro unit tests
  and trybuild coverage both enforce the public failure mode.
- Phase 5 derive invariant slice: `ShapeDescribe` now rejects empty
  item/variant `kind` and field `label` strings during macro parsing. This
  aligns generated shape descriptions with `ShapeGraph` and relation-table
  invariants that require non-empty node kinds and labels.
- Phase 5 runtime IR derive slice: `ShapeDescribe` now covers core MIR runtime
  type and instruction families, including runtime classes, scalar
  representations, layouts, local/block IDs, places, builtins, expressions,
  statements, and terminators. The slice adds tuple shape support for switch
  case rows and focused MIR regressions proving derived runtime shapes observe
  scalar type dimensions, address-space structure, child expression content,
  operators, operands, and switch-case constants. Db-backed references
  (`LayoutId`, `TyId`, `RuntimeInstance`, code regions, const regions) are
  explicitly classified as typed reference fields; resolving those into full
  structural shapes remains a later db-aware shape-policy step.
- Phase 5/10 runtime shape analysis slice: `fe analyze --shape-hashes` and
  `--shape-facts` now include compact `runtime_body` shape reports built from
  derived runtime block/statement/terminator shapes, alongside the existing
  const-region reports. The focused analyze regression now requires both
  `const_region` and `runtime_body` shape fact/hash exports.
- Phase 5 derive policy cleanup slice: `ShapeDescribe` now rejects unknown
  dimensions during macro parsing instead of relying on generated-code errors.
  Runtime shape tests prove declared struct and enum variant stable-key
  policies reach the generated `ShapeGraph`; identity-only fields in those
  fixtures are still explicitly skipped with a reason.
- Phase 2/5 origin-key boilerplate slice: `common::define_origin_key_type!`
  now generates the repeated private `OriginKey<Owner, Local>` wrapper pattern.
  HIR expr/stmt/semantic origins and MIR runtime stmt/terminator origins use
  the macro, keeping owner/local accessor APIs and `salsa::Update` derives while
  removing hand-written wrapper boilerplate from the most regular origin types.
- Phase 6/10 cleanup slice: runtime semantic-to-MIR origin fact projection is
  owned by `mir::origin`; `fe analyze` supplies a typed
  `RuntimeOriginFactTargetKey`, MIR combines it with each
  `RuntimePackageBodySymbol`, and analyze renders the returned typed fact set.
- Phase 6/9 cleanup slice: bytecode origin fact projection is owned by
  `codegen::origin`, with checked stable Sonatina function-key export instead
  of a Sonatina-module-local helper that panics on missing keys.
- Phase 6/9 end-to-end owner cleanup slice: codegen end-to-end runtime and
  semantic owner keys are now bundled by `EndToEndOriginOwnerKeys::for_function`,
  which accepts only a typed `SonatinaFunctionExportKey`. Downstream compile-fail
  coverage rejects raw function labels at that derivation boundary.
- Phase 6/9 dependency-boundary slice: `FrontendOriginLabelMap` is now a
  nominal Fe wrapper over Sonatina's `FrontendProvenanceMap`, with a single
  adapter method at the observability boundary and compile-fail coverage against
  passing it as the raw Sonatina map. The map insertion API now requires a
  nominal `FrontendOriginLabel`, so external callers cannot hand raw strings
  into Sonatina frontend-provenance metadata.
- Phase 6/9 fact-export cleanup slice: `common::facts` now exposes
  `try_origin_graph_facts` for fallible stable-key projection. Codegen and
  end-to-end origin fact exports use it to propagate
  `MissingSonatinaFunctionKey` through fact construction instead of relying on
  panic-backed "validated before export" closures.
- Phase 6/9 function-key cleanup slice: stable Sonatina function-key collection
  now lives in `crates/codegen/src/origin/function_keys.rs`. That module owns
  the internal typed map, collection helper, and `MissingSonatinaFunctionKey`
  error, while `codegen::origin` keeps re-exporting the public error type.
- Phase 6/9 graph-module cleanup slice: codegen-only graph/fact export now lives
  in `crates/codegen/src/origin/codegen_graph.rs`, and end-to-end graph/fact
  export now lives in `crates/codegen/src/origin/end_to_end_graph.rs`. Public
  callers keep the same `codegen::origin` re-export paths, while package
  construction reaches back through narrow internal helper hooks.
- Phase 6/8 bytecode identity cleanup slice: bytecode object, section, PC range,
  PC origin, and unmapped-reason keys now live in
  `crates/codegen/src/origin/bytecode_keys.rs`, separating identity/export-key
  invariants from package assembly and source-resolution logic.
- Phase 6/8/9 codegen origin test-facade cleanup slice: codegen origin
  regressions now live behind the `crates/codegen/src/origin/tests.rs` facade,
  with focused `crates/codegen/src/origin/tests/` modules for coverage,
  Sonatina records, frontend labels, bytecode origins, export keys, fact export,
  backend-prepared fallback, post-opt snapshot lineage, and graph shape. This
  leaves `crates/codegen/src/origin.rs` focused on Sonatina/bytecode origin
  package assembly while preserving the existing `origin::tests::*` coverage
  names.
- Phase 6/9 frontend-label cleanup slice: `FrontendOriginLabel` and
  `FrontendOriginLabelMap` now live in
  `crates/codegen/src/origin/frontend_labels.rs`, preserving
  `codegen::origin` re-export paths while keeping Sonatina dependency-boundary
  label wrappers separate from Sonatina/bytecode package assembly. The same
  module owns pre-opt source label classification and construction, so bytecode
  package assembly no longer encodes frontend-label policy inline.
- Phase 6/9 frontend-label error-boundary slice: deriving
  `FrontendOriginLabelMap` from bytecode origins now has a fallible
  `try_frontend_origin_label_map` path. Runtime-origin labels with missing
  stable Sonatina function keys propagate through origin-backed compilation
  instead of silently dropping labels from Sonatina observability, while
  synthetic and unmapped same-ID records remain explicit non-label sources.
- Phase 2/6 export-key validation slice: `OriginExportKey` rejects empty
  owner/local parts and the reserved canonical-storage separator, including at
  JSON decode boundaries. The common key type now provides both
  `canonical_storage_key()` for fact allocation and `display_label()` for
  diagnostics/frontend provenance.
- Phase 2/6 nominal string-key validation slice:
  `define_origin_string_key!` and `define_origin_owner_key!` now reject empty
  strings and the reserved canonical-storage separator at wrapper construction,
  so generated owner/object/function key types fail before malformed labels
  reach export-key allocation.
- Phase 6 cleanup slice: `TypedFactSet` no longer exposes an append/extend API
  for independently exported fact sets. Fact IDs are allocation-local, so
  combined views must be exported once from one typed graph.
- Phase 6 common-facts module cleanup slice: fact ID namespace and allocator
  infrastructure now lives in `crates/common/src/facts/ids.rs`, with
  `common::facts` preserving the public re-export path for `FactId`,
  `FactNamespace`, `FactNamespaceError`, and `FactIdAllocator`.
- Phase 6 origin-fact cleanup slice: origin node/link fact DTOs and namespace
  validation now live in `crates/common/src/facts/origin_fact.rs`, separating
  constructor invariants from graph export, reachability indexing, and relation
  validation.
- Phase 6 origin-path cleanup slice: reachability summaries, origin paths,
  path-witness exports, and source-path witness exports now live in
  `crates/common/src/facts/origin_path.rs`, separating query result DTO
  validation from index traversal and relation-table queries.
- Phase 6 origin-path module cleanup slice: origin path DTO code is now split
  below the facade: `origin_path/reachability.rs` owns
  `OriginReachabilitySummary` and per-kind aggregate validation;
  `origin_path/path.rs` owns internal fact-ID paths and kind-pair witnesses;
  `origin_path/witness.rs` owns stable export-key path witnesses; and
  `origin_path/source_witness.rs` owns source-span-attached path witnesses. The
  `common::facts` public re-export path remains unchanged.
- Phase 6 origin-path witness cleanup slice: `origin_path/witness.rs` is now a
  compact facade. `witness/error.rs` owns witness validation diagnostics,
  `witness/record.rs` owns `OriginPathWitnessExport` construction/accessors, and
  `witness/deserialize.rs` owns fail-closed JSON reconstruction.
- Phase 6 origin-reachability DTO cleanup slice:
  `origin_path/reachability.rs` is now a compact facade.
  `reachability/summary.rs` owns `OriginReachabilitySummary` and fail-closed
  serde reconstruction; `reachability/pair.rs` owns per-kind pair DTOs;
  `reachability/validation.rs` owns duplicate/total checks; and
  `reachability/error.rs` owns user-facing validation errors.
- Phase 6 typed-fact cleanup slice: typed fact export code is now split below
  the facade. `typed_fact/export.rs` owns `OwnedTypedFactSetExport`,
  `TypedFactSetExport`, schema-version validation, and origin/shape index
  validation for imported exports; `typed_fact/fact.rs` owns the `TypedFact`
  enum plus per-variant serde mapping. The `common::facts` public re-export
  path remains unchanged.
- Phase 6 typed-fact serde cleanup slice: `TypedFact` is now split below
  `typed_fact/fact.rs`. The parent module owns only the enum;
  `typed_fact/fact/serialize.rs` owns the stable per-variant JSON encoding; and
  `typed_fact/fact/deserialize.rs` owns the fail-closed tagged decoder and
  constructor validation. The wire schema and `common::facts` re-exports remain
  unchanged.
- Phase 6 typed-fact decode cleanup slice: `typed_fact/fact/deserialize.rs` is
  now a compact serde entry point. `deserialize/raw.rs` owns the tagged wire
  enum, while `deserialize/construct.rs` owns conversion into validated
  `TypedFact` variants through the existing fact constructors.
- Phase 6 relation-schema cleanup slice: typed relation names, column names,
  schema descriptors, and column matching now live in
  `crates/common/src/facts/relation_schema.rs`.
- Phase 6 relation-schema module cleanup slice: relation schema code is now
  split below the facade. `relation_schema/name.rs` owns the closed relation
  name enum plus origin/shape relation classification; `relation_schema/column.rs`
  owns the closed column enum; and `relation_schema/schema.rs` owns schema
  descriptors, raw-name lookup, column matching, and column indexing. The
  `common::facts` public re-export path remains unchanged.
- Phase 6 relation-schema catalog cleanup slice: `relation_schema/schema.rs` is
  now a compact facade. `schema/descriptor.rs` owns
  `TypedFactRelationSchema`, relation-name schema lookup, and column-index APIs;
  `schema/catalog.rs` owns the fixed relation catalog, raw-name lookup, and
  column matching. The wire schema and `common::facts` re-exports remain
  unchanged.
- Phase 6 relation-table cleanup slice: typed relation table DTOs,
  relation-count DTOs, relation-row views, and relation JSON validation errors
  now live in `crates/common/src/facts/relation.rs`, separating relation table
  serde/import validation from semantic query indexing.
- Phase 6 relation-table module cleanup slice: relation table code is now split
  below the facade. `relation/set.rs` owns `TypedFactRelationSet`;
  `relation/table.rs` owns `TypedFactRelation`; `relation/count.rs` owns
  `TypedFactRelationCount`; `relation/row.rs` owns relation row views;
  `relation/error.rs` owns relation diagnostics; and `relation/validation.rs`
  owns schema-version, column, and row-width validation. The
  `common::facts` public re-export path remains unchanged.
- Phase 6 relation-error cleanup slice: `relation/error.rs` now keeps the
  public `TypedFactRelationError` enum at the stable re-export path while
  `relation/error/display.rs` owns the display text for relation import,
  validation, source-span, and shape-hash diagnostics.
- Phase 6 shape-hash cleanup slice: shape-hash scope/key/digest/fact DTOs and
  validation errors now live in `crates/common/src/facts/shape_hash.rs`,
  separating digest canonicalization and node/scope invariants from
  relation-table validation and indexing.
- Phase 6 shape-hash module cleanup slice: shape-hash code is now split below
  the facade. `shape_hash/scope.rs` owns the closed string scope enum;
  `shape_hash/key.rs` owns lookup keys plus node/scope invariants;
  `shape_hash/digest.rs` owns canonical digest validation; and
  `shape_hash/fact.rs` owns fact construction and serde validation. The
  `common::facts` public re-export path remains unchanged.
- Phase 6 source-span cleanup slice: source-span kind/export/fact/file-count
  DTOs and validation errors now live in
  `crates/common/src/facts/source_span.rs`, separating range/file validation
  from graph indexing and relation-table validation.
- Phase 6 source-span module cleanup slice: source-span fact code is now split
  below the facade: `source_span/export.rs` owns `SourceSpanKind`,
  `SourceSpanExport`, and shared range/file validation; `source_span/fact.rs`
  owns allocated `SourceSpanFact` rows and namespace validation; and
  `source_span/file_count.rs` owns compact per-file summary DTOs. The
  `common::facts` public re-export path remains unchanged.
- Phase 6 source-span export module cleanup slice: `source_span/export.rs` is
  now a compact facade. `source_span/export/kind.rs` owns the closed span-kind
  enum; `export/error.rs` owns validation errors; `export/validation.rs` owns
  shared file/range checks; and `export/record.rs` owns `SourceSpanExport`,
  fail-closed serde construction, and deterministic sort keys.
- Phase 6 source-span fact module cleanup slice: `source_span/fact.rs` is now a
  compact facade. `source_span/fact/error.rs` owns origin-namespace/span
  validation error conversion and display text; `source_span/fact/record.rs`
  owns `SourceSpanFact`, namespace-checked construction, source-span export
  attachment, and fail-closed serde reconstruction.
- Phase 6 source-span record serde cleanup slice: `source_span/export/record.rs`
  and `source_span/fact/record.rs` now keep constructors/accessors separate
  from raw JSON reconstruction. Export record sorting lives in
  `export/record/sort_key.rs`; export and fact fail-closed decoders live in
  `export/record/deserialize.rs` and `fact/record/deserialize.rs`.
- Phase 6 shape-fact cleanup slice: shape node/field/child/edge,
  trace-event, and data-flow fact DTOs and text validation now live in
  `crates/common/src/facts/shape_fact.rs`, separating constructor invariants
  from shape graph export, relation validation, and query indexing.
- Phase 6 shape-fact module cleanup slice: shape fact code is now split below
  the facade. `shape_fact/text.rs` owns shared shape-node namespace and
  non-empty text validation; `shape_fact/node.rs` owns `ShapeNodeFact`;
  `shape_fact/field.rs` owns `ShapeFieldFact`; `shape_fact/edge.rs` owns
  child/edge facts; `shape_fact/trace_event.rs` owns trace-event facts; and
  `shape_fact/data_flow.rs` owns data-flow facts. The `common::facts` public
  re-export path remains unchanged.
- Phase 6 graph-export cleanup slice: origin and shape graph fact export
  builders now live in `crates/common/src/facts/graph_export.rs`, separating
  typed graph-to-fact projection from relation validation and query indexing.
- Phase 6 graph-export module cleanup slice: graph export is now split below
  the facade. `graph_export/origin.rs` owns origin graph key/link
  deduplication and fact ID allocation; `graph_export/shape.rs` owns shape
  graph node/field/edge/hash/trace/data-flow projection. The `common::facts`
  public re-export path remains unchanged.
- Phase 6 relation-export cleanup slice: typed fact to relation-row projection
  now lives in `crates/common/src/facts/relation_export.rs`, keeping the fixed
  schema export path separate from relation validation and query indexing.
- Phase 6 relation-export module cleanup slice: relation export is now split
  below the facade. `relation_export/cell.rs` owns fact-ID and graph-scope cell
  formatting; `relation_export/row.rs` owns per-variant typed fact row
  projection with schema-width assertions; and `relation_export/set.rs` owns
  deterministic row sorting and relation-set construction. The `common::facts`
  public API remains unchanged.
- Phase 6 typed-fact-set cleanup slice: the `TypedFactSet` container and typed
  iterator/source-span attachment facade now live in
  `crates/common/src/facts/typed_fact_set.rs`, keeping fact-set storage
  separate from relation-table validation and query indexing.
- Phase 6 typed-fact-set module cleanup slice: `TypedFactSet` code is now split
  below the facade. The parent module owns storage plus export/relation-export
  adapters; `typed_fact_set/iterators.rs` owns the typed per-variant iterators
  generated from one local macro; and `typed_fact_set/source_spans.rs` owns
  deterministic source-span attachment. The `common::facts` public re-export
  path remains unchanged.
- Phase 6 index-error cleanup slice: shared fact-index and source-span
  attachment error types plus namespace/text guard helpers now live in
  `crates/common/src/facts/index_error.rs`, keeping error formatting out of the
  parent index implementations.
- Phase 6 index-error module cleanup slice: index diagnostics are now split
  below the facade. `index_error/fact_index.rs` owns `FactIndexError` and its
  display text; `index_error/source_span.rs` owns `SourceSpanFactError`; and
  `index_error/helpers.rs` owns namespace/text guard helpers consumed by
  origin and shape indexes. The `common::facts` public re-export path remains
  unchanged.
- Phase 6 fact-index diagnostic cleanup slice: `index_error/fact_index.rs` now
  keeps the public `FactIndexError` enum at the stable re-export path while
  `index_error/fact_index/display.rs` owns the display text for origin,
  source-span, shape, and shape-hash index diagnostics.
- Phase 6 origin-index cleanup slice: `OriginFactIndex` now lives in
  `crates/common/src/facts/origin_index.rs`, preserving
  `common::facts::OriginFactIndex` while separating typed-fact graph traversal,
  endpoint/source-span validation, reachability summaries, and path witnesses
  from relation-table query indexing.
- Phase 6 origin-index module cleanup slice: `OriginFactIndex` is now a compact
  facade over focused implementation modules. `origin_index/build.rs` owns
  typed-fact index construction and endpoint/source-span validation;
  `origin_index/source_spans.rs` owns source-span lookups;
  `origin_index/reachability.rs` owns reachability sets and summaries; and
  `origin_index/paths.rs` owns shortest paths plus path-witness exports. The
  `common::facts` public re-export path remains unchanged.
- Phase 6 origin-index path-query cleanup slice: `origin_index/paths.rs` is now
  a compact facade. `origin_index/paths/search.rs` owns shortest-path BFS and
  stable-key path lookup; `paths/representative.rs` owns representative
  kind-pair witness selection; and `paths/exports.rs` owns stable export-key
  witness projection plus priority-ordered export selection.
- Phase 6 shape-index cleanup slice: `ShapeFactIndex` now lives in
  `crates/common/src/facts/shape_index.rs`, preserving
  `common::facts::ShapeFactIndex` while separating shape-node/hash lookup and
  completeness validation from relation-table query indexing.
- Phase 6 shape-index module cleanup slice: `ShapeFactIndex` is now a compact
  facade over focused modules. `shape_index/build.rs` owns typed fact indexing,
  namespace/text/reference validation, and required hash coverage checks;
  `shape_index/lookup.rs` owns source-id/stable-key/node/hash lookup APIs. The
  `common::facts` public re-export path remains unchanged.
- Phase 6 relation-index cleanup slice: `TypedFactRelationIndex` now lives in
  `crates/common/src/facts/relation_index.rs`, preserving
  `common::facts::TypedFactRelationIndex` while separating relation-table
  query indexing from the parent facts facade. Its implementation is now also
  split internally: `relation_index/origin_paths.rs` owns relation-backed
  reachability/path queries, source-path witnesses, and source-span summaries,
  while `relation_index/validation.rs` owns semantic row validation,
  reference checks, origin-key validation, source-span range checks, and
  shape-hash relation completeness.
- Phase 6 relation-index validation-helper cleanup slice:
  `relation_index/validation/helpers.rs` is now a compact facade.
  `helpers/ids.rs` owns relation fact-ID collection and namespace checks;
  `helpers/uniqueness.rs` owns duplicate-key checks; `helpers/references.rs`
  owns cross-relation reference checks; and `helpers/cells.rs` owns
  non-empty, closed-value, and numeric cell validation.
- Phase 6 relation-index origin-query cleanup slice:
  `relation_index/origin_paths.rs` is now a high-level query facade.
  `relation_index/origin_paths/graph.rs` owns origin-node/link relation
  decoding and deterministic reachability ordering; `path_search.rs` owns
  shortest-path reconstruction; and `source_spans.rs` owns source-span relation
  projection plus per-file summaries.
- Phase 6 relation-index origin-graph decoder cleanup slice:
  `origin_paths/graph.rs` is now a compact graph facade. `graph/nodes.rs` owns
  origin-node row decoding and export-key reconstruction; `graph/links.rs`
  owns origin-link row decoding and deterministic outgoing-edge ordering; and
  `graph/ordinals.rs` owns `origin_node:` fact-ID parsing used by graph and
  source-span relation joins.
- Phase 6 relation-index origin-query module cleanup slice: relation-backed
  origin queries are now split below that facade. `origin_paths/reachability.rs`
  owns reachability summaries; `origin_paths/paths.rs` owns plain path witness
  queries; `origin_paths/source_paths.rs` owns source-span-attached path
  witnesses; and `origin_paths/source_counts.rs` owns source-span file counts.
  The public `TypedFactRelationIndex` query API remains unchanged.
- Phase 6 relation-index path-query cleanup slice:
  `origin_paths/paths.rs` is now a compact facade. `paths/between_keys.rs` owns
  exact stable-key path lookup; `paths/representative.rs` owns representative
  kind-pair lookup; `paths/priority.rs` owns priority-ordered path export
  selection; and `paths/export.rs` owns shortest-path witness construction.
- Phase 6 relation-index source-span query cleanup slice:
  `origin_paths/source_spans.rs` is now a compact facade.
  `source_spans/columns.rs` owns source-span relation column lookup and
  `source_spans/decode.rs` owns relation-row reconstruction into
  `SourceSpanExport`. File-count aggregation now lives directly in
  `origin_paths/source_counts.rs`.
- Phase 6 relation-index validation cleanup slice:
  `relation_index/validation.rs` is now a high-level semantic-validation
  orchestrator. `validation/helpers.rs` owns shared ID/reference/cardinality
  and cell-shape checks; `validation/origin_keys.rs` owns stable origin-key
  validation; `validation/source_spans.rs` owns source-span row validation; and
  `validation/shape_hashes.rs` owns shape-hash node/scope/dimension/digest
  completeness checks.
- Phase 6 facts-facade cleanup slice: the large common facts test module now
  lives under focused modules in `crates/common/src/facts/tests/`, grouped by
  graph/source-span export, relation export/query, DTO schema invariants,
  relation-index semantic failures, typed JSON fail-closed boundaries, path
  witnesses, and shape index/export. The parent
  `crates/common/src/facts/tests.rs` now keeps only shared helpers and module
  declarations, leaving `crates/common/src/facts.rs` as the narrow public
  facade over focused implementation modules and their re-exports.
- Phase 6/7/8/9 fact slice: build-report and test-bytecode origin facts now
  export one end-to-end graph per bytecode object/test, linking runtime
  semantic/synthetic origins, MIR runtime statements/terminators, pre-opt
  Sonatina instructions, post-opt Sonatina instructions, and bytecode PC ranges
  in one fact-ID allocation.
- Phase 6 query-fixture slice: `common::facts::OriginFactIndex` provides an
  engine-agnostic typed query layer over `TypedFactSet`, with exact reachability
  oracle tests and malformed endpoint rejection. This gives fact consumers a
  correctness target before reintroducing Cozo/Souffle-specific adapters.
- Phase 6/8 query-backed regression: the real test-bytecode metadata path now
  indexes emitted origin facts and requires a runtime origin node to reach a
  bytecode PC node through the end-to-end fact graph.
- Phase 6/10 query-report slice: `OriginFactIndex` now derives a transitive
  reachability summary grouped by origin kind. `fe analyze --origin-facts`
  includes that summary in JSON and text reports, and focused analyze tests
  require semantic-to-runtime and runtime-to-bytecode reachable paths rather
  than only checking that fact rows exist.
- Phase 6/10 relation-query reachability slice: `TypedFactRelationIndex` now
  computes the same reachability summary from fixed relation-table rows, and
  `fe analyze --origin-facts` uses that engine-agnostic query path for grouped
  reachable kind-pair counts.
- Phase 6/10 reachability text slice: text origin-fact reports now render the
  grouped reachable kind-pair counts, not only the total reachable-pair count.
  This exposes the same cross-IR graph summary that JSON carries while keeping
  the compact text report query-engine agnostic.
- Phase 6/10 reachability summary boundary slice:
  `OriginReachabilitySummary` and `OriginReachableKindPairSummary` now reject
  zero-count kind pairs, duplicate kind pairs, unknown fields, and total counts
  that do not equal the per-kind sum. Analyze reachability JSON is therefore a
  fail-closed report DTO rather than a loose summary object whose aggregate
  count can drift from grouped rows.
- Phase 6 query-witness slice: `OriginFactIndex` now exposes deterministic
  shortest-path witnesses over typed fact IDs. Exact oracle coverage verifies
  the returned node sequence and link kinds, giving future debug/query backends
  a shared explanation primitive instead of another bespoke BFS.
- Phase 6/10 path-view slice: `fe analyze --origin-facts` now serializes
  representative path witnesses grouped by origin kind. The witness boundary
  shape is centralized as `OriginPathWitnessExport` in `common::facts`, so
  future debug/report/query exporters can reuse the same stable origin export
  keys plus link-kind rows. Text reports now render compact witness chains with
  stable origin labels and link kinds instead of only showing witness counts.
  Runtime and test-bytecode analyze tests require semantic-to-runtime and
  runtime-to-bytecode witnesses.
- Phase 6/10 relation-query path slice: `TypedFactRelationIndex` now exports
  deterministic representative origin path witnesses from fixed relation-table
  rows, using fact-ID ordinal ordering instead of lexical relation-cell order.
  `fe analyze --origin-facts` now uses that query path for path witnesses as
  well as reachability counts, while `OriginFactIndex` remains the typed-fact
  oracle in focused tests.
- Phase 6 relation-query stable-key slice: `TypedFactRelationIndex` now also
  resolves a path between two stable `OriginExportKey`s and exports the same
  `OriginPathWitnessExport` shape as the typed-fact index. This lets future
  query/debug adapters ask stable boundary-key questions without exposing
  allocation-local fact IDs.
- Phase 6/10 relation-count text slice: `fe analyze --origin-facts` text output
  now renders per-relation row counts from `TypedFactRelationIndex` over
  `TypedFactSet::relation_export` instead of only reporting how many non-empty
  relation tables exist. This keeps the human-readable fact view aligned with
  the JSON query-boundary metadata without exposing full relation rows outside
  JSON mode or duplicating relation scans in the CLI.
- Phase 6 relation-name API slice: relation table names are now represented by
  the closed `TypedFactRelationName` enum in public relation/query APIs. The
  JSON wire strings stay unchanged, but query/report code no longer passes raw
  relation-name strings into `TypedFactRelationSet` or `TypedFactRelationIndex`.
- Phase 6 relation-column API slice: relation-table column names are now
  represented by the closed `TypedFactRelationColumnName` enum in public query
  APIs, including `rows_where`, `column_index`, and `TypedFactRelationRow::cell`.
  The relation JSON schema still exports the same string column names, while
  query callers get compile-time protection against column-name typos.
- Phase 6 relation-constructor boundary slice: `TypedFactRelation::new` now
  takes a `TypedFactRelationName` and derives its fixed column list from the
  declared schema instead of accepting raw table/column strings.
  `TypedFactRelationSet::new` validates completeness immediately, so incomplete
  or malformed relation sets fail at construction rather than later in the query
  index. Raw relation names and columns remain only at serde/import validation
  boundaries.
- Phase 6 relation-schema descriptor slice: relation metadata is now represented
  by `TypedFactRelationSchema`, exposed through `TypedFactRelationName::schema`
  and `typed_fact_relation_schemas()`. Export, validation, relation counts, and
  schema tests consume this descriptor instead of unpacking parallel
  `(name, columns)` tuples.
- Phase 6/10 path-view cleanup slice: origin path witness export now supports
  typed kind-pair queries and priority-driven witness export through both the
  typed-fact oracle and relation-table query path. `fe analyze` uses this to
  keep high-value joins such as semantic-to-runtime, semantic-to-bytecode, and
  runtime-to-bytecode visible even when the report witness limit is small. The
  runtime bytecode analyze regression now requires both semantic-to-bytecode and
  runtime-to-bytecode path witnesses, not only reachability counts.
- Phase 6 query API cleanup slice: `OriginFactIndex` also exposes stable-key
  path helpers, so callers can ask for a path between `OriginExportKey`s and get
  a boundary-safe `OriginPathWitnessExport` without manually handling
  allocation-local fact IDs. The relation-table index now mirrors that stable
  key path query.
- Phase 6 export-schema slice: versioned typed fact JSON now round-trips through
  deserialization for origin nodes/links, shape facts, string-tagged origin
  kinds, link kinds, fact namespaces, shape dimensions, and shape hash scopes.
  The round-trip test re-indexes decoded origin facts, so exported facts are not
  only write-only report rows. Unsupported typed fact schema versions are
  rejected during deserialization, and unknown export, fact-row, or nested
  origin-key fields are rejected so consumers fail closed on future schemas.
- Phase 6 export-schema invariant slice: `OwnedTypedFactSetExport` now validates
  origin facts during JSON deserialization by building `OriginFactIndex`.
  Malformed origin links, duplicate origin fact IDs, and duplicate origin export
  keys fail at the schema boundary instead of only in later query consumers.
- Phase 6 origin fact namespace boundary slice: origin node, origin link, and
  source-span fact DTO constructors/serde now reject non-`origin_node` fact IDs
  before the whole-set index runs. Missing endpoints, missing source-span
  origins, duplicate IDs, and duplicate origin keys remain `OriginFactIndex`
  responsibilities because they depend on the full fact set.
- Phase 6/9/10 report fact-schema slice: build-report and Fe test-report
  regressions now deserialize emitted `origin_facts.json` and
  `snapshot_origin_facts.json` through `OwnedTypedFactSetExport`. Report writers
  are checked against the same fail-closed typed fact schema as downstream query
  and debug consumers, rather than only probing permissive JSON fields.
- Phase 6/10 analyze fact-schema slice: `fe analyze` JSON regressions now decode
  embedded origin/shape `facts` through `OwnedTypedFactSetExport` and embedded
  `relation_tables` through `TypedFactRelationSet` before asserting report
  contents. This keeps the CLI JSON boundary aligned with typed fact and
  relation schemas instead of preserving parallel `serde_json::Value` probes.
- Phase 6/10 analyze origin-fact report-boundary slice:
  `AnalyzeOriginFactReport` now validates decoded `total`, origin-node,
  origin-link, source-span, source-span file summary, and relation-count/table
  fields against the embedded `OwnedTypedFactSetExport`. Origin-fact reports can
  no longer deserialize with shape fact rows, duplicate or mismatched relation
  summaries, populated non-origin relation tables, duplicate source-span file
  summaries, empty identity fields, or source-span file totals that contradict
  the typed fact payload.
- Phase 6/10 analyze relation-summary cleanup slice: origin-fact and shape
  report relation-count/table validation now uses one shared helper for
  duplicate counts, unexpected relation families, and table count drift. The
  report-specific code only provides the expected relation-count policy and
  error constructors, reducing duplicated boundary logic.
- Phase 6/10 analyze path-schema slice: analyze origin-fact regressions now
  deserialize reachability summaries and path-witness payloads through
  `OriginReachabilitySummary`, `OriginPathWitnessExport`, and
  `OriginSourcePathWitnessExport`. Source-path witness tests now validate the
  embedded source span through the typed `SourceSpanExport` decoder instead of
  raw source-span JSON fields.
- Phase 6/10 path-witness boundary slice: `OriginPath`,
  `OriginPathWitnessExport`, and `OriginSourcePathWitnessExport` now reject
  empty paths, node/link count mismatches, non-`origin_node` fact IDs, kind
  mismatches at the first/last stable origin key, and source spans attached to
  the wrong terminal origin. Analyze path witnesses therefore fail closed at the
  shared report DTO boundary instead of relying on debug assertions or
  downstream graph consumers to notice malformed witness rows.
- Phase 9/10 analyze source-map schema slice: `fe analyze --source-maps` JSON
  regressions now deserialize embedded source-map `entries` through
  `BytecodeSourceMapEntry` and coverage payloads through the typed coverage DTOs
  before checking snippets or partition totals. Analyze JSON stays a report
  view, but its tests now exercise the same source-map row validators as
  artifact decoders.
- Phase 9/10 analyze source-map report-boundary slice:
  `AnalyzeSourceMapReport` now validates decoded summary invariants:
  `total == source + non_source`, non-source classifications sum to
  `non_source`, debug-location and debug-line row counts match source rows,
  debug-line file counts cannot exceed source rows, bytecode-origin coverage
  totals match report totals, and present entry rows match both total and
  classification counts. Source-map identity fields such as scope, label,
  object, section, and optional test name must be non-empty. Full entry rows
  must match the report object and must match the report section unless the
  report uses the explicit `<all>` aggregate section sentinel. Compact reports
  may still omit entries when the user did not request full source-map rows.
- Phase 10 analyze report-schema slice: internal analyze report DTOs now
  deserialize with `deny_unknown_fields`, and focused analyze JSON regressions
  parse the whole output through `AnalyzeReport` before checking typed nested
  DTO fields. This removes the remaining top-level permissive `Value` probes
  from those tests while keeping the CLI JSON as a boundary artifact.
- Phase 10 analyze report-boundary slice: `AnalyzeReport` now validates its
  schema version, non-empty profile, closed package kind, unique target labels,
  and target body-count summaries on decode and before rendering. The compact
  `runtime_bodies`, `runtime_statements`, and `runtime_terminators` fields can
  no longer drift from the emitted body rows inside otherwise valid report JSON.
  Target labels and per-target body symbols must also be non-empty, body-symbol
  validation lives on the body-row DTO, and duplicate body symbols are rejected
  at the target boundary.
- Phase 10 analyze origin-count boundary slice: `OriginCount` now validates
  `total == semantic + synthetic` during analyze report decoding. Runtime
  statement and terminator summaries can no longer deserialize with drift
  between aggregate and partition counts.
- Phase 10 analyze maintainability slice: analyze report DTOs, fail-closed
  schema decoding, relation-summary validators, compact count invariants, and
  origin-count helpers now live behind the `crates/fe/src/analyze/report.rs`
  facade, with focused submodules for source-map reports, origin-fact reports,
  shape reports, shared validation, and origin counts. Text and JSON report
  rendering helpers now live in `crates/fe/src/analyze/render.rs`.
  Sonatina/codegen source-map and origin-fact report assembly now lives in
  `crates/fe/src/analyze/codegen_reports.rs`. The parent `analyze.rs` keeps
  CLI/workspace orchestration, shape/runtime-origin summaries, and top-level
  report construction. This keeps the public behavior unchanged while reducing
  the risk that future CLI view work adds more schema, formatting, or codegen
  boilerplate to the already-large orchestration module.
- Phase 10 analyze test-facade cleanup slice: the large analyze test module now
  lives under focused modules in `crates/fe/src/analyze/tests/`, grouped by
  report-schema DTOs, origin-fact report boundaries, source-map report
  boundaries, shape report boundaries, and CLI integration views for basic,
  origin-fact, shape, and source-map analysis. The parent
  `crates/fe/src/analyze/tests.rs` now keeps shared helpers and module
  declarations, leaving `analyze.rs` focused on CLI/workspace orchestration and
  report construction rather than embedding thousands of lines of
  report-boundary fixtures.
- Phase 10 analyze source-map construction slice: `AnalyzeSourceMapReport`
  construction from codegen summaries is now fallible, and report assembly
  propagates schema-boundary errors through the normal analyze error path
  instead of panicking on inconsistent source-map entries or coverage metadata.
- Phase 10 source-map summary reuse slice: analyze full-entry validation now
  reuses `codegen::debug::bytecode_source_map_entries_summary` instead of a
  local duplicate classifier over `BytecodeSourceMapEntryKind`. Emitted entry
  rows must match report classification counts and debug line-table file/row
  counts under the codegen-owned taxonomy.
- Phase 10 relation-table equality slice: analyze origin-fact and shape reports
  now reject embedded relation tables whose normalized rows differ from the
  report's typed facts, even when relation row counts still match. Shape reports
  also reject relation tables when the backing typed facts are omitted.
- Phase 10 relation-count completeness slice: analyze origin-fact and shape
  reports with typed facts now require relation-count rows for every non-empty
  relation exported by those facts. Zero-row relations remain omitted from the
  sparse summary format.
- Phase 6 shape-hash constructor slice: shape-hash node/scope validity now
  lives on `ShapeHashFactKey::try_new` and `ShapeHashFact::try_new`. `graph`
  hashes cannot carry node IDs, and `local`/`tree` hashes cannot omit them,
  before the facts reach `ShapeFactIndex`.
- Phase 11 origin-key macro slice: origin owner/local/string wrapper macros now
  emit fallible `try_new` constructors returning `OriginKeyTextError`; `new`
  remains the panic wrapper for trusted internal construction. Tests assert
  empty and reserved-separator failures without relying only on `should_panic`.
- Phase 11 common origin test-facade cleanup slice: shared origin identity tests
  now live in `crates/common/src/origin/tests.rs`, leaving the parent
  `origin.rs` focused on the key/export-key/macro/link/graph API surface while
  preserving all `common::origin` paths.
- Phase 2/11 common origin module cleanup slice: shared origin identity,
  export-key policy, and graph containers are now split into focused production
  modules. `crates/common/src/origin/key.rs` owns `OriginKey`,
  `origin/export_key.rs` owns export kinds, stable key validation, and typed
  owner/local traits, `origin/graph.rs` owns link kinds plus graph containers,
  and `origin/macros.rs` owns the exported helper macros. The parent
  `origin.rs` remains the module/re-export facade so existing `common::origin`
  paths and exported macro behavior stay stable.
- Phase 8/9 test-metadata source-map schema slice: the Sonatina public test
  metadata regression now validates generated source-map JSON by deserializing
  `OwnedBytecodeSourceMapExport`, rather than checking raw `schema_version` and
  `entries` fields. This keeps test metadata, report artifacts, and analyze
  source-map views on the same source-map DTO boundary.
- Phase 6 shape-fact schema invariant slice: the same boundary now builds
  `ShapeFactIndex` for shape rows. Shape fields, children, edges, and local/tree
  hash rows must reference existing shape nodes; graph hash rows must remain
  graph-scoped; duplicate shape IDs, source IDs, and stable keys fail during
  deserialization.
- Phase 4/6 shape-hash schema invariant slice: shape hash rows now fail closed
  unless digests are canonical 16-character lowercase hex, hash keys are unique,
  and non-empty shape fact exports contain complete graph plus per-node
  local/tree hash coverage for every dimension.
- Phase 4/6 shape-hash query API slice: `ShapeFactIndex` now owns a typed
  `ShapeHashFactKey` map plus graph/local/tree lookup helpers, so query adapters
  and reports can ask for a digest by identity instead of scanning fact rows.
- Phase 4/6 shape identity invariant slice: `ShapeGraph` now rejects empty
  stable keys, node kinds, field names, child labels, and edge labels at
  construction time. Shape fact DTO constructors and serde boundaries now also
  reject empty stable keys, node kinds, field names, child/edge labels,
  trace-event kinds, and data-flow kinds before indexing. Field and trace-event
  values may still be empty when modeling real empty constants or strings, while
  relation-table query indexes retain the same checks for imported table cells.
- Phase 4/6 shape fact namespace boundary slice: shape node, field, child, edge,
  trace-event, data-flow, and hash fact constructors/serde now reject
  non-`shape_node` fact IDs locally. Missing node references, duplicate shape
  IDs/source IDs/stable keys, hash node/scope consistency, and complete hash
  coverage remain `ShapeFactIndex` responsibilities because they require the
  full fact set.
- Phase 4/6/10 typed relation slice: shape fact export now derives explicit
  `trace_event` rows from trace-event dimension fields and `data_flow` rows
  from graph edges, while keeping generic shape fields/edges for compatibility.
  `ShapeFactIndex` validates relation endpoints, and `fe analyze` shape
  summaries expose relation counts.
- Phase 4/6/10 shape relation-count text slice: shape analyze reports now carry
  the same compact per-relation row-count DTO as origin fact reports. Text
  `fe analyze --shape-hashes --shape-facts` shows populated shape relation
  tables such as `shape_node`, `shape_field`, `shape_child`, `trace_event`, and
  `shape_hash` without requiring full JSON relation-table output.
- Phase 4/10 shape hash report-boundary slice: shape analyze report hash
  dimensions now use the closed `ShapeDimension` enum internally while
  preserving the existing JSON wire strings. Report decoding now rejects
  unknown dimensions and unknown hash-report fields instead of accepting a
  permissive stringly-typed dimension cell.
- Phase 4/6/10 shape analyze report-boundary slice: `AnalyzeShapeReport` now
  validates decoded graph hash coverage, canonical hash digests, shape-fact
  aggregate counts, graph hash digests against embedded fact rows, and
  relation-count/table row counts. Duplicate relation-count summaries are
  rejected, and shape report scope/label fields must be non-empty. Compact
  reports may omit full fact rows, but present facts and query-table summaries
  must agree with the report counts.
- Phase 4/6 shape hash digest boundary slice: `ShapeHashFact` now validates
  canonical 16-character lowercase hex digests in its constructor and serde
  boundary. Typed fact JSON rejects malformed digest text before indexing,
  while graph/local/tree node-scope consistency remains part of shape fact
  index validation.
- Phase 4/6/10 shape hash digest policy cleanup slice: canonical digest text
  now flows through a `ShapeHashDigest` newtype in `common::facts`.
  `ShapeHashFact` and `AnalyzeShapeHashReport` reuse that boundary, removing a
  duplicate lowercase/length policy from the CLI report boundary while keeping
  the existing `digest_hex` JSON string. Shape report validation compares typed
  digest values and stringifies only for diagnostics. Public `ShapeHashFact`
  construction now rejects raw digest strings at compile time; JSON/import code
  uses explicit digest-hex decoding helpers, and the raw canonicality predicate
  remains private. `ShapeHashFactError` delegates digest-format diagnostics to
  `ShapeHashDigestError` so the canonical digest policy has one error owner.
  Shape fact indexes no longer revalidate canonical digest text; they validate
  graph/local/tree scope and completeness over already-typed hash rows. Raw
  relation-table imports still reject malformed `digest_hex` by constructing
  `ShapeHashDigest`.
- Phase 6 query-backend export slice: `TypedFactSet::relation_export` now
  derives fixed, engine-agnostic relation tables for origin nodes/links,
  source spans, shape nodes/fields/children/edges, trace events, data flow, and
  shape hashes. `TypedFactRelationIndex` now answers non-empty relation-count
  summaries in declared schema order, and `fe analyze` uses that shared query
  path for origin and shape reports. This gives future Cozo/Souffle/JSON
  adapters a typed table boundary without adding mutable compiler sinks or
  query-time side effects.
- Phase 6/10 source-span summary slice: `fe analyze --origin-facts` now also
  exposes compact per-file counts for exported `source_span` facts. The summary
  is derived through `TypedFactRelationIndex` over the fixed relation export and
  serialized with the same validating `SourceSpanFileCount` DTO that common
  facts use, so the human-readable and JSON reports exercise the same
  engine-agnostic table boundary future query adapters will consume.
- Phase 6/10 source-path witness slice: `TypedFactRelationIndex` now exports
  representative origin paths whose terminal origin owns a `source_span` row.
  `fe analyze --origin-facts` serializes those `source_path_witnesses` and text
  reports render a `source paths:` section, so source-to-bytecode explanations
  can be inspected from the cached relation-table boundary without mutable
  query-time sinks or format-specific debug emitters.
- Phase 6/10 query-backend CLI slice: `fe analyze --fact-relation-tables
  --format json` now emits the full engine-agnostic relation tables for any
  emitted typed fact set. The flag is gated by `--origin-facts` or
  `--shape-facts`, keeping large table-shaped output opt-in while giving query
  backend prototypes a real CLI artifact to consume.
- Phase 6 relation-table schema invariant slice: `TypedFactRelationSet` now
  round-trips through a fail-closed JSON schema. Decoding rejects unsupported
  schema versions, unknown or duplicate relations, missing required relations,
  wrong fixed columns, wrong row widths, and unknown export/relation fields, so
  query adapters do not need to trust permissive table JSON.
- Phase 6 relation-schema cleanup slice: relation export now constructs table
  columns from `TYPED_FACT_RELATION_SCHEMAS` instead of restating column lists
  at every export site. A schema-order test guards the declared relation set,
  reducing drift between export, decode, and query consumption.
- Phase 6 relation-row collector slice: relation export now accumulates rows
  through a schema-initialized `TypedFactRelationName` keyed collector instead
  of ten parallel row vectors and a hand-written table assembly list. The
  fact-to-row projection is owned by `TypedFact`, the collector checks row
  width against the declared schema, sorts rows per relation, and emits tables
  in schema order.
- Phase 6 relation-count boundary slice: `TypedFactRelationCount` now carries
  `TypedFactRelationName` instead of raw strings, with compile-fail coverage
  rejecting raw relation-count construction and fail-closed serde coverage for
  unknown relation names, unknown fields, and zero-row summaries. Analyze reuses
  the shared validating DTO for text and JSON reports and rejects duplicate
  relation summaries at report boundaries, so relation-count summaries no longer
  have a permissive CLI-only schema.
- Phase 6 relation-row query slice: `TypedFactRelationRow` now carries
  `TypedFactRelationName` internally and resolves `cell()` lookups from the
  declared typed schema rather than from exported column strings. Query results
  can report typed relation identity while preserving existing relation-table
  JSON.
- Phase 6 relation-table storage slice: `TypedFactRelation` now stores
  `TypedFactRelationName` and typed column IDs internally after construction or
  JSON decoding. Existing `name()`/`columns()` string accessors remain as
  compatibility views, while validation and query indexing can stay on closed
  schema data.
- Phase 6 relation-table schema-state cleanup slice: `TypedFactRelation` no
  longer stores a column vector at all. Columns are derived from the relation
  schema descriptor for accessors, validation, and JSON serialization, making
  the relation name the single in-memory source of table column order.
- Phase 6 relation-validation schema-state cleanup slice: the internal typed
  relation validator now accepts only relation name and rows, deriving expected
  width from the schema descriptor. Explicit column validation remains at the
  raw JSON/import boundary where caller-provided column strings actually exist.
- Phase 6 relation-set validation cleanup slice: duplicate and missing
  relation checks now track `TypedFactRelationName` directly, converting back
  to wire strings only when constructing error payloads.
- Phase 6 relation-index API slice: `TypedFactRelationIndex` now keys its
  relation map and column lookup API by `TypedFactRelationName` and
  `TypedFactRelationColumnName`. Raw-string helper paths are limited to
  validation/import adapters that parse into the closed schema before lookup.
- Phase 6 relation-column index cleanup slice: `TypedFactRelationIndex` now
  derives column positions from `TypedFactRelationName::schema()` on demand
  after verifying the relation table exists, instead of storing a second
  relation-to-column map that can drift from the schema descriptor.
- Phase 6 relation-column lookup cleanup slice: `TypedFactRelationName` now
  owns typed column-position lookup and closed-column mismatch diagnostics.
  Relation rows and relation indexes both call that shared schema API instead
  of duplicating column scans and error construction.
- Phase 6 relation-validation helper slice: relation semantic validation now
  passes closed relation and column enums through ID checks, uniqueness checks,
  reference checks, numeric parsing, and closed-value validation. Raw relation
  and column strings remain only as wire strings and formatted error payloads.
- Phase 6 shape-hash validation cleanup slice: relation-table shape-hash
  completeness checks now keep parsed `ShapeHashScope` and `ShapeDimension`
  values typed inside validation keys, stringifying only at diagnostic
  boundaries.
- Phase 6 deterministic relation-export slice: relation-table export now sorts
  rows per relation, so backend/query artifacts stay stable when equivalent
  typed facts are assembled in a different order. Regression coverage reverses
  origin and shape fact order, including source-span, trace-event, data-flow,
  child, edge, and hash rows, and requires identical relation exports.
- Phase 6 relation-query artifact slice: `TypedFactRelationIndex` now builds a
  pure query view over validated relation-table exports. Exact oracle coverage
  deserializes the JSON table artifact, queries origin/shape rows by column,
  joins origin links to origin nodes, and verifies trace-event, data-flow,
  source-span, and graph-hash answers without introducing a specific query
  engine or Salsa-side sink. The index also rejects schema-valid but
  semantically broken table artifacts, including missing origin/shape endpoint
  IDs and invalid closed string values such as origin link kinds.
- Phase 6 relation semantic-invariant slice: the relation-table query index now
  enforces duplicate origin export keys, relation ID namespaces, numeric
  source-span and child-order cells, source-span range ordering, duplicate
  shape stable/source keys, duplicate shape-hash keys, and complete graph plus
  per-node local/tree shape-hash coverage. This keeps the exported table
  artifact independently trustworthy before a Datalog or JSON query adapter is
  introduced.
- Phase 6 duplicate-link invariant slice: decoded typed fact JSON and
  relation-table artifacts now reject duplicate origin links. Compiler-produced
  origin graph fact export already deduplicates links, but external or cached
  artifacts now fail closed instead of making query consumers handle duplicate
  edge rows defensively.
- Phase 2/6 relation export-key invariant slice: relation-table `origin_node`
  rows now mirror `OriginExportKey` validation, rejecting empty owner/local
  cells and the reserved canonical-storage separator. This closes the raw-table
  bypass around the typed `OriginExportKey` JSON constructor.
- Phase 6/9 source-span fact slice: typed fact export now includes a
  `source_span` relation attached to bytecode PC origin nodes when
  `BytecodeSourceResolution` has a resolved LazySpan source result. The rows are
  added to the same end-to-end fact allocation as origin nodes/links, not
  concatenated from a separate export, and `OriginFactIndex` validates that each
  source span references an existing origin node.
- Phase 6 source-span schema invariant slice: `source_span` facts now use a
  closed `SourceSpanKind` enum, reject empty file labels, and validate byte
  plus line/column ordering during typed fact JSON deserialization. Unknown
  span kinds, empty files, and inverted source positions fail at the schema
  boundary.
- Phase 6 source-span constructor invariant slice: `SourceSpanExport::new`
  now enforces the same non-empty file and ordered byte/line-column range
  invariants before typed fact rows are produced. `SourceSpanFact` now shares
  that constructor/serde validation, so typed fact JSON rejects malformed
  source-span rows before origin graph indexing. Origin existence remains an
  `OriginFactIndex` responsibility.
- Phase 6 source-span export-schema slice: `SourceSpanExport` now also has a
  fail-closed JSON decoder and fallible `try_new` constructor. Direct
  source-span export JSON and source-path witness JSON round-trip through the
  typed DTO, while empty files, inverted ranges, unknown fields, and invalid
  origin keys fail before report/query tests consume them.

Still open before claiming full optimization lineage:

- Sonatina pass hooks for precise split, merge, replacement, deletion, and
  aliasing intent beyond conservative snapshot alias/loss classification.
  Same-`InstId` before/after joins must remain snapshot aliases, not inferred
  pass lineage, because Sonatina can rewrite instructions without exposing pass
  events.
- A durable backend-prepared instruction bundle independent of observability
  JSON for instructions created after `EvmCompile::optimize()`; current PC-map
  entries are no longer modeled as fake post-opt instructions, but they are
  still discovered from observability.
- DWARF, ethdebug, typed query backends, and richer cross-IR origin graph views.

## Phase 0: Preserve Prototype Learnings

Purpose: avoid losing useful tests and experiments while making it clear that
the prototype is not the final structure.

Tasks:

- Save review findings in a planning note or branch review.
- Mark known prototype hazards: raw IDs, namespace joins, pre/post-opt mismatch,
  graph hashing mode switch, temp-file CLI analysis.
- Mark transcript/code mismatches as risks until verified in code.
- Identify tests worth porting after the architecture exists.
- Identify tests that should be rewritten because they assert only existence.

Deliverable:

- `origin-overhaul-reconciliation.md`.
- A short prototype-lessons section in the design doc or decision record.

Gate:

- Everyone agrees the current branch is a prototype baseline, not a patch queue.

## Phase 1: Design And Invariants

Purpose: establish the architecture before more code is added.

Tasks:

- Define origin terminology and data model.
- Define Salsa-safe return-data architecture.
- Define phase ownership.
- Define typed identities.
- Define hash architecture.
- Define fact/export boundary.
- Define testing invariants.
- Define the online-capture vs post-hoc-derivation boundary.

Deliverable:

- `origin-graph-instrumentation-design.md`
- `origin-overhaul-plan-draft.md`
- `origin-overhaul-reconciliation.md`

Gate:

- Design answers where origin data lives, who owns each link, and where exports
  happen.
- Design says which data must be captured during lowering/passes and which data
  can be derived later.

## Phase 2: Typed Origin Core

Purpose: replace raw origin IDs with owner-aware typed identities.

Tasks:

- Add `OriginNode`, `OriginLink`, and `OriginGraph` types.
- Add phase-specific keys: HIR expr/stmt, semantic origin, MIR stmt, MIR
  terminator, Sonatina inst, bytecode PC range.
- Add stable export forms for keys that leave the compiler. Export-owner forms
  should be typed separately from generic string labels, so public helpers
  cannot accept raw `String`/`&str` owners at phase boundaries.
- Add source-origin resolution helpers that delegate to LazySpan.
- Keep compatibility wrappers if needed during migration.

Likely files:

- `crates/common/src/origin.rs` or a new origin crate.
- HIR source-origin helpers near existing span/origin code.
- MIR runtime origin key definitions.

First slice:

- Define owner-aware key types without changing exporters.
- Add compile/runtime tests for owner identity, kind identity, and stable export
  identity.
- Add only the compatibility adapters needed to keep prototype paths compiling.
- Do not introduce Datalog, DWARF, ethdebug, Sonatina optimization lineage, or
  derive-macro expansion in this slice.

Gate:

- It is impossible to represent a HIR expr origin without its body/owner.
- It is impossible to represent a HIR stmt origin as a HIR expr origin.
- It is impossible to represent a MIR stmt origin without runtime instance,
  block, and statement index.
- Same local IDs in different owners do not collide.

## Phase 3: Runtime Body Origins

Purpose: make MIR origin data cached and typed.

Tasks:

- Add `RuntimeBodyOrigins`.
- Make MIR lowering return or derive typed statement/terminator origins.
- Add a sibling tracked query if direct return changes are too invasive.
- Replace statement-origin raw vectors as source of truth.
- Add deterministic package aggregation.

Likely files:

- `crates/mir/src/instance/runtime.rs`
- `crates/mir/src/runtime/lower/body.rs`
- `crates/mir/src/runtime/package.rs`

Gate:

- `RuntimeInstance` origin data can be queried repeatedly with no side effects.
- Every MIR stmt/terminator has typed origin data or a classified synthetic
  origin.

## Phase 4: Shape Graph And Hashing

Purpose: replace callback-driven mixed tree/graph hashing with a clear model.

Tasks:

- Define `ShapeGraph`, shape nodes, shape fields, ordered children, and edges.
- Implement content/tree digest.
- Implement graph digest that augments content instead of disabling children.
- Implement dimension projection.
- Add hash policy types for structure/names/constants/types and explicitly
  modeled trace events or language effects.
- Hash full edge labels, not truncated label fingerprints.
- Export all hash dimensions that the core computes.

Gate:

- A CFG edge cannot cause statement children to stop contributing to hashes.
- Minimal graph tests prove content changes and edge changes are both observed.
- Endpoint constants/names/types behind graph edges do not pollute the
  structure projection.
- Local shape fields are unordered metadata; changing only field insertion
  order does not change local/tree/graph digests. Ordered content must use
  children and remains order-sensitive.
- Fact/debug/report views do not silently drop dimensions.

## Phase 5: Derive Macro Followthrough

Purpose: reduce boilerplate without weakening correctness.

Tasks:

- Extend derive macro to emit shape graph descriptions.
- Require every field to declare dimension or skip reason.
- Add support for stable-key policies.
- Add enum variant node metadata plus explicit trace-event or language-effect
  metadata where needed.
- Add compile-fail or coverage tests for unclassified fields, empty skip
  reasons, multiple policies, and unknown shape attributes.
- Convert manual impls gradually.
- Use narrow macros for repeated typed-origin wrappers where the constructor is
  just `(owner, local)`, and leave custom invariant-bearing origins manual.
- Ensure generated/desugared HIR goes through normal lowering builders so
  origins and scope registration are created together.

Gate:

- Adding a field to an instrumented IR type fails until hash/origin policy is
  declared.
- Empty skip reasons fail to compile; every explicit skip must document why the
  field is excluded from shape hashing/export.
- At least one real IR family is converted before claiming the macro path is
  sustainable.

## Phase 6: Typed Fact Export

Purpose: make Datalog facts trustworthy.

Tasks:

- Define a typed fact schema with explicit namespaces.
- Add export ID allocator that maps typed nodes to Cozo IDs.
- Generate `origin_node`, `origin_link`, `shape_node`, `shape_edge`,
  `source_span`, `trace_event`, and `data_flow` relations from typed data.
- Rewrite security queries against the typed schema.
- Add small synthetic query tests with known answers.
- Keep fact schema engine-agnostic so Cozo/Souffle/JSON exports can share the
  same typed source.
- Expose versioned JSON fact reports at report/export boundaries before binding
  the schema to a query engine.

Gate:

- No query joins unrelated raw integer namespaces.
- Every cross-namespace relation is explicit.
- Semantic fact-owner labels, runtime fact-owner labels, and synthetic local
  labels cannot be interchanged inside MIR fact graph construction.
- Runtime fact owner-key derivation accepts only a typed target label plus a
  typed runtime package body symbol, not raw strings.
- The same semantic-owner/runtime-owner/synthetic-local separation holds inside
  codegen-owned end-to-end fact graph construction.
- Codegen end-to-end owner-key derivation accepts only a typed Sonatina function
  export key, not a raw function label.
- Generic trace metadata does not overload Fe language-effect terminology.
- Export origin keys are validated before fact allocation and JSON decoding
  completes: owner/local parts are non-empty and cannot contain the reserved
  canonical-storage separator.
- Exporting facts twice produces deterministic typed IDs and rows for the same
  typed input graph.
- Relation-table artifacts fail closed on semantic invariants, not just JSON
  shape: duplicate keys, bad ID namespaces, malformed numeric cells, inverted
  source ranges, malformed origin key cells, duplicate origin links, missing
  endpoints, empty shape identity/label cells, and incomplete shape-hash
  coverage are rejected before query adapters consume them.

## Phase 7: Sonatina And Optimization Origins

Purpose: carry typed origins through backend lowering and optimization.

Tasks:

- Add typed MIR-to-Sonatina origin data.
- Cover prologue, helper-generated instructions, terminators, and synthetic
  backend instructions.
- Add pre-opt to post-opt origin mapping.
- Classify same-ID snapshot aliases, optimizer-created, merged, split, and
  deleted instructions.
- Avoid joining optimized PC maps with pre-opt instruction IDs.
- Cover pass operations such as instruction creation, replacement, aliasing,
  erasure, and layout-only moves.

Gate:

- Every post-opt instruction is mapped by same-ID snapshot alias, classified
  as a post-preopt snapshot gap, or explicitly unmapped with a reason.
- No resolver can hide missing Sonatina owner/pass data by scanning all bodies
  or all pre-opt instructions.
- Pre-opt and post-opt Sonatina origin records fail fast if constructed with
  the wrong instruction stage, backend-prepared records fail fast if handed a
  post-opt instruction origin, and post-opt function bundles reject records from
  another function.
- Same-ID post-opt records fail fast if the embedded pre-opt source does not
  have the same function and instruction ID as the post-opt origin.

## Phase 8: Optimization, PC Origins, And Source Resolution

Purpose: carry typed origins through post-opt and backend-prepared Sonatina
instruction references, bytecode PC ranges, and source-span resolution without
guessing across owners.

Tasks:

- Add phase-aware Sonatina instruction keys.
- Add observability-backed bytecode PC range origins.
- Keep bytecode edges pointed at post-opt or backend-prepared Sonatina nodes.
- Add optimized-module post-opt origin bundles before bytecode emission.
- Implement a pure resolver from bytecode PC origins to semantic/source spans.
- Preserve explicit classifications for synthetic, unmapped,
  post-preopt snapshot gaps, missing-runtime-origin, and missing-source-span
  cases.
- Classify backend-prepared/codegen-only instruction IDs as
  `post_preopt_snapshot_gap` until Sonatina exposes a precise prepared-module
  origin bundle.
- Reject overlapping bytecode PC-origin ranges inside one object section; allow
  adjacent half-open ranges.
- Add conservative post-preopt snapshot-gap classification when Sonatina does
  not expose a more precise pass origin.
- Add targeted Sonatina pass hooks for precise split/merge/delete/replace
  lineage.

Gate:

- Every bytecode origin record receives either a source span or an explicit
  non-source classification.
- Bytecode PC-origin ranges are non-empty and non-overlapping within each
  object section.
- Bytecode object and section identity is non-empty before any source-map or
  debug consumer interprets a PC range.
- No bytecode PC range links directly from a pre-opt Sonatina instruction ID.
- No debug/source resolver tries every HIR body until one works.
- Known PC/source mappings resolve to expected snippets.
- Every post-opt instruction is mapped, classified as a post-preopt snapshot
  gap, or explicitly unmapped with a reason once optimizer pass hooks exist.

## Phase 9: Debug Exporters

Purpose: rebuild debug outputs from typed origins.

Tasks:

- Add a typed source-map/debug export boundary over bytecode source
  resolutions.
- Emit source-map and debug-location artifacts in test/build reports so
  mappings can be inspected before full DWARF/ethdebug rebuilds.
- Rebuild DWARF generation over `PcRange -> SourceSpan`.
- Rebuild ethdebug export over the same mapping.
- Add multi-function tests that fail if body-local IDs are guessed incorrectly.

Gate:

- Debug exporters consume typed bytecode source resolutions instead of scanning
  HIR/MIR owners.
- Source-map JSON decoding fails closed on semantic invariants, not just JSON
  shape: closed span kinds and non-source reason strings, ordered source
  ranges, non-empty object/section identity in entries and export metadata,
  matching export object/section metadata, and non-overlapping PC rows are
  required.
- Internal source-map entry construction uses typed closed enums for span kinds
  and non-source reason classifications; raw-string construction is rejected at
  compile time.
- Public source-map entry construction uses typed bytecode PC origins, not raw
  object/section/PC tuples.
- Source-map export construction and serialization validate the same metadata,
  overlap, and coverage-count invariants as JSON decoding.
- Source-map artifacts include object, section, PC range, source span, and
  explicit non-source reason data.
- Debug-location artifacts include only validated source PC ranges and never
  coerce synthetic/unmapped classifications into fake source spans.
- Debug-location JSON decoding fails closed on schema version, unknown fields,
  empty payloads, metadata mismatch, invalid PC/source ranges, and overlapping
  PC rows.
- Invalid resolved source spans are classified as `source_span_invalid` rows
  with closed reasons instead of panicking during source-map/debug export.
- Source-map rows and `source_span` facts use the same source-span validation,
  so an invalid snippet range cannot be a valid fact row in another artifact.
- Known PC/source mappings resolve to expected snippets.

## Phase 10: Analyze CLI And Public API

Purpose: make analysis user-facing and realistic.

Tasks:

- Add a minimal typed-origin `fe analyze` summary command that uses normal CLI
  target resolution.
- Add test-runtime package analysis for Fe test suites.
- Add source-map summary analysis over typed bytecode source resolutions.
- Add opt-in full source-map entry analysis over the same typed codegen rows.
- Add opt-in origin-fact analysis over typed bytecode origin graphs.
- Add opt-in shape/hash analysis over derive-described runtime IR families.
- Make `fe analyze` use normal target/workspace resolution.
- Respect ingot config, profile, path, and dependencies.
- Generate report views from typed compiler data.
- Keep heavy exporters behind features.
- Decide whether `fe trace` should exist separately from `fe analyze`.

Gate:

- Analyzing a file in a workspace sees the same program as `fe build/check`.
- Analyze output is derived from typed cached compiler data, not temp-file
  source compilation.
- Test-only ingots can be analyzed through `fe analyze --tests`.
- Source-map analysis consumes typed codegen summaries and keeps explicit
  non-source classifications.
- Full source-map entry analysis is opt-in and uses typed codegen entries, not
  parsed JSON artifacts.
- Origin-fact analysis is opt-in and uses typed fact exports, not callback rows
  or parsed report artifacts.
- Shape/hash analysis is opt-in and comes from `ShapeDescribe`/`ShapeGraph`,
  not a parallel hash visitor.
- JSON output has a versioned schema.

## Phase 11: Cleanup And Removal

Purpose: eliminate duplicate paths and prototype artifacts.

Tasks:

- Remove raw `ProvenanceNodeId` or reduce it to a compatibility shim.
- Remove traversal-order package origin IDs.
- Remove fact emission from ad hoc `IrConsumer` callbacks as source of truth.
- Rename remaining "provenance" APIs to "origin" where feasible.
- Update docs and examples.

Gate:

- New code paths are the only source of origin/hash/fact/debug data.

## Test Matrix

Identity:

- HIR expr and HIR stmt with same local index do not collide.
- Two bodies with same expr index do not collide.
- MIR stmt keys include runtime instance, block, and statement.

Origins:

- One HIR expr lowering to many MIR stmts creates many links.
- One MIR stmt lowering to many Sonatina instructions creates many links.
- Optimization merge creates many-to-one links.
- Synthetic code is classified.

Hashing:

- Statement content changes affect structure hash.
- CFG-only changes affect graph hash.
- Rename changes names hash but not structure hash.
- Constants and types have independent dimensions.

Facts:

- Query over synthetic fact set returns exact expected rows.
- Dataflow joins only use shape IDs or explicit mapping IDs.
- Source span joins use origin node IDs.

Debug:

- Multi-function source mapping resolves to the correct body.
- Optimized PC maps use post-opt origins.
- Unmapped PC ranges are classified.
- SCIP-like multi-view/debug exports are validated as boundary consumers, not
  as origin-core state.

Salsa:

- Repeated queries are deterministic.
- Exporting facts twice produces the same output.
- No exporter mutates compiler query state.

## Risk Register

Risk: typed origin data becomes too large.

Mitigation: keep phase-local bundles compact, derive global views lazily, and
export only requested views.

Risk: stable keys are expensive or hard to compare.

Mitigation: use existing stable runtime keys where possible and separate
internal typed keys from exported stable strings.

Risk: macro complexity becomes its own maintenance problem.

Mitigation: keep macro scope narrow: field coverage, dimension policy, shape
description. Avoid encoding phase semantics in the macro.

Risk: the overhaul increases LoC instead of reducing it.

Mitigation: track old visitors/consumers removed per phase, make `IrConsumer`
transitional, and require derives/schema to replace at least one real manual IR
description before expanding scope.

Risk: Sonatina optimizer does not expose enough origin mapping.

Mitigation: start with conservative classification and upstream targeted origin
hooks only where needed.

Risk: design stalls because it is too broad.

Mitigation: land in small phases with gates. Do not wait for DWARF or Cozo to
complete typed MIR origins and hashing.

## Phase Sequence Draft

Phase 0: Prototype lessons

- Record original-session reconciliation and supersede historical
  `ProvenanceNodeId` plans.
- Capture prototype hazards and tests worth porting.
- No behavior change.

Phase 1: Docs, terminology, and invariants

- Add architecture/design docs.
- Rename future-facing language to origin.
- Define online-capture vs post-hoc-derivation boundary.
- Define gates for typed identities, hashing, facts, exporters, and tests.
- No behavior change.

Phase 2: Typed origin core

- Add origin types and source resolution helpers.
- Add tests for owner-aware and kind-aware identities.
- Keep exporters unchanged except for compatibility adapters.

Phase 3: MIR body origins

- Add typed `RuntimeBodyOrigins`.
- Add cached query or returned bundle.
- Preserve old path temporarily.

Phase 4: Shape graph and hash core

- Add `ShapeGraph`.
- Add tree and graph digest split.
- Add focused hash invariant tests.

Phase 5: Derive macro expansion

- Generate shape descriptions.
- Add field policy enforcement.
- Convert first small set of IR types.
- Delete the replaced manual descriptions in the same phase when practical.
- Surface the migrated IR family's shape/hash data through analysis so the
  derive policy is exercised outside unit tests.

Phase 6: Fact export

- Add typed fact schema.
- Port basic Datalog/security facts.
- Add synthetic query oracle tests.
- Emit versioned typed origin-fact JSON from build reports for non-test
  contract/runtime bytecode.
- Emit versioned typed origin-fact JSON from test reports and expose the same
  typed facts through `fe analyze --tests --origin-facts`.
- Expose runtime semantic-to-MIR origin facts through `fe analyze
  --origin-facts` without requiring test bytecode generation.

Phase 7: Sonatina origins

- Add MIR-to-Sonatina typed origin links.
- Add terminator/prologue/synthetic coverage.

Phase 8: Optimization and PC origins

- Add phase-aware Sonatina instruction keys.
- Add observability-backed bytecode PC range mapping.
- Keep bytecode edges pointed at post-opt or backend-prepared Sonatina nodes.
- Add conservative post-preopt snapshot-gap classification when Sonatina does
  not expose a more precise pass origin.
- Add targeted Sonatina pass hooks for precise split/merge/delete/replace
  lineage.

Phase 9: Debug exporters

- Emit typed source-map artifacts from bytecode source resolutions.
- Emit non-test build-report source-map artifacts for contract/runtime bytecode.
- Rebuild DWARF and ethdebug from typed PC origins.

Phase 10: Analyze CLI

- Rebuild `fe analyze` on real workspace target resolution.
- Emit typed runtime-origin text/JSON summaries as the first public view.
- Support test runtime package summaries with `fe analyze --tests`.
- Support bytecode source-map summaries with `fe analyze --tests --source-maps`.
- Support opt-in full bytecode source-map rows with
  `fe analyze --tests --source-maps --source-map-entries`.
- Support opt-in bytecode origin facts with
  `fe analyze --tests --origin-facts`.
- Support opt-in runtime origin facts with `fe analyze --origin-facts` for
  regular and test runtime packages.
- Support opt-in const-region shape hashes and facts with
  `fe analyze --shape-hashes --shape-facts`.
- Keep source-map entry serialization owned by the typed codegen row so report
  boundaries cannot drift from artifact boundaries.
- Keep analyze view selection in one options value instead of widening every
  workspace/ingot/module helper signature for each new report view.
- Keep rendered source-map JSON out of `TestMetadata`; regenerate it from typed
  entries at report/export boundaries.

Phase 11: Cleanup

- Remove old raw-ID and callback-source-of-truth paths.
- Finish terminology migration.
- Remove transitional `IrConsumer` paths or reduce them to export adapters.

## Reconciliation Step

The first reconciliation pass reviewed the coordinating and implementor
sessions plus earlier-session discovery. It confirms the spine and adds these
plan constraints:

- `SourceOrd` was valuable but is source-location tagging, not a full origin
  model.
- `ContentHash` as a tree-only abstraction is insufficient for IR graphs and
  pass provenance.
- Cross-level origin links must be captured online; hashes, facts, reports, and
  debug outputs should be derived later.
- Datalog/Cozo is an exporter/query backend, not compiler state.
- Proof obligations should eventually become native Fe/type/effect concepts,
  not only compiler metadata.
- Passing prototype tests are weak evidence unless they assert exact owner,
  optimization, and fact-schema invariants.

Before implementation, produce a final matrix:

- Original intent.
- Useful ideas to preserve.
- Constraints not captured in this plan.
- Ideas superseded by this design.
- Open questions.
- New risks.

Update the design and phase sequence only after that matrix is explicit.
