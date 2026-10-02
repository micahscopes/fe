# Physical layout consumer milestone

Historical record: the checked Fe packed-layout planner described below was
subsequently removed from mb3. Production now computes the same packed byte
sizes and offsets with checked arithmetic. The exact-byte fixture remains the
behavioral contract; the planner-specific validation claims below describe the
earlier implementation.

The three implementation chunks are delivered. The final acceptance receipt below
records the current evidence; earlier checkpoints retain their historical status.

## Boundary under validation

The selected candidate is the existing EVM raw-memory pointer representation.
This is distinct from ABI serialization, storage slots, constant-region object
serialization and GPU storage buffers. No universal physical layout is implied.

`core::size_of<T>()` supplies allocation and typed pointer stride. HIR historically
computed its size in `semantic/consts.rs::runtime_size_bytes`. MIR's separate
`runtime/layout_utils.rs::RuntimeMemoryLayout` computed raw field offsets, array
strides and sizes used by Sonatina pointer addressing. Both recurse over scalar,
record and array shape. The selected migration must connect these production
consumers to one checked Fe aggregate plan, not just add another descriptor.
Constant-region serialization in the same MIR file is a different representation
and must retain its own contract. Storage-slot units are also distinct.

The initial boundary inspection fetched mainline to `8deb62ee7`. At that time GitHub reported Sean's PR #1549 open, not
merged, at `6024935d504ff750af452739d7ee516a6a1b28b6`. The baseline consumer is
therefore EVM. Native, Wasm and GPU restoration are not prerequisites.

## Fixed executable case

The regression fixture is `crates/fe/tests/fixtures/fe_test/raw_record_layout.fe`.
It allocates two `Packet` values through `MemArray`, writes and reads nested
fields and array elements, and checks every byte against literal expectations.

| Shape | Field offsets | Size | Alignment | Array stride |
| --- | --- | ---: | ---: | ---: |
| `Inner { code: u16, ready: bool }` | 0, 2 | 3 | 1 | 3 |
| `[u16; 3]` | 0, 2, 4 | 6 | 1 | element stride 2 |
| `Packet { tag: u8, inner: Inner, samples: [u16; 3], tail: u8 }` | 0, 1, 4, 10 | 11 | 1 | 11 |

These alignment values describe the packed pointer representation, not natural
machine alignment. Both records together occupy 22 bytes. Their initial bytes:

```text
a1 12 34 01 56 78 9a bc de f0 7e
b2 43 21 00 11 12 13 14 15 16 6d
```

Updating the second record's code changes bytes 12/13 to 24/68. Updating its
second sample changes bytes 17/18 to ab/cd. The first record and second tail
must remain unchanged. Expected bytes are not computed by the layout provider.

## Ownership and proposed migration

- The compiler establishes type identity, declaration order, scalar widths,
  address space and pointer provenance. Runtime classes remain structured.
- Checked ordinary Fe computes the aggregate plan from compiler-established
  component descriptions. It must not depend on `size_of` for the layout it is
  currently defining or require automatic provider discovery.
- The compiler validates shape, representability, bounds and compatibility of
  the returned plan before consumption. A returned descriptor certifies nothing
  by itself. Validation does not supply an alternate production layout.
- EVM lowering retains scalar byte order and physical load/store operations.
  It consumes validated offsets and strides instead of independently planning
  the admitted record/array layout.

The planner probe and production integration status are recorded below. The
milestone remains incomplete until executable acceptance and extraction pass.

## Baseline prerequisite discovered

The fixture exposed a panic in bounds-check projection traversal for
`second.samples[1]`: `Projection::Deref` recognized borrows and capabilities but
not raw pointers. The local candidate adds the existing `TyId::as_ptr` case.
The initial mainline inspection suggested an analogous repair in
`source.rs::place_index_bounds`. The later executed extraction on `9c9840ce4`
showed that normalized data paths had removed the need for that traversal fix.
The extraction retains the fixture and excludes the obsolete repair. This is a bounded prerequisite, not evidence
that layout policy itself caused the panic. The exact 22-byte fixture passes
at O0 and O2 after the one-case traversal fix. This establishes the pre-migration
physical oracle; it does not establish Fe-derived layout consumption.

## Remaining acceptance and extraction

After the planner probe, freeze the supported surface and negative cases. Cover
empty/zero cases, overflow, bounds, malformed plans, missing prerequisites and
exhaustion; edits/restoration must match fresh compilation. Preserve earlier
ABI/accessor, selected-evidence, scoped-provider and frozen-binding regressions.
Measure consequential costs. Complete the goal's full validation commands.

Rehearse the smallest dependency stack on an isolated checkout of fetched
mainline and execute the same acceptance fixture there. Do not merge/rebase mb3
or require Sean's unmerged branch. Record proven extraction boundaries, not just
a conflict-free patch application. No push or hosted PR is authorized.

## Checked planner probe

`core::raw_layout::packed_layout<N>` now computes a `PackedLayout<N>` from
component widths and repetition counts, using checked u64 arithmetic. Fixed
arrays require one component and a repetition count, not an expanded list of
elements. The planner has no dependency on `size_of`. All offsets are packed
byte offsets; alignment is one and aggregate stride is its total extent.

HIR's `checked_packed_layout` resolves this function in the compilation scope,
checks its concrete signature and ordinary source (including borrow diagnostics),
and runs ordinary CTFE. It admits the language's read-only array parameter views;
mutable parameters, effects and entry requirements are not silently discharged.
The return must be the declared local `PackedLayout`, with the expected array and
scalar types. Independent validation checks every offset and extent before
exposing an immutable checked result. Invalid or incomplete results have no
fallback calculator. Scope and actual inputs participate in memoization.

Five focused driver tests pass: nested record/array planning, empty and zero
repetitions, checked addition/multiplication overflow, compiler consumption, and
provider edits/restoration against fresh databases. The latter reject wrong
returned offsets, nontermination, unsupported unused requirements and malformed
source. The focused release run passed on September 21, 2026.

That committed probe established the checked planning boundary only. The
subsequent production integration is described below; costs, extraction and full
validation remain required work for the active goal.

## Production integration checkpoint

The working implementation routes HIR aggregate `size_of` and MIR byte-addressed
aggregate sizes, record field offsets and array addressing through the checked
planner. Scalar widths and enum tag/maximum-payload policy remain compiler facts;
word-addressed storage and constant-region serialization retain their separate
representations. Dynamic indexing still performs the target's checked/address
arithmetic using the admitted stride. No scalarization or control-flow rewrite
is introduced by this integration.

Callers supply the semantic owner's compilation scope, including synthetic roots
through their underlying function or contract. The planner therefore resolves
against that source's core dependency. Rejected plans have a distinct diagnostic
from type-size overflow. Query-cycle recovery rejects recursive planning rather
than authorizing a result.

Seven driver tests pass, including production `size_of` observation,
invalid-provider and recursive-provider edits/restoration compared with fresh
compilation. Three MIR tests pass for overflow, byte-versus-word projections and
wide enum tags. The executable fixture passes `fe check` and both O0/O2 tests:
the original 22-byte oracle now uses `InnerAlias`; a distinct `other::Inner`
retains its independently expected four-byte size and eight-byte two-element
buffer. Unit and zero-array sizes and zero-length allocation are also checked.
All nine existing pointer regressions pass at O2, including bounds failure.

The successful release executable rebuild took 2 minutes 10 seconds in the
shared cached workspace. Driver tests took 2.79 seconds and MIR tests 0.79
seconds. These are individual observations, not benchmark comparisons or speed
claims. Logs are under `/workspace/scratch/mb3-physical-layout/production-*`.
Formatting, release-note validation and whitespace checks pass. The goal's
broader regression gate, cost assessment and mainline extraction remain pending;
this checkpoint does not complete that goal.

## Final validation checkpoint

Production code is committed through `df2eb10f1`. Nine driver layout tests now
cover provider rejection and restoration, recursive planning, type/alias/array
length edits, and independent analysis extents. The latter share the semantic
type traversal with production sizing, but do not execute the provider: source
checking needs these facts before it can authorize provider execution. A known
analysis extent cannot authorize a rejected production plan. This distinction
was required by mainline's newer borrow checker and was carried back to mb3.

The isolated mainline extraction has executed the same byte oracle and passed
its focused checks. See [the extraction receipt](PHYSICAL_LAYOUT_EXTRACTION.md)
for its pinned base, adaptations and review boundaries. This supersedes the
pending-extraction statements in the earlier checkpoints above.

The exact full mb3 nextest gate passed: 2,998 tests, zero failures and zero
skips in 692.909 seconds of test execution. The subsequent test-only addition
`3aff64663` closes supporting-implementation edit coverage; all ten layout tests
pass in 3.54 seconds. No production code changed after the full gate. Final
mb3 check, O0/O2 execution and ordinary contract build also pass.

The implementation adds source checking and CTFE on previously uncached layout
requests. Memoization includes the source scope, widths and repetition counts.
Arrays use one repeated component instead of expanding all their elements.
Independent validation still walks the component offsets; it is an intentional
check of the library proposal. These are structural cost observations, not
measured improvements in compilation speed or generated runtime performance.
The extraction records emitted bytecode size and individual test/build timings.

The broader followup remains multibackend reconstruction in bounded executable
slices. This packed EVM representation does not choose GPU buffer alignment,
Wasm host transport, synchronization or resource lifetime rules. The four broader
FCO contracts remain on their existing roadmap; this milestone neither resolves
nor redesigns them.

## Acceptance receipt, September 21, 2026

| Requirement | Authoritative evidence |
| --- | --- |
| Existing physical consumer migrated | HIR `runtime_size_bytes` and MIR byte layout consume `checked_packed_layout`; no rejected-plan fallback |
| Independent byte representation | `raw_record_layout.fe`: literal 22-byte nested/array buffer, field mutations, alias and distinct nominal cases, zero extents |
| Checked provider failure | `physical_layout.rs`: invalid offsets/source, missing requirements, checked overflow, exhaustion and recursive planning |
| Incremental behavior | Provider, type, alias, array length and supporting const-method implementation changes/restoration compared with fresh databases |
| Independent analysis | Same type walk supplies canonical analysis extents; regression rejects bad production plans despite known analysis extents |
| Preserved representations | Byte/word distinction, array stride/overflow and enum tags covered by MIR tests; pointer bounds and earlier ABI/accessor, evidence, scoped-provider and frozen-binding tests pass in the full gate |
| Normal execution | `final-check.log`, `final-o0.log`, `final-o2.log`: check passes and two tests pass at each optimization level |
| Ordinary build | `final-contract.log`: O2 contract wrapper built with mb3 `df2eb10f1` executable, 1,224 creation bytes and 3 runtime bytes |
| Executed extraction | Mainline branch through `67440dff6`: ten layout tests, 294 borrow tests, two footprint tests, O0/O2 byte oracle, pointer tests and ordinary build |

Logs live under `/workspace/scratch/mb3-physical-layout`. The exact full command was:

```sh
cargo nextest run --release --workspace --all-features --no-fail-fast --locked --exclude fe-language-server --exclude fe-bench
```

The gate includes all previous applicable metaprogramming regressions. The added
supporting-implementation test passed separately on mb3 and mainline; it did not
change compiler behavior. Formatting and release-note validation passed. Strict
workspace clippy passed again after that test-only addition, with warnings denied
and `-D clippy::all`. Build caches use sccache with incremental compilation off;
all artifacts are on `/workspace`.

The mainline wrapper emits 1,206 creation bytes versus mb3's 1,224. These are
observations from different compiler baselines, not an attribution of the
difference to layout planning or a speedup claim. The wrapper's assertions are
in its constructor and were executed through the Fe test fixture; no separate
deployment is claimed. Full gate timing and focused timings are cached-workspace
observations, not controlled before/after compilation benchmarks.

The tested extraction requires ordinary mainline const execution and source
checking, not the broader mb3 FCO foundation stack. Its two review groups are
checked planning and production integration. No backend was restored, no active
mb3 rebase/merge occurred, and no push or hosted PR was made. Broader public
obligations and multibackend work remain subsequent goals.
