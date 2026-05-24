# Fe Origin Graph Instrumentation Architecture

Status: draft
Date: 2026-05-22

## Summary

This document sketches a maintainable replacement architecture for the current
instrumentation prototype. The central idea is to make origins first-class,
typed, Salsa-compatible compiler data.

The current branch proves that source mapping, structural hashing, Datalog
facts, and debug exports are useful. It also shows the main risk: if every layer
invents its own side channel, raw integer IDs, and traversal-specific mappings,
the reports become hard to trust and harder to maintain.

The proposed architecture is an origin graph:

```text
source/HIR origins
  -> semantic origins
  -> MIR origins
  -> Sonatina origins
  -> optimized Sonatina origins
  -> bytecode PC origins
```

The graph is many-to-many. One source construct can create many lowered nodes,
many lowered nodes can merge into one optimized node, and synthetic nodes can be
linked to the construct or phase that caused them.

## Original Intent And Success Criteria

The creation sessions reinforce the current spine: the goal is a trustworthy
compilation record that can support source mapping, structural comparison,
security/query facts, debug views, and later verification workflows.

The architecture should be judged by correctness outcomes, not by the existence
of reports. Success means:

- A bytecode PC can be traced back to the right source construct without body
  guessing.
- A real or synthetic bug can be explained by origin links and shape facts.
- Hash changes are explainable by dimension, graph edge, and origin context.
- Optimized instructions are either linked to pre-opt origins by an explicit
  snapshot alias/pass-lineage edge or classified with an explicit non-source
  reason.
- Tests assert exact invariants, not merely that exporters produce rows.

## Naming

Use "origin" as the normal compiler term. Avoid "provenance" in new APIs unless
interacting with existing names that still need migration.

Preferred names:

- `OriginNode`
- `OriginLink`
- `OriginGraph`
- `OriginChain`
- `OriginResolver`
- `RuntimeBodyOrigins`
- `RuntimePackageOrigins`
- `SonatinaOrigins`
- `OptimizationOrigins`
- `TraceFacts`

Reserve "trace" for exported/user-facing analysis artifacts, not the internal
identity model.

Do not casually use "effect" for generic trace metadata. Fe already has
language-level effect concepts, so exported analysis labels should use names
like `trace_event`, `operation`, `capability`, or a domain-specific relation
unless they really describe Fe effects.

## Goals

- Make origin tracking correct across compiler phases.
- Respect Salsa caching and query purity constraints.
- Remove raw cross-phase `u32` identity assumptions.
- Reduce boilerplate with derives and schema-driven generation.
- Make structural hashes stable, explainable, and testable.
- Generate Datalog/debug/DWARF/ethdebug data from typed origin data.
- Keep heavy analysis/export dependencies out of the core compiler path.
- Reduce long-term boilerplate by generating IR shape descriptions and deriving
  exporters from one typed source of truth.

## Maintainability And LoC Reduction

The overhaul should reduce net code over time. The current prototype pays for
similar information repeatedly:

- Manual IR traversal implementations.
- Raw origin side tables.
- Hash callback protocols.
- Fact callback protocols.
- Debug/source-map resolvers.
- Test-only report adapters.

The target architecture centralizes this into:

- Online origin links only where relationships are otherwise lost.
- A typed `ShapeGraph` description for structural traversal.
- Derive/schema policy for IR field classification.
- Boundary exporters that derive facts, debug views, reports, and hashes from
  the same typed data.

There may be a temporary LoC increase while compatibility shims exist. The
steady-state goal is fewer manual visitors, fewer ad hoc side channels, and
fewer tests that need bespoke fixtures for each exporter.

## Non-Goals

- Do not make all compiler internals externally stable in the first pass.
- Do not require every synthetic node to have a source span.
- Do not put Cozo, DWARF, or ethdebug into the origin core.
- Do not make query bodies emit mutable side effects.

## Salsa Constraint

Salsa queries may be skipped on cache hits, rerun after invalidation, or
evaluated in a different order than expected. Therefore compiler phases must not
emit origin events by mutating global sinks or external collectors from inside
query bodies.

The safe rule is:

```text
tracked query -> returns immutable compiler data
driver/export boundary -> consumes returned data
```

If a phase needs to expose origin information, it should return it as part of a
tracked value or through a sibling query derived from the same inputs.

Examples of unsafe patterns:

- Passing a mutable `FactConsumer` into a Salsa query.
- Appending to a global origin map during lowering.
- Allocating traversal-order origin IDs in a query and treating them as stable.

Examples of safe patterns:

- `runtime_body_origins(db, instance) -> RuntimeBodyOrigins`
- `runtime_package_origins(db, package_key) -> RuntimePackageOrigins`
- Exporting Cozo facts from returned `RuntimePackageOrigins` at the CLI boundary.

## Online Capture Vs Post-Hoc Derivation

Only information that would be lost later should be captured online:

- Cross-level origin links.
- Desugaring origins.
- Split, merge, replacement, and deletion relationships from compiler passes.
- Synthetic-node reasons.

Everything else should be derived post-hoc from typed compiler data:

- Hashes.
- Datalog/Cozo/Souffle/JSON facts.
- DWARF and ethdebug records.
- SCIP-like multi-view debug indexes.
- CLI reports.

This keeps Salsa queries pure while preserving the relationships that traversal
alone cannot reconstruct.

## Relationship To LazySpan And HIR Origin

The existing LazySpan/HIR-origin model is the precedent to preserve.

LazySpan answers:

```text
Where in source did this HIR construct come from?
```

The origin graph answers:

```text
Which compiler artifact came from which earlier compiler artifact?
```

Source span resolution should eventually delegate back to existing LazySpan
machinery where possible. The new architecture should not duplicate source span
resolution logic. It should carry enough typed owner context to call the right
LazySpan resolver without guessing.

## Core Data Model

The source of truth is a typed origin graph.

Sketch:

```rust
pub enum OriginNode<'db> {
    HirExpr {
        body: hir::hir_def::Body<'db>,
        expr: hir::hir_def::ExprId,
    },
    HirStmt {
        body: hir::hir_def::Body<'db>,
        stmt: hir::hir_def::StmtId,
    },
    Semantic {
        instance: hir::analysis::semantic::SemanticInstanceKey<'db>,
        origin: hir::analysis::semantic::SemOrigin<'db>,
    },
    MirStmt {
        instance: mir::RuntimeInstance<'db>,
        block: mir::RBlockId,
        stmt: u32,
    },
    MirTerminator {
        instance: mir::RuntimeInstance<'db>,
        block: mir::RBlockId,
    },
    SonatinaInst {
        stage: SonatinaInstStage,
        func: sonatina_ir::module::FuncRef,
        inst: sonatina_ir::InstId,
    },
    BytecodePc {
        object: ObjectKey,
        section: SectionKey,
        pc_start: u32,
        pc_end: u32,
    },
    Synthetic {
        phase: OriginPhase,
        reason: SyntheticReason,
        owner: SyntheticOwner<'db>,
    },
}

pub struct OriginLink<'db> {
    pub from: OriginNode<'db>,
    pub to: OriginNode<'db>,
    pub kind: OriginLinkKind,
}

pub struct OriginGraph<'db> {
    pub links: Vec<OriginLink<'db>>,
}
```

The exact representation may need adjustment for `salsa::Update`, stable keys,
or serialized export. The important property is owner-aware identity.

## Stable Keys

Raw local IDs are allowed inside one owner. They are not allowed across phase or
body boundaries without their owner.

Bad:

```rust
OriginNodeId { level: Smir, node: expr_id.as_u32() }
```

Better:

```rust
OriginNode::HirExpr { body, expr }
```

For persisted/exported facts, use explicit stable keys:

- HIR expression key: body key plus expression index.
- Semantic key: `SemanticInstanceKey` plus semantic origin.
- MIR statement key: `RuntimeInstanceKey` plus block and statement index.
- Sonatina key: function identity plus instruction identity.
- Sonatina instruction key: function identity plus compilation stage plus
  instruction identity. Pre-opt, post-opt, and backend-prepared `InstId`s are
  different origin nodes even if their numeric IDs match.
- Sonatina synthetic key: synthetic reason in the Sonatina export namespace.
- Bytecode unmapped key: unmapped reason in the bytecode export namespace.
- Bytecode key: object/section identity plus PC range. Sonatina observability
  reports section-local PC offsets, so object-only ownership is insufficient.

Use existing runtime stable-key infrastructure where practical.

## Phase Ownership

Each phase owns only the origin links it can state accurately.

HIR/source:

- Owns source/HIR identity and LazySpan resolution.
- Should not know about MIR or backend instruction IDs.

Semantic/SMIR:

- Owns links from HIR origins to semantic operations.
- Can reuse existing `SemOrigin`, `ValueProvenance`, `PlaceProvenance`, and
  effect-provider origin data.

MIR lowering:

- Owns semantic-to-MIR links.
- Should emit `RuntimeBodyOrigins` as cached return data or a sibling query.
- Must distinguish statements from expressions and terminators.

Sonatina lowering:

- Owns MIR-to-Sonatina links.
- Must include prologue, helper-generated instructions, and terminators.
- Synthetic instructions need classified synthetic origins.
- Sonatina is not Salsa-backed, so pass instrumentation needs explicit
  sidecar/pass-framework support rather than query side effects.

Optimization:

- Owns pre-opt to post-opt Sonatina links.
- Must handle many-to-one, one-to-many, deleted, and newly created instructions.
- Must cover instruction creation, replacement, aliasing, erasure, and
  layout-only moves.
- Current Fe code can only observe optimized-module snapshots from Sonatina.
  Same-`InstId` pre/post joins are therefore `alias` edges, not proof of a
  specific optimization transform. Real `transformed` lineage needs Sonatina
  pass-event hooks or equivalent origin-preserving transforms.

Bytecode emission:

- Owns post-opt Sonatina-to-PC-range links.
- Debug exporters consume this mapping. They should not join optimized PC maps
  against pre-opt origins.
- The immediate source of bytecode PC ranges is either an optimized-snapshot
  post-opt Sonatina instruction or a backend-prepared Sonatina instruction
  reference reported by observability. Pre-opt Sonatina origins currently
  connect through a same-`InstId` snapshot alias edge when one exists.

## Cached Origin Bundles

Introduce phase-local bundles that are cheap to compare and safe for Salsa.

Examples:

```rust
pub struct RuntimeBodyOrigins<'db> {
    pub owner: mir::RuntimeInstance<'db>,
    pub stmt_origins: Vec<(MirStmtKey<'db>, Vec<OriginNode<'db>>)>,
    pub terminator_origins: Vec<(MirTerminatorKey<'db>, Vec<OriginNode<'db>>)>,
    pub links: Vec<OriginLink<'db>>,
}

pub struct RuntimePackageOrigins<'db> {
    pub functions: Vec<RuntimeBodyOrigins<'db>>,
    pub links: Vec<OriginLink<'db>>,
}
```

The current `stmt_origins: Vec<OriginId>` can remain as a convenience cache
during migration, but it should not be the source of truth.

## Export Boundary

Exporters are pure consumers of returned compiler data:

- `TraceFactExporter`
- `DebugExporter`
- `DwarfExporter`
- `EthdebugExporter`
- `JsonOriginExporter`
- `AnalyzeReportExporter`

They can allocate fact IDs, Cozo rows, DWARF sections, and report tables because
they run outside Salsa query bodies.

The exporter boundary is also where heavy optional dependencies should live.

## Hashing Architecture

The current hash consumer mixes tree hashing, graph hashing, and fact traversal.
That is fragile.

Split hashing into explicit layers:

- `ShapeGraph`: typed immutable description of nodes, fields, children, and
  edges.
- `NodeDigest`: local hash from node kind and unordered dimension/name/value
  fields.
- `TreeDigest`: ordered hash of child content.
- `GraphDigest`: canonical hash over node digests plus non-tree edges.
- `DimDigest`: separate dimensions for structure, names, constants, types, and
  explicitly modeled trace events or language effects.
- `HashPolicy`: schema or derive-produced mapping from fields to dimensions.

Important rule:

```text
graph edges augment node hashes; they never disable child hashing globally
```

The current implementation gates that rule with focused regressions: child
content reaches parent tree/exact graph digests, constant-only endpoint changes
do not pollute the structure projection, full edge-label changes affect graph
structure, local field insertion order does not affect digests, child order
does affect tree digests, and `ShapeDescribe` fails closed at downstream
compile-test boundaries.

The production hash policy is isolated in
`crates/common/src/shape/hash.rs`: it owns `StableDigest`,
`DimensionDigests`, node/tree/graph hash aggregation, canonical node/edge
ordering, and dimension projection. `crates/common/src/shape.rs` remains the
public facade and re-exports the hash DTOs without mixing hash policy back into
shape construction. Shape construction is also split by responsibility:
`shape/graph.rs` owns graph identity/types, `shape/describe.rs` owns builder
and `ShapeDescribe` APIs, and `shape/field_value.rs` owns field-value text
conversion.

Process:

1. Describe IR into a `ShapeGraph`.
2. Compute local field digests.
3. Compute ordered child/tree digests.
4. Add CFG/dataflow/callgraph edges.
5. Compute canonical graph digest from the graph, including full edge labels.
6. Project dimensions for dedup, rename-insensitive comparison, exact hashes,
   and user-facing reports.

## Macro And Schema Strategy

The derive macro should become the normal way to describe IR data.

Required capabilities:

- Every field must be classified into a dimension or explicitly skipped.
- Every skip requires a reason.
- Owner/id fields must declare whether they are local, stable, or ignored.
- Enum variants must declare node kind and optional trace events or language
  effects.
- Manual impls should be rare and documented.
- Macro output should support both `ShapeGraph` construction and direct
  exporters if needed.
- Generated HIR/desugaring should flow through normal lowering builders so
  origins and scope registration are created together.

The derive macro should fail closed. Adding a new field without a policy should
fail compilation or tests. The current WIP has `trybuild` coverage for missing
field policies, empty skip reasons, multiple field policies, and unknown
item/field attributes so this stays a compile-time invariant. Empty node kinds
and labels are also rejected during macro parsing, matching the non-empty
`ShapeGraph` and fact-relation invariants.

## Fact Schema

Datalog facts should be generated from typed data, not from callback-time raw
integers.

Principles:

- Each relation declares its ID namespace.
- Joins across namespaces require explicit mapping relations.
- Serialization to Cozo `Int` or `String` happens only at export.
- Fact IDs are export IDs, not compiler identity.

Example relation families:

```text
origin_node(export_id, kind, stable_key)
origin_link(from_id, to_id, kind)
source_span(origin_node_id, file, line, col, end_line, end_col)
shape_node(shape_id, kind, hash_structure, hash_names, ...)
shape_child(parent_shape_id, child_shape_id, ordinal)
shape_edge(source_shape_id, target_shape_id, label)
trace_event(shape_id, event_kind)
data_flow(source_shape_id, target_shape_id, kind)
```

The exact schema can be optimized later. The key is that `shape_id`,
`origin_node_id`, and `fact export id` cannot be confused.

Current implementation checkpoint:

- `TypedFactSet` exports origin and shape facts with explicit fact namespaces
  and stable origin keys.
- `OriginExportKind` uses boundary strings such as `sonatina.inst`,
  `sonatina.synthetic`, `bytecode.unmapped`, and `bytecode.pc` instead of raw
  enum variant names.
- `CodegenOriginNode` can export stable keys for Sonatina instructions,
  synthetic Sonatina nodes, bytecode unmapped reasons, and bytecode PC ranges.
  Sonatina instruction export requires an explicit stable function key; the
  other backend nodes do not depend on a function owner.
- Runtime semantic-to-MIR fact projection is owned by `mir::origin`, and
  bytecode/backend fact projection is owned by `codegen::origin`. CLI/report
  layers supply stable owner labels but do not build phase-specific fact graphs
  themselves.
- Runtime fact graph construction wraps semantic-owner, runtime-owner, and
  synthetic-local labels in distinct nominal types before building fact nodes,
  with downstream compile-fail coverage for namespace mixups.
- Runtime fact export callbacks return a `RuntimeOriginFactOwnerKeys` bundle,
  not raw strings keyed by an enum. MIR derives that bundle from a
  `RuntimeOriginFactTargetKey` plus each `RuntimePackageBodySymbol`, keeping
  semantic/runtime owner namespace separation at the API boundary where stable
  labels enter MIR fact export.
- Runtime package origin bodies carry a `RuntimePackageBodySymbol` rather than
  accepting raw symbol strings at construction. The public constructor requires
  that typed wrapper, and downstream compile-fail coverage blocks raw string
  calls before empty or namespace-ambiguous runtime labels can enter runtime
  origin summaries or fact-owner derivation. `RuntimePackageOrigins::new`
  sorts bodies by symbol and rejects duplicate runtime body symbols as well as
  duplicate instances, because the default fact-owner policy derives stable
  semantic/runtime namespaces from those symbols.
- Repeated closed-string enum boilerplate is centralized in
  `define_closed_string_enum!`. Origin export kinds, origin link kinds, fact
  namespaces, shape dimensions/scopes, source-span kinds, and codegen
  debug/origin reason enums now share one generated `STRINGS`/`as_str`/
  `from_str`/Serde policy, preserving the same wire strings while making future
  closed classifications less error-prone to add.
- HIR and semantic export helpers now use typed owner-key boundaries:
  HIR expr/stmt export requires `HirOriginBodyOwnerKey`, and semantic-origin
  export is restricted to types that implement the semantic owner-key marker.
- Runtime statement/terminator export helpers are restricted to runtime
  owner-key marker types, so helper calls cannot reintroduce raw owner strings
  after fact graph nodes have been typed.
- Runtime terminator origins now use `RuntimeTerminatorSite` as their typed
  local key instead of accepting a raw `RBlockId` in the primary constructor.
  The previous public string helper for terminator local keys is gone; MIR and
  codegen synthetic-local wrappers derive their labels through the typed site.
- HIR semantic origins, MIR runtime sites, and bytecode PC ranges do not expose
  public inherent helpers that return raw export-local-key strings. Export-key
  construction goes through typed local-key wrappers or the shared
  `OriginExportLocalKey` trait at internal adapter points.
- Codegen end-to-end fact graph construction applies the same nominal owner
  separation before combining semantic, runtime, Sonatina, and bytecode nodes
  into one allocation. The semantic/runtime owner-key pair is derived through
  `EndToEndOriginOwnerKeys::for_function`, which accepts a typed
  `SonatinaFunctionExportKey` instead of a raw function label.
- Sonatina function export keys are nominal wrappers in codegen origin APIs.
  Stable function-key callbacks must return that wrapper, not a raw `String`,
  before instruction export keys or frontend-provenance labels can be derived.
- Frontend origin-label derivation uses the same checked key policy for runtime
  stmt/terminator labels. Missing stable function keys now fail through
  `MissingSonatinaFunctionKey` instead of silently dropping runtime labels
  before Sonatina observability JSON is emitted; synthetic and unmapped
  same-ID records intentionally produce no frontend label.
- Codegen stable function-key collection is centralized behind an internal map
  shared by codegen-only and end-to-end fact export paths. This keeps missing
  function-key errors, deduplication, and checked export-key construction in
  one place instead of open-coding `FuncRef` maps per graph flavor. The stable
  `SonatinaFunctionExportKey`, map, collector, and `MissingSonatinaFunctionKey`
  error now live in `crates/codegen/src/origin/function_keys.rs`, with public
  types re-exported from `codegen::origin`.
- Bytecode source-resolution DTOs and resolver helpers now live in
  `crates/codegen/src/origin/source_resolution.rs`, keeping source-span
  resolution policy separate from codegen origin identity, graph construction,
  and fact-export plumbing.
- Codegen-only and end-to-end graph/fact export plumbing now live in
  `crates/codegen/src/origin/codegen_graph.rs` and
  `crates/codegen/src/origin/end_to_end_graph.rs`. The parent `origin` module
  still re-exports the public graph/node APIs, while object/package builders use
  narrow internal helper hooks.
- Bytecode object, section, PC-range, PC-origin, and unmapped-reason identity
  types now live in `crates/codegen/src/origin/bytecode_keys.rs`. Package
  construction and source resolution still consume those types through the
  parent `codegen::origin` re-exports.
- Codegen origin regressions now live behind
  `crates/codegen/src/origin/tests.rs`, with focused
  `crates/codegen/src/origin/tests/` modules for coverage, Sonatina records,
  frontend labels, bytecode origins, export keys, fact export, backend-prepared
  fallback, post-opt snapshot lineage, and graph shape. This keeps the same
  `origin::tests::*` test names while removing the large fixture block from the
  implementation facade.
- Codegen Sonatina and bytecode origin implementation is split by phase:
  `sonatina_pre_opt.rs` owns MIR-to-pre-opt Sonatina records,
  `sonatina_post_opt.rs` owns optimized-snapshot, backend-prepared, and
  snapshot-loss records. `bytecode_origins.rs` owns PC-map ingestion,
  source-span resolution entry points, object/section filtering, and package
  orchestration; `bytecode_coverage.rs` owns bytecode origin coverage counting;
  `bytecode_graph.rs` owns bytecode fact graph projection; and
  `frontend_labels.rs` owns frontend-origin labels plus pre-opt source label
  classification. The parent `codegen/src/origin.rs` is now a compact public
  facade over focused modules.
- `FrontendOriginLabelMap` is also nominal on the Fe side. It wraps Sonatina's
  raw `FrontendProvenanceMap` and exposes that map only through an explicit
  adapter at the observability boundary, keeping external "provenance" types out
  of origin-facing APIs. Insertion requires a nominal `FrontendOriginLabel`, so
  public callers cannot attach arbitrary raw strings to Sonatina observability
  rows. The label wrapper and map now live in
  `crates/codegen/src/origin/frontend_labels.rs`, with public re-exports kept
  on `codegen::origin`.
- `define_origin_string_key!` and `define_origin_owner_key!` centralize the
  boilerplate for nominal exported key wrappers, so adding these type barriers
  does not require repeatedly hand-writing identical `String` tuple structs and
  accessors. Owner-key wrappers also implement the shared export-owner marker
  trait used by the stricter helper signatures. Generated string and owner
  wrappers reject empty strings and the reserved origin storage separator at
  construction, so malformed stable labels fail before export-key allocation.
- `define_origin_key_type!` centralizes the equally regular
  `OriginKey<Owner, Local>` newtype pattern. HIR expr/stmt/semantic origins and
  MIR runtime stmt/terminator origins use it for their private key fields,
  constructor, owner/local accessors, and `salsa::Update` derive. The macro does
  not expose a raw-key accessor; consumers stay on nominal wrappers plus typed
  export-key helpers. Custom origins whose constructors enforce additional
  invariants, such as Sonatina instruction stages or bytecode PC ranges, remain
  hand-written.
- The generated key wrappers derive `salsa::Update`. The generic
  `OriginKey`, `OriginLink`, and `OriginGraph` containers keep manual
  `salsa::Update` impls because deriving would force bounds onto the generic
  type definitions; those impls are documented and tested as shared
  infrastructure, including no-op and changed fieldwise update behavior.
- Public phase-specific origin graph APIs use nominal graph wrappers generated
  by `define_origin_graph_type!`, not `OriginGraph<Node>` type aliases. This
  keeps HIR, MIR, codegen, and end-to-end graph values distinct at crate
  boundaries while avoiding repeated wrapper boilerplate.
- `OriginExportKey` owns export-key validation and formatting policy. Owner and
  local parts must be non-empty and cannot contain the reserved canonical
  storage separator; the same validation runs during JSON deserialization.
  Fact allocation uses `canonical_storage_key()`, while diagnostics and
  frontend provenance use `display_label()`. Compiler construction sites use
  typed owner/local key traits through `OriginExportKey::new` or `try_new`;
  decoded relation rows and JSON artifacts use the explicitly named
  `try_from_raw_parts` boundary. Local-key wrappers are generated with
  `define_origin_local_key!`, matching the existing owner-key wrapper pattern.
  Origin owner/local/string key macros now emit `try_new` constructors with a
  shared `OriginKeyTextError`; `new` remains the trusted panic wrapper for
  internal construction, while tests and import/report boundaries can assert
  invalid key text directly.
- Common origin-core regression coverage now lives in
  `crates/common/src/origin/tests.rs`. The production common origin core is
  split by responsibility: `origin/key.rs` owns `OriginKey`,
  `origin/export_key.rs` owns export kinds, stable key validation, and typed
  owner/local traits, `origin/graph.rs` owns link kinds plus graph containers,
  and `origin/macros.rs` owns the exported helper macros.
  `crates/common/src/origin.rs` remains the module/re-export facade,
  preserving all public `common::origin` paths.
- Common shape construction is split by responsibility:
  `shape/graph.rs` owns graph identity/types, `shape/describe.rs` owns builder
  and `ShapeDescribe` APIs, `shape/field_value.rs` owns field-value text
  conversion, and `shape/hash.rs` owns deterministic hashing. The parent
  `shape.rs` stays as the stable `common::shape` facade and derive-facing test
  host.
- `TypedFactSet` intentionally does not concatenate independently exported fact
  sets. Fact IDs are allocation-local, so a combined cross-IR view must build
  one typed graph and export it once.
- Fact ID namespace and allocation infrastructure now lives in
  `crates/common/src/facts/ids.rs`, while `common::facts` keeps re-exporting
  `FactId`, `FactNamespace`, `FactNamespaceError`, and `FactIdAllocator`. This
  starts splitting the large fact module around allocation-local identity before
  relation and query code are moved.
- Origin node/link fact DTOs and namespace validation now live in
  `crates/common/src/facts/origin_fact.rs`, again with `common::facts`
  preserving the public re-export path. Origin graph export, reachability
  indexing, and relation-table validation continue to consume them through
  dedicated sibling modules.
- Origin reachability summaries, origin paths, path-witness exports, and
  source-path witness exports now live in
  `crates/common/src/facts/origin_path.rs`, again with `common::facts`
  preserving the public re-export path. Query traversal now lives in the
  origin-fact index module; the DTO module owns only constructor and serde
  validation for those query results.
- Origin path DTO code is split below that facade:
  `origin_path/reachability.rs` owns `OriginReachabilitySummary` and per-kind
  aggregate validation; `origin_path/path.rs` owns internal fact-ID paths and
  kind-pair witnesses; `origin_path/witness.rs` owns stable export-key path
  witnesses; and `origin_path/source_witness.rs` owns source-span-attached path
  witnesses. Public `common::facts::*` re-exports remain stable.
- Origin path witness code is split below `origin_path/witness.rs`.
  `witness/error.rs` owns validation diagnostics, `witness/record.rs` owns the
  `OriginPathWitnessExport` DTO, and `witness/deserialize.rs` owns fail-closed
  JSON reconstruction.
- Origin reachability DTOs are split below `origin_path/reachability.rs`:
  `reachability/summary.rs` owns `OriginReachabilitySummary` and fail-closed
  serde reconstruction; `reachability/pair.rs` owns per-kind pair DTOs;
  `reachability/validation.rs` owns duplicate/total checks; and
  `reachability/error.rs` owns user-facing validation errors.
- Typed fact export code is split below that facade.
  `typed_fact/export.rs` owns `OwnedTypedFactSetExport`, `TypedFactSetExport`,
  schema-version validation, and origin/shape index validation for imported
  exports; `typed_fact/fact.rs` owns the `TypedFact` enum plus per-variant
  serde mapping. Public `common::facts::*` re-exports remain stable.
- `TypedFact` is split below `typed_fact/fact.rs`. The parent module owns only
  the enum; `typed_fact/fact/serialize.rs` owns stable per-variant JSON
  encoding; and `typed_fact/fact/deserialize.rs` owns the fail-closed tagged
  decoder and constructor validation. The wire schema and public re-exports
  remain stable.
- `TypedFact` decoding is split one level deeper:
  `typed_fact/fact/deserialize.rs` is a compact serde entry point,
  `deserialize/raw.rs` owns the tagged wire enum, and
  `deserialize/construct.rs` owns conversion into validated `TypedFact`
  variants through the fact constructors.
- Typed fact relation names, column names, schema descriptors, and raw column
  matching now live in `crates/common/src/facts/relation_schema.rs`, again with
  `common::facts` preserving the public re-export path.
- Relation schema code is split below that facade.
  `relation_schema/name.rs` owns the closed relation name enum plus origin/shape
  relation classification; `relation_schema/column.rs` owns the closed column
  enum; and `relation_schema/schema.rs` owns schema descriptors, raw-name
  lookup, column matching, and column indexing. Public `common::facts::*`
  re-exports remain stable.
- Relation schema descriptor/catalog code is split one level deeper.
  `relation_schema/schema.rs` is a compact facade; `schema/descriptor.rs` owns
  `TypedFactRelationSchema` and relation-name lookup helpers; and
  `schema/catalog.rs` owns the fixed schema catalog, raw-name lookup, and column
  matching. The relation-table wire schema remains unchanged.
- Typed relation table DTOs, relation-count DTOs, relation-row views, and
  relation JSON validation errors now live in
  `crates/common/src/facts/relation.rs`, again with `common::facts` preserving
  the public re-export path. Relation semantic validation and query indexing
  live behind the relation-index module. That module is now a small facade over
  `relation_index/origin_paths.rs` for relation-backed reachability/path
  queries and source-span summaries, plus `relation_index/validation.rs` for
  semantic row/reference/source-span/hash validation.
- Relation table code is split below that facade. `relation/set.rs` owns
  `TypedFactRelationSet`; `relation/table.rs` owns `TypedFactRelation`;
  `relation/count.rs` owns `TypedFactRelationCount`; `relation/row.rs` owns
  relation row views; `relation/error.rs` owns relation diagnostics; and
  `relation/validation.rs` owns schema-version, column, and row-width
  validation. Public `common::facts::*` re-exports remain stable.
- Relation diagnostics are split below `relation/error.rs`: the public
  `TypedFactRelationError` enum stays at the stable re-export path, while
  `relation/error/display.rs` owns display text for relation import,
  validation, source-span, and shape-hash diagnostics.
- Shape-hash scope/key/digest/fact DTOs and validation errors now live in
  `crates/common/src/facts/shape_hash.rs`, again with `common::facts`
  preserving the public re-export path. Digest canonicalization and node/scope
  invariants stay beside the constructors while relation validation and query
  indexing continue to consume them through the parent module.
- Shape-hash code is split below that facade. `shape_hash/scope.rs` owns the
  closed string scope enum; `shape_hash/key.rs` owns lookup keys plus
  node/scope invariants; `shape_hash/digest.rs` owns canonical digest
  validation; and `shape_hash/fact.rs` owns fact construction and serde
  validation. Public `common::facts::*` re-exports remain stable.
- Source-span kind/export/fact/file-count DTOs and validation errors now live
  in `crates/common/src/facts/source_span.rs`, again with `common::facts`
  preserving the public re-export path. Span range/file validation stays beside
  the serde constructors while graph indexing and relation-table validation
  continue to consume source-span facts through the parent module.
- Source-span fact code is split below that facade:
  `source_span/export.rs` owns `SourceSpanKind`, `SourceSpanExport`, and shared
  range/file validation; `source_span/fact.rs` owns allocated `SourceSpanFact`
  rows and namespace validation; and `source_span/file_count.rs` owns compact
  per-file summary DTOs. Public `common::facts::*` re-exports remain stable.
- Source-span export code is split below that facade as well:
  `source_span/export/kind.rs` owns the closed span-kind enum;
  `export/error.rs` owns validation errors; `export/validation.rs` owns shared
  file/range checks; and `export/record.rs` owns `SourceSpanExport`,
  fail-closed serde construction, and deterministic sort keys.
- Source-span fact code is split below `source_span/fact.rs` too:
  `source_span/fact/error.rs` owns origin-namespace/span validation error
  conversion and display text; `source_span/fact/record.rs` owns
  `SourceSpanFact`, namespace-checked construction, source-span export
  attachment, and fail-closed serde reconstruction.
- Source-span record serde is split below the export/fact record modules.
  `export/record/deserialize.rs` and `fact/record/deserialize.rs` own raw
  fail-closed JSON reconstruction; `export/record/sort_key.rs` owns
  deterministic export ordering. Constructors and accessors stay in the record
  modules that define the DTOs.
- Shape node/field/child/edge, trace-event, and data-flow fact DTOs now live
  in `crates/common/src/facts/shape_fact.rs`, again with `common::facts`
  preserving the public re-export path. Shape-node namespace checks and
  non-empty text validation stay beside the constructors while relation
  validation and query indexing remain in the parent module.
- Shape fact code is split below that facade. `shape_fact/text.rs` owns shared
  shape-node namespace and non-empty text validation; `shape_fact/node.rs` owns
  `ShapeNodeFact`; `shape_fact/field.rs` owns `ShapeFieldFact`;
  `shape_fact/edge.rs` owns child/edge facts; `shape_fact/trace_event.rs` owns
  trace-event facts; and `shape_fact/data_flow.rs` owns data-flow facts. Public
  `common::facts::*` re-exports remain stable.
- Origin and shape graph fact export builders now live in
  `crates/common/src/facts/graph_export.rs`, again with `common::facts`
  preserving the public re-export path. This keeps allocation-time projection
  from typed origin/shape graphs into fact rows separate from relation
  validation and query indexes.
- Graph export is split below that facade. `graph_export/origin.rs` owns origin
  graph key/link deduplication and fact ID allocation; `graph_export/shape.rs`
  owns shape graph node/field/edge/hash/trace/data-flow projection. Public
  `common::facts::*` re-exports remain stable.
- Typed fact relation export projection now lives in
  `crates/common/src/facts/relation_export.rs`. It owns converting typed fact
  variants into sorted relation rows for the declared schemas, while relation
  validation and query indexes stay in the parent module.
- Relation export is split below that facade. `relation_export/cell.rs` owns
  fact-ID and graph-scope cell formatting; `relation_export/row.rs` owns
  per-variant typed fact row projection with schema-width assertions; and
  `relation_export/set.rs` owns deterministic row sorting and relation-set
  construction. Public `common::facts::*` re-exports remain stable.
- The `TypedFactSet` container and its typed iterator/source-span attachment
  facade now live in `crates/common/src/facts/typed_fact_set.rs`, keeping the
  fact-set container separate from relation-table validation and query
  indexing.
- `TypedFactSet` code is split below that facade. The parent module owns
  storage plus export/relation-export adapters; `typed_fact_set/iterators.rs`
  owns typed per-variant iterators generated from one local macro; and
  `typed_fact_set/source_spans.rs` owns deterministic source-span attachment.
  Public `common::facts::*` re-exports remain stable.
- Shared fact-index and source-span attachment error types now live in
  `crates/common/src/facts/index_error.rs`, keeping the error formatting and
  namespace/text guard helpers out of the parent index implementation while
  preserving `common::facts::*` re-exports.
- Index diagnostics are split below that facade. `index_error/fact_index.rs`
  owns `FactIndexError` and its display text; `index_error/source_span.rs`
  owns `SourceSpanFactError`; and `index_error/helpers.rs` owns namespace/text
  guard helpers consumed by origin and shape indexes. Public
  `common::facts::*` re-exports remain stable.
- Fact-index diagnostics are split below `index_error/fact_index.rs`: the
  public `FactIndexError` enum stays at the stable re-export path, while
  `index_error/fact_index/display.rs` owns display text for origin,
  source-span, shape, and shape-hash index diagnostics.
- `OriginFactIndex` now lives in `crates/common/src/facts/origin_index.rs`,
  preserving `common::facts::OriginFactIndex` while keeping typed-fact graph
  traversal, source-span validation, reachability summaries, and path witnesses
  out of the parent facts module.
- `OriginFactIndex` is now a compact facade over focused implementation
  modules. `origin_index/build.rs` owns typed-fact index construction and
  endpoint/source-span validation; `origin_index/source_spans.rs` owns
  source-span lookups; `origin_index/reachability.rs` owns reachability sets
  and summaries; and `origin_index/paths.rs` owns shortest paths plus
  path-witness exports. Public `common::facts::*` re-exports remain stable.
- Origin-index path queries are split below `origin_index/paths.rs`:
  `paths/search.rs` owns shortest-path BFS and stable-key path lookup;
  `paths/representative.rs` owns representative kind-pair witness selection;
  and `paths/exports.rs` owns stable export-key witness projection plus
  priority-ordered export selection.
- `ShapeFactIndex` now lives in `crates/common/src/facts/shape_index.rs`,
  preserving `common::facts::ShapeFactIndex` while keeping shape-node/hash
  lookup and completeness validation out of the parent facts module.
- `ShapeFactIndex` is a compact facade over focused modules.
  `shape_index/build.rs` owns typed fact indexing, namespace/text/reference
  validation, and required hash coverage checks; `shape_index/lookup.rs` owns
  source-id/stable-key/node/hash lookup APIs. Public `common::facts::*`
  re-exports remain stable.
- `TypedFactRelationIndex` now lives in
  `crates/common/src/facts/relation_index.rs`, preserving
  `common::facts::TypedFactRelationIndex` while keeping relation-table semantic
  validation, relation-backed reachability/path queries, source-path witnesses,
  and shape-hash relation completeness checks out of the parent facts module.
- Relation-index validation helpers are split below
  `relation_index/validation/helpers.rs`: `helpers/ids.rs` owns relation
  fact-ID collection and namespace checks; `helpers/uniqueness.rs` owns
  duplicate-key checks; `helpers/references.rs` owns cross-relation reference
  checks; and `helpers/cells.rs` owns non-empty, closed-value, and numeric cell
  validation.
- Relation-backed origin query code is split below that facade:
  `relation_index/origin_paths/graph.rs` owns origin-node/link relation
  decoding and deterministic reachability ordering; `path_search.rs` owns
  shortest-path reconstruction; and `source_spans.rs` owns source-span relation
  projection plus per-file summaries. The public
  `TypedFactRelationIndex` query API remains unchanged.
- Relation-backed graph decoding is split below `origin_paths/graph.rs`:
  `graph/nodes.rs` owns origin-node row decoding and export-key reconstruction;
  `graph/links.rs` owns origin-link row decoding and deterministic
  outgoing-edge ordering; and `graph/ordinals.rs` owns `origin_node:` fact-ID
  parsing shared with source-span relation joins.
- The high-level query API is also split by responsibility:
  `origin_paths/reachability.rs` owns reachability summaries;
  `origin_paths/paths.rs` owns plain path witness queries;
  `origin_paths/source_paths.rs` owns source-span-attached path witnesses; and
  `origin_paths/source_counts.rs` owns source-span file counts. Public
  `TypedFactRelationIndex` query APIs remain stable.
- Plain relation-backed path queries are split below `origin_paths/paths.rs`:
  `paths/between_keys.rs` owns exact stable-key path lookup;
  `paths/representative.rs` owns kind-pair representative lookup;
  `paths/priority.rs` owns priority-ordered witness selection; and
  `paths/export.rs` owns shortest-path witness construction shared by
  source-path queries.
- Relation-backed source-span queries are split below
  `origin_paths/source_spans.rs`: `source_spans/columns.rs` owns source-span
  relation column lookup, `source_spans/decode.rs` owns relation-row
  reconstruction into typed `SourceSpanExport` values, and file-count
  aggregation lives in `origin_paths/source_counts.rs`.
- Relation semantic validation is split below the same facade:
  `validation/helpers.rs` owns shared ID/reference/cardinality and cell-shape
  checks; `validation/origin_keys.rs` owns stable origin-key validation;
  `validation/source_spans.rs` owns source-span row validation; and
  `validation/shape_hashes.rs` owns shape-hash node/scope/dimension/digest
  completeness checks.
- The common facts unit tests now live under focused modules in
  `crates/common/src/facts/tests/`, with `tests.rs` keeping shared helpers and
  module declarations. This keeps graph/export, relation-query, schema, JSON,
  path-witness, and shape-index fixtures from accumulating in one large test
  facade.
- Build-report and test-bytecode origin fact artifacts now use a combined
  end-to-end origin graph for each bytecode object/test, so runtime,
  Sonatina, and bytecode nodes share one coherent fact-ID allocation.
- `OriginFactIndex` is the first typed query surface over exported origin
  facts. It indexes `OriginExportKey` to fact IDs, validates link endpoints, and
  supports exact reachability queries without binding the compiler to a specific
  Datalog engine.
- The first real artifact regression using this query surface checks that an
  emitted test-bytecode fact set contains a path from a runtime origin node to a
  bytecode PC node.
- The same index now derives a reusable reachability summary grouped by origin
  kind. This lets report boundaries expose semantic-to-runtime and
  runtime-to-bytecode query results without adding a separate graph walker or
  mutable analysis sink.
- The reachability summary DTO owns its aggregate invariants. Decoding rejects
  zero-count kind-pair rows, duplicate kind pairs, unknown fields, and total
  counts that do not equal the per-kind sum, so JSON reports cannot claim graph
  coverage that the grouped rows do not support.
- `OriginFactIndex` also exposes deterministic shortest-path witnesses over
  fact IDs. This is deliberately engine-agnostic: later CLI, debug, or Datalog
  adapters can explain one path using shared indexed facts instead of each
  maintaining a custom BFS implementation.
- `fe analyze --origin-facts` consumes those witnesses at the report boundary.
  JSON reports include representative paths as stable origin export keys plus
  link kinds; text output renders compact path chains with the same stable
  labels and link kinds. The JSON boundary uses
  `common::facts::OriginPathWitnessExport`, so CLI, debug, report, and query
  exporters can share one stable path-witness schema.
- Witness selection is now typed and priority-aware. Callers can request a
  representative path for a specific origin-kind pair, and `fe analyze` uses a
  stable priority list for the joins that explain the current compilation
  record: semantic-to-runtime, semantic-to-bytecode, runtime-to-Sonatina,
  runtime-to-bytecode, Sonatina-to-bytecode, and explicit bytecode-unmapped
  classifications. This prevents a small witness limit from hiding the paths
  most useful for review and debugging.
- The query layer also has stable-key path helpers. That keeps external
  adapters and report code from depending on allocation-local `FactId`s when
  the question is naturally "does exported origin key A reach exported origin
  key B?"
- `TypedFactSet::export()` wraps facts in `schema_version: 1` for JSON report
  artifacts. The core fact set remains typed compiler/export data, while JSON is
  only a boundary representation.
- `OwnedTypedFactSetExport` can be deserialized back from that boundary JSON.
  String-tagged origin kinds, link kinds, fact namespaces, shape dimensions, and
  shape hash scopes have explicit parsers, and round-trip tests re-index decoded
  origin facts. Unknown schema versions, export fields, fact-row fields, and
  nested origin-key fields are rejected during deserialization. This keeps
  future query backends from depending on ad hoc or silently widening JSON shape
  knowledge.
- Origin export keys are also validated on construction and deserialization:
  malformed owner/local parts fail before they can collide in fact allocation or
  leak into serialized query artifacts.
- Deserializing `OwnedTypedFactSetExport` also builds `OriginFactIndex`, so
  origin fact JSON rejects duplicate origin IDs/keys, duplicate origin links,
  and links to missing origin nodes at the boundary instead of deferring
  malformed graph discovery to a later query consumer.
- Origin node, origin link, and source-span fact constructors/serde reject
  non-`origin_node` fact IDs before whole-set indexing. Whole-set checks such as
  missing endpoints, missing source-span origins, and duplicate origin keys stay
  in `OriginFactIndex`.
- Build-report and Fe test-report regressions parse emitted `origin_facts.json`
  and `snapshot_origin_facts.json` through `OwnedTypedFactSetExport`. That keeps
  report artifact production pinned to the same typed schema boundary as
  downstream query/debug consumers instead of validating only loose JSON fields.
- `fe analyze` JSON regressions do the same for embedded origin/shape fact
  payloads and decode embedded relation-table payloads through
  `TypedFactRelationSet`. CLI JSON remains a boundary representation, but its
  tests now exercise the same schema validators as future query adapters.
- Analyze origin-fact report decoding now validates count fields against the
  embedded typed fact payload: total, origin-node, origin-link, source-span,
  source-span file summaries, relation counts, and relation-table rows must
  agree. Duplicate source-span file summaries, duplicate relation summaries,
  empty identity fields, non-origin relation summaries, and populated
  non-origin relation tables are rejected in origin-fact reports so shape/query
  rows cannot hide behind an origin report label.
- Analyze origin-fact tests also decode reachability summaries and path-witness
  payloads through `OriginReachabilitySummary` and `OriginPathWitnessExport`.
  Source-path witnesses decode through `OriginSourcePathWitnessExport`, including
  the embedded `SourceSpanExport`, so source-path report tests no longer need raw
  source-span JSON probes.
- The path-witness DTOs now own their local invariants. `OriginPath`,
  `OriginPathWitnessExport`, and `OriginSourcePathWitnessExport` reject empty
  paths, node/link arity mismatches, non-`origin_node` path IDs, first/last
  origin-kind mismatches, and source spans whose origin key does not match the
  terminal path node. Malformed witness JSON fails before a report, debug, or
  query consumer can treat it as a valid explanation path.
- `fe analyze --source-maps` JSON regressions parse embedded source-map entries
  through `BytecodeSourceMapEntry` and coverage payloads through the typed
  coverage DTOs. This keeps source-map report tests aligned with codegen's
  source-map entry and partition validators instead of duplicating permissive
  `kind`/count JSON checks.
- Analyze source-map report decoding now validates its own summary counts:
  source plus non-source must equal total, non-source classifications must sum
  to non-source, debug-location and line-table row counts must match source,
  line-table file counts cannot exceed source, bytecode-origin coverage totals
  must match report totals, and present entry rows must match the reported total
  plus per-kind classification counts. Source-map scope, label, object, section,
  and optional test names must be non-empty. Full entry rows must agree with
  the report object and with the report section, except for the explicit
  `<all>` aggregate section sentinel. Entry rows remain optional for compact
  reports.
- Analyze shape report decoding now validates graph hash dimensions and digest
  text, checks embedded shape facts against report counts and graph hashes, and
  verifies relation-count/table summaries against the report's typed shape
  counts. Shape report scope and label must be non-empty. Compact shape hash
  reports can still omit fact rows.
- Origin-fact and shape report relation summaries now share one validation
  helper for duplicate relation counts, unexpected relation families, and table
  count drift. Each report supplies only its expected relation-count policy and
  error mapping, reducing the chance that origin and shape summary invariants
  diverge as relation schemas evolve.
- When relation tables are embedded in analyze reports, the rows must match the
  report's typed facts exactly after relation/row normalization. Same-count but
  different owner/local keys, span cells, or shape hash rows are rejected at the
  report boundary. Shape reports also reject relation tables without the typed
  facts needed to verify them.
- Analyze relation-count summaries keep the sparse format: zero-row relations
  may be omitted. For reports carrying typed facts, every non-empty relation in
  `facts.relation_export()` must have a matching relation-count row, so compact
  summaries cannot silently omit populated relation families.
- Focused `fe analyze` JSON regressions deserialize the full output through
  `AnalyzeReport`, whose nested DTOs reject unknown fields. Assertions now read
  typed source-map, origin-fact, shape, relation-count, reachability, and path
  witness fields directly instead of keeping a permissive top-level
  `serde_json::Value` layer.
- The `AnalyzeReport` boundary also rejects unsupported report schema versions,
  empty profiles, unknown package kinds, duplicate target labels, and target
  body/statement/terminator count drift, and producers validate the report
  before rendering. This keeps compact top-level summaries aligned with their
  emitted typed child rows. Target labels and body-row symbols are non-empty at
  their own DTO boundaries, while duplicate body symbols are rejected within
  each target.
- The analyze report boundary now lives behind a dedicated
  `analyze/report.rs` facade. Focused `analyze/report/` submodules own
  source-map report DTOs, origin-fact report DTOs, shape report DTOs, shared
  relation validation, and origin-count helpers. Text and JSON report rendering
  live in `analyze/render.rs`. Sonatina/codegen source-map and origin-fact
  report assembly lives in `analyze/codegen_reports.rs`, while `analyze.rs`
  remains responsible for target resolution, compilation, shape summaries,
  runtime origin summaries, and top-level report assembly.
- Analyze report tests now live under focused modules in `analyze/tests/`, with
  `analyze/tests.rs` keeping shared helpers and module declarations. This keeps
  report DTO schema fixtures, origin-fact/source-map/shape boundary fixtures,
  and CLI integration regressions from accumulating in one large test file.
- Analyze source-map report construction is fallible at the same schema
  boundary. Inconsistent codegen/source-map summaries, coverage totals, or
  entry identities are reported through `fe analyze` error handling instead of
  panicking while assembling a report.
- Analyze source-map entry validation reuses `codegen::debug` source-map
  summaries instead of keeping a second entry-kind classifier in the CLI report
  layer. Full-entry reports now validate classification counts and debug
  line-table file/row counts against the same codegen summary logic used to
  produce source-map summaries.
- Analyze runtime origin count rows now validate `total == semantic +
  synthetic` when decoded, so runtime statement and terminator summaries cannot
  claim aggregate counts that their semantic/synthetic partitions do not
  support.
- Sonatina public test metadata uses the same source-map schema boundary in its
  regression tests: generated source-map JSON is deserialized through
  `OwnedBytecodeSourceMapExport` before checking source entries.
- The same deserialization path builds `ShapeFactIndex`. Shape fact JSON rejects
  duplicate shape IDs/source IDs/stable keys, shape fields/children/edges/hash
  rows that reference missing nodes, and hash rows whose node reference does not
  match local/tree vs graph scope. Shape fact constructors and serde boundaries
  reject empty stable keys, node kinds, field names, child/edge labels, trace
  event kinds, data-flow kinds, non-`shape_node` fact IDs, non-canonical
  digests, and invalid shape-hash node/scope states before indexing, while
  field and trace-event values may still be empty. Shape fact indexing remains
  defensive for duplicate `(node, scope, dimension)` keys and incomplete graph
  or per-node local/tree hash coverage, so downstream query adapters do not
  need to guess whether a missing hash means "not exported" or "not computed".
  `ShapeFactIndex` exposes typed graph/local/tree hash lookups keyed by
  `ShapeHashFactKey`, keeping that validated shape usable without ad hoc row
  scans in every consumer.
- Shape fact export now includes typed `trace_event` and `data_flow` relations
  derived from the same `ShapeGraph` data. Trace events come from fields in the
  trace-event dimension; data-flow rows mirror graph edges with typed
  source/target endpoints. The generic field/edge rows remain available, but
  query backends no longer need to parse those generic rows for the common
  relation shape.
- `TypedFactSet::relation_export` now provides the backend-neutral table
  boundary for query engines. It emits fixed schemas for origin nodes/links,
  source spans, shape nodes/fields/children/edges, trace events, data flow, and
  shape hashes. Adapters can translate those tables into Cozo, Souffle, JSON, or
  another store without making the compiler core depend on that engine or
  passing mutable sinks through Salsa queries.
- `fe analyze --fact-relation-tables --format json` exposes those relation
  tables for emitted origin and shape fact sets. It requires `--origin-facts` or
  `--shape-facts`, so normal summaries stay compact while backend prototypes
  and reviewers can consume the same table-shaped data the adapters will use.
- `TypedFactRelationSet` also deserializes as a closed schema. It rejects
  unsupported schema versions, unknown relation names, duplicate tables, missing
  required tables, wrong fixed columns, row-width mismatches, and unknown fields.
  This keeps the relation-table artifact trustworthy before a specific query
  engine is introduced. Public relation lookups use the closed
  `TypedFactRelationName` enum rather than raw relation-name strings, keeping
  query/report code aligned with the fixed schema at compile time while
  preserving the JSON wire names. Public column lookups similarly use the
  closed `TypedFactRelationColumnName` enum for `rows_where`, `column_index`,
  and relation-row `cell` access, so query code cannot spell arbitrary column
  strings even though the exported JSON column names stay unchanged.
- Public relation-table construction is also schema-driven:
  `TypedFactRelation::new` takes `TypedFactRelationName` and derives columns
  from the fixed schema, while `TypedFactRelationSet::new` validates the full
  required relation set at construction. Raw relation names and raw column lists
  are confined to serde/import validation.
- Relation schema metadata is exposed as `TypedFactRelationSchema` through
  `TypedFactRelationName::schema` and `typed_fact_relation_schemas()`. Export,
  validation, relation counts, and schema tests use that descriptor instead of
  carrying parallel name/column tuples.
- Relation export uses the same declared schema table to populate column names,
  so the exporter, decoder, and query index no longer carry three independent
  copies of relation column order. Schema-order tests keep this boundary
  explicit.
- Relation export also accumulates rows through a schema-initialized collector
  keyed by `TypedFactRelationName`. This removes the prior parallel vectors and
  manual relation assembly list. `TypedFact` owns the fact-to-row projection,
  while the collector checks row width against the descriptor and keeps final
  table order tied to `typed_fact_relation_schemas()`.
- Relation-count summaries carry `TypedFactRelationName` internally and only
  stringify at report DTO boundaries. This keeps compact CLI summaries aligned
  with the same closed relation schema as table export and query APIs. Analyze
  report DTOs also reject duplicate relation summaries so imported compact
  views preserve the same one-row-per-relation shape as produced query indexes.
- Relation-row query results carry `TypedFactRelationName` as well. Cell lookup
  uses `TypedFactRelationName::schema()` instead of the exported JSON column
  vector, so validated relation tables become typed query views immediately
  after decoding.
- Relation tables themselves also store typed relation and column identifiers
  once constructed or decoded. JSON still carries string names for portability,
  but the in-memory `TypedFactRelation` shape is aligned with the closed schema
  rather than mirroring wire strings as compiler state.
- Relation tables no longer store column identifiers separately from their
  relation name. Accessors and serialization derive columns from the fixed
  schema descriptor, keeping the JSON artifact unchanged while removing another
  drift point from compiler memory.
- Internal typed relation validation also derives expected width from the
  schema descriptor instead of accepting redundant columns. The separate raw
  relation validator still checks incoming JSON column strings before decoding.
- Relation-set validation tracks duplicate and missing tables with
  `TypedFactRelationName`, preserving typed invariants until diagnostics need
  the portable wire string.
- `TypedFactRelationIndex` keeps that closed-schema shape internally by using
  typed relation keys for its table map and typed column keys at its lookup
  API. Raw-string names are parsed only at JSON/import validation adapter
  edges.
- Column-position lookup in `TypedFactRelationIndex` now comes directly from
  `TypedFactRelationName::schema()` after the requested relation is known to
  exist, so the fixed schema remains the single source of column ordering.
- `TypedFactRelationName` also exposes the shared typed column-position helper
  used by row access and relation-index filtering, centralizing
  closed-column mismatch errors at the schema boundary.
- Semantic validation over relation tables also uses closed relation and column
  keys. Error payloads still render wire strings, but validation logic itself
  no longer routes ordinary checks through arbitrary string names.
- Relation-table export sorts rows per relation. This keeps backend/query
  artifacts deterministic even when equivalent typed fact sets are assembled in
  different fact order, and avoids making downstream query adapters depend on
  producer insertion order.
- `TypedFactRelationIndex` is the corresponding lightweight consumer for the
  relation-table artifact. It validates publicly constructed relation sets,
  exposes row and column-filter queries, and keeps exact origin/shape join
  tests independent of any Datalog engine. This is a post-export view over
  immutable data, not a mutable sink passed through Salsa queries. The index
  also validates endpoint references, closed string values, duplicate origin
  export keys, malformed origin key cells, duplicate origin links, relation ID
  namespaces, numeric source-span and child-order cells, source-span range
  ordering, non-empty source-span file cells, non-empty shape identity/label
  cells, duplicate shape source/stable keys, duplicate shape-hash keys, and
  complete graph plus per-node local/tree shape-hash coverage. A schema-shaped
  but semantically broken table artifact fails before query adapters consume
  it.
- Shape-hash duplicate and completeness validation keeps `ShapeHashScope` and
  `ShapeDimension` typed after parsing, rather than storing scope/dimension as
  strings in internal validation keys. Wire strings remain only in relation
  rows and diagnostics.
- `source_span` is now a typed fact relation. Codegen derives it at the
  report/analyze boundary from `BytecodeSourceResolution` rows, attaching
  resolved source spans to bytecode PC origin nodes in the same end-to-end fact
  allocation as origin nodes and links. `OriginFactIndex` validates those
  endpoints, so consumers can join origin reachability to source locations
  without parsing source-map JSON.
- Source-span facts fail closed: the span kind is a typed enum
  (`original`, `expanded`, `not_found`), byte ranges must be ordered, and
  line/column ranges must be ordered. File labels must be non-empty in both
  the typed `SourceSpanExport` constructor/decoder, the `SourceSpanFact`
  constructor/decoder, typed fact JSON, and relation-table artifacts. This keeps
  source locations from becoming another permissive string-shaped side channel.
- Source-span per-file summaries use a shared `SourceSpanFileCount` boundary
  DTO with fail-closed JSON decoding for empty file labels, zero counts, and
  unknown fields. CLI analysis reuses that type instead of carrying a separate
  permissive report-only shape.

## Debug And Source Mapping

Debug mapping should flow through the origin graph:

```text
bytecode PC range
  -> post-opt or backend-prepared Sonatina inst
  -> pre-opt Sonatina inst(s)
  -> MIR stmt/terminator
  -> semantic/HIR origin
  -> LazySpan
  -> source span
```

A core source resolver should be pure compiler data construction:

```text
BytecodePackageOrigins + RuntimePackageOrigins + SpannedHirAnalysisDb
  -> Vec<BytecodeSourceResolution>
```

Each bytecode record should produce exactly one resolution result. Successful
source results should keep both the semantic origin and the resolved span.
Non-source results should remain explicit classifications such as synthetic,
unmapped, post-preopt snapshot gap, missing runtime origin, or missing semantic
span. Exporters can then decide how to render those cases without inventing fake
source locations.

The first exporter boundary is intentionally small:

```text
Vec<BytecodeSourceResolution> -> source_map.json
```

This is not a Salsa query and it does not mutate compiler state. It is a
boundary renderer that can allocate JSON fields, report rows, DWARF entries, or
ethdebug records after the cached compiler data has already been constructed.

Rules:

- No resolver should try every body until something resolves.
- Stmt origins and expr origins must be distinct.
- Synthetic nodes should be classified, not forced into fake source spans.
- Optimizer-created nodes should eventually carry explicit optimizer-created
  origins, with optional links to input nodes when available. Until Sonatina
  exposes that lineage, Fe uses a `post_preopt_snapshot_gap` classification and
  tests that those nodes are never silently joined to source.
- PC ranges are section-local. Any source map or debug record must include the
  bytecode object and section owner before interpreting a PC range.
- Bytecode object selection inside origin/fact graph projection uses a nominal
  `BytecodeObjectKey`; raw object strings remain only at artifact and JSON
  boundaries.
- `BytecodeSectionKey` is built from `BytecodeObjectKey` plus
  `BytecodeSectionNameKey`, so object and section-name identity are both nominal
  before PC origins are created. Source-map JSON decoding mirrors the non-empty
  checks for serialized entry rows and optional export metadata so boundary
  artifacts cannot collapse section ownership into an empty string.

Current implementation checkpoint:

- `BytecodePackageOrigins::resolve_source_spans` joins bytecode PC records
  through post-opt Sonatina, pre-opt Sonatina, MIR runtime origins, semantic
  origins, and LazySpan source spans.
- The resolver returns classified results for every bytecode origin record; it
  does not use mutable sinks or query-time side effects.
- A multi-body resolver test requires two similarly shaped test functions to
  both produce source mappings for their own snippets.
- `SonatinaPostOptPackageOrigins` classifies every instruction present in the
  optimized Sonatina module. Same-`InstId` pre/post joins are exported as
  `alias` edges. Bytecode PC origin construction consumes that bundle before
  falling back to a distinct `backend_prepared` Sonatina instruction stage for
  backend-prepared/codegen-only instruction IDs that do not appear in the
  optimized snapshot. Those nodes are linked from the
  `post_preopt_snapshot_gap` synthetic origin so consumers cannot mistake them
  for optimized-snapshot instructions.
- `SonatinaPostOptOriginRecord` enforces the same-ID claim at construction:
  a `SameInstId` source must reference a pre-opt record with the same function
  and instruction ID as the post-opt origin. Snapshot aliases cannot be used to
  smuggle cross-function or cross-instruction lineage.
- `BytecodePackageOrigins::coverage` counts bytecode PC records by source
  classification: post-opt Sonatina, backend-prepared Sonatina, and unmapped.
  Tests require those categories to partition the PC-origin records exactly.
- `BytecodePackageOrigins` sorts consumed artifact records by object, section,
  and PC range before exposing them. This keeps source-map rows, fact exports,
  and coverage consumers deterministic even if artifact or section insertion
  order changes.
- The same constructor rejects overlapping PC ranges inside a single object
  section while allowing adjacent half-open ranges. This prevents bytecode
  source maps and fact exports from carrying multiple origin classifications for
  the same bytecode byte.
- Artifact ingestion also exposes a fallible
  `BytecodePackageOrigins::try_from_artifacts` API. Malformed or overlapping
  Sonatina PC-map rows become typed validation errors that report/codegen
  callers can propagate, rather than panic-only checks after object emission.
- Runtime body/package origin bundles and post-opt Sonatina function bundles
  reject duplicate local identities. This keeps lookup helpers from silently
  choosing the first of several conflicting origin records.
- `BytecodeOriginCoverage` is constructor-backed and exposes totals through
  getters, so external callers cannot manufacture an inconsistent
  total-vs-classification partition by struct literal.
- Sonatina pre-opt and post-opt coverage now follow the same pattern:
  constructor-derived totals, private counters, and partition helpers. This
  keeps optimizer/debug coverage as derived data instead of mutable report
  counters.
- Runtime bytecode outputs and test metadata carry that coverage to report
  boundaries. `fe analyze --source-maps` includes it beside source-map
  summaries so CLI/debug consumers can distinguish "not source mapped" from
  "not present in the optimized snapshot" without reading raw fact rows.
- `BytecodePackageOrigins` also derives object/section-filtered
  `SonatinaPostOptOriginCoverage` by selecting the Sonatina functions actually
  referenced by bytecode PC records. Runtime bytecode outputs, test metadata, and
  `fe analyze --source-maps` expose this snapshot-diff coverage beside
  bytecode-origin coverage, keeping same-ID aliases, post-preopt created or
  unmatched instructions, and pre-opt snapshot losses visible without pretending
  to know optimizer pass lineage.
- `BytecodeSourceMapEntry` is the typed source-map row. Test-report JSON is
  rendered from those entries as a boundary artifact rather than being treated
  as compiler data. Each entry keeps object, section, PC range, source span data
  when available, and explicit non-source reason data otherwise.
- `BytecodeSourceMapFilter` carries a `BytecodeSectionKey`, not loose
  object/section fields, so internal filtering cannot split the owner pair. The
  exported source-map row still uses strings because it is the serialized JSON
  boundary.
- The same typed entry serialization is reused by test-report artifacts and
  `fe analyze --source-map-entries`, avoiding separate hand-written JSON and CLI
  DTO schemas.
- Source-map artifact JSON now round-trips through
  `OwnedBytecodeSourceMapExport`, including schema version, optional
  object/section filter metadata, optional bytecode-origin coverage, optional
  post-opt snapshot coverage, typed PC rows, explicit non-source reasons, and
  source snippets. The current source-map artifact schema is `schema_version: 3`;
  version 2 made `snippet` a required non-empty source-row field, and version 3
  adds `post_opt_origin_coverage`. Source rows also reject empty file labels.
  Unknown schema versions, export fields, entry fields, nested coverage fields,
  and source-variant fields are rejected during deserialization. Entry
  deserialization uses an explicit closed row decoder rather than relying on a
  flattened serde enum, so unit non-source variants cannot silently drop stray
  fields. This keeps debug/source-map exports testable as a fail-closed schema
  boundary rather than a write-only string.
- Build-report and Fe test-report regressions now deserialize emitted
  `source_map.json` artifacts through that owned source-map decoder, so report
  writers exercise the same schema boundary as downstream source-map consumers.
- `BytecodeOriginCoverageExport` and `SonatinaPostOptOriginCoverageExport` have
  downstream compile-fail coverage rejecting struct literals, keeping coverage
  exports tied to typed coverage conversion or fail-closed JSON decoding.
- Internally, `BytecodeSourceMapEntryKind` stores closed classifications as
  typed enums: `SourceSpanKind`, `SonatinaSyntheticOrigin`,
  `SourceSpanInvalidReason`, `SonatinaSyntheticOrigin`,
  `SonatinaUnmappedReason`, and `BytecodeUnmappedReason`. Serialization still
  emits the same string fields, but producers cannot construct arbitrary
  classification strings without leaving the typed API.
- Resolved source spans that cannot be sliced into a non-empty UTF-8 snippet
  are not allowed to panic source-map/debug export. They become
  `source_span_invalid` rows with closed reasons for inverted byte ranges,
  invalid snippet ranges, or empty snippets. Source-map summaries and analyze
  reports count those rows as explicit non-source classifications. The
  `source_span` fact projection uses the same validated source-span details and
  skips invalid rows, so facts and source-map artifacts do not disagree about
  whether a resolved source span is usable.
- Public source-map row construction uses
  `BytecodeSourceMapEntry::try_from_origin` or
  `BytecodeSourceMapEntry::from_origin` and a typed `BytecodePcOrigin`.
  `try_from_origin` returns structured validation errors for non-invariant
  callers; `from_origin` is the convenience wrapper for producers that have
  already committed to valid rows. The raw object/section/PC tuple constructor
  is not public; it exists only as a JSON deserialization boundary where the
  same range and semantic validations run before an entry is accepted. Both
  public constructors validate source-row semantics, so Fe-owned producers
  cannot create source-map entries with empty source files/snippets or inverted
  source ranges and rely on serialization to catch them later.
- Source-map export construction now runs the export-level validations before
  serialization: optional object/section metadata must match every entry,
  per-section PC ranges must not overlap, and optional coverage totals must
  match the number of exported rows. Writers cannot produce artifacts that the
  owned decoder would later reject for these invariants.
- Source-map decoding also validates semantic invariants: source rows must use
  known span kinds, non-source reason strings must match the closed origin
  classifications, source file and snippet strings must be non-empty, source
  byte and line/column ranges must be ordered, optional export object/section
  metadata must be non-empty and match every entry row, and PC ranges must not
  overlap within one object section.
- Source-map row validation is centralized in one kind validator shared by
  `from_origin`, JSON deserialization, and export construction.
- Public Sonatina test metadata tests compare source-map JSON to
  `OwnedBytecodeSourceMapExport::SCHEMA_VERSION`, not a copied literal, so the
  coverage follows intentional schema bumps.
- Coverage metadata in source-map JSON is validated against both invariants:
  classification counts must sum to `total`, and `total` must equal the number
  of exported PC rows.
- `BytecodeSourceMapEntry` is constructor-backed and deserialization validates
  non-empty PC ranges, keeping the artifact row aligned with `BytecodePcRange`
  instead of allowing invalid boundary rows.
- `TestMetadata` stores typed source-map summaries and entries, not rendered
  source-map JSON. Report generation renders JSON from typed entries at the
  artifact boundary.
- Codegen also derives `debug_locations.json` from the same validated
  `BytecodeSourceMapEntry` rows. This boundary exports only real source
  PC-range mappings and drops synthetic/unmapped classifications instead of
  creating fake locations, giving DWARF/ethdebug rebuilds a typed
  `PcRange -> SourceSpan` input without adding a format-specific emitter to the
  origin core.
- The debug-location artifact is also readable as an owned schema DTO. Its
  decoder rejects unknown schema versions and fields, empty location payloads,
  object/section metadata mismatches, invalid or overlapping PC ranges, and
  invalid source file/snippet or byte/line-column ranges before any
  DWARF/ethdebug-specific renderer sees the data.
- Build-report and Fe test-report regressions parse their emitted
  `debug_locations.json` files with that owned DTO rather than generic JSON,
  keeping artifact production and artifact consumption pinned to one schema
  boundary.
- `BytecodeDebugLocationEntry` is constructor/decoder-backed at the public API
  boundary. Downstream compile-fail coverage rejects struct literals, matching
  the source-map row boundary and preventing external callers from fabricating
  unchecked compact debug locations.
- Report-boundary tests now also force `debug_locations.json` write failures
  in build and Fe test reports. Those failures are surfaced with the compact
  debug artifact path after `source_map.json` succeeds, so later emitters do
  not inherit a silent best-effort debug-location path.
- Analyze source-map reports now include a `debug_locations` count derived from
  `BytecodeSourceMapSummary::debug_locations()`. This keeps CLI summaries aware
  of the compact debug-location boundary without serializing report artifacts
  back into compiler data.
- Codegen exposes a typed `BytecodeDebugArtifactsExport` bundle and layers
  `BytecodeDebugArtifactsJson` rendering above it for source-map/debug-location
  artifacts. Report writers choose paths and perform I/O, but codegen owns the
  export ordering and option policy. Future DWARF/ethdebug work can consume the
  typed bundle directly instead of re-parsing JSON.
- `BytecodeDebugArtifactKind` and `BytecodeDebugArtifactsJson::artifacts()`
  keep the debug artifact set, order, and filenames in codegen. Build and Fe
  test report writers prepend their own directory/base names and perform I/O,
  but they no longer duplicate the list of debug artifact filenames.
- The bundled debug artifact export rejects mismatched source-map and
  debug-location metadata, so a report/debug adapter cannot accidentally combine
  object-scoped and section-scoped views in one artifact set.
- `OwnedBytecodeDebugLineTableExport` is the first format-neutral debug emitter
  artifact over that typed bundle. It interns source files and keeps validated
  PC/source rows in row order without selecting a concrete DWARF line program
  or ethdebug schema, so format-specific emitters share one source of truth.
  Build and Fe test reports emit it as `debug_line_table.json` beside the
  source-map and compact debug-location artifacts.
- Debug line-table source files, rows, and owned exports reject public struct
  literal construction through downstream compile-fail coverage. Producers use
  the debug-location-derived builder or the fail-closed JSON decoder instead.
- `BytecodeDebugLineTable` and `OwnedBytecodeDebugLineTableExport` expose a
  `line_records()` view that resolves interned file indices before consumers see
  the rows. This keeps DWARF/ethdebug adapters focused on format encoding rather
  than duplicating the schema join and file-index invariant.
- Analyze source-map summaries expose `debug_line_table_files` and
  `debug_line_table_rows` from `BytecodeSourceMapSummary`. This is intentionally
  a typed summary path over source-map rows, not a read-back of the rendered
  debug-line-table artifact.
- Codegen debug/source-map regressions now live behind the
  `crates/codegen/src/debug/tests.rs` facade, with focused
  `crates/codegen/src/debug/tests/` modules for source-map JSON, source-map
  export/entry construction, debug locations, debug artifacts, debug line
  tables, and source-map summaries. This leaves `crates/codegen/src/debug.rs`
  focused on the public facade and source-map/source-span orchestration.
- Source-map coverage export DTOs now live in
  `crates/codegen/src/debug/coverage.rs`, separating bytecode-origin and
  post-opt coverage schema validation from the source-map/debug artifact
  assembly code while preserving the public `codegen::debug` re-exports.
- Source-map option, filter, and export metadata DTOs now live in
  `crates/codegen/src/debug/source_map_options.rs`. This keeps optional
  artifact policy separate from source-map row/schema logic while preserving
  the public `codegen::debug` re-exports for downstream callers.
- Source-map row schema now lives in
  `crates/codegen/src/debug/source_map_entry.rs`. It owns
  `BytecodeSourceMapEntry`, typed entry kinds, invalid source-span reasons, and
  fail-closed row decoding while preserving public `codegen::debug` re-export
  paths.
- Source-map export schema now lives in
  `crates/codegen/src/debug/source_map_export.rs`. It owns
  `OwnedBytecodeSourceMapExport`, export errors, JSON/export helpers, and
  shared PC-range/metadata validation used by source maps, debug locations,
  debug line tables, artifacts, and summaries.
- Source-span conversion for bytecode source-map rows now lives in
  `crates/codegen/src/debug/source_spans.rs`, keeping snippet validation,
  line/column indexing, and source-span fact conversion beside the code that
  classifies resolved spans. The parent debug module remains the public API
  boundary rather than the owner of every helper implementation.
- Source-map summary policy now lives in
  `crates/codegen/src/debug/source_map_summary.rs`. It owns
  `BytecodeSourceMapSummary` and the source/non-source/debug-line count
  aggregation over typed source-map rows, while `codegen::debug` keeps stable
  public re-export paths.
- Debug line-table DTOs now live in
  `crates/codegen/src/debug/line_table.rs`, including source-file interning,
  line-row validation, resolved line-record views, and fail-closed owned export
  decoding. The parent debug module retains shared metadata/PC-range validation
  and public re-exports.
- Debug-location DTOs now live in
  `crates/codegen/src/debug/location.rs`, keeping compact source-PC row
  construction, fail-closed decoding, source-map row filtering, and location
  export validation together while preserving the public `codegen::debug`
  re-export paths.
- Debug artifact orchestration now lives in
  `crates/codegen/src/debug/artifacts.rs`, keeping source-map/debug-location/
  line-table bundling, metadata mismatch errors, and artifact filename policy
  together while the parent module remains the stable public facade.
- Analyze source-map reports reuse the source-map artifact coverage DTOs and a
  single summary-to-report constructor, so test bytecode and runtime bytecode
  reports cannot drift in count or coverage field mapping.
- Source-map summaries are derived from validated `BytecodeSourceMapEntry` rows,
  not directly from source-resolution results. This keeps invalid source-span
  classifications aligned with the entries that source-map JSON and analyze
  reports expose.
- Source-map summary filtering now uses typed
  `BytecodeSourceMapExportMetadata`, matching source-map export metadata
  instead of accepting parallel raw object/section string filters.
- Source-map JSON/export rendering takes `BytecodeSourceMapExportOptions`,
  keeping optional artifact metadata such as filters and bytecode-origin
  coverage in one typed value instead of adding a new helper function for each
  schema field. Export metadata itself is represented by
  `BytecodeSourceMapExportMetadata`, so Fe-owned writers choose either
  object-level `BytecodeObjectKey` metadata or section-scoped
  `BytecodeSectionKey` metadata instead of passing loose object/section strings.
  The section key requires a `BytecodeSectionNameKey`, keeping section names
  nominal until the final serialized artifact boundary.
- A focused test covers this boundary by constructing typed entries and
  verifying the report writer emits `source_map.json`.
- Codegen-internal names use `frontend_origin_labels` for runtime labels applied
  to Sonatina observability records; the `frontend_provenance` spelling is kept
  only where Sonatina's external API and JSON schema require it. Fe-owned code
  uses `FrontendOriginLabelMap` as a nominal wrapper around that external
  Sonatina map type.
- `fe build --report` emits contract/runtime source maps at
  `artifacts/<scope>/<contract>.source_map.json`. This path uses typed bytecode
  source-map entries from origin-backed Sonatina compilation only for report
  generation, so ordinary bytecode builds do not pay for debug source-map
  export work.
- The same report path also emits
  `artifacts/<scope>/<contract>.origin_facts.json` from the typed
  `BytecodePackageOrigins` graph. These are versioned typed-fact exports, not
  callback rows accumulated during lowering.
- Test Sonatina metadata also carries typed origin facts. Test reports render
  those as `artifacts/tests/<test>/sonatina/origin_facts.json`, keeping the
  JSON artifact at the report boundary.
- `SonatinaPostOptPackageOrigins` records both post-opt instruction
  classifications and conservative pre-opt snapshot losses. Snapshot losses
  mean a pre-opt instruction has no same-`InstId` post-opt match; they are
  explicitly not proof of deletion vs replacement vs merge/split.
- The stage and snapshot-loss labels use the shared closed-string enum helper,
  so string rendering, fail-closed decoding, and display diagnostics stay on one
  policy path while the model remains conservative.
- The same post-opt bundle can export a typed snapshot-diff origin graph/fact
  set. Same-ID survivors use `alias`; pre-opt losses use a
  `pre_opt_snapshot_loss` synthetic node and `synthetic` edges, deliberately
  avoiding pass-lineage `transformed` claims until Sonatina exposes pass hooks.
- Build-report, test-report, and analyze boundaries expose those snapshot-diff
  facts separately from end-to-end bytecode facts. This keeps source-to-bytecode
  explanation paths focused on executable PC ranges while still making
  optimizer snapshot loss queryable.
- Analyze source-map reports also expose filtered post-opt snapshot coverage, so
  consumers can see the conservative snapshot-diff counts without parsing the raw
  snapshot-fact export.
- The remaining optimization gap is precise Sonatina pass lineage for split,
  merge, delete, replace, and alias operations, plus a durable prepared-module
  bundle from Sonatina that does not depend on bytecode observability.
  Fe should not infer those events from before/after snapshots because
  same-`InstId` preservation is not proof that a pass preserved semantic
  identity.

## CLI/API Integration

`fe analyze` should use normal compiler target resolution:

- Real file path.
- Workspace/ingot config.
- Profile/settings.
- Existing dependency graph.
- Same compilation mode as build/check where relevant.

The analysis API should return typed data and reports separately:

```rust
pub struct CompilationAnalysis<'db> {
    pub package: RuntimePackage<'db>,
    pub origins: RuntimePackageOrigins<'db>,
    pub shape: ShapeGraph<'db>,
    pub hashes: PackageHashes,
}
```

Reports should be views over that data, not the data source.

Current implementation checkpoint:

- `fe analyze` exists as a minimal origin-analysis boundary.
- It uses `resolve_cli_target`, ingot/workspace config, workspace member
  selection, profile settings, dependency diagnostics, and recovery mode in the
  same style as `fe build`/`fe check`.
- Its first view summarizes `RuntimePackageOrigins` by target and runtime body,
  with semantic/synthetic counts for statements and terminators.
- `fe analyze --tests` applies the same summary view to Fe test runtime
  packages, matching the test runner's ingot/module traversal instead of
  reporting empty regular runtime packages for test-only ingots.
- `fe analyze --source-maps` consumes typed `BytecodeSourceMapSummary` values
  from codegen for both regular runtime packages and test runtime packages. The
  JSON source maps remain a boundary artifact; analyze does not parse them back
  into compiler data.
- `fe analyze --source-maps --source-map-entries` additionally exposes the typed
  `BytecodeSourceMapEntry` rows on demand, keeping the default summary output
  compact. Text output now renders the same classification counts and opt-in
  entry rows as JSON instead of hiding the richer view behind `--format json`.
- Full source-map entry analysis exposes the same derived snippets as the JSON
  source-map artifacts, so callers can validate source-to-PC explanations
  without reopening source files from offsets.
- `fe analyze --tests --origin-facts` exposes versioned typed origin-fact
  exports for test bytecode, with compact counts in text output and full typed
  facts in JSON output.
- `fe analyze --origin-facts` also exports runtime semantic-to-MIR origin facts
  for regular runtime packages. With `--tests`, the report includes both runtime
  origin facts and test-bytecode origin facts.
- MIR runtime-origin regression coverage now lives in
  `crates/mir/src/origin/tests.rs`. The production MIR origin implementation is
  split by responsibility: `runtime_identity.rs` owns typed statement,
  terminator, and code-region identity plus export-key helpers; `package.rs`
  owns runtime body/package origin records and the cached package-origin query;
  and `fact_graph.rs` owns runtime semantic-to-MIR fact graph construction plus
  typed fact-owner policy. The parent `mir::origin` module remains the compact
  re-export facade while preserving `origin::tests::*` coverage paths.
- Origin-fact analyze reports include a query-derived reachability summary
  grouped by origin kind. The summary is computed by `TypedFactRelationIndex`
  from fixed relation-table rows, proving that the engine-agnostic query
  boundary can answer this cross-IR coverage question. Text output shows total
  reachable pairs plus grouped kind-pair counts; JSON includes the same grouped
  counts next to the versioned fact export.
- Origin-fact analyze reports also include representative path witnesses in
  JSON output. Each witness is now derived from `TypedFactRelationIndex` over
  fixed relation-table rows, uses stable `OriginExportKey` nodes through the
  shared `common::facts::OriginPathWitnessExport` shape, and records link kinds,
  so a user can inspect an actual semantic-to-runtime or runtime-to-bytecode
  chain without parsing the raw fact rows.
- The same relation index derives representative `source_path_witnesses`: path
  witnesses whose terminal origin also has a typed `source_span` relation row.
  This gives analyze a concrete source-to-bytecode explanation view while
  keeping origin graph traversal and source-span joins in cached, returned data
  rather than side-effecting salsa queries or debug-format-specific emitters.
- Text origin-fact reports render the same representative witness chains in a
  compact `node --link--> node` form, giving quick source-to-runtime/backend
  explanations without switching to JSON.
- `TypedFactRelationIndex` also supports stable-key path lookup between two
  `OriginExportKey`s and returns `OriginPathWitnessExport`, so downstream query
  and debug adapters do not need to traffic in allocation-local fact IDs.
- Origin-fact analyze reports include relation-count summaries from
  `TypedFactRelationIndex` over `TypedFactSet::relation_export`. This verifies
  that the CLI/report boundary can surface the table-shaped backend export
  without exposing a specific query engine or duplicating relation scans in the
  CLI. Text output renders the per-relation row counts directly, while JSON
  serializes the same shared `TypedFactRelationCount` DTO with closed relation
  names, non-zero rows, and fail-closed field validation.
- Origin-fact analyze reports also include compact per-file counts for
  validated `source_span` fact rows. The full source-span rows remain in the
  typed fact export and relation tables; the summary is answered by
  `TypedFactRelationIndex` so the text report shows source-location coverage
  without dumping every PC/source row or bypassing the query-table boundary.
- With `--fact-relation-tables --format json`, origin and shape reports include
  full relation-table rows under `relation_tables`, giving downstream query
  experiments an artifact path that does not parse the raw typed-fact JSON.
- `fe analyze --shape-hashes` reports graph hash dimensions for runtime
  const-region shapes, the first runtime IR family migrated to the
  `ShapeDescribe` derive policy. `--shape-facts` includes the corresponding
  versioned typed shape facts.
- Text shape reports include every graph hash dimension, so the default
  human-readable hash view does not silently drop names, constants, types, or
  trace-events compared with the JSON view.
- Shape hash report rows keep using the existing JSON dimension strings, but
  analyze now carries those dimensions as `ShapeDimension` internally and rejects
  unknown dimensions on decode.
- Shape analyze report rows fail closed when graph hash dimensions are missing
  or duplicated, when hash digests are not canonical lowercase hex, when
  embedded shape facts disagree with summary counts, or when relation summaries
  contain populated non-shape tables.
- Shape hash fact rows validate canonical lowercase 16-character digest text at
  the `ShapeHashFact` constructor/serde boundary. Index validation still owns
  graph/local/tree node-scope consistency and completeness checks.
- Canonical digest text is carried as the `ShapeHashDigest` newtype in
  `common::facts`. `ShapeHashFact` and analyze shape-hash reports reuse that
  boundary, so report DTOs and typed fact rows cannot drift on lowercase/length
  policy while preserving the existing `digest_hex` JSON string. Shape report
  validation compares typed digest values and stringifies only for diagnostics.
  Public `ShapeHashFact` construction now requires a `ShapeHashDigest`; raw
  digest strings are accepted only at explicit JSON/import helpers, and the raw
  canonicality predicate remains private. Digest-format diagnostics are owned by
  `ShapeHashDigestError` and delegated through fact import errors instead of
  being duplicated on `ShapeHashFactError`. Shape fact indexes therefore check
  graph/local/tree scope and completeness, not canonical digest text again.
  Raw relation-table imports still reject malformed `digest_hex`, but they do
  so by constructing `ShapeHashDigest`.
- Shape analyze reports also include compact relation-count summaries from the
  shared `TypedFactRelationIndex` path and the same validating
  `TypedFactRelationCount` DTO. Text output exposes the populated shape tables
  without requiring full JSON relation rows.
- `ShapeDescribe` rejects unknown dimensions during macro parsing, and runtime
  tests verify declared stable-key policies for structs and enum variants reach
  generated `ShapeGraph` nodes. Identity-only fields still require explicit
  skip reasons.
- Empty shape kinds and labels are rejected by the derive macro before code is
  generated, preventing invalid shape identity/label data from escaping into
  runtime graph construction or relation-table export.
- Shape graph hashing now keeps edge topology in the structure projection
  without folding endpoint exact digests into structure. Constant/name/type-only
  endpoint changes still affect the exact graph digest and their own dimensions,
  but they do not pollute the structure dimension merely because an edge exists.
- Local shape fields are hashed as unordered dimension/name/value metadata, so
  manual builder insertion order cannot perturb hashes. Ordered IR structure
  remains represented by child order and stays hash-sensitive.
- Internal analyze traversal now carries a single `AnalyzeOptions` value through
  file, ingot, workspace, and module helpers. This keeps future report views
  from widening every helper signature independently.
- It emits text or versioned JSON. The JSON view is a boundary report; it is
  not produced by Salsa query side effects.

Still open:

- Format-specific DWARF/ethdebug emitters and richer cross-IR origin graph
  views. The typed query-table summaries and typed debug-location boundary now
  exist, but backend-specific exporters still need to consume them.
- Factor duplicated CLI target traversal once the analyze/check/build boundary
  has settled.

## Testing Strategy

Tests should prove correctness invariants.

Identity and origin tests:

- Every origin link points to existing typed nodes.
- Every node key includes required owner context.
- Bytecode PC-origin ranges are non-empty and non-overlapping within each
  object section.
- Malformed Sonatina PC-map rows with empty or inverted PC ranges fail closed at
  `BytecodePackageOrigins` construction instead of disappearing before origin
  coverage, source-map, and fact export.
- Origin-backed bytecode compilation propagates those PC-map validation
  failures through its normal `LowerError` path instead of panicking at the
  artifact boundary.
- Bytecode object and section owner strings are non-empty in typed section
  keys and decoded source-map artifacts.
- Expr and stmt origins resolve to different expected snippets when applicable.
- Nominally distinct origin wrappers, such as HIR expr and stmt origins, cannot
  cross API boundaries in downstream compile-fail tests.
- HIR/MIR/codegen origin wrappers do not expose raw
  `OriginKey<Owner, Local>` escape hatches at public boundaries; consumers use
  nominal wrappers and stable export helpers instead. Compile-fail coverage
  rejects raw-key deconstruction for HIR expr/stmt/semantic, MIR runtime,
  Sonatina, and bytecode origins.
- MIR terminator origins cannot be constructed from a raw block ID at public
  boundaries; compile-fail coverage requires callers to go through
  `RuntimeTerminatorSite`.
- Public HIR/MIR/codegen APIs do not expose raw local export-key strings for
  semantic origins, runtime statement/terminator sites, or bytecode PC ranges;
  compile-fail coverage catches those escape hatches.
- Sonatina pre-opt/post-opt origin records reject the wrong instruction stage,
  and post-opt function bundles reject records owned by another function.
- Same-`InstId` post-opt aliases reject pre-opt records from a different
  function or instruction ID.
- Synthetic origins are classified and not counted as source coverage.

Hash tests:

- Tree content always contributes even when graph edges exist.
- CFG-only changes affect graph digest.
- Full edge-label changes affect graph digest.
- Edge endpoint content changes affect exact/per-content dimensions without
  polluting the structure dimension.
- Statement content changes affect content digest.
- Rename-only changes affect names dimension but not structure dimension.
- Constants/types dimensions behave independently.

Fact tests:

- Each relation uses a declared ID namespace.
- Cross-namespace joins require mapping relations.
- Security queries are tested on minimal synthetic fact sets with known answers.
- ERC20-scale tests remain as smoke tests, not the primary oracle.

Debug tests:

- Pre-opt origins are not joined directly with optimized PC maps.
- Every emitted PC range is mapped, synthetic, or explicitly unmapped with a
  reason.
- Known snippets in multi-function files resolve to the correct function body.

Salsa tests:

- Origin queries are deterministic across repeated calls.
- Exporters can run multiple times without changing cached compiler state.
- Changing one source file invalidates only expected origin/hash data.

## Migration Strategy

Do not harden the current branch by patching each bug in isolation. Use it as a
prototype and harvest tests/ideas from it.

Suggested sequence:

1. Add this design and plan.
2. Introduce typed origin keys alongside existing IDs.
3. Add cached `RuntimeBodyOrigins`.
4. Convert MIR origin stamping to typed origins.
5. Add `ShapeGraph` and new hashing.
6. Move facts to typed export.
7. Rebuild Sonatina and optimization origins.
8. Rebuild debug exporters.
9. Rebuild `fe analyze`.
10. Remove obsolete raw-ID paths.

## Open Questions

- Which origin keys need stable string forms for export?
- Should origin graph data live in `common`, `hir`, `mir`, or a new crate?
- How much Sonatina optimizer origin support requires upstream changes?
- Should hash descriptions be built directly from compiler IR or from a shared
  shape graph generated by derives?
- How should desugared origins represent both surface source and lowered
  constructs?

## Reconciliation With Original Sessions

This design was drafted from an independent review of the current branch. The
original creation sessions were then sampled for intent and nuance. They confirm
the architecture spine but add these refinements:

The detailed decision record is `origin-overhaul-reconciliation.md`.

- Keep the typed many-to-many origin graph, not a single shared cross-level hash.
- Treat `SourceOrd` as a useful historical source-location prototype, not a
  sufficient origin model.
- Capture origin links online, then derive hashes/facts/debug views post-hoc.
- Keep Datalog/Cozo/Souffle/JSON as export/query backends, not compiler state.
- Do not overload Fe language-effect terminology for generic trace metadata.
- Do not trust transcript claims without checking code; the current prototype
  still contains known mismatches such as edge-label hash truncation and
  incomplete fact dimensions.
- Preserve the interactive multi-view/debug idea as a boundary exporter.

Sessions worth deeper review before implementation:

- `b7af97db-676a-4a1e-987b-f5251554d1cd`: foundational debug-info prototype.
- `b1ea79d7-e7c1-4c21-9a18-8a2e7c17816d`: proof/debug intent and boundary
  between metadata and real Fe verification features.
- `06abe0e6-e08a-4556-84b8-c5f7a242cc2a`: Salsa/query-driven exporter
  precedent.
- `f74d55cd-26b4-4d91-8e33-75f238a8861d`: derive/HirBuilder/desugaring
  lessons.
- `d8352797-0e72-45ce-b63a-6070634e6f22`: Sonatina observability planning.
- `8c1f6bad-5c1d-4278-9bb5-aa84535a3fcc` and
  `4774b549-6737-49ce-91f7-acc645d38189`: multi-backend/Sonatina context.
