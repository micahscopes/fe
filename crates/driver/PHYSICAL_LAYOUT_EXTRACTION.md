# Packed layout extraction rehearsal

Historical record: this rehearsal used a checked Fe packed-layout planner that
was subsequently removed from mb3. Its extraction and test results describe
that earlier implementation, not the current production layout path.

The extracted consumer executes successfully on fetched mainline. The active mb3
branch has not been rebased or merged. The full mb3 regression gate and final
executable acceptance have passed; see the consumer receipt for the precise
evidence and test-only supplement.

## Tested checkout

The isolated branch is `mb3-layout-mainline` at
`/workspace/fe-worktrees/mb3-layout-mainline`, with production tested through
`c07163a3e` and supporting-implementation regression coverage added and passed
at `67440dff6`. The amendment to `c07163a3e` changed formatting only.
The base is `9c9840ce4fd8fbb13ae39d8688af784cb9aad5de`, fetched September 21,
2026, including semantic borrow-checker PR #1557. Sean's native-build PR #1549
was open and unmerged when checked; no unmerged branch was imported.

## What needed adaptation

- Mainline's normalized data paths no longer contain mb3's old explicit
  dereference traversal. The regression fixture was retained, while the obsolete
  traversal fix and its release note were excluded from the resulting change.
- Mainline has no mb3 const-predicate API. The bridge uses its available ordinary
  source and trait-requirement checks, without importing const-predicate support.
- The diagnostic conflict was resolved with just the new layout error arm.
- Mainline's borrow checker needs physical byte extents before executing a
  provider whose source it must check. `canonical_runtime_size_bytes` shares the
  semantic type walk with production sizing but uses independent canonical
  extent constraints. Production still consumes `checked_packed_layout`, and
  invalid provider output has no fallback. The same shared traversal is carried
  back to mb3. A regression proves that a known analysis extent does not authorize
  an invalid provider result.
- New memory-reference projection call sites carry the source owner's scope,
  retaining the distinction between byte memory and word-addressed storage.

## Executed evidence

At the tested extraction checkpoint:

| Check | Result |
| --- | --- |
| Driver `physical_layout` | 10 passed, including supporting const-method edits/removal/restoration, malformed/recursive providers and warm/fresh provider, type and alias edits |
| HIR physical footprint tests | 2 passed, including bounded interval coverage and conservative unknown/overflow handling |
| HIR `semantic_borrowck` | 294 passed in 108.78 seconds |
| `fe check` on the physical fixture | Passed |
| Physical fixture, O0 and O2 | 2 tests passed at each level, preserving exact bytes, projections, aliases, distinct nominal types and zero extents |
| Existing pointer fixture, O2 | 9 passed, including bounds failure |
| Ordinary `fe build`, O2 | Contract wrapper invoking the same assertions produced creation/runtime bytecode and ABI |

The contract wrapper is `/workspace/scratch/mb3-physical-layout/acceptance-contract.fe`.
Its O2 output has 1,206 creation-bytecode bytes and 3 runtime-bytecode bytes; the
assertions are in the constructor. The executable assertions were tested through
`fe test`; no separate contract deployment is claimed. These sizes and the
recorded build times are observations, not speed comparisons. Logs and artifacts
are under `/workspace/scratch/mb3-physical-layout/extraction-*`.

## Coherent review boundaries

This particular consumer needs no additional mb3 foundation stack on current
mainline: ordinary const evaluation, generic arrays and source checking already
suffice. Two implementation groups are suitable for later review:

1. **Checked Fe packed-layout planning:** the core planner, source/signature and
   result validation, explicit failure/cycle behavior and focused planner tests.
2. **Production integration:** shared type facts, allocation and pointer consumers,
   independent borrow-analysis extents, diagnostics and executable byte regressions.

The local mechanical extraction history is `3da7eeacf`, `061e26521`,
`eaaab4cfd`, `d6bd20f18`, `039998201` and `c07163a3e`. It records the rehearsal,
not a proposed six-PR series. The later analysis adaptation belongs with the
production integration when preparing reviewable commits. The raw regression
and contract documentation can accompany that group.

This establishes an executed extraction of this consumer. It does not establish
extractability of every other mb3 feature. No push or hosted PR was made.

## Preservation rule for reconstruction

Missing historical evidence is a gap in our knowledge, not authorization to
replace an mb2 design choice. Preserve documented intent by default. Distinguish
an established decision, an implementation defect, a temporary supported subset,
an unresolved interaction and a decision whose record has not yet been recovered.
In particular, pure const execution and ordinary coherent lookup in today's
bounded APIs do not settle the broader effectful-elaboration or scoped-selection
designs in mb2.

Before deliberately changing language meaning, identify the old decision and its
rationale, the concrete evidence motivating a change, and the proposed difference.
Recover missing relevant history before substituting a different behavior. A
mechanical integration repair need not reopen unrelated history. This milestone
changes the packed-layout computation's implementation while retaining its fixed
byte representation; it does not reopen the four broader semantic contracts.

## Sean's ownership boundary

A subsequent `gh` review confirmed that PR #1557 merged as the exact extraction
base `9c9840ce4`. Its shared operation contracts own moves, loans, capability
provenance and caller-visible effects. Physical overlap, typed initialization and
native authority remain distinct facts. Raw operations retain a whole-range
validity precondition; borrow acceptance alone is not an allocation-bounds proof.
The checked packed plan therefore supplies sizes and offsets, not ownership,
initialization or permission to access a raw allocation.

The extraction's 294 passing semantic borrow tests exercise this merged checker.
This supports the bounded consumer integration, not compatibility of all mb3
features. PR #1549 (native builds) remains unmerged at the review checkpoint and
is not required by this rehearsal. PR #1559's validated ABI array access is a
separate representation and does not authorize changes to this packed-memory
contract. Both remain relevant followups when selecting the next executable slice.
