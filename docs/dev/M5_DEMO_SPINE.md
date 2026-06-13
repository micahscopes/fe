# FCO-M5 demo spine

M5 makes `where`-clause const predicates **first-class obligations**: a
backend/platform fact written as an associated const (or const generic) can
appear in a `where` predicate, become an obligation, and be **discharged at the
obligation level** — never inside the trait solver.

M5 is successful when Fe demonstrates:

> A backend/platform fact expressed as an associated const can appear in a
> `where` predicate, become a first-class obligation, discharge for a
> satisfying backend, fail for a non-satisfying backend with a useful
> diagnostic, record evidence with room for premises/dependencies, and allow
> only the satisfying program to proceed toward lowering.

## Required demo

- A platform-like trait or equivalent (`trait Platform { const WORD_BITS: u256 }`).
- An associated-const fact such as `WORD_BITS`, `HAS_STORAGE`, or `SIZE`.
- A generic function requiring that fact (`fn word_op<B: Platform>() where B::WORD_BITS == 256`).
- A satisfying backend/type (`Evm`, `WORD_BITS == 256`) — compiles.
- A failing backend/type (`Tiny`, `WORD_BITS == 16`) — fails with a named diagnostic.
- Obligation-level discharge (CTFE under the call's type substitution), **not**
  trait-solver CTFE.
- An evidence route/origin/premises slot visible from a test or debug hook.

## How the spine runs (current implementation)

```
where B::WORD_BITS == 256          parser + HIR (S1): a const-predicate body
        │                          on WhereClauseId::const_predicates
        ▼
call site word_op<Evm>()           enqueue_constraints: one
        │                          ConstPredicateObligation per callee predicate,
        ▼                          carrying the call's type args [Evm]
DeferredTask::ConstPredicate       same deferred queue as trait obligations
        │                          (the unified obligation path)
        ▼
eval_body_owner_const(body,[Evm])  CTFE substitutes B := Evm, resolves
        │                          Evm::WORD_BITS, evaluates `256 == 256`
        ▼
true  → DischargedConstPredicate   evidence: route = Ctfe, premises = []
false → 8-0085 const predicate not satisfied  (hard error, at the call site)
fault → hard error                 (CTFE fault is never a skipped candidate)
```

CTFE runs in the obligation loop, never in the trait solver / proof forest.

## Non-goals

- Full Sonatina v2 / VSDG / separation logic.
- Predicate-based specialization (gate, do not select).
- General implication solver / theorem proving.
- Full chained-projection support (`B::Memory::ADDRESS_SPACE`) — named
  diagnostic, not an ICE.
- Symbolic associated-const predicates that *discharge* against a generic
  caller's restated assumption (verbatim assumption matching): the term
  representation is interned now; assumption-route discharge is later work.
- Public stability for the term / evidence formats.

## Hard semantic constraints (must hold)

1. Gate, do not select.
2. Hard error on predicate fault; no SFINAE / candidate removal.
3. CTFE must not run inside the trait solver / proof forest.
4. Symbolic associated constants are accepted and interned as neutral terms.
5. Const predicates use the unified constraint/obligation path.
6. Evidence has a premises/dependencies slot (empty at M5).
7. Unsupported projection forms get named diagnostics, not ICEs.
8. Internal term/evidence versioning never blocks the demo.
