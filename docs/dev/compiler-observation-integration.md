# Compiler observation integration

Status: integration contract, not a claim that the observer is on mb2.

## Outcome

One compiler-owned observation boundary feeds the Riffcat tooling. It must answer
where shader expansion occurs, which representations and transformations account
for it, and whether a controlled change improves an exact, behavior-tested
artifact. Ethdebug integration is out of scope and is not a prerequisite.

The compiler emits observations and transformation records. Riffcat owns reports,
structural comparison and cross-run alignment claims. No compiler legality,
optimization or cache decision may depend on observation output or Riffcat hashes.

## Existing candidate and integration ownership

The initial Fe candidate is `908116b4b0714a36fb77bb53808d2762c2f6c4e8` on the
`bloat-toolkit` branch. It combines capture hooks, a named force-inline experiment,
and example harnesses. Its separate InstanceIndex prerequisite must not overwrite
the shared branch's existing raster support.

Shared mb2 includes repeated-loop helper retention added after the candidate base.
Preserve that policy when integrating the hook changes. Do not replace
`spirv_lower.rs` wholesale with the candidate version.

The shared worktree is `/workspace/fe-worktrees/mb2`. Compiler integration and
acceptance gates belong here. Agree with the Riffcat owner on recorder-file
ownership before changing the candidate; consumer work and compiler integration
can proceed independently, overlapping recorder rewrites cannot.

## First implementation slice

Use a small typed recorder interface with an explicit caller-owned context.
Configure it at a driver/request boundary. Record types carry scope-local entity
references; producer-local arena numbers are not cross-stage or cross-run identity.
Keep JSONL as an external encoding, not the internal API. Preserve a projection
readable by the existing structured Riffcat importer while migrating producers.

Retain the useful existing hooks: pre/post exact merge, normalized helper graph,
helper eligibility and retention decisions, inlining frontiers, cleanup and final
IR, followed by exact backend artifacts. Record new clone events separately from
cumulative original-ID survival observations. Do not call the latter descendant
tracking. Record missing fast-path or rewrite attribution explicitly.

Observation must not choose optimization policy. Keep named force-inline controls
in a separate experimental configuration and record both requested changes and
their dependency-closure consequences. Do not make that experiment a prerequisite
for ordinary capture.

Prefer one small module or crate initially. Do not require a multi-crate registry,
new compiler IR, debugger integration or universal provenance system before the
existing producer can use it. Remove superseded capture paths as replacements
pass their compatibility gates.

## Required gates before promotion

1. Disabled observation performs no snapshot construction or clone-detail
   collection and creates no output files.
2. Compare exact WGSL and SPIR-V bytes with observation disabled and enabled in
   separate fresh processes, using the same compiler, source and settings. Cover
   the scalar helper, a shared/multi-entry resource fixture, a repeated loop helper
   and the production sparse round-interaction kernel. Preserve the ordinary pass
   schedule; do not silently substitute pass-by-pass scheduling for batched runs.
3. Import and replay captures through Riffcat with artifact verification. Preserve
   separate backend and outer WebBundle byte counts. Unknown lineage stays unknown.
4. Exercise missing completion, forced truncation, write failure, artifact
   tampering, request isolation and no-clobber behavior. Ordinary observation
   failure should leave compilation semantics intact and report partial capture;
   an explicit strict mode may fail the observation gate.
5. Run the independent finite-domain oracle on the exact saved scalar shaders and
   its wrong-result negative control. Record software Vulkan as software Vulkan.
   Compiler validation, browser execution and proof correctness are distinct gates.
6. Measure counts-only and detailed capture separately: compiler wall time, peak
   RSS, capture bytes and exact emitted bytes. Output limits alone do not bound
   clone collection or snapshot construction memory. Use optimized builds and one
   heavy build at a time. Do not infer speedup from instrumented timings alone.

## Next tooling capabilities, in order of usefulness

- A stage waterfall with explicit scopes and the first growth boundary highlighted.
- A per-helper expansion ranking separating new clones, cumulative observations,
  surviving original IDs, unknown rewritten descendants and retained shared code.
- Representation breakdowns for typed locals, byte-arena operations, numeric
  legalization, resource specialization and structurization clones. Different
  causes must not be collapsed into one duplicate-instruction count.
- Sonatina-to-Naga expression attribution and WGSL function/range measurements.
  Report exact, estimated and unattributed quantities separately; do not allocate
  final bytes proportionally to instruction counts and call that exact evidence.
- Revision-pair comparisons keyed by source, Fe revision and dirty patch, Sonatina
  pin, target configuration and intervention. Stage alignment is an explicit claim,
  not identity inferred from equal stage labels or function names.
- Saved-artifact experiment replay with behavior results and regression budgets.
  Smaller output is not an improvement if validation or behavior regresses.

Use the Mandelbrot round-interaction kernel and Quilting triangulation as real
consumers after the small fixtures pass. Budgets should flag regressions relative
to reviewed baselines, not invent a universal shader-size correctness limit.
