# Public structural generation release contract

This records the bounded first structural metaprogramming milestone.
The subsequent [reflected-trait extension](REFLECTED_TRAIT_QUERIES.md) adds checked
trait queries and concrete nominal field codecs; its verification is recorded
separately below that contract. The two fixed
applications are `crates/fe/tests/fixtures/structural_generation/abi.fe` and
`accessors.fe`. Their source programs run through ordinary `fe check`,
`fe build`, and `fe test`, without a separate generation command or
application-specific Rust. The legacy scalar-generation experiment is separate.

## Admitted interface

- A record invokes a reusable const provider through
  `#[generate(path)]`. Ordinary resolution and visibility select the provider.
  Its root is a safe nongeneric module function with one `Record` parameter,
  no entry requirements, and no effects. Checked helpers and explicit coherent
  scoped witnesses inside the provider are supported.
- The record's defining module authorizes inspection and construction through
  this invocation. Compiler-issued record, field, and field-type handles retain
  owner identity. Same-shaped Fe structs cannot substitute for those handles.
- Reflection exposes actual field names and types. `type_is<T>` admits
  `bool`, `u64`, and `u256` candidates, including aliases. Other candidates
  diagnose; they do not return false. General reflected equality is excluded.
- Providers return complete `FunctionDefinition` values, bounded arrays of
  optional functions, or `RecordImplementation` values. Implementations carry
  their record handle, a trait path, up to two associated constants, and one
  optional owned-receiver method. Constants can use the same bounded expression
  nodes as generated functions. These values do not certify their own correctness.
- All explicit providers see authored inputs. Their declarations become visible
  together in an isolated output database, before ordinary checking of consumers.
  A provider cannot observe a declaration from its own output phase. Recursion
  and evaluation limits produce errors. Authored files remain unchanged.
- Output passes ordinary type, requirement, ownership, and coherence checks,
  including unused functions and implementation members. Generated function
  name collisions and conflicting implementations diagnose.
- Named source paths explicitly resolve in the owner's output scope. They are
  not captured provider bindings or evidence. This interface does not transport
  a scoped impl selection into a later generated call. Scoped policy choices
  affect the emitted declarations; subsequent calls use ordinary Fe resolution.
- Preparation covers the selected compilation target and its resolved dependency
  closure. Unrelated loaded workspace members are not evaluated.

## Fixed applications and oracles

The ABI provider derives `AbiSize` and `Encode<Sol>` implementations plus offset
accessors. Fe walks the record, computes layout using `abi_record_layout`, and
emits calls to existing codecs on field projections. Rust constructs generic
declaration syntax and checks owner authority; it contains no ABI layout or
encoding policy. `HEAD_SIZE` is emitted as a sum of field-codec calls and checked
as an ordinary associated constant. `IS_DYNAMIC` is a separate constant.

| Record | Fields and values in declaration order | Offsets | Bytes | Exact payload words |
| --- | --- | --- | ---: | --- |
| Empty | none | none | 0 | none |
| Single | u256: 42 | 0 | 32 | 42 |
| Packet | u256: 42, bool: true, u64: 258 | 0, 32, 64 | 96 | 42, 1, 258 |
| Reordered | u64: 258, bool: false, u256: 42 | 0, 32, 64 | 96 | 258, 0, 42 |

Each word is exactly 32 big-endian bytes, including leading zeros. Tests compare
payload length and raw complete words, without a decoder or generated expected
offset. There is no selector or outer tuple offset. The generated encoders are
ordinary runtime methods. All four records are static, so `IS_DYNAMIC` is false.

The accessor provider uses the same handles, type syntax, and function syntax.
Two explicit scoped policy witnesses select all fields or only bool fields.
Generated getters preserve actual field types and can expose private fields
because their owner authorized the invocation. Runtime callers check values;
ordinary checking rejects calls to omitted or wrongly typed accessors.

## Evidence map

All driver test names below are in `tests/elaboration_inputs.rs`.

| Requirement | Evidence |
| --- | --- |
| Normal project and standalone commands, O0/O2 execution, unchanged files | `structural_generation_cli` integration suite in `crates/fe/tests` |
| Actual heterogeneous fields, aliases, distinct nominal types | `record_inputs_preserve_field_types_and_reject_foreign_descriptors`, `reusable_fe_provider_observes_actual_heterogeneous_fields` |
| Visibility and malformed invocations | `malformed_and_inaccessible_invocations_do_not_become_grants` |
| Fabricated handles and unauthorized untaken source | `structural_intrinsics_reject_fabricated_values_and_out_of_bounds_fields`, `untaken_source_cannot_construct_a_reflection_grant` |
| Wrong-owner functions and implementations | `construction_rejects_wrong_owner_and_checks_even_unused_body_nodes`, `generated_implementation_cannot_be_transferred_to_another_owner` |
| Complete constraints even when unused | `generated_calls_check_complete_requirements_and_track_supporting_impl_edits`, `unused_generated_encoder_requires_each_codec_after_supporting_impl_edits` |
| Invalid generated bodies, signatures and impl coherence | `construction_rejects_wrong_owner_and_checks_even_unused_body_nodes`, `generated_implementations_check_trait_contracts_and_coherence` |
| Untaken invalid generator source and unchecked root premises | `project_preparation_rejects_unchecked_entry_premises_and_untaken_invalid_source` |
| Selection-dependent API and provider edits | `scoped_fe_policy_selects_a_checked_accessor_set` |
| Record, field-type and provider edits versus fresh output | `abi_record_field_type_and_provider_edits_match_fresh_generated_output` |
| Supporting codec removal, restoration, and fresh comparison | `unused_generated_encoder_requires_each_codec_after_supporting_impl_edits` |
| Deferred associated-constant expressions and unused-node rejection | `generated_const_expression_keeps_generic_field_sizes`, `generated_const_expression_checks_unused_nodes` |
| Output visibility and name collisions | `project_preparation_admits_all_invocations_before_checking_authored_consumers`, `generation_rejects_output_collisions_and_observation_of_its_own_output` |
| Dependency selection and isolation | `selected_generation_skips_unrelated_ingots_but_includes_dependencies`, `elaboration_input_views_preserve_resolution_and_isolate_real_edits` |
| Recursion, step, declared allocation and emitted-byte limits | `structural_generation_reports_evaluation_exhaustion_and_declared_output_limits`, `generated_declaration_limits_report_named_positions` |
| Named construction limits and authored generation-site positions | `generated_declaration_limits_report_named_positions` and the CLI `invalid_generated_constant_reports_its_generation_site` |
| Fixed 0/1/3/8/32 scaling series | `structural_abi_fixed_scaling_series_and_declared_field_limit`; measurements and limits in `README.md` |

The descriptor types also exercise ordinary nested layouts. The HIR
`layout_evidence` suite checks that finite nested `Option` fields remain finite,
while expanding structural type arguments and nonregular selected-provider
targets are still rejected. The companion `layout_root_graph` suite covers
recursive provider graphs and layout-root identity.

## Exclusions and verification

The ABI application admits up to 32 fields of the three primitive types above.
Dynamic tails, recursive and generic record constructors, automatic provider
discovery, unrestricted evidence transport, general reflected equality, captured
binding quotation, and LSP integration are outside this release contract.
Construction is bounded and does not expose every Fe declaration or expression.
Declaration-name literals and prefixes, named paths, and argument labels use
31-byte inline strings.

Focused verification is not the full release gate. The required workspace test
command is:

```text
cargo nextest run --release --workspace --all-features --no-fail-fast --locked --exclude fe-language-server --exclude fe-bench
```

The release gate passed on September 21, 2026, for compiler checkpoint
`26caf65d65b4e01915956d03349874013ae76404`. The exact command above ran 2,976 tests:
all passed, none skipped, one reported slow. Test execution took 683.322 seconds.
This included all 29 `elaboration_inputs` tests, all three
`structural_generation_cli` tests, and the 58 layout-evidence and 79 layout-root
tests. Both application CLI tests execute project and standalone inputs at
O0 and O2. Their expected values and payload words remain the fixed oracles
described above.

The same source also passed `cargo fmt --all -- --check`,
`python3 newsfragments/validate_files.py`, and the workspace lint command:

```text
cargo clippy --locked --workspace --all-targets --all-features -- -D clippy::all
```

Lint used Rust 1.97.1 with `RUSTFLAGS="-D warnings"`; release tests used the
configured Rust 1.100 nightly. Builds used sccache with incremental compilation
disabled. These results establish the fixed release contract, not general
semantic soundness or support for the excluded capabilities.

Multibackend reconstruction follows this milestone. Generated programs should
eventually exercise shared language semantics across appropriate targets while
preserving target-specific aggregate representation, control flow, memory,
interfaces, resources, and legality. Multibackend parity, a broader mb2 audit,
and universes or (co)inductive/cubical ambitions are not this release's finish line.
