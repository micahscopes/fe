# One type reduction boundary

Type normalization should give the same answer regardless of whether a type
reaches it through an associated projection, a generic associated family, or a
recursive type function. These are different redexes in one reduction, not
independent normalization services.

The entry takes a type, lexical scope, and proven trait assumptions. One
root-local reducer uses iterative worklists to reduce children before their
parent and retains each result under its const-evaluation policy. A rewrite
schedules its result on the same worklist. Family application substitutes its
body, a projection resolves an associated output, and a ground recursive type
function selects a checked arm. Symbolic projections remain symbolic.

Each rewrite consumes the same root-local depth and work budget. Nested
projection searches use the same reducer state and budget. Cached
results retain their depth cost, so reusing a deep result in a deeper context
cannot bypass the bound. A family or projection that grows without repeating a
type reaches the limit; a repeating active redex reports a cycle. Both paths
produce named errors. Sibling normalization requests start fresh budgets.

The normalizer does not recover a Salsa cycle by guessing a provisional type.
Associated-output selection is not monotone: adding a second candidate can
turn a unique result into ambiguity. Cross-query re-entry therefore needs a
specific diagnostic or an audited uncached route. A tracked entry may cache a
completed reduction and its measured cost, but local in-progress state and
the budget belong to one execution, and a cache hit must still register every
dependency needed for incremental revalidation. The historical audit is
`/workspace/mb2/SALSA_TRACKED_NORMALIZATION_DESIGN.md`.

## Const evaluation and containment

Type functions are pure and first-order. Their own recursive calls decrease
the distinguished size argument, checked before an arm is used. A type
function may call an ordinary const function in a type-level payload. Const
evaluation has step and call-depth limits; it cannot perform generation or
other elaboration effects. The evaluator may ask for another type's normal
form, for example through `size_of`, so a fuel limit alone does not make the
combined query graph acyclic. Re-entry into a type function while checking
that definition is rejected with a named invalid-definition error. A payload
that inspects a different completed type remains valid. No normalization-value
cycle handler may turn this situation into a successful type.

The const policy is explicit. Ordinary type metadata stays unevaluated unless
the surrounding operation needs its value. A projection result evaluates its
ground output consts after impl substitution. A ground type-function
application evaluates its const arguments and payloads with the checked CTFE
engine. This policy is part of the worklist key, because the same interned
type can have different intermediate forms under these contexts.

## Acceptance cases

- Finite computed chains, including the existing 4,095-step depth probe, keep
  working without Rust stack growth; the next step gets a named limit
- Mixed computed type, projection, and family growth uses one budget and
  terminates with a named limit or cycle
- Symbolic projections remain symbolic until an actual binding or unique
  implementation establishes an output
- A const payload querying its own type-function definition reports an error;
  a query of a different completed computed type succeeds
- Existing family, projection, selected-output, CTFE, and warm/fresh edit
  fixtures retain their meanings
