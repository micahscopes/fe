# Generated HIR carrier

`#[generate]` evaluates a checked Fe provider and freezes its result before
the output view is built. Functions, arrays of functions, and trait impls now
become HIR through `HirBuilder`, including declarations on const-generic
owners. A structural budget on the frozen provider result limits resource use
before HIR construction: each value node costs eight units plus its scalar
payload bytes, with 65536 units per request and 1048576 across a project.
The completed HIR then enters ordinary checking.
Printed text is for display only and cannot change admission.

Construction errors from provider evaluation, descriptor validation, and
direct HIR construction point to the authored `#[generate]` attribute. Named
limits report actual and maximum values.

## Two views, one staging boundary

The authored view contains declarations lowered from input files, including
local syntactic desugarings. Provider lookup, type checking, reflection, and
CTFE use that view. A provider returns a
bounded, checked `core::meta` description. A separate expansion query decodes
that value and uses `HirBuilder` to make ordinary HIR items. The output view's
module index combines authored items with those built items; ordinary name
resolution, coherence, type checking, MIR, and codegen consume the output view.
The decoder checks designated core-item identity and interprets record fields
and enum cases by their declared names, so reordering the core definitions
does not change generated meaning.
The provider result must cross the query boundary as a frozen copy of its
checked `core::meta` value. It contains scalar data, named record fields,
named enum cases, and owner-qualified structural handles, not a second syntax
or builder-command language. Salsa inputs cannot retain database-lifetime HIR
or CTFE handles across revisions. Freezing the value also means the expansion
query never has to evaluate a provider while constructing the output graph.
Freeze all results and validate their owners before publishing any output;
the expansion input changes once for the completed pass. Re-analysis after an
edit clears that input before evaluating providers, so they see authored items
only. The output graph reads the frozen input and builds HIR; its cache key
includes that input. This keeps one ordinary Fe CTFE evaluator and one
declaration decoder.
The expansion query may read the authored view, but no query reachable from a
provider may read the output view or a generated-item query. A provider cannot
observe its own output or another provider's output from the same pass. This
keeps the existing explicit generation stage, without a second evaluator or a
third representation for generated declarations. Local syntactic desugarings
such as `#[event]` continue to use `HirBuilder` inside lowering.

## Identity and names

Every generated item receives a key made from the authored generation-site ID,
the resolved provider identity, the target's nominal type identity, and its
ordinal within that provider invocation's own output. Children extend that key
with their role and local ordinal. Neither file offset, sibling invocation
order, printed content, nor query execution order is part of the key. Equal
looking outputs from distinct sites remain distinct. Re-analysis and inserting
an unrelated sibling before a site must preserve its generated identities;
changing one invocation's own output may change only that invocation's keys.
The function carrier uses the target's frozen file and nominal path, the
provider's frozen identity, the attribute index, and an ordinal within that
provider result. A raw HIR `Struct` handle is not suitable as the nominal key:
re-lowering can replace it after an unrelated sibling edit. Salsa 0.20 also
includes unmarked `Func` and `ImplTrait` fields in tracked-struct identity, so
the changing body and source origin must be tracked values rather than identity
fields.

Explicit descriptor path roots now carry an ingot identity in generated HIR:
`ingot` binds to the provider's ingot, and `core` and `std` bind to their
built-in ingots. The path printer shows a readable root name, but later name
resolution starts from the captured ingot. A fixture with competing `bump`
functions and a target-local module called `policy` checks that both calls
reach the provider's function. Built-in primitive descriptor types use a
direct HIR primitive node, so target aliases cannot capture them. The
admitted explicit-root descriptors therefore do not depend on target-local
names; broader lexical paths are not part of this descriptor language.
Owner-bound handles identify the target's fields; output-scope visibility and
trait rules are checked on the completed item. A name in the target module
must not silently capture a provider's reference, and a name visible only in
the target module must not make an otherwise invalid provider description
valid.

## Remaining verification

The current fixture set prints generated functions and trait impls, re-lowers
them in an isolated test view, and compares normalized HIR structure while
excluding origins and interned IDs. Expand this as admitted shapes grow. Check
resolved bindings and diagnostics separately, since printing cannot prove
hygiene. Identity tripwires cover functions and impls after sibling insertion,
plus warm re-analysis and provider output edits for both functions and impls.
The CLI tests check, build, and execute generated accessors and ABI code at
O0 and O2; separate tests reject invalid output. Generated impls also pass
through the ordinary coherence checks. Continue extending structural and
execution coverage as new descriptor shapes are admitted, including `#[fixed]`
interactions. Source insertion and its old renderer are gone. Display uses
ordinary HIR printing after construction.
