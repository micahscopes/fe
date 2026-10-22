# Bound schemas and closed instance evidence

This follow-up builds on the verified scoped-derive stack at
`cdf485b8f848e14e6e6bfd653d593281fcdb4137`. It changes compiler-internal
proof transport; it adds no language-level Evidence or Constraint values.

## One transported result

`BoundSchemaId` names the declaration or adapter basis owning a dictionary.
`BoundSlotRef` retains that identity and its immutable position alongside the
position-preserving applied goals. Substitution may make two goals equal; it
must not deduplicate their slots. Supertrait projection searches the applied
view but returns the original schema position.

`UseEvidence` transports both the selected dispatch proof and the target's
bound dictionary. `BoundAdaptation` gives method and constant adapters explicit
source and destination schemas. Binding, folding, visitation and deferred-codec
completion are shared. A failed completion of a supplied tree remains failure;
it cannot become an absent entry and authorize ordinary parent selection.

Closures retain their captured dictionary in `ClosureContext`. This is finite
proof data, without a pointer to an enclosing instance key. Effect-row proofs
travel with their originating `RowPath`. `ProviderEvidenceAdaptation` performs
the same declaration-side provider substitution and row projection for entry
completion and body binding. It does not query the instantiated body or select
an implementation. A failed supplied row projection blocks body admission.

## Closed instances own their normalization facts

The shared call, constant and root finalizer validates the destination schema,
applies substitutions, completes explicitly deferred leaves and checks each
retained proof against its goal. A proof use retains associated equalities and
mode separately from the implementation head. Missing ordinary obligations may
be proved by this producer; supplied trees are never replaced.

`InstanceEvidence::Closed` stores the declaration schema, applied goals, full
proofs, dispatch use and ingot origin. It stores no caller `ImplEnv`.
Its compatibility normalization view is derived from those proved goals and
row/dispatch uses. The instantiated body binds the closed proofs directly.

This removes the old context-independent/nominal-redundant re-proving walks,
ordinary proof stripping during key construction, root context reconstruction,
and late ordinary solving for the closed subset. Concrete method signature
validation remains because it checks the selected body's instantiated shape.
Structural key construction and type folding do not run proof completion.

Runtime instances preserve caller-ingot lookup identity. Constant invocation
canonicalization follows the existing empty-facts rule and rebases only the
origin of a closed dictionary, preserving its proofs and facts. Two ingots
calling the same constant therefore share a term without conflating their
runtime instances.

## Exact residual boundary

The producer classifies why completion remains residual and preserves the
original supplies. The classification is not a semantic key input: merely
attempting completion cannot create a distinct constant term. Residual
instances are not a second independent selection policy:

- Generic or projected inputs/goals/proofs still require symbolic declaration
  facts. In particular, a selected local implementation may capture a generic
  argument absent from its trait head even when its callee's goal is ground.
  Its selected parent must survive until that captured argument is applied.
- A declaration identity with an unsupplied effect binding retains its
  declaration environment. Actual calls with concrete provider dictionaries
  use the shared provider adaptation and may close.
- Closures and contract bodies do not yet expose a declaration-only complete
  bound schema. Closure inference currently checks inline in its parent, so
  calling that inference from schema production would reverse the dependency.
  They retain their existing captured dictionary and owner environment. The
  removal condition is a checker-produced schema for the exact captured layout,
  including effect-row slots, transported with closure metadata; no parent-body
  inference may be introduced into the schema query.
- Structural constant-predicate keys remain structural while the predicate's
  own candidate is being established. Finalizing by proving that same candidate
  would create a dependency cycle. Their declaration context is an input to the
  existing obligation producer, not evidence of a completed runtime instance.

Destination mismatches, incompatible closed proofs and failed deferred supplies
are errors at completion, not executable residual results. Missing ordinary
proofs in schematic declaration analysis remain explicit residuals; executable
admission still checks the underlying obligations. No new consumer should
construct an independent normalization or selection context to bypass the
shared finalizer.

Type-key effects also supply a checked handle capability. A projected field can
have target `StorageMap<K, V>` while its provider representation is the enclosing
`Store`; this does not establish a global implementation of
`Store: EffectRef<StorageMap<K, V>>`. Shared provider adaptation retains a finite
`EffectBinding` certificate for an absent induced slot, after checking the
resolved binding, target and required mutability. Runtime place projections
remain call operands. Existing supplied implementation trees are preserved.

Constant terms with empty fact contexts canonicalize their origin to the
callee's ingot, including symbolic calls written in different anonymous bodies.
Runtime origins retain their existing behavior. Closed proof validation compares
constant arguments using the compiler's existing constant identity rules rather
than requiring identical intermediate representations.

## Verification

Final exact-source receipts will be added after the complete gates. Preserve
all failed intermediate histories: index collapse, captured-proof loss,
constant-origin identity and overbroad provider completeness each have distinct
controls. A test-only shadow result is not a production-coverage claim.
