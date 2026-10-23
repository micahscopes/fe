# Computed types

A `recursive type fn` computes a type from type parameters and constants.
Its last parameter is a `usize` subject; its body is one `match` on the
subject whose arms are types.

```fe
struct Leaf { v: u256 }
struct Node<A> { l: A, r: A }

// VTree<0> is Leaf, VTree<1> is Node<Leaf>, VTree<2> is Node<Node<Leaf>>, ...
recursive type fn VTree<const N: usize>() -> * {
    match N {
        0 => Leaf
        _ => Node<VTree<{N - 1}>>
    }
}

fn same(t: own VTree<2>) -> Node<Node<Leaf>> { t }
```

An application with a known subject, such as `VTree<3>`, is the type its
arm gives: `VTree<3>` and `Node<Node<Node<Leaf>>>` are the same type, in a
signature, a field, a `let`, a trait bound or a call. An application whose
subject is a parameter, such as `VTree<N>`, is a type of its own that is
equal only to itself; a generic function states what it needs of it:

```fe
fn total<const N: usize>(_ x: own VTree<N>) -> u256 where VTree<N>: Sum {
    x.sum()
}
```

A caller at a concrete size, `total<2>(tree)`, meets `VTree<2>: Sum`
through the implementations for `Node` and `Leaf`.

## Declaring one

```fe
recursive type fn Tree<T, const O: usize, const N: usize>() -> (*) where T: Marker {
    match N {
        0 => T
        _ => Pair<Term<{label(N - 1, O)}>, Tree<T, O, {N - 1}>>
    }
}
```

- The parameters are type parameters, `const` parameters, and a final
  `const` subject of type `usize`. The empty `()` and the return kind after
  `->` (`*`, `* -> *`, ...) are required.
- The `match` inspects the subject. Arms are integer literals and a final
  `_`, which is required.
- At least one arm applies the function itself. A recursive call passes
  every other parameter unchanged and in order, and a subject smaller than
  the one its arm can see: `{N - k}` with a literal `k` no larger than the
  smallest value the arm matches, `{N / k}` with a literal `k` of at least
  2 in an arm that cannot see 0, or a smaller literal. So every
  application unfolds in finitely many steps.
- A body applies no other `recursive type fn`, directly or through a type
  alias, and an alias or associated type in an arm may not hide or add a
  recursive call.
- A const argument in an arm is a literal, a `const` parameter, a step of
  the subject as above, or a call of a `const fn`, named without generic
  arguments, whose arguments are of those forms. It is evaluated at compile
  time when the arm is used.
- A `where` clause bounds type parameters only. A written application must
  meet it, even when its result does not mention the parameter.

A mistake in a declaration is reported once, at the declaration; uses of
it report nothing further.

## Where they can appear

Computed types can be used wherever a type is written, including struct
and enum fields, `size_of`, aliases, associated types and explicit call
arguments. They cannot appear in an implementation header, whether trait
or inherent and whether written directly, through an alias or as a trait
argument: an application is the type it computes, so implement the trait
for that type (or for its constructors, as `Node` and `Leaf` above).
A computed type is only a type when fully applied: `VTree` alone, or
`Tree<bool>` with missing arguments, is an error.

## Limits and errors

Unfolding shares the normalization limits of associated types: at most 64
unfoldings and projections nested inside one another, and at most 65,536
type nodes written out per use. A linear chain such as `VTree<N>` therefore
unfolds up to `N = 63`, and one that halves its subject takes about
`log2(N)` steps. An arm that applies the function twice doubles the type at
each step: `Pair<Bush<{N - 1}>, Bush<{N - 1}>>` is accepted at `N = 13` and
reaches the work limit at `N = 14`. A limit is reported where the type is
written as "type normalization limit exceeded".

A const payload that faults, such as a division by zero for one subject, is
reported where the application is written, with the faulting operation as
a secondary label. A payload that needs the size or layout of the type
being computed, directly or through another computed type or const fn,
depends on itself; every application on that cycle is reported as
"computed type depends on itself", whichever is used first.

Not provided: proofs that a bound holds for every subject (state the bound
on the generic function instead), computed types in implementation headers,
and partial application.
