# Fe Derive/Metaprogramming Sketches: NOW → HEADED, plus `fixed` / `fix`

**Date:** 2026-06-17 · **Status:** code-forward design sketches (design-wizard, task #55).
**NOW** = quoted from real in-tree Fe (cited; spot-verified 2026-06-17 ✓). **HEADED / `fixed` /
`fix`** = PROPOSED, clearly labeled. Design-only, NON-BLOCKING.

> Verification note (2026-06-17): all "NOW" citations spot-checked against the live tree —
> `derived_eq_default.fe:7-11`, `derived_decl.fe:16-17`, `derived_using_provider.fe:45`,
> `static_abi_alias.fe:105`, `core_derives/lib.fe:26-114`, `std/abi.fe:120-160`,
> `core/derive.fe:21-40` (opaque `{ handle: u256 }` structs), `core/error.fe:9-12`,
> `storage_map.fe:8-11` all verified real. The `std/abi.fe` snippet below is lightly compressed
> (shows the const fold without the `payload`/`dynamic_payload_size_of` detail) — faithful, not
> exhaustive.

---

## A. NOW (real, grounded)

### A.1 Using a derive — three real spellings

```fe
// crates/fe/tests/fixtures/fe_test/derived_eq_default.fe:7-11  (attribute form)
#[derive(Eq)]
struct Point {
    x: u256,
    y: u256,
}

// crates/fe/tests/fixtures/fe_test/derived_decl.fe:16-17  (standalone, canonical provider auto-selected)
derive Eq for It
derive Default for It

// crates/fe/tests/fixtures/fe_test/derived_using_provider.fe:45  (named provider selection)
derive Eq for Point using StableEq
//                       ^^^^^^^^^^  ← SHIM: `using StableX` = named provider selection by bare ident

// crates/fe/tests/fixtures/fe_test/static_abi_alias.fe:105  (the std AbiSize provider, NAMED only)
derive AbiSize for Word using StableAbiSize
//     ^^^^^^^                ^^^^^^^^^^^^^  ← std provider; NEVER auto-selected (multi-backend, non-canonical)
```

### A.2 A full provider with quasiquote + a capability `uses(...)` signature

```fe
// ingots/core_derives/src/lib.fe:26-48 (StableEq, struct arm)
impl StableEq: Derive for Eq {
//             ^^^^^^      ← SHIM: `Derive` is a STRING-MARKER / special parser head
//                           (is_named_derive_provider_head), NOT a real kinded trait.
//                           Selection compares resolved goal-def == head-def (#1 done); the
//                           token still means "special-executor provider" (board #7, gated on TD5).
    const fn derive<T>(ev: own Evidence<Eq<T>>) -> Evidence<Eq<T>>
//                         ^^^ own       ^^^^^^^    ← SHIM: Evidence<G> is an OPAQUE core::derive
//                                                    struct (derive.fe:38, single private `handle`,
//                                                    no ctor) — recognized by RESOLVED IDENTITY
//                                                    (#2/#3 done), NOT an application of a real
//                                                    `Constraint -> *` constructor. Kept linear
//                                                    (`own` in / out) by convention, not by a kind.
        uses (
            reflect: Reflect<T>,                  // ← SHIM: opaque read capability, identity-recognized
            builder: mut ImplBuilder<Eq<T>>,      // ← SHIM: opaque write capability, identity-recognized
        )
    {
        if reflect.is_struct() {                  // ← SHIM (post-TD5c): typed ReflectHandle scalar read
            let mut body = quote { true }         // ← SHIM: `quote` = template strings expanded by the
            for field in reflect.fields() {       //          bespoke provider executor, NOT a real
                builder.require<Eq>(field.ty())   //          compile-time effect. `reflect.fields()` is
                body = quote(other) { ${body} && self.${field} == other.${field} }
            }                                     //   ← SHIM: `builder.require<Eq>` records a typed
            let mut sig = builder.method("eq")    //     ProviderEffect::Require (TD5.1/5.2a done) but
            sig = builder.with_self(sig)          //     does NOT yet flow through the ordinary obligation
            sig = builder.with_arg(sig, "other", builder.target_ty())
            sig = builder.returns(sig, builder.ty<bool>())
            builder.emit_method(sig, body)        //     queue (TD5.2b PS-MR-gated).
        }
        // ... enum arm (lib.fe:49-110): folds `match self { ${arms} }` over reflect.variants()
        builder.finish()                          // ← SHIM: one of the frozen executor ops (TD5.0)
        ev
    }
}
```

The std `AbiSize` provider is the same shape over a *fact* fold instead of a method body:

```fe
// ingots/std/src/abi.fe:120-160 (StableAbiSize — the Fe replacement for the deleted Rust generator, #5b)
impl StableAbiSize: Derive for AbiSize {
    const fn derive<T>(ev: own Evidence<AbiSize<T>>) -> Evidence<AbiSize<T>>
        uses ( reflect: Reflect<T>, builder: mut ImplBuilder<AbiSize<T>> )
    {
        if reflect.is_struct() {
            let mut head    = builder.trait_const(builder.ty<()>(), "HEAD_SIZE")   // 0-seed = <() as AbiSize>::HEAD_SIZE
            let mut dynamic = builder.bool(false)
            for field in reflect.fields() {
                builder.require<AbiSize>(field.ty())                               // re-enters checking → 6-0003 on bad field
                head    = builder.add(head, builder.trait_const(field.ty(), "HEAD_SIZE"))
                dynamic = builder.or(dynamic, builder.trait_const(field.ty(), "IS_DYNAMIC"))
            }
            builder.emit_const("HEAD_SIZE",  builder.ty<u256>(), head)
            builder.emit_const("IS_DYNAMIC", builder.ty<bool>(), dynamic)
        }
        builder.finish()
        ev
    }
}
```

### A.3 The desugar shape (what `#[error]` / `derive` produce)

```fe
// What `derive Eq for Point` / `#[derive(Eq)]` produce (synthesized impl, carries a
// `Desugared(Derive)` origin — crates/hir/.../diagnosable.rs:750, derive.rs:10):
impl Eq for Point {
    fn eq(self, other: Point) -> bool {
        true && self.x == other.x && self.y == other.y   // MIR folds the `true` seed away
    }
}
// Provenance is RECONSTRUCTED (no stored state, #4 done): generated-impl → provider → goal
// from the `Desugared(Derive)` origin + trait_ref via resolved-identity selection.

// `#[error] struct E { .. }` desugars to ErrorVariant + (scheduled NAMED AbiSize derive, Route A):
//   ingots/core/src/error.fe:9-12
pub trait ErrorVariant<A: Abi>: Encode<A> + AbiSize {
    const SELECTOR: A::Selector
}
// → impl ErrorVariant<A> for E { const SELECTOR = <compile-time keccak of sig> }
// → schedule_error_abi_size enqueues a synthetic `derive AbiSize for E using StableAbiSize`
//   (expansion.rs) — the std provider above produces the `impl AbiSize for E`.
```

---

## B. HEADED — the HKT / kinded target (PROPOSED, not implemented)

> **UPDATE 2026-06-17/18 — `Derive`'s kind decided (TWO-STAGE) + spike-validated. See
> `FCO_DERIVE_KIND_DECISION_2026-06-17.md` (SSOT).** Stage 1 INTERMEDIATE = the **associated-type
> form**: `Derive` a **first-order ordinary trait** (`Derive : * -> Constraint`), derived trait on
> `type Goal : * -> Constraint`, provider implements it (`impl Derive for StableEq { type Goal = Eq }`).
> Stage 2 END = the **elegant param-head form shown below** (`Derive : (* -> Constraint) -> Constraint`,
> `impl Derive<Eq> for StableEq`) — pushed through after the intermediate. A spike confirmed Stage 1's
> generic skeleton already kind-checks (projection head `Self::Goal<T>` computes `Constraint`, unlike a
> param head `P<T>` which hits the shelved `6-0008` frontier). Stage 2 is the "True Derive" track
> (`P:=Eq` pinned concrete → substitution, not variable-headed solving). So the `(*->C)->C` below IS
> the end target — just reached via the assoc-type intermediate.

> **⚠️ SUPERSEDED (2026-06-21) — see `FCO_THE_SLIDE_2026-06-19.md` "KEYSTONE INSIGHT".** The framing below ("ordinary CTFE" / "executor → CTFE de-magic" / "provider bodies become ordinary effectful CTFE") describes engine **fusion** and is superseded. The settled decision is **stage, don't fuse** (twice-measured): the executor is a **quasiquoter backend** (GenExpr→HIR, not a value-evaluator) run as a **downstream salsa query** producing a real `impl`; it is NOT folded into the CTFE value-evaluator (CTFE-inside-the-solver = Salsa-cycle ICE, a measured dead-end). Near-term the **cascade SELECTS** among existing impls; the keystone later **RUNS** a deriver to **GENERATE** one — distinct steps.

Per `fe-design-wizard-kinded-derive-verdict-2026-06-15.md` (D7): project over `TraitInstId`/`PredicateListId`, **no `TyData::ConstraintTerm`**, abstract `P<T>` head SHELVED. The kinds become real; the `Derive` marker retires (board #7); provider bodies become ordinary effectful CTFE (TD5 complete).

### B.1 The kind signatures that become real

```fe
// PROPOSED — kind ascriptions the compiler enforces (K02a `Constraint` kind landed; D7 projection)
//   traits are            * -> Constraint        // `Eq`, `AbiSize`, `StorageKey`
//   Derive    : (* -> Constraint) -> Constraint  // takes a trait constructor, yields a constraint
//   Evidence  :  Constraint -> *                 // Evidence<Eq<T>> is a real application
//   ImplBuilder: Constraint -> *
//   Reflect   :  * -> *
```

### B.2 Provider — before → after

```fe
// ── NOW (real) ──────────────────────────────────────────────────────────────
impl StableEq: Derive for Eq {                                  // `Derive` = string marker (SHIM)
    const fn derive<T>(ev: own Evidence<Eq<T>>) -> Evidence<Eq<T>>
        uses ( reflect: Reflect<T>, builder: mut ImplBuilder<Eq<T>> )
    {
        if reflect.is_struct() {                                // executor-interpreted reflection
            let mut body = quote { true }                       // template-string `quote` (SHIM)
            for field in reflect.fields() {
                builder.require<Eq>(field.ty())                 // executor command, not the queue
                body = quote(other) { ${body} && self.${field} == other.${field} }
            }
            builder.emit_method(/* sig */, body)                // executor op
        }
        builder.finish(); ev
    }
}

// ── HEADED (PROPOSED) ────────────────────────────────────────────────────────
// `Derive` marker GONE (#7). A provider is an ordinary `impl` of a real kinded
// `Derive` constraint-constructor; the body is ordinary effectful compile-time Fe.
impl Derive<Eq> for StableEq {                                  // real `Derive : (*->Constraint)->Constraint`
    const fn derive<T>(ev: own Evidence<Eq<T>>) -> Evidence<Eq<T>>   // Evidence : Constraint -> * (real application)
        uses ( reflect: Reflect<T>, quote: mut Quote, emit: mut Emit )  // `quote`/`emit` = REAL effects (TD5.4/5.6)
    {
        if reflect.is_struct {                                  // typed ReflectHandle (TD5.3, already landed-ish)
            let mut body: Hir<bool> = quote { true }            // typed-hole quote → HIR, hygienic (TD5.4)
            for field in reflect.fields() {
                require Eq<field.Ty>                             // ORDINARY obligation onto the queue (TD5.2b)
                body = quote { ${body} && self.${field} == other.${field} }
            }
            emit.method("eq", |other: Self| -> bool { ${body} })  // ordinary generated-HIR builder (TD5.5)
        }
        ev                                                      // no `builder.finish()` op — body just returns
    }
}
```

> **⚠️ SUPERSEDED (2026-06-21) — see `FCO_THE_SLIDE_2026-06-19.md` "KEYSTONE INSIGHT".** The framing below ("ordinary CTFE" / "executor → CTFE de-magic" / "provider bodies become ordinary effectful CTFE") describes engine **fusion** and is superseded. The settled decision is **stage, don't fuse** (twice-measured): the executor is a **quasiquoter backend** (GenExpr→HIR, not a value-evaluator) run as a **downstream salsa query** producing a real `impl`; it is NOT folded into the CTFE value-evaluator (CTFE-inside-the-solver = Salsa-cycle ICE, a measured dead-end). Near-term the **cascade SELECTS** among existing impls; the keystone later **RUNS** a deriver to **GENERATE** one — distinct steps.

What each NOW-shim becomes:

| NOW shim | HEADED |
|---|---|
| `: Derive for Eq` (string marker, special parser head) | `Derive<Eq>` — real `(*->Constraint)->Constraint` application; marker retired (#7) |
| `Evidence<Eq<T>>` opaque struct, identity-recognized | real `Evidence : Constraint -> *` application over `Eq<T>`'s `TraitInstId` |
| `mut ImplBuilder<Eq<T>>` opaque write cap | typed generated-HIR builder effect (`emit`), `Constraint -> *` |
| `builder.require<Eq>(field.ty())` executor command | `require Eq<field.Ty>` — ordinary obligation onto the solver queue |
| `quote { .. }` template strings via executor | `quote` real effect → hygienic HIR with typed holes |
| `builder.finish()` + frozen ops | gone — body is ordinary effectful CTFE |

### B.3 Use-site — before → after (consumer bounds)

```fe
// NOW: generic consumers spell trait bounds the old way; goals are concrete already.
fn cmp<T>(a: T, b: T) -> bool where T: Eq { a.eq(b) }

// HEADED (PROPOSED): identical surface, but `Eq<T>` is a real concrete constraint
// (`TraitInstId`, Self = first arg, D7) usable directly as `where Eq<T>`:
fn cmp<T>(a: T, b: T) -> bool where Eq<T> { a == b }

// Derive use-sites are UNCHANGED (verdict F: ship as-is). `using` stays; bare = canonical.
derive Eq for Point                  // canonical
derive AbiSize for Word using StableAbiSize   // named, multi-backend
```

---

## C. `fixed` in use (PROPOSED) — the storage money case

`fixed` (decided surface keyword, `FCO_SCOPE_CONTROL_PRECEDENT_2026-06-17.md`) = the non-overridable / consistent-per-deployment tier. Internally `global ≡ canonical`, **never surfaced**. No "witness/uniform/provenance" in devx.

### C.1 WITHOUT `fixed` — the bug (real today: `storage_map.fe:8-11`, `:138`, `:165-172`)

```fe
// ingots/std/src/evm/storage_map.fe:8-11 — StorageKey is an ordinary, user-extensible trait.
pub trait StorageKey {
    fn write_key(ptr: u256, self) -> u256
}

// get() and set() each independently resolve K::write_key (storage_map.fe:165-172 → :138).
// Nothing demands the two resolve the SAME impl:
contract Bank {
    balances: StorageMap<UserId, u256>

    pub fn balance_of(self, id: UserId) -> u256 { self.balances.get(id) }   // resolves StorageKey for UserId  (A)

    pub fn pay(mut self, id: UserId, amt: u256) {
        // a local `impl StorageKey for UserId` in THIS scope shadows (B) ...
        impl StorageKey for UserId { fn write_key(ptr: u256, self) -> u256 { /* different encoding */ } }
        self.balances.set(id, amt)                                          // resolves StorageKey for UserId  (B)
    }
}
// CONSEQUENCE: `pay()` writes slot keccak(encodingB(id) ++ salt); `balance_of` reads
// slot keccak(encodingA(id) ++ salt). Funds credited to a slot that is never read.
// SILENT — both halves type-check. This is the money bug (P-failure, the construct doc §"decisive reframe").
```

### C.2 WITH `fixed` — std seals it; the dev writes nothing; error is plain language

```fe
// PROPOSED. std marks the storage-shaped trait `fixed` ONCE. Everyday contract devs
// write NOTHING — consistency is automatic (devx constraint: invisible-by-default).
fixed trait StorageKey {                 // ← `fixed` = one encoding per deployed contract, non-shadowable
    fn write_key(ptr: u256, self) -> u256
}

// The everyday contract is UNCHANGED and just works — there is no `where`-qualifier,
// no annotation, nothing PL-ish at the use site:
contract Bank {
    balances: StorageMap<UserId, u256>
    pub fn balance_of(self, id: UserId) -> u256 { self.balances.get(id) }
    pub fn pay(mut self, id: UserId, amt: u256) { self.balances.set(id, amt) }
}

// The shadow attempt from C.1 now fails to COMPILE, in plain language (no jargon):
//
//   error: `UserId` is stored two different ways in contract `Bank`
//     a contract must store a type one way everywhere it touches storage
//     --> the standard encoding is used in `balance_of` (reads `balances`)
//     --> a different encoding is introduced in `pay` (line 12)
//     `StorageKey` is fixed: a contract cannot quietly swap how a type is stored.
//     help: remove the local `impl StorageKey for UserId`, or change the encoding
//           for the whole contract (see `fix`, below).
```

Under the hood this is the (kept) Construct-3 mechanism: the dangerous op (`storagemap_slot`) demands the resolved witness be from-at-or-above the contract root and identical everywhere — checked **after** the goal eliminates to a concrete `TraitInstId` (no live `P`). `fixed` is the surface for that; the devx never says any of those words.

---

## D. `fix` sketch (PROPOSED / FUTURE — deferred; "for now `fixed` is great")

`fix` (verb) = a permissioned override of a `fixed` provision **in the context of a held capability**. The `fixed` (adjective, sealed) / `fix` (verb, authorized-override) symmetry. The capability is the gate — a contract that *legitimately* needs a custom `StorageKey` encoding (e.g. a migration shim, or matching an external contract's layout) holds an authority token and uses it.

```fe
// PROPOSED / FUTURE. The capability that authorizes re-fixing a fixed storage encoding.
// (Unforgeable; granted at deploy/module setup; attenuatable downward — ocap.)
capability StorageLayoutAuthority

contract LegacyBridge {
    // This contract must match an already-deployed contract's non-standard key layout.
    balances: StorageMap<UserId, u256>

    // Holding the authority, the contract `fix`es StorageKey for the WHOLE contract.
    // Symmetry: `fixed`(seal) is undone only by `fix`(re-seal at a higher scope) + a capability.
    fix StorageKey for UserId using LegacyEncoding
        with (auth: StorageLayoutAuthority)        // ← the gate: no token, no override
    // ^^^                       ^^^^^^^^^^^^^      one new encoding, applied uniformly:
    //  |                        the replacement   `get` and `set` now BOTH resolve LegacyEncoding,
    //  re-fix at contract scope (non-shadowable)  so the money bug stays impossible.

    pub fn balance_of(self, id: UserId) -> u256 { self.balances.get(id) }   // → LegacyEncoding
    pub fn pay(mut self, id: UserId, amt: u256) { self.balances.set(id, amt) }  // → LegacyEncoding
}

// WITHOUT the capability, the same `fix` is a compile error — plain language:
//
//   error: changing how `UserId` is stored requires authority
//     `StorageKey` is fixed; overriding it for a contract needs a `StorageLayoutAuthority`
//     --> `fix StorageKey for UserId` at line 9 has no `with (.. : StorageLayoutAuthority)`
//     help: this is intentional — storage layout is sealed so it can't be changed by accident.
```

**The symmetry, end to end:**

| keyword | role | who | effect |
|---|---|---|---|
| `fixed` | adjective, **seal** | std author (once) | one encoding per deployed contract; non-shadowable; everyday devs write nothing |
| (nothing) | the default path | contract dev | inherits the `fixed` encoding automatically; local shadow = plain-language compile error (C.2) |
| `fix … with (auth: Cap)` | verb, **authorized re-seal** | a contract holding the capability | replaces the encoding **uniformly for the whole contract** (still non-shadowable below) — never per-method, so the forgotten-override money bug can't recur |

Key property preserved: `fix` re-fixes at a higher scope for the **whole** contract — it never lets `get` and `set` diverge. The capability gates *who* may change the layout; `fixed` guarantees *that it stays consistent* regardless.

---

## Sources

- **NOW (real, quoted; spot-verified 2026-06-17):** `ingots/core_derives/src/lib.fe:26-114`;
  `ingots/std/src/abi.fe:120-165`; `ingots/core/src/derive.fe:21-40`; `ingots/core/src/error.fe:9-12`;
  use sites `crates/fe/tests/fixtures/fe_test/derived_eq_default.fe:7-11`, `derived_decl.fe:16-17`,
  `derived_using_provider.fe:45`, `static_abi_alias.fe:105`; desugar/provenance
  `crates/hir/src/diagnosable.rs:750`, `crates/hir/src/core/lower/derive.rs:10`, `provider_synthesis.rs`;
  shim status `docs/dev/FCO_BRIDGE_BURN_DOWN.md` (#1–#7, TD5).
- **HEADED (PROPOSED):** `/workspace/fe-design-wizard-kinded-derive-verdict-2026-06-15.md` (D7
  projection over `TraitInstId`/`PredicateListId`, no `ConstraintTerm`; abstract head shelved); board #7 + TD5 ladder.
- **`fixed` / `fix` (PROPOSED):** `docs/dev/FCO_SCOPE_CONTROL_PRECEDENT_2026-06-17.md` (keyword decision),
  `docs/dev/FCO_PROVISION_AUTHORITY_CONSTRUCT_2026-06-17.md` (mechanism + jargon-free devx constraint);
  money case `ingots/std/src/evm/storage_map.fe:8-11,138,165-172`.
