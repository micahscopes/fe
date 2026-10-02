# Quotation and derive on the generated HIR carrier

`quote { ... }` parses ordinary Fe expressions, and `quote { fn ... }` parses
ordinary method syntax. A splice `${value}` is an expression evaluated by the
existing CTFE machine. The quoted syntax is inert: CTFE may construct, copy,
and compose a code value, but it does not execute the quoted body. The code
value uses one compiler-owned structural representation. A checked pure
function body can later be read into that same representation, so quotation
does not define a second language of `core::meta` operations.

The first consumer is a provider returning a generated method description.
The provider still runs against authored declarations only. Its result is
frozen under the generation budget before the output view is published.
`HirBuilder` constructs the method in the target module, and the ordinary
checker validates its full signature and body. A generated method and a
source method then enter the same coherence and code generation paths.

Names have three different roles:

* A name resolved in the provider's scope is captured by resolved identity,
  including its ingot, before the result is frozen. A target-local name with
  the same spelling cannot capture it
* A name declared in `quote(name, ...)` is an explicit parameter of the
  eventual destination. Closing the quote requires a matching destination
  binder; it cannot silently bind to a provider local
* A field or variant splice carries its owner-qualified structural handle.
  Emission checks that the handle belongs to the authorized target and that
  it is used in the appropriate syntactic position

The code value must retain its fragment kind, source spans, resolved paths,
open binders, and splice kinds through freezing. It must not retain a live
database handle across the stage boundary. A malformed quote or a wrong-kind
splice fails at the quote or generation site. A well-formed quote whose
generated method violates Fe's type, visibility, ownership, or trait rules
fails in ordinary checking with a link back to the generation site.

Derive authoring uses this same path. `impl Derive<G> for P` supplies a
provider for a concrete goal `G`. Ordinary `uses` and `with` select among
providers. Unscoped lookup prefers the derived `Default` tier, then the sole
unnamed impl, and never selects a named impl implicitly. `#[derive(G)]` is
surface sugar for a checked generation request, not a second executor or a
separate class of impl. Generic record and enum cases must use the same
selection and generated HIR path.

The first vertical fixture should generate and execute a method with an open
parameter, a reflected field splice, a provider-scoped call, and a bool
splice. A companion fixture must put a same-spelled call in the target module
and prove that the provider binding wins. The follow-up fixtures cover wrong
splice kinds, mismatched open names, complete generic signatures, enum arms,
and readback of an ordinary checked pure method body into the code value.
