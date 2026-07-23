# FCO plan normalization bridge: phase options

## Finding

The current provider executor cannot safely call semantic type-function
normalization. This is a phase boundary, not missing glue.

The relevant query chain today is:

```text
scope_graph_impl(top_mod)
  -> expanded_items_impl(top_mod)              [&dyn HirDb]
     -> expand_provider_impl(key)               [&dyn HirDb]
        -> ProviderExecutor::run
        -> synthesize_provider_impl
  -> merged_scope_graph_impl
```

`expanded_items_impl` intentionally reads only `base_scope_graph_impl`.
`scope_graph_impl` cannot exist until provider-generated impl fragments have
been synthesized and merged.

The requested normalization chain is:

```text
lower_hir_ty(hir_ty, scope, assumptions)        [&dyn HirAnalysisDb]
  -> resolve_path(...)
     -> scope_graph_impl(...)                   [merged graph]
  -> normalize_type_fn_app(semantic_ty)
```

Calling that chain while `expanded_items_impl` is constructing the graph asks
for the graph currently being constructed:

```text
expanded_items_impl
  -> lower_hir_ty
     -> resolve_path
        -> scope_graph_impl
           -> expanded_items_impl               CYCLE
```

The type mismatch reflects this invariant: `ProviderExecutor` receives
`&dyn HirDb`, whereas `lower_hir_ty` and `normalize_type_fn_app` require
`&dyn HirAnalysisDb`. There is no valid trait-object conversion from the former
to the stronger latter.

This boundary is explicitly pinned in the source:

- `core/lower/mod.rs`: base expansion must not read `scope_graph_impl`
  transitively.
- `core/lower/derive.rs`: `expand_provider_impl` is a lowering-phase query,
  strictly upstream of analysis.
- `derive.rs::sgk_solver_guard`: solver sources may not reference generation
  queries.
- `analysis/ty/provider_goal.rs`: semantic provider-goal checks live after the
  merge because doing them during expansion would cycle.
- `core/lower/provider_synthesis.rs::requirement_where_clause`: crossing
  `ProviderOutput` into analysis is already recorded as deferred staged
  generation work.

## Options

### A. Duplicate a HIR-level recursive-type-function normalizer

Rejected.

It could run against the base graph, but it would duplicate type-function
well-formedness, const evaluation, unfolding fuel, normalization, and error
semantics. The two implementations could disagree on the exact plan used for
generated code versus the type checker.

### B. Defer provider execution until after ordinary analysis

A direct move after the current merged analysis is impossible: ordinary
analysis itself consumes `scope_graph_impl`, which currently requires provider
execution first.

The viable form is a two-world staged graph:

```text
base_scope_graph
  -> base-only name/type analysis
  -> normalized provider plan/effect trace
  -> synthesize provider HIR fragments
  -> merged_scope_graph
  -> ordinary full analysis
```

The smallest credible seam is therefore not “change
`expand_provider_impl(db: &dyn HirDb)` to `HirAnalysisDb`”. It requires a
base-only semantic environment whose path resolver is parameterized to read
`base_scope_graph_impl`, never `scope_graph_impl`.

Suggested query split:

1. `provider_request_skeleton(top_mod) -> requests`
   - lowering-only;
   - collects targets, providers, reflection, and stable expansion keys;
   - reads the base graph only.
2. `analyze_provider_input(key) -> NormalizedProviderInput`
   - analysis-grade but base-graph-scoped;
   - lowers reflected `TypeId`s and normalizes ground type-function apps;
   - returns semantic constructor identities and evaluated const arguments;
   - contains no generated impl lookup or solver call.
3. `execute_provider(key, normalized_input) -> ProviderOutput`
   - bounded interpreter;
   - consumes immutable normalized facts.
4. `synthesize_provider_impl(key, output) -> graph fragment`
   - existing HIR replay.
5. `merged_scope_graph_impl`
   - merges fragments, after which normal name resolution, solver, type
     checking, MIR, and codegen run unchanged.

This preserves the important direction:

```text
base facts -> generation -> merged analysis
```

and does not allow:

```text
merged solver -> generation -> merged solver
```

### C. Make normalized plan/support an explicit CTFE input

This avoids a compiler phase change: the program publishes a finite support or
plan sequence that the lowering-time provider can inspect. It is useful as a
short-term experiment, but it weakens the desired abstraction: the provider is
no longer deriving from the exact normalized `Schedule<N>` type alone.

## Recommended implementation sequence

1. Extract a resolver mode used by type lowering:
   `ScopeGraphMode::{BaseOnly, Merged}`.
2. Add a narrowly scoped base-only type lowering/normalization query for
   provider reflection. It must reject any operation requiring impl selection,
   associated-type normalization, or generated items.
3. Represent normalized inspection facts with semantic identities (`AdtDef`,
   evaluated const values), not reconstructed strings or source paths.
4. Execute providers over those facts and retain the existing step/command
   budgets plus the 256-node inspection cap.
5. Keep the SGK solver guard. Add an inverse guard ensuring the base-only
   provider-normalization module does not call trait resolution,
   `scope_graph_impl`, or `expanded_items_impl`.
6. Prove the seam with `Schedule<small N>` first:
   reflection normalizes to the exact `Add<Term<I>, ... Zero>` tree, provider
   traversal sees every term, and the synthesized method is then checked by
   ordinary merged analysis.

## Cycle and correctness risks

- Any use of the ordinary `resolve_path` from the base-only query can silently
  restore the cycle because it reads the merged graph.
- Associated-type normalization and trait selection cannot be admitted into
  the pre-generation island; generated impls may affect their answers.
- Diagnostics must be attributed to the derive request/provider expression,
  not emitted as an unrelated early type-check cascade.
- Semantic identities must cross the interpreter as interned compiler handles.
  Rendering and reparsing names would lose module identity and alias hygiene.
- Cache keys must include the base graph inputs and normalized reflected type,
  but not mutable fuel. Type-function normalization already demonstrates the
  rooted local-counter pattern.

## Decision

Do not call `lower_hir_ty` from the current executor and do not add a second
normalizer. Build the base-only semantic island, or use explicit CTFE support
as a temporary experiment while that staged seam is implemented.
