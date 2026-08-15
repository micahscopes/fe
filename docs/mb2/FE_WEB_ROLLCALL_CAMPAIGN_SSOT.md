# Fe web and Rollcall campaign: gate-anchored status

Status: authoritative campaign burn-down

Updated: 2026-08-15

Goal spine: write the math, get the kernel, keep the proof.

This is the single current status ledger for the campaign. The larger
`FE_NATIVE_GALLERY_PLAN.md` remains the design record, evidence narrative, and
Definition of done. Do not reconstruct another master checklist from the
session history. Add scope here only when it changes the goal or a named exit
gate.

Legend:

- `[x]` implemented and backed by the cited gate
- `[~]` the current uncommitted or partially verified slice
- `[ ]` required by the Definition of done
- `[!]` an explicit risk or environmental qualification
- `[M]` needs Micah's hardware or product decision

## Campaign gates

- G1, anti-bake: a new consumer works without compiler/runtime demo names.
- G2, semantic exactness: an independent model checks values and behavior, not
  just bytes, hashes, or successful compilation.
- G3, quality: performance and generated-kernel claims use measured receipts.
- G4, no scaffolding: application policy is Fe-authored; JavaScript and Rust
  remain fixed standards adapters, toolchain code, or independent oracles.
- G5, boundary: the exact repository CI command is green:
  `cargo nextest run --release --workspace --all-features --no-fail-fast --locked --exclude fe-language-server --exclude fe-bench`.

## A. Shared precision axis

- [x] Generic `Fixed<L>` integer fixed-point and GPU-faithful projection.
  Gates: `precision_fixed_orbit_gpu_oracle.rs` and
  `precision_fixed_projection_oracle.rs`.
- [x] Loop-form BN254 field arithmetic and Poseidon parity across executable
  backends. Gates: `precision_field_bn254fr_oracle.rs`,
  `loop_form_bn254_poseidon_hash2_matches_circomlib_and_u256_form_on_wasm_at_o0_and_o2`,
  and `poseidon_bn254_loop_native_cranelift_leg_is_honestly_reported`.
- [x] Reusable field arithmetic now has an array-native `FieldWords<L>` core
  and modulus-branded `FieldElement<L, M>` values with `+`, `-`, unary `-`,
  `*`, `square`, `pow5`, and canonical Montgomery conversion. A separate
  `U32EmbeddingModulus<L>` promise prevents direct `u32`/`i32` embedding into
  moduli too small to represent every input. The structural HList API delegates
  to the same CIOS core. The ProbeP51 gate checks both APIs, all four arithmetic
  operations, `pow5`, signed and unsigned roundtrips, and zero canonicalization
  against an independent bigint model; the BN254 gate remains limb-identical
  to both established kernels.
- [!] The reusable `FieldElement` API is executed on Wasm but is not yet a GPU
  application API. The honest SPIR-V leg currently fails closed because the
  call-free shader lowering retains the array-returning private `mul_words`
  call. The proven generated Poseidon/Merkle kernels remain the GPU path.
  Closing this requires aggregate-return inlining or real SPIR-V function-call
  lowering, not consumer-side source expansion.
- [!] Type-level limb expansion is intentionally not the crypto scaling path.
  The loop-form field kernel is the answer unless a new measured gate disproves
  it.

## B. Runtime control and reactive spine

- [x] Typed suspend/resume CPS, exact live frames, generated Wasm re-entry,
  affine pending identity, cancellation, and the fixed browser task executor.
  Gates: `suspension_plan.rs`, `wasm_e2e.rs`, and
  `host-completion.test.mjs`.
- [x] Fe stream policy includes scan, bounded queues, merge, switch/latest,
  replay/share, race, throttle, debounce, and deterministic time. Gates:
  `wasm_e2e.rs`, `host-completion.test.mjs`, and
  `fe-render-runtime.test.mjs`.
- [x] Typed browser sources cover render-surface facts, visibility, animation
  frames, viewport, raw pointer events, Fe-selected capture, wheel, shared
  WebGPU lifecycle, queue-idle completion, and Fe-owned recovery. Gates:
  `native_e2e.rs`, `wasm_e2e.rs`, `bootstrap.test.mjs`, and the focused real
  Chromium SourceInspector/gallery tape.
- [~] Typed `MessagePort<u64>` observation is implemented through the ordinary
  `EventSource` and completion broker. The focused broker suite and
  `fe_message_port_event_source_resumes_from_a_real_port` pass, and the slice is
  landed at `c1817e477`. This item closes when the final G5 run passes.
- [ ] Add fetch as a typed Fe source and consume it from SourceInspector without
  application-specific host policy.
- [ ] Attach opaque ports through Fe-owned spawn/Worker placement, then derive
  rich canonical message payloads from Fe types.
- [ ] Add structured child scopes, admission, supervision, and restart/backoff
  policy. Recursive resumable SCCs must remain explicitly refused until linked
  affine frames are sound.

## C. GPU pass graphs and perturbation

- [x] General typed compute/multipass pass graphs, including a
  compute-to-compute-to-fragment Rollcall graph. Gates:
  `rollcall_pipeline_pass_graph_compiles_with_external_resources_and_private_mem`,
  `known_color_pass_graph_e2e.rs`, and `rollcall_pass_graph_e2e.rs`.
- [x] Perturbational Mandelbrot has a full-GPU reference orbit, Fe-owned deep
  state and navigation, glitch handling, and independent CPU/GPU receipts.
  Gates: `perturbational_mandelbrot_gpu_oracle.rs`,
  `precision_fixed_orbit_gpu_oracle.rs`, and `demo_compile_gate.rs`.
- [ ] Replace modeled shader operation counts with measurements derived from
  the lowered Naga representation, and establish frame/submission budgets.
- [M] Execute the GPU gates on real WebGPU hardware. This host has no
  `/dev/dri`, and `MB2_ALLOW_GPU_SKIP` is not completion evidence. This is the
  campaign's highest-risk external gate.

## D. Geometric algebra compiler and examples

- [x] Reflection-driven sparse GA expression planning, metric selection, CSE,
  and generated incidence evaluation are real Fe/compiler constructions, not
  a demo-name switch. Gates: the QCGA sparse fixtures in `wasm_e2e.rs` and
  `spirv_e2e.rs`.
- [x] QCGA pencil pick, drag, re-solve, and render is end-to-end Fe-owned. Gates:
  the independent tests in `support/qcga_pencil_acceptance.rs`,
  `qcga_pencil_de_oracle.rs`, the QCGA receipt in `demo_compile_gate.rs`, and
  the real pointer tape in `source_inspector.browser.mjs`.
- [x] The duplicate vertex-shaded QCGA application is retired; the shared
  solver/projection code remains as a non-rendering oracle for the canonical DE
  actor. Gate: `qcga_pencil_de_compiles_as_a_fe_owned_iterative_fragment_surface`.
- [ ] Generalize off-diagonal Clifford products beyond the current supported
  vector/scalar contraction and metric paths.
- [ ] Add measured scalar, packed, subgroup, and workgroup schedule selection,
  including shared memory and barrier lowering where WebGPU permits it.

## E. Fe-native web and gallery

- [x] The gallery page, TodoMVC, SourceInspector, and Event Studio are composed
  and controlled by Fe actors. Gates: `page_projection.rs`,
  `todomvc_actor.rs`, `source_inspector_actor.rs`, the precompiler structural
  tests, and the TodoMVC/SourceInspector Chromium tapes.
- [x] Pointer, wheel, pan/zoom/orbit, picking, batching, and recovery policy are
  Fe-owned for the canonical interactive examples. The fixed host observes
  browser standards facts and transports canonical values.
- [x] The canonical gallery rejects authored browser JavaScript and undeclared
  bundle inputs. Gates: `canonical_gallery_rejects_authored_browser_javascript`
  and `canonical_gallery_rejects_forbidden_non_fe_bundle_inputs`.
- [ ] Delete the runtime render manifest. Replace it with compiler-derived typed
  or binary exports for resource, pass, recovery, presentation, and artifact
  location semantics.
- [ ] Contract the fixed JavaScript runtime to standards observation and GPU
  realization, with no application policy or demo vocabulary.
- [ ] Finish migrating, reclassifying, or retiring every legacy showcase.
- [ ] Establish cold, warm, and invalidation compile budgets. The 2026-08-15
  Rollcall evidence refresh took 5m 14s to rebuild a broad release codegen
  chain after a generator-only source edit, which is the current invalidation
  baseline to beat.

## F. Rollcall cryptographic capstone

- [~] Current exact full-workspace release CI baseline. The repo-root Fe test
  boundary passes all 20 workspace inputs after `54e0ff2d5`; the one exact G5
  run is reserved for the final DONE gate.
- [x] Fe Poseidon-Merkle executes on Wasm and native Cranelift and agrees with
  independent/circomlib-derived values. Gates:
  `rollcall_prove_on_wasm_commit_on_evm_and_verify_membership_end_to_end`,
  `rollcall_merkle_root_native_cranelift_leg_is_honestly_reported`, and
  `rollcall_merkle8_root_native_cranelift_leg_is_honestly_reported`.
- [x] EVM registry membership, replay rejection, mutation rejection, and an
  honest depth-20 gas measurement exist. Gates:
  `rollcall_registry_accept_reject_and_claim_at_depth4` and
  `rollcall_registry_gas_at_depth20_is_l2_honest`.
- [x] The same Fe loop-form Merkle body compiles to Naga-valid SPIR-V. Gates:
  `poseidon_merkle_root_loop_compiles_naga_valid_spirv`,
  `poseidon_merkle8_root_loop_compiles_naga_valid_spirv`, and
  `rollcall_merkle_root_spirv_validation_is_honestly_reported`.
- [x] `demos/rollcall/evidence.json` is regenerated from source digest
  `8b8303603bd4937fb883c8d970a2fcd10e551ba8c424873f1e5f04eb15de06db`.
  It records Wasm and native execution with equal 20-limb roots, local EVM
  acceptance and rejection receipts, and SPIR-V validation without claiming
  live GPU execution. Gates: the four tests in
  `rollcall_evidence_verify.rs`.
- [M] Execute the Rollcall pass graph and exact result/pixel oracles on real
  WebGPU hardware. Gate: `rollcall_pass_graph_executes_exact_u32_and_pixel_oracles_on_webgpu`.
- [M] Confirm the on-chain product scope before extending the verifier: target
  chain/L2, gas target, and whether v1 is plain Merkle membership or adds a
  nullifier/ZK claim.

## G. Bounded-proof capstone

- [x] `EscapesByQ12` is specified as a least-terminal-row claim with an exact
  signed Q12 domain, arithmetic-shift rounding, i32 safety argument, and a
  separate future `EntersAttractor` claim. Gate:
  `mandelbrot_bounded_claim_oracle.rs` and
  `MANDELBROT_BOUNDED_PROOF_SPEC.md`.
- [~] Fe now generates nominal `EscapeWitness`, `EscapeTraceRow`, and expanded
  `EscapeAirRow` values. Compiled Wasm matches an independent i64 replay across
  directed, invalid, and 512 deterministic cases. Every signed Q12
  quotient/remainder and directed transition is checked. Fe evaluates five
  widened integer polynomial residuals, emits a canonical 15-word
  sign-plus-magnitude row encoding, and verifies alleged directed row pairs.
  The gate mutates all 11 columns on both sides, the public point, and the
  bound, and rejects residual-zero noncanonical shift decompositions. The
  first proof-field slice now evaluates nine row-local and nine transition
  residuals directly in BN254 Fr. It reconstructs signed values algebraically,
  constrains every supplied sign as a bit, executes as self-contained Fe Wasm
  with zero function imports, and rejects directed one-unit mutations. Fe also
  derives the semantic and next-power-of-two trace lengths, emits active rows
  followed by deterministic terminal-state padding, and marks exactly one
  terminal row. The independent gate checks every directed padded row and
  rejects invalid, non-escaping, and out-of-domain requests. Fe integer and
  BN254 first/pair/last constraints make activity monotone, terminal unique,
  select Mandelbrot transitions only on active nonterminal rows, and make
  padding an exact terminal-state fixed point. The field boundary uses nested
  nominal Fe rows rather than a numeric lane protocol. The gate rejects
  non-bit flags, premature inactivity, nonterminal closure, public-point
  mutation, and mutated padding. A generic low-degree range witness now adds
  boolean bits, exact BN254 reconstruction, and a quadratic prefix OR. Its
  signed form enforces canonical positive zero and the asymmetric i32 minimum.
  The Q12 remainder gate uses 12 bits. Because the escape threshold is exactly
  `2^26`, a five-step high-bit OR now constrains the semantic terminal flag.
  The independent oracle proves that these companion constraints close the
  formerly accepted alternate remainder, negative-zero, and premature-terminal
  counterexamples, including exact threshold boundaries and malformed OR
  witnesses. `RangedAirRow` now wires every trace-row column to a Fe type-level
  width: step 21, coordinates 15, squares 30, magnitude 31, real quotient 18,
  imaginary quotient 19, and remainders 12. One nominal Fe entry checks the
  whole row, and its gate accepts a terminal row while reporting exactly the
  three deliberately malformed columns in a combined adversarial row.
  Public-claim validation/comparison, the domain-separated Poseidon trace
  commitment, composition, and FRI remain pending. The reusable field API's
  SPIR-V helper-call seam and permutation lift also remain open. No trace root
  is claimed yet. This is executable initial AIR evidence rather than a proof.
- [ ] Produce a succinct proof whose verifier is demonstrably cheaper than
  replaying the orbit.
- [ ] Define a typed proof encoding and prove browser/native verifier parity,
  malformed-proof rejection, and mutation rejection.
- [ ] Run proof submission and verification through structured Fe tasks,
  Worker/MessagePort effects, cancellation, and backpressure.

## External real-GPU handoff

Run these from commit `0d7f3eddd` or later on a Linux host whose printed
adapter is a hardware adapter. Do not set `MB2_ALLOW_GPU_SKIP`; a skip is a
failed campaign gate. `WGPU_BACKEND=vulkan` keeps the path browser-profile
compatible. On a non-Linux host, omit that variable and retain every other
condition.

```console
mkdir -p /workspace/tmp /workspace/.sccache
env -u MB2_ALLOW_GPU_SKIP TMPDIR=/workspace/tmp CARGO_INCREMENTAL=0 SCCACHE_DIR=/workspace/.sccache WGPU_BACKEND=vulkan cargo nextest run --release --locked -p fe-codegen --test known_color_pass_graph_e2e
env -u MB2_ALLOW_GPU_SKIP TMPDIR=/workspace/tmp CARGO_INCREMENTAL=0 SCCACHE_DIR=/workspace/.sccache WGPU_BACKEND=vulkan cargo nextest run --release --locked -p fe-codegen --test rollcall_pass_graph_e2e
env -u MB2_ALLOW_GPU_SKIP TMPDIR=/workspace/tmp CARGO_INCREMENTAL=0 SCCACHE_DIR=/workspace/.sccache WGPU_BACKEND=vulkan cargo nextest run --release --locked -p fe-codegen --test precision_fixed_orbit_gpu_oracle
env -u MB2_ALLOW_GPU_SKIP TMPDIR=/workspace/tmp CARGO_INCREMENTAL=0 SCCACHE_DIR=/workspace/.sccache WGPU_BACKEND=vulkan cargo nextest run --release --locked -p fe-codegen --test perturbational_mandelbrot_gpu_oracle
```

Acceptance requires four passing test binaries, no `SKIPPED` line, and a
printed adapter that is not SwiftShader, llvmpipe, or another software
fallback. The semantic receipts are:

- Known Color preserves storage bits `0x3f800000` and `0xc0000000`, then paints
  pixel `[0, 0, 128, 255]`.
- Rollcall preserves leaves `[3, 5, 7, 9, 11, 13, 15, 17]`, produces nodes
  `[896599, 12151, 30583, 185, 377, 569, 761, 8]`, leaves its private-memory
  trap at zero, and paints pixel `[95, 166, 5, 255]`.
- `Fixed<8>` matches every packed and exact audit word for all directed orbit
  checkpoints and emits the explicit invalid-reference sentinel for the
  escaping `(1, 1)` case.
- Perturbation matches the independent BigInt `Fixed<8>` classifier, reports
  no false classifications, resolves the escaping-reference overlap with zero
  magenta pixels, and shows magenta only for the four deliberately ambiguous
  boundary samples.

## Immediate burn-down order

1. Run the external real-GPU handoff above.
2. Start the bounded-proof specification and independent oracle before adding
   more gallery polish.
3. Finish typed fetch, then Worker/port placement and supervision on the one
   runtime-control spine.
4. Delete the runtime manifest and finish the legacy disposition.
5. Run the exact G5 command once at the final DONE gate.

The Definition of done is not yet met. In particular, the real-GPU gate,
manifest deletion, Worker/DEC general messaging, complete legacy disposition,
and bounded proof remain open.
