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
  `fe_message_port_event_source_resumes_from_a_real_port` pass. This item closes
  when the current checkpoint passes G5 and lands.
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

## F. Rollcall cryptographic capstone

- [~] Current exact full-workspace release CI baseline. The present checkpoint
  must pass G5 before any further capstone claim is promoted.
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
- [ ] Regenerate `demos/rollcall/evidence.json`. Its native and WebGPU entries
  are stale and must report the current four legs honestly from one source
  digest.
- [M] Execute the Rollcall pass graph and exact result/pixel oracles on real
  WebGPU hardware. Gate: `rollcall_pass_graph_executes_exact_u32_and_pixel_oracles_on_webgpu`.
- [M] Confirm the on-chain product scope before extending the verifier: target
  chain/L2, gas target, and whether v1 is plain Merkle membership or adds a
  nullifier/ZK claim.

## G. Bounded-proof capstone

- [ ] Specify the exact bounded claim, separating `EscapesBy<N>` from any
  attractor or convergence claim.
- [ ] Generate a Fe witness and commit to its trace.
- [ ] Produce a succinct proof whose verifier is demonstrably cheaper than
  replaying the orbit.
- [ ] Define a typed proof encoding and prove browser/native verifier parity,
  malformed-proof rejection, and mutation rejection.
- [ ] Run proof submission and verification through structured Fe tasks,
  Worker/MessagePort effects, cancellation, and backpressure.

## Immediate burn-down order

1. Land the typed MessagePort checkpoint after G5.
2. Regenerate and verify the Rollcall four-leg evidence, with WebGPU marked
   honestly if hardware remains unavailable.
3. Hand the exact real-GPU commands and expected receipts to Micah.
4. Finish typed fetch, then Worker/port placement and supervision on the one
   runtime-control spine.
5. Start the bounded-proof specification and oracle before adding more gallery
   polish.

The Definition of done is not yet met. In particular, the real-GPU gate,
manifest deletion, Worker/DEC general messaging, complete legacy disposition,
and bounded proof remain open.
