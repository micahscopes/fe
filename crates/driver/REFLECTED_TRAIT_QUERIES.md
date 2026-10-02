# Checked trait use for reflected fields: release contract

This work extends the completed structural release at `966e7167c`. It does not
reopen that release's acceptance criteria. The aim is one checked path from an
actual reflected field type to ordinary Fe trait facts and existing codec calls,
replacing primitive dispatch in the ABI provider.

## Fixed acceptance

The nested types have authored existing `AbiSize` and `Encode<Sol>` impls. Only
their containing records are generated. All records are concrete and static.

| Case | Fields in declaration order | Offsets | Head/payload bytes | Complete 32-byte big-endian words |
| --- | --- | --- | --- | --- |
| Packet | code: u64 = 258; inner: PairAlias = left::Inner { nonce: 42, ready: true }; active: bool = false | 0, 32, 96 | 128 | 258, 42, 1, 0 |
| Other | inner: right::Inner { value: 17 }; code: u64 = 258 | 0, 32 | 64 | 17, 258 |

`PairAlias` aliases `left::Inner`, which has a 64-byte static codec.
`right::Inner` is a distinct same-spelled nominal type with a 32-byte static
codec. Expected words and offsets above are independent constants, not results
captured from the generator or a round-trip decoder. Both report IS_DYNAMIC=false.
The existing empty/single/heterogeneous/reordered primitive ABI cases and both
accessor policies retain their current expected outcomes.

The provider must consume actual HEAD_SIZE and independent IS_DYNAMIC during
ordinary const evaluation, compute layout in Fe, and emit calls using resolved
field types. Rust must not implement ABI policy or enumerate accepted nominal
type names. Removing either required trait implementation must reject an unused
generated encoder. Alias, record, field-type, provider and supporting-impl edits,
including restoration, must agree with fresh compilation. Wrong authority,
unsupported operations, false requirements and exhaustion cannot become evidence.

Both fixed cases must use normal project check/build/test and O0/O2 execution.
Record consequential cost/output changes. Finish focused checks and the same
fmt/news/clippy/exact full nextest gate as the structural release. No push.

## Bounded design probe

The implemented API uses this bounded design: a Fe-authored unary type family
`F` supplies an ordinary safe nullary const method through a designated query trait. Applying
`F` to the actual reflected field type produces a concrete ordinary trait goal.
Prove that goal with existing machinery and execute its method through ordinary
CTFE, retaining evaluation limits. The output type is explicit and does not
depend on exposing the reflected type as a first-class source type.

The executable probe in `tests/reflected_trait_queries.rs` passed both controls:
existing higher-kinded application, ordinary prerequisites and const method
execution compose for `AbiFacts<Inner>` and return its actual 64-byte size;
removing Inner's AbiSize impl rejects the query even when its method returns a
constant and ignores that prerequisite. The focused release test passed on
September 21, 2026.

Core now exposes `FieldQuery<R>::evaluate()` and `query_field<F, R>(FieldType)`.
The operation applies the unary family to the actual field type, proves the
complete ordinary implementation requirements, and evaluates the selected method
in the current CTFE machine. Failure cannot become symbolic successful evidence.
The ABI provider uses separate `Head<T>` and `Dynamic<T>` families, keeping size
and dynamic classification independently demandable. Rust contains no ABI policy.

The selected method must have an explicit implementation body, be safe and const,
accept no arguments or effects, and introduce no method generic parameters or
const predicates. Ordinary trait prerequisites must be proved before execution.
Unresolved types and associated projections are rejected at this boundary. This
is a checked ordinary implementation lookup, not selection from an arbitrary
scoped witness supplied by the caller. Existing scoped-provider APIs retain their
separate contract.

Generated nominal field types retain the original checked spelling in the owner's
module, preserving private aliases and qualified paths. Generic nominal field
constructors remain excluded. Focused tests cover the fixed nested payloads at
O0/O2, missing unused prerequisites, assertion failure and exhaustion. The final
workspace gate passed; see the verification receipt below.

Do not turn this into general evidence packages, new overlap/coherence rules,
unrestricted type quotation, recursive generation or automatic discovery. If
the fixed cases require those decisions, reassess rather than silently extending
the finish line. Preserve selected-impl, scoped-provider and frozen-binding
contracts. Multibackend reconstruction remains a broader followup.

## Semantic regression coverage

`tests/reflected_trait_queries.rs` exercises the following contracts:

- Ordinary family application and full supporting requirements, even when the
  selected method ignores its bound and returns a constant.
- Actual nominal trait facts obtained through a compiler-issued field handle.
- Alias, direct field type, record shape, provider and supporting-constant edits,
  each restored and compared against a fresh database. Both the generated source
  and evaluated size/offset constants must agree.
- Removal and restoration of required `AbiSize` and `Encode<Sol>` impls, with
  no runtime caller to force checking of the generated encoder.
- Independent dynamic classification when evaluating the size would exhaust.
- Assertion failure, unavailable extern results and evaluation exhaustion.
- Repeated individually terminating queries exhausting one shared provider budget.
- Dynamic codec rejection and restoration to a supported static codec.

The provider/supporting-constant edit cases intentionally alter policy facts;
they verify dependency invalidation, not runtime correctness of those altered
codec layouts. Runtime correctness is anchored separately by the fixed payloads
in `structural_generation_cli` at both optimization levels.

Existing structural authority tests continue to cover forged handles, owner
mismatches, inaccessible seals in untaken source, output collisions, and ordinary
checking of unused generated bodies. Query selection uses the actual core trait
identity, complete existing requirement proofs and the selected method service.
It does not export an implementation-selection token or grant access to nested
record fields.

## Cost observations

The fresh-database primitive scaling probe still emits 270, 552, 1121, 2566 and
9548 generated bytes at 0, 1, 3, 8 and 32 fields respectively. These match the
previous structural milestone exactly. One focused run measured 2948, 1879,
1906, 1680 and 1762 milliseconds respectively, including ordinary output checking.
These are individual contended wall-clock observations, not an isolated benchmark
or evidence of a speed improvement. The provider now performs checked trait
selection instead of primitive classification. Caching and query costs deserve
measurement with stable conditions before making performance claims.

## Completion evidence map

| Goal requirement | Authoritative evidence |
| --- | --- |
| Probe before API choice | `4d7ae3e7f`; first two ordinary-family tests in `tests/reflected_trait_queries.rs` |
| One Fe policy for primitive and nominal fields | `authors` module in `crates/fe/tests/fixtures/structural_generation/abi.fe`, prepended unchanged to `nested_abi.fe` by the acceptance harness |
| Actual size and independent dynamic facts at provider time | `Head<T>` and `Dynamic<T>` query families; independent-demand and exhaustion tests |
| Existing requirement and selection machinery | `eval_field_query` in HIR CTFE calls `resolve_trait_impl_instance`, `prove_selected_impl_evidence` and `nullary_const_method`; no new solver |
| Same reflected nominal type reaches codec calls | Checked `DeclarationType::Field` rendering in the owner's module; distinct same-spelled nominal and private-alias runtime cases |
| Independent offsets, length and raw bytes | Fixed oracle table above and explicit `word_at` assertions in `nested_abi.fe` |
| Normal project and standalone commands, O0/O2 | `nested_abi_preserves_aliases_and_distinct_nominal_codecs` in `structural_generation_cli.rs`; helper asserts executed test names, counts and build artifact |
| Missing unused requirements | `unused_nested_encoder_requires_supporting_impls_after_edits_and_restoration` |
| Record/type/alias/provider/supporting-impl edits | `nested_edits_and_restoration_match_fresh_facts_and_generated_calls`; warm/fresh generated-source equality and evaluated constants |
| Unsupported, false and exhausted outcomes reject | Query assertion, extern, dynamic-codec and shared-budget regression tests |
| Original primitive/accessor outcomes | Existing CLI fixtures and all 29 `elaboration_inputs` tests; original oracles unchanged |
| No authority or binding unification | Existing structural authority checks retained; query API returns a value, not an exported selected-impl witness; scoped/frozen APIs unchanged |
| Costs and explicit scope | Cost observations and exclusions in this contract; multibackend remains a followup |

## Verification receipt

The implementation and regression checkpoint `d21b2d368` passed the exact full
workspace gate on September 21, 2026:

```text
cargo nextest run --release --workspace --all-features --no-fail-fast --locked --exclude fe-language-server --exclude fe-bench
```

All 2,987 tests passed, none skipped, five reported slow. Test execution took
873.202 seconds; the release build took 10 minutes 13 seconds. This includes all
ten reflected-query tests, all 29 structural-input tests and all four structural
CLI tests. The latter execute the fixed nested, primitive and accessor programs
through project and standalone check/build/test, with O0/O2 execution.

The same code passed all of:

```text
cargo fmt --all -- --check
python3 newsfragments/validate_files.py
cargo clippy --locked --workspace --all-targets --all-features -- -D clippy::all
```

Lint used Rust 1.97.1 with `RUSTFLAGS="-D warnings"`; release tests used the
configured Rust 1.100 nightly. Builds used sccache, `CARGO_INCREMENTAL=0`, two
Cargo build jobs, and disk-backed build/cache/temp directories under `/workspace`.
The only subsequent changes document acceptance and these results.

This closes the fixed reflected-trait milestone. It establishes the admitted
contracts and regression outcomes above, not a general semantic soundness proof
or support for the excluded capabilities. Multibackend reconstruction remains
an explicit followup. The work is committed locally; no push was performed.
