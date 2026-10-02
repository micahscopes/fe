# Generated HIR carrier

`#[generate]` evaluates a checked Fe provider returning `Code<Item>` and freezes
its quote before the output view is built. Quoted functions and trait impls
become HIR through `HirBuilder`, including declarations on const-generic
owners. A structural budget on the frozen quote limits resource use
before HIR construction: each value node costs eight units plus its scalar
payload bytes, with 65536 units per request and 1048576 across a project.
The completed HIR then enters ordinary checking.
Printed text is for display only and cannot change admission.

Construction errors from provider evaluation, quote validation, and
direct HIR construction point to the authored `#[generate]` attribute. Named
limits report actual and maximum values.

## Two views, one staging boundary

The authored view contains declarations lowered from input files, including
local syntactic desugarings. Provider lookup, type checking, reflection, and
CTFE use that view. A provider returns a bounded, checked `Code<Item>` value.
A separate expansion query transplants its checked syntax and uses `HirBuilder`
to make ordinary HIR items. The output view's
module index combines authored items with those built items; ordinary name
resolution, coherence, type checking, MIR, and codegen consume the output view.
The carrier checks designated core-item identity for structural handles.
The provider result must cross the query boundary as a frozen copy of its
checked quote. It contains authored template locations, checked holes, rooted
references, and owner-qualified structural handles. Salsa inputs cannot retain
database-lifetime HIR
or CTFE handles across revisions. Freezing the value also means the expansion
query never has to evaluate a provider while constructing the output graph.
Freeze all results and validate their owners before publishing any output;
the expansion input changes once for the completed pass. Re-analysis after an
edit clears that input before evaluating providers, so they see authored items
only. The output graph reads the frozen input and builds HIR; its cache key
includes that input. This keeps one ordinary Fe CTFE evaluator and one
checked HIR construction path.
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
The quote carrier uses the target's frozen file and nominal path, the
provider's frozen identity, the attribute index, and an item ordinal within
that provider result. A raw HIR `Struct` handle is not suitable as the nominal key:
re-lowering can replace it after an unrelated sibling edit. Salsa 0.20 also
includes unmarked `Func` and `ImplTrait` fields in tracked-struct identity, so
the changing body and source origin must be tracked values rather than identity
fields.

Quoted paths retain checked provider-side references when copied into output
HIR. A target-local name cannot redirect a provider function or qualified
constant. Primitive and owner field type holes are checked before copying.
Owner-bound handles identify the target's fields. A quoted body's names keep
their provider-scope meaning and visibility, including access to private
helpers there. Generated item signatures obey output visibility and trait
rules, so a public signature cannot expose a provider-private type. A name in
the target module must not capture a provider reference or make an invalid
provider description valid.

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
execution coverage as new quote shapes are admitted, including `#[fixed]`
interactions. Source insertion and its old renderer are gone. Display uses
ordinary HIR printing after construction.
