# CASCADE dream fixtures — executable spec

> **LIVE (executable spec for the cascade).** The cascade model + drive-green spine these fixtures encode is described in `FCO_THE_SLIDE_2026-06-19.md` (the "CASCADE" + "SUMMIT DESIGN" sections) and the ratified `Fix`/establishment model; `FCO_MAP.md` is the one-page entry point.

These seven fixtures are the **executable spec** for the first-class-obligations
CASCADE feature ("coherent cascading shadowing"): multiple REAL global trait
impls for the same `(Trait, Type)` may coexist, and a lexical SCOPE selects which
one is used (recorded so codegen uses exactly that one). They are written
source-only and staged HERE (outside the test harness) on purpose — install them
into the real fixture dirs and drive them green via the spine
**C3b → C3c-2-wire → C3c-3-flip** (scoped-selection surface → unscoped default
resolution wiring → the non-canonical coherence demotion flip). The canonical
**money-floor** stays at exactly-one and must never flip (#1).

The cascade rules these demonstrate:

- **Scoped selection** — an inner scope names a goal via the PROVISIONAL surface
  `with (Trait<Type>) { .. }` (a `with`-binding whose value path resolves to a
  trait instance); inside the block calls resolve to the selected impl.
- **Shadowing** — nested scopes; innermost selection wins.
- **Unscoped default-tier** — with >1 impl and no selecting scope, the
  DEFAULT-marked (canonical/CoreDerives-origin) impl wins; never an ambiguity
  panic.
- **Canonical money-floor** — for the canonical set
  (`AbiSize`, `Encode`, `Decode`, `StorageKey`) a 2nd impl is STILL rejected
  (`5-0001`). Soundness guard.
- **MIR determinism** — a scoped selection driving a runtime trait call lowers to
  exactly the recorded implementor (no re-resolution window).

### PROVISIONAL `with (...)` surface — read before driving

The selection surface is **provisional** and WILL CHANGE with the "keystone".
Every fixture that uses it carries a `PROVISIONAL-SURFACE` note in its header.
Two spellings appear, both reusing the existing `with (..) { .. }` block
expression (`WithExpr`) so they parse with today's grammar:

- `with (<T as Trait>) { .. }` — names the `(Trait, Type)` GOAL. Used where one
  alternate is being selected against the default (#4, #6). Selects the sole
  `Anonymous`-discriminator override.
- `with (Name) { .. }` — names a SPECIFIC impl by its `as Name` ALIAS (FCO #84
  T-Nway: `impl Trait for Type as Name`). Now REAL (inc2/inc3): a bare-ident
  `with` head is looked up as an impl alias. Used where ≥2 non-default impls must
  be told apart at one goal (`cascade_nested_shadowing`,
  `cascade_greeting_dialects`).

If the keystone unifies these (e.g. always select by goal+value, or always by
impl identity), re-spell the `with (..)` heads and KEEP the assertions — the
assertions encode the semantics, the heads are placeholders.

## Fixtures

| Fixture | The dream | Pass criteria | Driven-by rung | Install target | Expected status TODAY |
|---|---|---|---|---|---|
| `cascade_canonical_floor.fe` | Two impls of a CANONICAL trait (`AbiSize`) for one type are still rejected. | `5-0001` conflicting-impl diagnostic fires on the two `impl AbiSize for Token` blocks. | works today (money-floor wired; C3c-1 seam) | ty_check (`.fe`+`.snap`) | **GREEN-ish** — already conflicts today; needs a `.snap` capturing `5-0001`. The soundness guard; should still behave after C3c-3. |
| `cascade_noncanonical_two_impls.fe` | Two impls of a user (non-canonical) trait for one type coexist with no conflict. | Compiles with NO `5-0001`; both impls accepted; `two_impls_coexist_and_compile` passes. | C3c-3 | fe_test | **RED** — today the 2nd `impl Volume for Speaker` is a `5-0001` conflict. Goes green at the demotion flip. |
| `cascade_unscoped_default.fe` | Two coexisting impls, no scope → default-tier impl used, no ambiguity. | Unscoped `level()` returns default (3); no ambiguity panic; both tests pass. | C3c-2-wire + C3c-3 | fe_test | **RED** — `5-0001` today; after coexistence, also needs the unscoped resolution to deterministically pick the default tier. |
| `cascade_scoped_alt_impl.fe` | Inner `with (Volume<Speaker>)` selects the non-default impl; outside uses default. | Same `level()` call = 11 inside the block, 3 outside; helper calls inside also use alt. | C3b + C3c-3 | fe_test | **RED** — `5-0001` today; needs the scoped-selection surface (C3b) + demotion. |
| `cascade_nested_shadowing.fe` | Three nested scopes select different impls; innermost wins. | `level()` reads 3→5→7→9→7→5→3 across nested `with` blocks. | T-Nway (#84 inc2) | fe_test | **GREEN** — real surface: an unaliased default `impl Volume for Speaker` (`Anonymous`) + `impl Volume for Speaker as LevelA/LevelB/LevelC`; nested `with (LevelA/B/C)`. Innermost-wins comes free from the effect-env frame stack. |
| `cascade_mir_determinism.fe` | A scoped selection drives a runtime loop of trait calls; sum matches the selected impl deterministically. | Unscoped loop sum = n·3; scoped loop sum = n·11; no per-call drift; runs through codegen. | C1 rail + C3b + C3c-3 | fe_test | **RED** — `5-0001` today; the executable determinism guard once the spine lands. |
| `cascade_greeting_dialects.fe` | Showcase: `Greeting` for `User`; scope picks the dialect. | `greet()` = 1 (unaliased default) unscoped, = 2 in `with (Casual)`; distinguishable per scope, incl. through a `welcome<T: Greeting>` helper. | T-Nway (#84 inc2) | fe_test | **GREEN** — real surface: unaliased `impl Greeting for User` (`Anonymous`, == 1) + `impl Greeting for User as Casual` (== 2); `with (Casual)` selects the alias. |

## Notes for the install + drive pass

- **#1 needs a `.snap`.** I authored only the `.fe`. ty_check fixtures pair with a
  `<name>.snap` (insta). Generate/accept it on install; the expected diagnostic is
  `error[5-0001]: conflicting trait implementations` pointing at the two
  `impl AbiSize for Token` blocks. If you instead install it under
  `cli_output/single_files`, that harness emits a `.snap` with the `=== STDERR ===`
  / `=== EXIT CODE: 1 ===` framing — pick whichever matches the canonical-floor
  regression home.
- **#2 install target.** Written as an fe_test (it has a `#[test]`), but its real
  signal is "compiles, no `5-0001`". If you prefer a pure no-diagnostics snap,
  move it to `cli_output/single_files` and drop the `#[test]` body — the dream is
  coexistence, not a runtime value.
- **Default-tier marking is unresolved upstream.** #3/#5/#7 assume the default
  tier is the first-declared / CoreDerives-origin impl. If the driver adds an
  explicit `default impl` / `#[default]` surface, re-spell impl A accordingly and
  keep the value assertions (default == 3 / Formal == 1).
- All `with (...)` heads are PROVISIONAL (see above) — they parse on today's
  `WithExpr` grammar but their *meaning* (select a trait impl for a scope) is the
  thing C3b implements.
