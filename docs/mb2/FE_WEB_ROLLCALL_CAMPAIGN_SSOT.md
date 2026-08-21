# Fe web and Rollcall campaign: gate-anchored status

Status: authoritative campaign burn-down

Updated: 2026-08-21

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
- [x] A non-destructive `Select` loser can now be consumed explicitly through
  the backend-generic affine `PendingCancellation<B>` effect. The fixed broker
  aborts active browser work, consumes already-settled unclaimed tokens, and
  rejects stale or claimed tokens. The heterogeneous select capstone preserves
  its sink loser across two source wins, then Fe cancels it exactly once and
  leaves no broker slot. Gates:
  `fe_select_preserves_a_heterogeneous_loser_across_repeated_source_wins` and
  `typed pending cancellation consumes active and already-settled losers`.
- [x] Generated WebIDL scalar `Promise<T>` operations now emit
  `Pending<WasmBackend, T>` and execute on the same completion and continuation
  rail. The transport derives its success width from the canonical codec plan,
  adds no operation-specific JavaScript schema, preserves byte-identical sync
  binders, and does not require an allocator for scalar-only worlds. The full
  gate compiles the generated Fe declaration, retains the generated Wasm
  import, settles through the fixed broker, resumes the compiler-derived Fe
  continuation, checks the semantic value `42`, and observes no token leak.
  Gates: `scalar_promises_use_generated_pending_and_the_completion_rail`,
  `generated_scalar_promise_transport_executes_its_semantic_completion`, and
  `generated_webidl_scalar_promise_resumes_a_real_fe_task`.
- [~] Direct rich generated Promise results now use deferred canonical
  lowering. The broker retains the standards value opaquely across nested
  races and selects, allocates only after the final Fe continuation wins
  custody, invokes that exact continuation synchronously, and runs its
  generated post-return cleanup on success or trap. The compiler now wires
  Sonatina's checked LIFO `cabi_realloc` and operation-specific post-return
  exports instead of inventing a JavaScript allocator. A real generated
  `Promise<USVString>` reaches Fe UTF-8 logic and returns the allocator to its
  exact baseline. Settled losers remain allocation-free, and materialization
  fails closed when a direct or record-nested borrowed host descriptor would
  survive another suspension without first being copied into Fe-owned
  storage. Indirect canonical results remain blocked. Gates:
  `generated_webidl_string_promise_releases_after_the_real_fe_continuation`,
  `browser_task_rejects_borrowed_host_storage_across_a_second_suspension`, and
  the generated-result cases in `host-completion.test.mjs`.
- [x] Typed browser sources cover render-surface facts, visibility, animation
  frames, viewport, raw pointer events, Fe-selected capture, wheel, shared
  WebGPU lifecycle, queue-idle completion, and Fe-owned recovery. Gates:
  `native_e2e.rs`, `wasm_e2e.rs`, `bootstrap.test.mjs`, and the focused real
  Chromium SourceInspector/gallery tape.
- [~] Typed `MessagePort<u64>` observation is implemented through the ordinary
  `EventSource` and completion broker. The focused broker suite and
  `fe_message_port_event_source_resumes_from_a_real_port` pass, and the slice is
  landed at `c1817e477`. This item closes when the final G5 run passes.
- [x] Fetch is a generated typed Fe source consumed by SourceInspector without
  application-specific host policy. The generated
  boundary now models `[Global=Window]` without minting a fake Window handle,
  lowers URL-only `fetch`, `Response.text`, and `Response.arrayBuffer` through
  the existing `Pending<WasmBackend, T>` continuation rail, maps byte results
  to canonical `BrowserBytes`, and emits explicit generation-safe disposal for
  owned Response resources. The precompiler derives the selected adapter from
  actual imports and publishes one self-contained, content-addressed module
  containing the fixed runtime, fixed codec, generated semantic adapter, and
  generated transport. The bootstrap attaches it before import preflight, with
  no runtime adapter-selection JSON or caller-supplied environment object.
  Direct borrowed guest arguments and scalar or resource results no longer
  allocate codec scratch memory. Generated completion conversion now returns
  an ownership receipt: Response handles become Fe-owned only after the exact
  continuation returns successfully, while losing races, lowering failures,
  cancellation, and continuation traps roll them back. SourceInspector now
  runs one actor-scoped Fe resource loop. A whole-state compiler-derived task
  input gives it only its own Fe state, while a zero-payload notification edge
  reports that a new command is available. Fe owns request revisions,
  text/binary selection, switch-latest cancellation, HTTP classification,
  response limits, stale suppression, presentation, content copying, and
  explicit Response release. JavaScript owns only opaque Promise/resource
  custody, cancellation mechanics, continuation invocation, and generated
  canonical conversion. Component opcodes 12/13, `ComponentWriter.load_text`,
  `ComponentWriter.load_bytes`, `_loadResource`, and `_deliverResource` are
  deleted. Dependency-bearing resident components compile through their real
  initialized ingot, so `browser_fetch` is part of SourceInspector's compiler
  and watch inventory without adding a manifest. Gates:
  `global_fetch_uses_standards_authority_and_owned_response_resources`, the
  59-test `fe-webidl-bindgen` suite plus its four provenance tests, the
  13-test `fe-host-wasm-codec` suite,
  the 12-test host-runtime suite, the 36-test completion suite, the 16-test
  bootstrap suite, the eight Bun codec cases, the resident-actor contract
  suite, `source_inspector_actor.rs`, the complete 34-test HTML precompile
  suite, an immutable 3-module/12-render/80-asset gallery publication, and the
  real remote-Chrome SourceInspector/gallery tape. Switch-latest suppresses
  stale Fe delivery and rolls back unclaimed authority, but does not yet abort
  the browser's underlying request. Actual request abort waits for the general
  WebIDL/host-ABI representation of borrowed resources nested in
  `RequestInit.signal`; it must not be smuggled into a handwritten fetch case.
  This gate admits no permanent handwritten `fe:web-fetch` import whose
  JavaScript mirrors Fe field order or success-lane widths.
- [ ] Attach opaque ports through Fe-owned spawn/Worker placement, then derive
  rich canonical message payloads from Fe types.
- [~] Add structured child scopes, admission, supervision, and restart/backoff
  policy. The fixed Module Worker runtime no longer owns clocks, retry windows,
  backoff, or automatic recovery. Ordinary `std::runtime` Fe types and the
  pure `RestartWindow` reducer now own epoch advancement, rolling-window
  admission, restart/backoff selection, exhaustion, and parent cancellation.
  `ChildPlacement<B, C>` plus `supervise_child` now run the complete owning scope
  through the ordinary `Pending`/`Suspend`/`Timer` rail. Worker readiness now
  races the typed spawn pending value against an ordinary Fe timer through
  `Select`; Fe consumes the losing affine operation through
  `PendingCancellation`, classifies startup timeout, and spends the same typed
  restart budget as an immediate spawn failure. Independent Bun mechanics,
  compiled-Fe Wasmtime reducer, and compiled-Fe/Bun structured-scope gates
  pass.
  The compiler now recovers the nominal child type `C` from the parent runtime
  package, resolves the actor whose state has that type, selects its `Worker`
  behaviors, and compiles a distinct zero-import canonical child Wasm artifact.
  The precompiler publishes that child, its generated interface, and the fixed
  actor runtime beside the parent continuation package. Production bootstrap
  installs the capability only when the parent imports `fe:worker-scope`.
  There is no authored child ID, child URL, child manifest, behavior-name table,
  or application routing JavaScript. The reproducible Chromium gate
  `structured_worker.browser.mjs` loads the immutable parent, child, generated
  interface, and Module Worker host over HTTP without errors.
  DEC now uses this same path from a render actor. `DecSurface` owns two typed
  `ScopedTask` behaviors: one runs Fe supervision policy for the nominal
  `DecOperator` child, and one sends a typed `Cochain0 -> Cochain1` mailbox
  request and computes the response receipt in Fe. Render-bundle compilation
  derives those tasks only from the selected GPU actor, compiles the six Worker
  behaviors into a separate zero-import child, and emits one manifest-free
  task package. The fixed render runtime inspects only standard Wasm imports,
  supplies the generated Worker scope/mailbox capability, starts task machines
  from their compiler-derived input widths, and binds cancellation to the
  surface lifetime. Direct workspace builds now resolve local `core` and `std`
  as one nominal graph, eliminating the split `Handles` identity that had
  invited handwritten relation boilerplate. Gates: the four-test DEC operator
  suite; `direct_workspace_member_uses_workspace_core_and_std_as_one_graph`;
  the HTML publication, cache, and deployment-verifier tests; a real one-tile
  `fe web precompile`/`fe web verify` run with 18 files; and a Bun execution of
  those exact published parent, child, interface, mailbox, and completion
  artifacts whose Fe receipt was `1` with zero leaked tokens. Landed at
  `7d881bcc6`, `8817e726e`, and `838d1c01b`.
  Multiple nominal sibling scopes are now landed at `db088b0ea`. The Fe raw
  boundary retains `C` on spawn, failure, and close; MIR derives three opaque
  lifecycle import identities from each semantic child type; resident and
  render artifacts carry a deterministic collection of compiled children; and
  generated packages publish each child under its derived key without an actor
  name, selector, or manifest. The fixed host accepts the resulting typed
  capability collection but keeps every child on one affine completion-token
  rail. The semantic two-child gate performs requests through both child Wasm
  programs, proves `7 -> 14 -> 19 -> 26`, cancels both Fe supervisors, observes
  the correct close for each nominal scope, and finishes with zero live tokens.
  The 44-test fixed browser-runtime suite, ten-test resident-actor suite,
  four-test DEC oracle, and focused HTML publication gate pass.
  Recursively scalar canonical variants now cross those type-derived child
  mailboxes without a JavaScript operation schema. Canonical memory remains a
  tagged union, while the Fe Wasm value ABI remains the declaration-order tag
  plus every case payload lane. One compiler-owned wrapper validates each
  active tag, loads only active request fields, supplies exact zero values for
  inactive Fe lanes, clears reusable response storage, and stores only the
  active result. The generated parent mailbox codec derives the same lane order
  from the child interface and rejects noncanonical inactive values. The real
  two-child gate now sends nested `ScaleCommand` and `ScaleMode` variants,
  receives a `ScaleResult` variant, preserves the semantic
  `7 -> 14 -> 19 -> 26` result, proves inactive response-union bytes were
  scrubbed after arena reuse, and still finishes with zero live tokens.
  Variants containing bytes, strings, or lists remain fail-closed pending the
  canonical post-return memory bridge. The 16-test canonical-interface suite,
  ten-test resident-actor suite, four-test DEC suite, 53-test fixed browser and
  codec suite, and focused scoped-task publication verifier pass. The canonical
  interface guide now documents nominal role selection, typed mailbox edges,
  private generated transport names, fixed resident exports, the remaining
  spelling-based render compatibility paths, and the possible future omission
  of semantically unnecessary behavior names.
  Remaining: compose recursive nested scopes, attach opaque ports, derive rich
  descriptor-bearing canonical message values, and run the render-owned DEC
  task path in real Chromium. Recursive resumable SCCs must remain explicitly
  refused until linked affine frames are sound.

## C. GPU pass graphs and perturbation

- [x] General typed compute/multipass pass graphs, including a
  compute-to-compute-to-fragment Rollcall graph. Gates:
  `rollcall_pipeline_pass_graph_compiles_with_external_resources_and_private_mem`,
  `known_color_pass_graph_e2e.rs`, and `rollcall_pass_graph_e2e.rs`.
- [x] Perturbational Mandelbrot has a full-GPU reference orbit, Fe-owned deep
  state and navigation, glitch handling, and independent CPU/GPU receipts.
  Gates: `perturbational_mandelbrot_gpu_oracle.rs`,
  `precision_fixed_orbit_gpu_oracle.rs`, and `demo_compile_gate.rs`.
- [x] All four named GPU substrate gates execute locally without a skip on the
  llvmpipe Vulkan CPU driver. Known Color, Rollcall, the independent bigint
  `Fixed<8>` orbit oracle, and the independent perturbation classifier all
  pass. This proves actual shader dispatch and readback on a software Vulkan
  implementation, not merely SPIR-V or WGSL validation.
- [ ] Replace modeled shader operation counts with measurements derived from
  the lowered Naga representation, and establish frame/submission budgets.
- [M] Execute the same GPU gates on real WebGPU hardware. This host still has
  no `/dev/dri`; llvmpipe is execution evidence but not hardware performance
  or driver-diversity evidence. `MB2_ALLOW_GPU_SKIP` remains forbidden at this
  gate.

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
- [x] The duplicate Trunk gallery lane is retired. `demos/gallery.html` is the
  sole gallery source; `demos/gallery/`, its `copy-dir` declaration, and the
  legacy landing link are gone. Gate:
  `repository_has_one_canonical_gallery_source`.
- [~] The current compiler worktree again compiles all 12 canonical render
  actors in one sweep, plus all three resident actors and the Fe page
  projection. The perturbation regression was a redundant fresh-record
  materialize/load round trip inside `build_reference`; a representation-
  checked RMIR identity fold removes it, with its own focused semantic gate.
  This slice closes when it is landed and the browser/runtime gates below are
  repeated.
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
- [x] The field-agnostic S0 browser engine exists as `fe-revm-browser`. It is a
  generic persistent revm session that accepts only raw Fe EVM runtime bytes
  and raw calldata, with no ABI, proof, packing, or application logic in Rust
  or JavaScript. The existing native Rollcall test derives and byte-pins the
  depth-4 runtime plus commit/accept/reject calldata. The `wasm32-unknown-unknown`
  engine executes those vectors and returns the exact native-derived true and
  false ABI words. The same test now executes in actual Chrome for Testing
  152.0.7977.42 through its matching ChromeDriver. Gate:
  `wasm-pack test --chrome --headless --chromedriver <driver> --release`
  `crates/revm-browser --features browser-tests` with
  `WASM_BINDGEN_TEST_WEBDRIVER_JSON` selecting the browser binary;
  `fe_rollcall_verifier_matches_native_accept_and_reject_vectors` passes. This
  is same-source cross-target parity. The independently derived
  Rollcall/Poseidon gates remain semantic truth.
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
- [x] Canonical Poseidon BN254 Fr t=3 parameters now derive inside Fe from the
  typed Grain seed, LFSR, self-shrinking rule, rejection sampling, Cauchy MDS
  construction, and batch inversion. There is no generated parameter table.
  The opt-in const budget remains compiler-capped, while ordinary consts retain
  the one-million-step default. Gate: `poseidon_bn254_derived_oracle.rs`
  independently reproduces the plain-field constants, exhaustively checks all
  262,144 self-shrinker inputs, checks all 4,080 derived Montgomery words, and
  executes the concise Fe permutation against canonical and independent bigint
  hash vectors in zero-import Wasm. This closes parameter provenance and the
  Wasm permutation lift. The first bounded trace commitment is recorded below.
- [x] Allocation-heavy proof code now has explicit, checked Wasm arena scopes
  rather than a larger memory ceiling or an application reset convention.
  Sonatina commit `c0252605862035f812ce0ef2cd0e4c82d2261d51` adds typed
  `mem.checkpoint` and `mem.rewind` IR operations; rewind traps unless its token
  lies between the arena base and current cursor. Fe derives scopes through a
  fail-closed interprocedural escape proof. Whole memory-lowerable aggregate
  references may cross proved-safe Fe calls as borrows, while host/effect/GPU
  calls, providers, raw addresses, transport returns, transport stores, and
  nonlocal address formation remain ineligible. The real four-row statement
  call still matches the independent orbit, encoding, Poseidon, Merkle, and
  mutation oracle, then leaves the next canonical allocation at exactly byte
  1024. The canonical-arena suite separately proves that returned browser
  allocations remain live. Gates: `mandelbrot_trace_commitment_oracle.rs`,
  `wasm_canonical_arena.rs`, and
  `local_u32_array_runtime_index_runs_on_wasm_and_traps_out_of_bounds`.
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
  A separate cheap Fe verifier boundary validates the canonical public point
  and bound, terminal step, `trace_length = terminal_step + 1`, and exact
  next-power-of-two domain without replaying the orbit. Its gate rejects each
  public/shape mutation independently. A first real commitment slice now packs
  the 17 range-constrained columns into one injective 210-bit field encoding,
  derives typed `"MR01"` row and `"MN01"` node domains in Fe, and computes a
  Poseidon Merkle root in zero-import Wasm. The tree shape is no longer
  hand-authored: a 22-slot Fe frontier folds any CTFE-proved nonzero
  power-of-two domain through `2^21` while retaining O(log N) digests. Four-row
  compatibility and an eight-row domain both execute. The canonical transition
  and encoded-row schema now live in one reusable Fe kernel ingot consumed by
  both the AIR and commitment layers. A stateful Fe cursor retains the current
  expanded AIR row and advances from its exact Q12 quotients. The production
  commitment consumes that cursor once, folds each active row immediately,
  derives terminal-state padding in Fe, and accepts no host-authored witness
  rows. Its gate covers 4-row, 8-row, and 16-row domains, invalid and
  non-escaping rejection, and exact arena reclamation to byte 1024. The
  independent oracle reconstructs the orbit, encoding, canonical permutation,
  and trees, mutates every four-row column, proves row-order sensitivity, and
  binds both inactive padding positions in a six-active-row/eight-row trace. A
  distinct typed `"MT01"` domain binds each root to an injective 114-bit
  encoding of the public point, bound, terminal step, semantic length, and
  padded length. The same independent gate mutates every public field. The
  low-degree range checks consume bit-decomposition and prefix-OR auxiliary
  columns. The kernel now derives all ten decompositions, their inclusive
  prefix ORs, and the five-bit terminal-threshold prefix in Fe. Their canonical
  411-bit row encoding is split into two injective 253-bit BN254 elements,
  committed under typed `"AR01"` leaves and `"AN01"` nodes, and folded beside
  the main trace in the same pass. Typed `"AT01"` binds the public main-trace
  statement to the auxiliary root before `"MC01"` derives the composition
  challenge. The independent oracle reconstructs every auxiliary bit, packing,
  both trees, the ordered transcript, and the challenge for 4-row, 8-row, and
  16-row domains. Invalid and non-escaping claims fail closed, no host-authored
  range witness enters the production API, and the arena returns to byte 1024.
  The shared Fe field substrate now derives BN254 Fr's maximal two-adic root
  from the prime and generator 5 at compile time, converts it to Montgomery
  form without a generated table, and supplies generic field exponentiation,
  fail-closed Fermat inversion, and roots through order `2^28`. An independent
  bigint Wasm gate checks ordinary powers, the full-width `p - 2` inverse,
  exact root orders, and unsupported-order rejection while retaining the
  existing BN254 and second-modulus gates. One generic Fe Cooley-Tukey
  transform now provides forward NTT and inverse interpolation with no root
  table or size-specialized butterfly schedule. Const predicates reject zero,
  non-power-of-two, and field-unsupported domains. Independent direct bigint
  DFTs and round trips gate the same generic implementation at 4, 8, and 16
  points. Generic Fe coset low-degree extension now interpolates the base trace,
  shifts its coefficients, zero-pads, and evaluates on the larger subgroup. A
  typed validity bit rejects zero shifts and shifts inside the output subgroup,
  preventing the evaluation coset from intersecting the trace zerofier's
  roots. Independent direct
  bigint interpolation/evaluation gates 4-to-16 and 8-to-16 extensions and
  fail-closed invalid cosets. The first genuine composition layer is now
  Fe-authored end to end. It derives the exact four-row canonical trace in Fe,
  evaluates all 17 main and 411 auxiliary column polynomials on a disjoint
  16-point coset, and folds 708 constraints with consecutive powers of the
  post-auxiliary `"MC01"` challenge. All-row, pair-row, first-row, and last-row
  families retain their distinct zerofiers. The fold streams scalar column
  evaluations from compact semantic rows, avoiding both a 428-by-16 field
  matrix and Wasm's current flattened-aggregate parameter limit. There is no
  host witness or generated column table. Typed `"CR01"` leaves and `"CN01"`
  nodes commit the composition evaluations, `"CT01"` binds that root to the
  ordered proof transcript, and `"FC01"` derives the first FRI fold challenge.
  `mandelbrot_composition_oracle.rs` independently reconstructs the integer
  trace and every auxiliary bit, uses a direct bigint inverse DFT rather than
  Fe's radix-2 schedule, evaluates every constraint and zerofier at all 16
  points, and independently derives the main, auxiliary, composition, and
  transcript Poseidon trees. Changed challenges, changed claims, invalid
  claims, non-escaping claims, and out-of-domain evaluations are covered. The
  zero-import composition boundary agrees through the `"FC01"` field value.
  The complete FRI fold chain then pairs evaluations at `x` and `-x` through the
  16-to-8-to-4-to-2-to-1 domains. One const-generic Fe fold implements every
  round, while a Fe const function derives the `"FR01"` through `"FR04"`,
  `"FN01"` through `"FN04"`, `"FT01"` through `"FT04"`, and `"FC02"` through
  `"FC04"` Poseidon domains from the round index. There is no copied round
  table or host-derived protocol constant. The independent bigint gate checks
  every folded value both from the pair formula and from separately
  interpolated even/odd coefficients, then reconstructs every root,
  transcript, next challenge, and fail-closed invalid result. Typed `"FQ01"`
  now derives one query index only after `"FT04"`. Compact pair paths
  authenticate the selected composition values and each corresponding FRI
  pair through depth 3, 2, 1, and the explicit two-leaf base case. A pure
  reusable `poseidon_merkle` ingot adapts zk-kit binary-path semantics to
  field values and typed Fe capacity domains. The Fe verifier rebuilds the
  public transcript, checks every path and index, and checks all four folds.
  The independent bigint oracle separately derives the query, every opened
  evaluation, every sibling, and every root. The composition opening is now
  reconnected to authenticated main and auxiliary AIR columns. The prover
  commits all 16 field-valued rows under typed `"MR02"`/`"MN02"` and
  `"AR02"`/`"AN02"` domains, binds both roots through `"AT02"`, and opens the
  four rows needed for the queried current/next pair. The Fe verifier rebuilds
  both quartet paths, the ordered transcript and composition challenge, then
  recomputes the alleged composition evaluations through the same generic
  `AirColumnView` constraint interpreter. The independent bigint gate derives
  every LDE field, leaf, sibling, root, challenge, and recomputed numerator; it
  passes against zero-import Wasm. A separate typed-verifier gate mutates an
  opened row, quartet path, AIR roots, FRI values and paths, query index, and
  public metadata; every mutation is rejected. Reaching those gates exposed
  and fixed a general Wasm lowering bug where forwarding an `AddrOf` borrow
  deep-copied its pointee and detached mutable writes from caller storage.
  Separate execution gates now prove mutable-borrow identity and preserve
  ordinary aggregate deep-copy semantics. A reusable `canonical_words` ingot
  now derives exact word counts and ordered codecs from nominal Fe records via
  reflection and FCO. Its browser writer and reader interpret those same typed
  codecs over bounded linear memory without a JSON schema. Signed `i32` words
  use an exact Fe bitcast, field decoding rejects limbs outside the 13-bit
  radix and values greater than or equal to the modulus, bool decoding rejects
  non-bit tags, and const-generic arrays include the zero-length case without
  transport lanes. The outer `"MBP1"` versioned receipt derives its complete
  35,503-word layout from the public claim and nested authenticated AIR/FRI
  query types. One zero-import Wasm gate generates the receipt in Fe, copies it
  across the generic canonical host ownership/reset boundary, verifies it in
  Fe, and checks every emitted word against the independent bigint model. It
  rejects truncated, trailing, and misaligned byte lengths, changed protocol
  fields and public inputs, invalid bool tags, oversized limbs, the field
  modulus itself, changed roots, and invalid proof requests. The accepted gate
  took 1,779.46 seconds and peaked near 10.2 GB RSS in this sandbox, exposing a
  real generated-function lowering cost that must not become the ordinary
  edit loop. Verifier-cost evidence and succinctness remain pending. The
  reusable field API's SPIR-V helper-call seam remains open as a compiler
  issue, but BN254 will not be ported into the proof WGSL path. This is a
  complete authenticated toy receipt boundary, not yet a succinct proof.
- [ ] Produce a succinct proof whose verifier is demonstrably cheaper than
  replaying the orbit.
- [x] Finish one complete BN254 toy proof and verifier accept/reject boundary.
  The Fe-derived canonical receipt and independent whole-word/mutation oracle
  described above are the acceptance gate.
- [~] Retarget the protocol to BabyBear before any prover GPU work. BN254 Fr
  must not be ported to WGSL. The BabyBear gate requires extension-field
  challenges, new injective packing, Fe-derived field-specific Poseidon, and
  new independent vectors. Same-source Fe Wasm/native agreement is parity;
  the separately implemented bigint model remains the semantic oracle. The
  u32-native base-field slice is now real: generic `WordField<M>` derives
  one-word Montgomery arithmetic from a prime parameter block using portable
  16-bit partial products, and the BabyBear instance derives its inverse,
  `R^2`, and maximal two-adic root in Fe. The independent u64 gate covers
  arithmetic, inverses, powers, and all subgroup orders; the same multiply
  lowers to Naga-validated u32-only SPIR-V/WGSL. Generic
  `BinomialExtension4<M>` now supplies the canonical BabyBear challenge field
  `F[X]/(X^4 - 11)` without a BabyBear-specific arithmetic body. Its
  zero-import Wasm gate checks addition, schoolbook multiplication, powers,
  and the tower inverse against an independently implemented u64/BigUint
  polynomial model. Every output coefficient of the same Fe multiplication
  lowers to branch-free, u32-only, Naga-valid WGSL. This slice also fixes the
  shared semantic place model so literal array indices remain
  `Index(Constant)`: distinct affine element moves are now accepted while a
  repeated move of the same element still fails. Gates:
  `precision_baby_bear_oracle.rs`, `semantic_borrowck.rs`, and the three
  `arr_const_index_*` UI fixtures. The canonical Plonky3 width-16 Poseidon2
  permutation is now derived in Fe from the shared Grain construction, with
  no authored round-constant table. Its zero-import Wasm gate checks all 141
  constants and five complete permutations against Plonky3. A reusable
  `word_field_encoding` interpreter injectively packs exact-length bounded bit
  strings into 30-bit field payloads, rejects undersized moduli, invalid
  widths, range violations, and output overflow. A security review caught and
  removed the initial one-field, 31-bit digest checkpoint before composition
  or FRI could depend on it. Production commitments now use Plonky3's
  width-16 shape: a rate-8, capacity-8 fixed-length sponge absorbs the nominal
  domain, exact bit length, and payload, then returns an eight-field digest.
  Merkle nodes concatenate two eight-field children into one permutation,
  matching `TruncatedPermutation<_, 2, 8, 16>` and giving roughly 124-bit
  collision security rather than the toy checkpoint's 31 bits. The
  independent bigint/Plonky3 gate covers 128
  randomized schemas, every bit of its directed mutation case, trailing-zero
  length ambiguity, and the complete 411-bit capacity. The production
  Mandelbrot row, public claim, and Fe-derived auxiliary schema now interpret
  through that same utility as 7, 4, and 14 BabyBear fields. A distinct
  zero-import gate independently reconstructs all three encodings, mutates all
  210 main-row source bits and all 203 auxiliary source bits, checks maximum
  values, and proves range violations fail closed. Commits `034a3ac78`,
  `d3b1f1fbe`, and `6fc7dbea2`; gates `poseidon_baby_bear_oracle.rs` and
  `mandelbrot_baby_bear_encoding_oracle.rs`. The portable tree layer now
  preserves zk-kit.fe's `TwoToOneHasher` API and little-endian binary path
  semantics without fixing the field or execution backend. One effect-free
  `merkle_core` body supplies ordinary paths, compact half-pair and
  four-quarter openings, and fail-closed streaming frontiers. Its zero-import
  Wasm gate independently checks all shapes, mutations, invalid indices,
  incomplete trees, and frontier overflow. Typed BabyBear Poseidon2
  interpreters now drive production main and auxiliary trace roots, statement
  binding, and auxiliary transcript binding through that same core. Their
  Plonky3 gate checks full eight-field roots, distinct nominal transcript
  domains, every leaf bit mutation, and
  invalid digest propagation. The transcript now squeezes its composition
  challenge from four Poseidon2 output lanes directly into the canonical
  quartic BabyBear extension, with a dedicated nominal domain and no
  base-field soundness shortcut. Plonky3 independently checks all four
  coefficients and invalid transcripts return no challenge. One shared
  fixed-length field-vector absorber now handles both base-field arrays and
  value-major flattened quartic arrays. The production 17-field main LDE row,
  411-field auxiliary LDE row, and quartic composition row each have distinct
  typed domains over that same mechanism, with independent Plonky3 values and
  position/coefficient mutation checks. Their typed root wrappers reuse the
  same eight-field Merkle compression. The transcript order is now enforced
  by nominal Fe types: proof transcript, main LDE root, auxiliary LDE root,
  quartic composition challenge, composition root, then round-indexed FRI
  challenge and layer stages. FRI domain tags are derived from the const
  round (`FC01`, `FC02`, `FR01`, and so on), not passed as numeric runtime
  IDs. The independent chain gate checks the ordered bindings, first and
  second round challenge coefficients, and a quartic FRI row. Gates:
  `merkle_core_oracle.rs` and
  `mandelbrot_baby_bear_encoding_oracle.rs`. The first production BabyBear FRI
  binary fold now operates directly in the quartic challenge field. It pairs
  evaluations at `x` and `-x`, derives the base-field root from the domain
  size, and returns evaluations at `x^2`. Its zero-import gate independently
  reconstructs extension multiplication, subgroup roots, point inverses, and
  all four output coefficients, mutates the evaluation seed, challenge, and
  shift, and rejects zero coset shifts. That fold now drives a complete typed
  16-to-8-to-4-to-2-to-1 commitment chain. Each round commits its folded
  quartic codeword before deriving the next challenge, squares the coset shift,
  and carries nominal round-indexed roots and transcripts. The independent
  Plonky3 gate reconstructs every layer root, final evaluation, and final
  transcript, and mutates the input codeword, starting transcript, and shift.
  Fiat-Shamir query sampling and compact authenticated openings are now
  landed for that exact 16-to-1 chain. The final typed FRI transcript squeezes
  an `FQ01` BabyBear challenge and derives an index in the eight-point half
  domain. The receipt retains only the queried `x` and `-x` composition pair,
  the corresponding pair at each folded layer, compact generic `merkle_core`
  sibling paths, the typed roots, and the final evaluation. The Fe verifier
  reconstructs every transcript and challenge, authenticates every retained
  pair, checks all four folds, and binds the final row directly to its root.
  The independent Plonky3 gate checks exact query indices and rejects changed
  indices, composition and FRI values, sibling paths, roots, final values,
  validity, and the external AIR transcript. Compiler commit `e794781d1`
  preserves addressable shape for zero-length const-generic arrays without
  inventing payload lanes, with both MIR-shape and executed Wasm regressions.
  Typed main and auxiliary BabyBear LDE quartet adapters now reuse the same
  field-generic `merkle_core` four-quarter algorithm. Their executable gate
  independently reconstructs both 16-leaf Plonky3 roots, checks every legal
  quarter position, rejects an out-of-quarter index, and rejects changed
  leaves, siblings, roots, validity, and path indices. Commit `239b3cf39`
  now factors the exact 708-constraint Mandelbrot fold behind one
  `AirColumnView<F>` and `ConstraintNumerators<F>` interpretation over the
  shared `Radix2Field` boundary. BN254 trace interpolation and authenticated
  openings both consume that field-neutral body, with no backend branch or
  numeric column-ID API. Commits `20ac9e47d`, `39fa0177f`, and `25df3cbe5`
  add the general FCO borrow constructor and recursively borrowed canonical
  receipt encoding rather than a proof-local copying shim. The complete
  independent BN254 bigint composition, authenticated FRI, canonical receipt,
  and mutation gate passed 1/1 in 2,205.48 seconds. An opt-in AIR-only edit
  slice retains all coset-point independent parity checks, while the complete
  receipt remains the default authoritative gate. Commit `04533f5cf` now
  interprets the same 708 constraints over `BabyBearExt4` through an
  `AirColumnView<BabyBearExt4>` backed by authenticated base-field main and
  auxiliary rows. `composition_value<F, TRACE>` derives every trace-domain
  zerofier from its field and compile-time trace size, while
  `radix2_query_geometry<TRACE, LDE>` derives the next-row stride, negative
  offset, and local query position from one transcript-selected index. The
  checkpoint verifier authenticates the four required LDE rows and binds its
  independently recomputed positive and negative AIR composition values to
  the opened FRI pair. Its zero-import Wasm gate reconstructs the quartic
  arithmetic and every constraint family independently, executes the geometry
  at both 4-by-16 and 8-by-64 sizes, and rejects eleven mutations plus an
  out-of-domain query. The final gate passed 1/1 in 49.47 seconds. Fixed
  4-by-16 values now occur only at checkpoint instantiations, not in reusable
  production function signatures. Commit `e9691826f` adds the field-neutral
  recursive `FriSchedule<FIRST_ROUND, INPUT_LOG>` protocol description. Each
  normalized node carries its derived round, input logarithm, output width,
  compact pair width, Merkle sibling depth, and tail. One structural metadata
  interpreter executes at both 16 and 64 points and independently matches
  round count, terminal round, retained evaluations, pair openings, and Merkle
  sibling depth. Compiler commit
  `ad0678f23` makes explicit-return checking normalize both sides before
  unification, closing the language seam where a ground recursive type
  function used as an associated-type receiver normalized in only one
  direction. Commit `d0a2b83b5` then interprets the shared schedule into the
  nested BabyBear complete-codeword carrier and its recursively derived
  fail-closed value. Compiler commit `3976b30ac` makes an enclosing impl's
  symbolic const predicates available as exact assumptions inside its method
  bodies. This closes the language seam that otherwise forced the schedule
  interpreter either to repeat node predicates on a trait method that cannot
  name them or to bypass checked crypto helpers. Commit `6c0317c95` uses that
  machinery to interpret the valid fold traversal from the same
  `FriSchedule<1, 4>` that derives its carrier. `BabyBearFriFirst` handles the
  one composition-transcript boundary, `BabyBearFriTail` recursively commits
  each output layer before deriving the next challenge, and
  `FriScheduleDone` stops without an unused terminal squeeze. This deletes the
  manual 16-to-8-to-4-to-2-to-1 round types and staging functions. The renamed
  `fold_checkpoint_fri16` gate remains zero-import Wasm, matches every
  independent Plonky3 layer oracle, rejects mutations, and passed 1/1 in 97.98
  seconds after the traversal replacement. The complete 41-test const
  predicate suite also passes. Commit `ab1e39966` interprets the same schedule
  into the compact query carrier: every nonterminal node contains its typed
  root, authenticated pair, derived sibling depth, and recursive tail; the
  one-evaluation terminal contains only its root and evaluation. Commit
  `a877cf113` interprets the complete codeword carrier into that compact
  receipt, samples Fiat-Shamir only at the structurally discovered terminal,
  and derives every pair position and path from the schedule's pair width and
  depth. Commit `5b93b90cf` removes the last manual verifier walk. One
  interpreter reconstructs the terminal transcript and query through the
  typed root chain; another authenticates each pair, replays its fold, checks
  the selected child, advances the transcript, and terminates at the one-row
  commitment. The zero-import independent Plonky3 gate passed 1/1 in 148.36
  seconds and rejects changes to the query index, composition and folded
  values, path indices and siblings, final evaluation, intermediate root,
  validity, and external AIR transcript. No 16-to-8-to-4-to-2-to-1 layer list
  remains in the carrier, opener, or verifier. Commits `9f608fe52` and
  `9979f0700` add a field-neutral recursive `FriQueryPlan` and its BabyBear
  interpreter. FQ01 through FQn are independently domain-separated and
  sampled by one structural implementation rather than an authored call
  sequence. Commit `9ba8a581e` extends the field-generic, effect-free
  `merkle_core` with a canonical sparse multipath: requested indices are
  sorted and deduplicated, sibling nodes are emitted bottom-up and
  left-to-right only when they cannot be reconstructed, and nonzero unused
  capacity is rejected. Its independent affine-hash Wasm oracle passed 1/1 in
  0.99 seconds, including eight malformed-proof cases. Commit `d69a06aa6`
  supplies separately domain-checked composition and round-typed BabyBear FRI
  wrappers. Commits `43680a832` and `abf1c8259` then interpret the same query
  plan into each layer's positive and negative leaf requests and feed those
  requests directly into the sparse opener. The zero-import Plonky3 gate
  independently recomputes the actual FQ samples, unique-leaf sets, and
  sibling counts, and passed 1/1 in 205.35 seconds. A complete shared
  multi-query FRI carrier and fold verifier are now landed. Commit
  `59e67aba2` interprets `FriSchedule<1, 4>` and the four-sample query plan into
  one composition multipath, one shared multipath for every nonterminal FRI
  layer, and the one-evaluation terminal. Capacities, round brands, layer
  widths, and sibling bounds remain derived from the schedule and query count;
  there is no second layer list or FQ call sequence. Commit `fc5c766f9` adds a
  two-pass verifier. Its authentication interpretation checks each Merkle
  multipath exactly once while reconstructing the typed transcript and fold
  challenges. A second query-plan interpretation lowers the typed samples to
  one fixed buffer, then an ordinary Fe loop reuses a single
  schedule-specialized fold checker for all four queries. This reduced focused
  Fe analysis from more than 9 minutes 55 seconds to 35.6 seconds without
  moving protocol work into the host. The optimized zero-import Wasm gate
  independently derives the FQ samples, canonical leaf sets, sibling counts,
  roots, and terminal value, then rejects mutations at the composition, each
  of the three sparse FRI layers, terminal, external transcript, and coset
  shift boundaries. It passed 1/1 in 568.12 seconds. Commit `e0ae2416e` now
  interprets those same typed FQ samples through
  `radix2_query_geometry<TRACE, LDE>` into current, next, negative, and
  negative-next row requests, then opens one canonical shared multipath in
  each separately branded main and auxiliary base-field tree. The large row
  arrays are borrowed with `ref`, openings are populated with `mut`, and
  verifiers borrow the completed records. This avoids backend-invalid Wasm
  signatures with thousands of flattened parameters or results without
  moving storage into a host shim. Its zero-import Plonky3 gate independently
  reconstructs both roots, the actual FQ row set, canonical leaf ordering,
  and sibling counts, and rejects value, sibling, and index mutations in both
  trees. The focused opener-only gate passed 1/1 in 27.18 seconds. Commit
  `1e16febd1` then adds the generic `BabyBearAirFriReceipt` carrier and the
  concrete `Checkpoint4BabyBearReceipt16` interpretation. Its verifier
  authenticates each AIR tree and each FRI layer once, reconstructs the typed
  query buffer from the terminal transcript, recomputes every selected
  composition pair through the shared 708-constraint AIR, and replays every
  FRI fold. The combined zero-import Wasm seam gate builds the receipt in Fe,
  accepts the clean value, and rejects mutations to main rows, auxiliary rows,
  composition values, the main root, pre-LDE transcript, coset shift, and
  claimed coordinate. It passed 1/1 in 147.31 seconds. Commit `c05b4cb37`
  closes the production LDE input seam. A typed `EscapesByQ12` claim is now
  interpreted into its exact canonical four-row trace, then the shared
  field-generic radix-2 coset LDE derives all 17 main and 411 auxiliary
  columns. The Fe path commits every generated row without accepting a host
  witness. Its zero-import Wasm gate independently reconstructs the trace,
  performs a direct inverse DFT and polynomial evaluation rather than replaying
  Fe's butterfly schedule, and computes both roots with Plonky3 Poseidon2. It
  passed 1/1 in 157.08 seconds and rejects non-escaping claims and a zero coset
  shift. Commits `5ffa02d5c`, `b3f0123b0`, and `c34720fc2` close the canonical
  BabyBear receipt boundary without a host codec. `ImplBuilder.borrow_mut`
  lets FCO providers synthesize explicitly qualified mutable field decoders,
  with positive replay, negative fail-closed, and frozen command-surface
  gates. Generic canonical codecs now cover u32-native prime fields, quartic
  extensions, Poseidon2 digests, and counted sparse Merkle multipaths. The
  proof carrier consolidates composition and round-branded FRI openings into
  one role-branded `LdeMultiOpening`, derives nested stream codecs from the Fe
  record and recursive schedule types, and emits only counted leaf, sibling,
  and value prefixes. The optimized zero-import Wasm gate parses the public
  carrier topology independently, proves that no unused capacity slots enter
  the wire receipt, roundtrips through Fe decode and verification, and rejects
  truncation, trailing data, noncanonical booleans, the BabyBear modulus,
  over-capacity counts, and authenticated value mutation. It passed 1/1 in
  467.98 seconds. The same run peaked near 10 GB RSS, so whole-receipt
  generated-function lowering is now a measured compile-cost risk. It also
  exposed an outstanding backend bug for an uninitialized local first written
  through a `mut` call: semantic checking accepts it, but Sonatina SSA lowering
  treats the local as undefined. The receipt currently begins from an explicit
  Fe canonical-empty value, not a host workaround. Fixing the general backend
  path and reducing derived decoder compile cost remain compiler work.
  The explicitly named checkpoint remains a four-row, 16-point toy protocol
  and is not yet a succinct production proof. The same permutation has also
  lowered to Naga-valid u32-only WGSL with the local Sonatina conditional-loop
  structurizer fixes, but that browser gate is not landed until those commits
  are published and the Fe dependency pin advances. The complete protocol
  retarget remains pending.
- [~] Make the production claim a chunked recursive high-precision recurrence,
  not one monolithic fixed-Q12 trace. BabyBear is the proof field, not the
  numeric precision ceiling: derive each signed fixed-point coordinate from a
  const-generic vector of bounded base-field limbs, initially reusing the
  established 13-bit `Fixed<L>` representation. Leaf proofs certify exact
  iteration chunks and their multi-limb boundary states. Parent proofs require
  adjacent boundary equality and compress two certified intervals into one
  typed accumulator. Derive convolution tiles and staged carry normalization
  from the limb count, commit boundary states through fixed-size typed digests,
  and independently gate rounding, range, carry, chunk-continuity, and mutated
  boundary failures. The browser should progressively schedule leaf chunks and
  binary merges through the same Fe task, cancellation, and backpressure spine
  used by rendering.
  The field-neutral semantic boundary is now landed at `5eb714b91`. The
  canonical `precision::fixed::Fixed<L>` value stores one sign and an
  LSB-first `[u32; L]` of bounded 13-bit limbs, normalizes signed zero, and
  converts in Fe to the existing recursive limb arithmetic. Generic
  `HighPrecisionEscapeClaim<L>`, `OrbitBoundary<L>`, `OrbitInterval<L>`, and
  `RecursiveOrbitAccumulator<L>` records define exact post-step orbit chunks.
  Leaf certification replays `z_(n+1) = z_n^2 + c`; parent merging requires
  valid children, matching claims, identical shared boundaries, ordered
  intervals, and overflow-safe leaf counts. Their canonical codecs are
  reflection-derived from the nominal records. The zero-import Wasm gate
  `mandelbrot_recursive_fixed_oracle.rs` compares all 37 carrier words against
  an independent bigint model over directed and randomized inputs, and rejects
  malformed limbs, signed zero, invalid booleans, truncation, trailing words,
  changed endpoints, statement mismatch, invalid children, and discontinuous
  boundaries. Focused nextest passes 1/1. This is the recursive semantic
  contract, not yet a succinct recursive proof: fixed-size typed boundary
  digests, multi-limb AIR constraints, BabyBear leaf receipts, and parent proof
  verification remain open.
  Commits `ad20370d7` and `e12032a13` close the fixed-size digest boundary.
  `poseidon_baby_bear::commit_canonical` is a reusable `WordSink`
  interpretation that streams any reflection-derived `CanonicalWords` record
  directly into the typed field sponge. It binds the nominal domain and
  schema-derived word count, rejects non-field words and codec count failures,
  and materializes no second protocol array or host buffer. Its independent
  Plonky3 gate passes 2/2 at unoptimized Wasm. The gate also caught an
  initially over-wide `usize` cursor at the Wasm ABI; the actual bounded cursor
  is now `u32`, rather than hiding the mismatch behind optimization.
  The separate `mandelbrot_proof_recursive_baby_bear` ingot derives
  precision-branded statement and boundary digests from the canonical raw
  carriers. Its 31-word `RecursiveCommittedInterval<L>` exposes only typed
  endpoint digests, iteration bounds, and a leaf count to parent proofs.
  Parent merges reject changed statement or shared-boundary digests, invalid
  children, non-adjacent iterations, and count overflow. The recursive bigint
  oracle now also compares every digest lane against independent Plonky3 and
  passes 1/1. The remaining leaf and parent proof receipts must authenticate
  these carriers; the semantic leaf adapter still replays its interval and is
  deliberately not presented as succinct.
  Commit `92d779ecc` begins the exact multi-limb AIR at the unsigned magnitude
  boundary. One limb-count-derived sparse convolution schedule supplies the
  scalar witness, integer relation verifier, arbitrary-`Radix2Field` residuals,
  and eventual placement plan. It derives BabyBear's safe 13-bit convolution
  width as 30 limbs from the modulus and coefficient bound, rather than baking
  a precision tier. The witness retains every normalized product digit and
  carry in two `L`-sized stages, then constrains guard-bit rounding, its carry
  ripple, wrapped output, overflow, and unique sign-zero normalization. The
  zero-import Wasm gate compares directed and randomized `Fixed<4>` products
  against an independent `BigUint` schoolbook model, checks generated L4 and
  L8 schedules term by term, and requires exact nonzero BabyBear residuals for
  mutated digits, carries, rounding, high digits, and outputs. Focused nextest
  passes 1/1.
  Commits `3968077ed`, `78256ed78`, and `8085a4936` extend that relation into
  an exact signed recurrence DAG. `FixedLinearWitness<L>` retains the addition
  ripple plus both directed subtraction ripples, derives magnitude comparison
  from the final borrow, and constrains signed selection and unique zero for
  both addition and subtraction. One reusable `RadixRangeWitness<L>` derives
  all 13 bits of every radix digit and a running OR prefix, giving local
  degree-two bit, reconstruction, nonzero, and sign-zero relations without a
  precision-specific table. `FixedMandelbrotTransitionWitness<L>` then passes
  the typed outputs of `x*x`, `y*y`, and `x*y` directly into four signed linear
  relations for `z*z + c`; it introduces no copied intermediate columns. The
  expanded zero-import Wasm gate independently reconstructs the 33-word
  linear, 105-word range, and 205-word transition carriers. It checks 19
  directed and randomized transitions against bigint arithmetic and rejects
  mutations in every arithmetic stage, range bit and prefix, signed-zero
  state, or final boundary coordinate. Focused nextest passes 1/1 in 22.85
  seconds after compilation. This is still not a succinct leaf proof. The
  trace interpreter must apply the reusable range carrier to every selected
  intermediate, constrain the wider product-carry bound, and authenticate the
  resulting rows in a BabyBear leaf receipt before a recursive parent may rely
  on the transition. Commit `962f8b4c5` closes the wider product-carry bound.
  Each carry has an 18-bit value decomposition plus an 18-bit slack
  decomposition satisfying `carry + slack = L * (B - 1)`. Together with the
  modulus-derived `L <= 30` condition, this keeps both sides of every
  convolution equation below BabyBear's modulus. The independent gate checks
  all eight L4 carries and rejects changed carries, value bits, and slack bits;
  focused nextest passes 1/1 in 54.92 seconds. Instantiating the typed range
  relations across every transition intermediate and interpreting the same
  plan over arbitrary field-valued opening rows remain the next leaf-AIR gate.
  Commits `f4236566b` and `a3d653c9b` close that scalar polynomial
  interpretation gate. Rounding carry-outs are now explicit boolean witness
  columns, so no residual recomputes them through a host integer shift. The
  separate `mandelbrot_proof_fixed_air_core` ingot mirrors the recurrence as
  nested field-valued `AirProduct`, `AirLinear`, and `AirMandelbrotTransition`
  records and folds 6,185 L4 constraints using field operations only. The
  count is derived from `L`, range width, and operation structure. Three fold
  challenges over 19 directed and randomized transitions are zero, while
  changed convolution digits, carries, rounding carries, signed borrows,
  boundary limbs, and noncanonical inputs are nonzero. The zero-import Wasm
  gate passes 1/1 in 31.26 seconds. Its first adapter attempted to return the
  complete field row as one flattened aggregate and correctly failed Wasm
  validation with an out-of-bounds return size. The executable adapter now
  streams smaller typed subrecords into the same fold, retaining the typed
  authoring schema without placing a 4,000-field value on a backend ABI.
  This establishes the exact relation oracle, not the production trace
  placement. A narrow sparse multi-row schedule must now interpret the same
  convolution and range plan so a leaf proof does not commit thousands of
  columns per transition.
  Commit `79c8e9977` derives that alternative placement plan from the same
  nominal DAG. `SparseTransitionTask` has typed radix, carry, product, linear,
  and boundary variants whose payloads name `ProductNode`, `LinearNode`,
  `RangeNode`, and `CoordinateNode`; application code does not maintain a
  phase-ID or 31-column table. The plan contains `3*L*L + 683*L + 41`
  microtasks after the linear-role refinement recorded below. Independent Rust
  enumeration matches every one of the 2,821 L4
  task signatures, including triangular convolution ranks and the first
  invalid padding row. The derived radix-two placements are 4,096 rows at L4,
  8,192 at L8, and 16,384 at L20. Focused nextest passes 1/1 in 34.89 seconds.
  The wide field fold and sparse task plan are now two interpretations of one
  relation, not competing arithmetic implementations. The next gate must give
  sparse rows their narrow accumulator payload and adjacency constraints, then
  compare their folded denotation to the already gated wide interpreter before
  attaching Merkle, FRI, or WebGPU machinery.
  Commits `409b75d33` and `e8a44c12c` establish the first executable sparse
  subtrace. The task generator now places each close row directly after the
  rows it reduces: radix bits precede their limb close, carry bits precede
  their carry close, product terms precede their coefficient close, and
  boundary limbs stay with their coordinate. Task counts and power-of-two
  domains are unchanged. `SparseTaskRow<F>` is a uniform six-field carrier
  whose two accumulator pairs are independent of limb count. Its radix
  interpretation proves bit and prefix booleans, the running OR recurrence,
  incremental digit reconstruction, limb close and reset, global nonzero,
  signed-zero normalization, and every adjacent accumulator link. An
  independent Rust integer model reconstructs all 1,767 L4 radix rows across
  the 31 semantic range nodes exactly, while the zero-import Fe Wasm audit
  checks 10,553 local and adjacency constraints under three challenges and
  rejects mutations in every semantically used row lane. Focused nextest
  passes 1/1 in 32.69 seconds. This is zero-set equivalence for the complete
  range family, not yet equality of the full sparse and wide folds. The next
  bounded gate is to interpret carry-bit reconstruction in the same row
  carrier, then add convolution term reduction and require both sparse and
  wide interpreters to accept and reject the same directed mutation set.
  Commit `64905d933` completes that bounded-carry slice. The same six columns
  now reconstruct each product carry from 18 boolean bits, reset the active
  scan, retain the completed carry while reconstructing its 18-bit slack, and
  close with `carry + slack = L * (B - 1)`. The independent integer oracle
  matches all 888 L4 rows exactly. The zero-import Fe Wasm audit checks 4,488
  local and adjacency constraints under three challenges and rejects every
  used-lane mutation, including both sides of the value-to-slack handoff.
  Focused nextest passes 1/1 in 40.20 seconds. Convolution term reduction is
  now the next sparse equivalence gate.
  Commit `99e1d6256` adds the convolution reduction without severing it from
  those earlier phases. Seventy-two L4 product rows reduce each generated
  triangular term sequence, close every coefficient with its incoming carry,
  radix digit, and outgoing carry, and prove both initial and terminal zero
  state. `SparseCopyAddress` is nominal and hierarchical. A Fe-derived copy
  bus assigns a distinct challenge power to every range digit and product
  carry, derives source multiplicities from the product DAG, and equates those
  producers with all term and coefficient reads. It therefore introduces no
  application-maintained address table. The independent integer model matches
  every product row exactly, while the zero-import Wasm audit checks 315 local,
  adjacency, terminal, and copy-balance relations under three challenges.
  Mutations in all six row lanes fail. A coordinated two-row mutation that
  preserves the convolution equations and adjacency is rejected by the copy
  bus alone, preventing a locally consistent trace from substituting distant
  source values. Focused nextest passes 1/1 in 37.73 seconds. Product rounding,
  product signs, and linear ripples remain before full sparse and wide
  transition equivalence can be claimed.
  Commit `2125f88e6` closes product rounding and sign normalization in the
  sparse projection. Each product's `L` rounding rows now stay adjacent to its
  finish row. The ripple constrains guard carry-in, retained digit, output
  digit, and boolean carry-out, while the finish constrains the final carry and
  `left_sign XOR right_sign`, masked by output nonzero for canonical zero. A
  second typed copy bus binds the retained window, output digits, guard bit,
  input signs, output sign, and output nonzero to their independently range-
  constrained rows. Redundant wide-carrier aliases such as `round_overflow`
  are projected out rather than copied into the sparse trace. The independent
  model matches all 15 L4 rows. Fe Wasm checks 85 local, group-boundary,
  adjacency, and copy-balance relations under three challenges, rejects every
  row-lane mutation, and rejects a locally valid retained/output substitution
  through the copy bus alone. Focused nextest passes 1/1 in 42.17 seconds.
  The next gate is the four signed linear ripples plus their copy bus.
  Commit `0710f2e75` completes those four signed linear nodes. The former
  overloaded `LinearLimb` task is now a nominal role family for sum, both
  directed differences, and output selection. This adds 12 rows per limb but
  keeps every arithmetic row at six fields and leaves the L4, L8, and L20
  domains at 4,096, 8,192, and 16,384 rows. Independent enumeration now checks
  2,821, 5,697, and 14,901 semantic tasks. The four L4 nodes occupy 68 rows and
  constrain all carry and borrow chains, magnitude comparison from the final
  borrow, signed addition/subtraction selection, output nonzero, and canonical
  output sign. Four per-node typed copy buses bind every repeated input,
  intermediate, selector, final borrow, and output back to the range rows or
  its unique arithmetic producer. The independent model matches every row;
  zero-import Fe Wasm checks 276 local, adjacency, and copy-balance relations
  under three challenges, rejects every lane mutation, and uses the copy bus
  alone to reject a locally valid input/output substitution. Focused nextest
  passes 1/1 in 49.26 seconds. Only final transition boundary equality and a
  combined sparse-versus-wide directed mutation gate remain in this AIR slice.
  Commit `d70304511` closes the final transition boundary. Two sign rows and
  `2*L` limb rows equate the computed `next_real` and `next_imaginary` outputs
  with the claimed next boundary, and a finish row constrains transition
  validity plus zeroed unused lanes. A typed copy bus binds both sides to their
  range rows, so repeating an equal but unrelated pair cannot satisfy the
  relation. The independent model matches all 11 L4 rows. Fe Wasm checks 61
  local and copy-balance relations under three challenges, rejects every lane
  mutation, and uses the copy bus alone to reject a coordinated equal-pair
  substitution. The complete focused gate now passes 1/1 in 67.39 seconds.
  Every wide relation family now has an executable narrow sparse projection.
  The remaining equivalence gate must apply one shared directed mutation set
  to both interpretations before this AIR slice is attached to Merkle and FRI.
  Commit `1c165affa` closes that equivalence gate at the intended zero-set
  boundary. Six shared directed mutations cover a convolution digit, product
  carry, rounding carry, signed borrow, claimed next-boundary limb, and an
  out-of-range public-point limb. For each of three challenges, both the wide
  6,185-constraint interpreter and the corresponding sparse phase reject the
  same semantic mutation. This is not a misleading byte or fold equality
  claim: the sparse projection deliberately adds scan adjacency and typed copy
  buses while removing redundant wide aliases. Together with exact row
  reconstruction and copy-only coordinated mutations for every phase, the
  gate establishes equivalent accepted arithmetic statements. Focused nextest
  passes 1/1 in 84.69 seconds. The multi-limb arithmetic and wide-to-sparse AIR
  equivalence item is complete. The next item is to commit the sparse rows as
  authenticated BabyBear leaf openings and bind recursive parent receipts to
  those leaf proofs rather than replaying a semantic interval.
  Commits `451f35a27`, `2a8924d3c`, and `f09d1d14a` close the base-trace
  authentication slice without overstating it as a STARK. One
  `sparse_transition_row_from_witness` interpreter now derives every active
  six-column row and canonical zero padding from `SparseTransitionTask`; all
  existing independent phase oracles execute through that shared entry. The
  reusable `merkle_core` API can now borrow a large leaf matrix and emit a root
  plus normalized sparse path in one traversal. This avoids both duplicate
  hashing and a 32,768-lane Wasm signature, a failure the L4 consumer exposed
  when `[Poseidon2Digest; 4096]` was initially passed by value. The separate
  `mandelbrot_proof_fixed_air_baby_bear` ingot commits a nominal leaf containing
  limb count, semantic task count, power-of-two trace length, row index,
  active flag, and the six readable witness lanes under the `SL01` Poseidon2
  domain. Its limb-branded root, canonical variable-length multipath, and
  authentication verifier reuse the shared Poseidon2, canonical-word, and
  zk-kit-derived Merkle interpreters. An independent Rust model reconstructs
  all 4,096 leaf messages, applies Plonky3 Poseidon2, and matches the complete
  Fe root. The gate accepts active and padded openings and rejects mutations
  to the root, path index, sibling, metadata, active flag, row value, codec
  length, and out-of-range request. It passes 1/1 in 66.20 seconds; the older
  independent BabyBear LDE multipath regression remains green at 1/1 in
  545.02 seconds.
  Commit `cfdea6d36` closes the deterministic sparse-control relation. Fifteen
  named `SparseControlSelectors<F>` columns identify semantic task kinds
  without a numeric phase ID. Six field-valued payload columns carry the
  generated node, limb, bit or term counters, carry/convolution flag,
  convolution width, and radix weight. One field-neutral AIR constrains every
  selector to be boolean and one-hot, zeros unused payloads, fixes the initial
  and terminal rows, and admits only the exact radix, carry, convolution,
  rounding, linear, boundary, finish, and padding transitions. Its evaluator
  takes only current and next field rows. It does not consult a row index or
  dispatch through `SparseTransitionTask`, so it remains meaningful on LDE
  evaluations. The independent Rust oracle reconstructs all 4,096 L4 control
  rows, while zero-import Fe Wasm evaluates 954,172 local, adjacency, and
  boundary constraints under three challenges. Mutations to selectors,
  counters, flags, widths, weights, phase boundaries, and padding all fail.
  The dedicated gate passes 1/1 in 13.20 seconds, and the complete recursive
  fixed-point regression remains green at 1/1 in 148.06 seconds.
  Commit `fc2d1b4d3` closes the task-selection seam in the arithmetic AIR.
  Four named linear-role selectors avoid cubic role interpolation, and eight
  named range-role selectors distinguish coordinates, product low/high/output,
  and linear sum/differences/output without a 31-way numeric lookup. The range
  roles constrain the exact signed-node pattern, so unsigned sign lanes remain
  canonical zero rather than weakening into don't-care witnesses. The complete
  control row is now 33 readable field columns: 15 task selectors, four linear
  roles, eight range roles, and six semantic payloads. Its latest independent
  zero-import audit reconstructs all 4,096 rows and evaluates 1,109,798
  constraints under three challenges, rejecting mutations across every task,
  role, payload, handoff, and padding family. It passes 1/1 in 24.32 seconds.
  `evaluate_sparse_arithmetic_row` and its adjacency interpreter consume only
  current/next control and six-lane witness rows. Radix weights, carry reset,
  convolution and rounding scans, linear roles and subtraction mode, boundary
  equality, transition finish, and canonical padding are all polynomially
  selected. No task enum or row index enters either production-facing
  evaluator. The complete selector-only L4 audit evaluates 413,678 arithmetic
  constraints under three challenges and rejects directed mutations in every
  semantic family. The older enum-based phase oracles remain independent
  compatibility checks. The full recursive fixed-point gate passes 1/1 in
  97.91 seconds.
  Commit `5bf69746b` upgrades each sparse leaf to the nominal `SL02` protocol
  and authenticates its exact 33-column control row beside the six witness
  lanes. Both leaf hashing and structural control equality are now derived in
  Fe from the record shape: `CanonicalWords` streams the typed leaf directly
  into Poseidon2, while the Fe-authored `Eq` derive provider generates the
  nested comparison. There is no manually maintained 44-field Fe table.
  Generic `WordField<M>` now supplies core `Eq`, so the same structural route
  remains available to other word-field records. The independent Rust oracle
  still spells out a separate field layout, reconstructs all 4,096 `SL02`
  leaves with Plonky3 Poseidon2, and matches the complete Fe root. Directed
  mutations now cover task selector, linear role, range role, semantic
  payload, flag, witness lane, metadata, path, and root fields. The full gate
  passes 1/1 in 139.83 seconds.
  Commit `2f2105011` closes the first committed-copy relation at the AIR level.
  The product bus is no longer a scalar `challenge^address` total that would
  require a host integer address at an LDE point. A generic Fe
  log-derivative interaction instead compresses each nominal `(address,
  value)` pair with `beta` and `gamma`, constrains two inverse ports per row,
  and advances one prefix accumulator from zero back to zero. Address formulas
  consume only the constrained selectors, roles, and semantic counters. Scan
  adjacency lets the new relation remove the older redundant carry-in copy:
  each range-checked carry is bound once to its coefficient output, while the
  arithmetic AIR already propagates it to the next coefficient. An independent
  Rust model reconstructs the conceptual copy multiset from task semantics and
  matches a field receipt over every accumulator and inverse in all 4,096
  rows under three challenge pairs. Source, consumer, inverse, and accumulator
  mutations fail. The established coordinated mutation that preserves the
  local product scan fails exactly once at the interaction terminal, proving
  the new relation is not duplicate arithmetic. The full focused gate passes
  1/1 in 93.53 seconds. This exhaustive gate currently uses the BabyBear base
  field for speed. The production transcript must derive quartic-extension
  compression challenges and use the same field-generic relation, with batch
  inversion before the interaction trace is committed.
  Commit `c40dccaa9` adds two reusable low-degree limb-position selectors for
  the fixed-point rounding boundary. `penultimate` and `last` are generated
  from the limb-count-generic task plan, constrained as boolean and mutually
  exclusive, propagated through radix bit and limb rows, and forced at the
  exact `L - 2` and `L - 1` transitions. This avoids an equality
  interpolation whose degree would grow with the limb count. The authenticated
  control row is now 35 field columns and the sparse leaf domain is `SL03`.
  The independent Rust control model covers both new columns. Its exhaustive
  three-challenge mutation gate evaluates 1,167,135 constraints and passes
  1/1 in 26.97 seconds. The complete recursive receipt, root reconstruction,
  and authentication mutation gate passes 1/1 in 94.80 seconds.
  Commits `3b42093d6` and `ca4ad8d96` extend the same committed interaction
  machinery through fixed-point rounding. One const-generic
  `SparseCopyInteractionRow<F, PORTS>` now carries the prefix accumulator and
  a type-level fixed array of inverse ports; product is its two-port
  interpretation and rounding is its five-port interpretation. The retained
  low/high window is expressed as one contiguous address interval, so output
  index `i` selects low limb `L - 1` followed by high limb `i - 1` through a
  linear address formula rather than an `i == 0` interpolation. Selector-only
  Fe ports bind that window, every rounded output digit, the guard bit, output
  sign and nonzero flag, and both input signs. The independent Rust model
  reconstructs the conceptual multiset directly from semantic task kinds and
  matches all five Fe inverse columns across 4,096 rows under three challenge
  pairs. Source, consumer, inverse, accumulator, and coordinated locally valid
  rounding mutations reject; the coordinated mutation fails exactly once at
  the terminal balance. The complete gate evaluates 45,057 rounding
  interaction constraints and passes 1/1 in 101.44 seconds.
  Commit `49b048194` adds the eight-port linear interpretation. It binds
  left/right inputs, sum and directed-difference intermediates, selected
  outputs, signs, nonzero flags, terminal borrows, and the derived same-sign
  and select-right controls. The address formulas follow the four-node linear
  DAG with constant-degree field polynomials, including the even-node reuse of
  `RealDifference` and `DoubleXy` outputs by their downstream additions. The
  log-derivative model also corrects a weakness in the legacy scalar audit:
  each central range-checked intermediate digit is now counted once for its
  arithmetic production and once for its later selection use, rather than
  relying on an aggregate power-sum cancellation. The independent Rust model
  reconstructs this nominal graph directly from semantic task kinds and
  matches all eight Fe inverse columns under three challenge pairs. Its source,
  consumer, inverse, accumulator, and coordinated locally valid mutations all
  reject, with the coordinated mutation failing only at the terminal balance.
  The complete gate evaluates 69,633 linear interaction constraints and passes
  1/1 in 112.73 seconds.
  Commit `0e1a2ebff` closes the final base-copy family with a two-port boundary
  interpretation. Only the claimed `NextReal` and `NextImaginary` ranges and
  their computed `LinearOutput(NextReal)` and
  `LinearOutput(NextImaginary)` counterparts enter this namespace. The Fe AIR
  selects those four radix sources with constant-degree control polynomials,
  then binds each computed/claimed sign or limb pair without enum dispatch or
  row indices. The independent semantic model matches both inverse columns
  under three challenge pairs. Source, consumer, inverse, accumulator, and a
  locally valid equal-sign flip all reject; the equal-sign flip fails exactly
  once at the terminal balance. The complete gate evaluates 20,481 boundary
  interaction constraints and passes 1/1 in 119.57 seconds. Product, rounding,
  linear, and boundary nominal copy families are now all represented by the
  shared const-generic log-derivative relation.
  Commit `28c5d6a89` replaces per-port witness inversion with the reusable
  zero-tolerant `batch_inverse_or_zero<F, N>` field interpreter. Each typed
  interaction row now performs at most one inversion regardless of its port
  width; inactive zero lanes stay zero and active denominators share prefix and
  suffix products. The complete independent receipts and mutations remain
  byte-identical and pass 1/1 in 117.45 seconds. The same const-generic utility
  can later batch larger row tiles in scalar, packed, or WebGPU schedules.
  Commit `f719d5181` moves interaction challenge derivation to the production
  transcript boundary. The authenticated `SL03` root is squeezed under eight
  independent Poseidon2 domains: beta and gamma for each of product, rounding,
  linear, and boundary. Every challenge is a full quartic BabyBear element.
  The independent Plonky3 oracle matches all 32 base-field coefficients, and
  an invalid root fails closed to a false validity bit plus 32 zero words. The
  complete gate passes 1/1 in 121.93 seconds.
  Commit `f7e09f527` generates and commits the complete quartic interaction
  trace. Every `SI01` leaf has one FCO-derived 97-field schema containing exact
  trace metadata, the `SL03` base root, and the product, rounding, linear, and
  boundary accumulators plus inverse ports. One row-local Fe interpretation
  boundary derives control and witness rows, evaluates all four buses, checks
  their local relations, hashes the nominal leaf, and updates caller-owned
  accumulators. The reusable batch-inversion plan now also has a caller-owned
  placement interpretation. Together these let scalar Wasm reclaim quartic
  temporaries after every row without increasing memory or moving arithmetic,
  scheduling, or commitment work into Rust or JavaScript. The independent
  Rust model reconstructs all 4,096 semantic rows and every port, implements
  the quartic extension separately, checks every inverse and all four terminal
  balances, applies Plonky3 Poseidon2, and matches the Fe root. The optimized
  gate accepts that independently derived root, rejects a one-coefficient
  mutation, and fails closed on an invalid base commitment with the canonical
  zero root. The complete two-test recursive fixed-point binary passes 2/2
  with zero skips in 234.23 seconds.
  Commit `deb6d8314` gives the shared radix-2 arithmetic plan caller-owned
  forward NTT, inverse NTT, and disjoint-coset LDE interpretations. The
  value-returning APIs remain convenience wrappers over that same body. The
  existing direct-u64 BabyBear DFT/LDE oracle now enters through the writer,
  retains invalid-coset rejection, and passes 1/1 in 5.20 seconds.
  Commit `db1f8fac8` adds the nominal 41-field `SparseBaseAirRow`, exposes the
  84-field quartic interaction row as a caller-owned tile interpretation with
  explicit starting accumulators, and composes those pieces into a full-table
  writer with a small typed root digest. This is a placement boundary, not a
  numeric column table: FCO derives canonical field order from the same Fe
  records used by the commitments. An attempted function returning both full
  4,096-row tables honestly failed Sonatina's instruction-result index, so the
  large tables remain caller-owned. Executing both initialized tables in the
  scalar checkpoint remained CPU-bound beyond 14 minutes, so it is not used as
  the ordinary semantic gate. Instead, one zero-import O2 checkpoint executes
  four exact caller-owned base and interaction rows plus their ending
  accumulators, then continues through the complete 4,096-row `SL03` and
  `SI01` roots without allocating the full tables. Its independent model
  matches all 564 canonical words, including exact-root acceptance, a directed
  coefficient mutation, and invalid-root zeroing. It passes 1/1 in 81.17
  seconds. The full-table writer Fe-checks, but its practical execution remains
  part of the production LDE and WebGPU placement gate, not a scalar-Wasm
  performance claim.
  Commit `d9120e986` carries those nominal rows through the next two protocol
  seams without introducing a 125-column index map. The generic fixed-AIR
  control, witness, and four interaction records now derive their canonical
  word traversal directly, and the BabyBear commitment layer aliases those
  exact records instead of maintaining field-for-field copies. One generic
  column interpreter places the reflection-derived 41 base and 84 interaction
  columns through the shared caller-owned radix-2 LDE. The inverse interpreter
  reconstructs typed quartic opened rows from authenticated base-field lanes.
  All local, pair, initial, and terminal constraint evaluators feed a shared
  four-family numerator record, whose zerofier quotient now lives beside
  `Radix2Field` rather than in a Mandelbrot-specific backend. The zero-import
  Wasm gate independently reconstructs every LDE field with a direct u64 DFT,
  checks current and next typed-row geometry, applies the quartic quotient
  independently at four evaluation points under two challenges, rejects two
  invalid requests, and proves directed base and interaction mutations alter
  both the relevant numerator family and final composition. It passes 1/1 in
  273.67 seconds. The executed checkpoint remains the honest four-row to
  sixteen-point placement gate; full 4,096-row placement is reserved for the
  production GPU schedule.
  A transcript-order audit then found that the raw `SL03` root could not safely
  remain the production interaction seed: without a proof linking that raw
  tree to the separately committed LDE, a prover could choose the seed before
  fixing the codeword checked by FRI. Commit `c9f7e9e8c` corrects that boundary
  before it ossifies. Base evaluation and LDE placement are now independently
  callable, every `LD01` base-LDE leaf absorbs limb count, trace size, LDE
  size, position, and all 41 reflection-derived fields, and the typed Merkle
  root is fixed before any interaction witness exists. Production beta and
  gamma values then squeeze from that root under eight distinct `*02` domains;
  the earlier `*01` derivation remains explicitly a semantic-checkpoint tool.
  The independent Plonky3 gate reconstructs the `LD01` root, proves a directed
  base-field mutation changes it, checks all 32 quartic challenge coefficients
  for both clean and mutated roots, and rejects invalid roots and mutation
  selectors. The focused zero-import gate passes 1/1 in 275.53 seconds.
  Commit `192a0950f` completes the corrected interaction side. Production
  challenges now cross the API in a shape-branded wrapper whose payload is
  private, so downstream code cannot repackage raw-checkpoint challenges as
  base-LDE-derived values. The same row-local four-bus interpretation then
  generates caller-owned interaction tiles, a split 84-column LDE writer
  places them independently, and each `LD02` leaf absorbs its proof shape,
  position, exact `LD01` root, and every interaction field. The typed
  interaction root retains that base-root dependency. The independent model
  derives the production `*02` challenges, reconstructs the first four
  interaction rows and every inverse from semantic copy ports, applies a
  direct u64 LDE to all 84 columns, and matches both clean and mutated Plonky3
  roots. It also rejects an unknown mutation selector. The focused zero-import
  gate passes 1/1 in 389.36 seconds.
  Commit `1aff766be` closes the production composition seam without creating a
  second FRI protocol. The compact and sparse provers now share the extracted
  `mandelbrot_proof_baby_bear_transcript` ingot for the canonical `BC01`
  quartic challenge, `BC02` composition leaves, and `BC03` post-composition
  binding; the established compact prover re-exports the same API. The sparse
  transcript commits the exact one-step `RecursiveCommittedInterval<L>` under
  `AS01`, binds the exact `LD01` and dependency-carrying `LD02` roots under
  `AT01`, then binds the statement and roots under `AT02`. Its caller-owned
  codeword writer recomputes both roots and all interaction challenges
  internally before evaluating every composition point, so callers cannot
  pair a codeword with roots or challenges from another commitment. A typed
  composition digest retains the complete statement and LDE dependency chain.
  The independent gate reconstructs the recursive statement commitment,
  `LD01`, `LD02`, nested transcript, all four `BC01` coefficients, every one of
  the 16 four-zerofier quotients, and the `BC02` Plonky3 Merkle root. Separate
  base-field, interaction-field, and valid-statement mutations change exactly
  the expected upstream roots and every downstream transcript artifact; an
  unknown mutation fails closed. The focused zero-import Wasm gate passes 1/1
  in 558.10 seconds.
  Commit `9bc0be73a` binds the named recursive leaf statement to the sparse AIR
  without replaying the transition at the public boundary. One semantic
  `SparsePublicCoordinate` interpretation derives the six point, current, and
  next coordinate positions directly from the generated transition task plan.
  For each coordinate, all `L` fixed-point limbs and its sign become
  fixed-position rational constraints. An executable coherence check resolves
  every derived position back through the task plan, so a layout refactor
  cannot silently preserve a stale numeric row table. Distinct `PC01` and
  `PM01` quartic challenges fold the public constraints and mix them into the
  existing `BC02` composition codeword. The production writer requires the
  exact sparse trace length, recommits the raw one-step interval, rebinds the
  `AT02` transcript, and adds the public contribution to every composition
  point. The four-row scalar placement writer is now explicitly named as a
  checkpoint and cannot be mistaken for a receipt-valid production writer.
  A focused zero-import Wasm gate instantiates the real `L = 4`, `TRACE =
  4096`, `LDE = 8192` public quotient at one extension-field point. Its
  independent Rust model enumerates the semantic task list to recover all 30
  positions, reconstructs both transcript challenges, and matches the complete
  quartic rational composition. It rejects a different valid interval
  statement and proves a changed opened value changes the public contribution.
  The focused gate passes 1/1 in 52.21 seconds. The established independent
  4-by-16 composition, root, and mutation gate remains green at 1/1 in 976.01
  seconds. The full 4,096-row production writer Fe-checks but has not been
  executed as a scalar full-codeword claim; that placement belongs to the
  pending WebGPU schedule.
  `SL03` and `SI01` remain exact semantic checkpoints rather than production
  proof roots. `LD01`, `LD02`, and the shared `BC02` composition root now cover
  the production codewords, including the fixed-position public relation, but
  this is not yet a succinct recursive leaf proof. The next layer must carry
  the required `LD01` public positions through authenticated multi-query
  openings, then reuse the existing FRI and canonical receipt interpreters.
  Only that complete AIR/FRI receipt may replace semantic replay in the
  recursive parent carrier.
- [~] Interpret the BabyBear proof dependency plan through the Conal/CTFE
  WebGPU scheduler for NTT/LDE, AIR composition, Poseidon/Merkle, and FRI.
  The first arithmetic-plan consolidation is landed at `ba2ded864`: one
  `Radix2Field` interpretation boundary now drives the same forward NTT,
  inverse interpolation, and disjoint-coset LDE dependency plan for both the
  multi-limb BN254 field and u32-native BabyBear. The independent direct
  bigint BN254 DFT/LDE gate remains 3/3 green, while the new zero-import
  BabyBear gate checks direct u64 DFT/LDE values, round trips, invalid cosets,
  and u32-only browser SPIR-V. The gate exposed a general semantic defect in
  which generic `Copy` values could be reloaded from mutable source lineage;
  `6cc7f996a` fixes compact and array-containing snapshots and adds executable
  regressions rather than reordering the butterfly around the bug. Commit
  `c5cdca47e` now interprets that exact radix-2 plan over the quartic BabyBear
  challenge field as well. The direct oracle checks every coefficient of NTT
  and coset LDE outputs at multiple seeds and shifts, plus invalid cosets. FRI
  can therefore consume one shared Fe transform body instead of introducing
  a field-specific transform fork.
  The scheduling step must now consolidate that arithmetic plan with the two
  existing, independently gated Conal strands:
  `ntt_schedule.fe` derives the `RBin<Pair, k>` stage tree and
  `BarrierReq<k>` from one type-level depth, while `ntt_par_exec.fe` executes
  the corresponding explicit fork/barrier schedule under revm and checks it
  against the sequential transform. The production arithmetic plan is not yet
  an interpretation of that `RBin` schedule, and no workgroup/shared-memory
  proof transform has executed. The remaining work is a backend interpreter
  and placement policy over the shared plan, not a fresh NTT implementation.
- [ ] Build and run the complete recursive proof experience in the canonical
  gallery after the BabyBear prover exists. The Fe-authored component lets a
  user select a high-precision Mandelbrot point and iteration bound, schedules
  leaf chunks progressively on WebGPU, recursively merges adjacent certified
  intervals, presents the typed receipt plus proof-size and timing evidence,
  and verifies it in-browser through Fe. The Chrome gate must exercise point
  selection, successful generation and verification, cancellation and
  backpressure, deliberate receipt mutation rejection, and console and
  device-loss recovery. Shader compilation alone cannot satisfy this gate.
- [x] Define a typed proof encoding and prove malformed-proof and mutation
  rejection in zero-import Fe Wasm against the independent bigint model.
  Browser/native parity for the eventual BabyBear protocol remains part of the
  published-page gate above rather than a claim about this BN254 checkpoint.
- [ ] Run proof submission and verification through structured Fe tasks,
  Worker/MessagePort effects, cancellation, and backpressure.

## External real-GPU handoff

Run these from commit `0d7f3eddd` or later on a Linux host whose printed
adapter is a hardware adapter. Do not set `MB2_ALLOW_GPU_SKIP`; a skip is a
failed campaign gate. `WGPU_BACKEND=vulkan` keeps the path browser-profile
compatible. On a non-Linux host, omit that variable and retain every other
condition.

On Nix, `vulkaninfo` may find a wrapped loader while `wgpu` cannot dynamically
load `libvulkan.so.1`. Make the Vulkan loader's `lib` directory discoverable
through `LD_LIBRARY_PATH` before running these commands. This sandbox required
`/nix/store/7krvb015vp4wq7lj6v3wadjy4q9asc8q-vulkan-loader-1.4.341.0/lib`.
Use `--no-capture` when collecting the final receipt so the adapter name is in
the evidence.

```console
mkdir -p /workspace/tmp /workspace/.sccache
env -u MB2_ALLOW_GPU_SKIP TMPDIR=/workspace/tmp CARGO_INCREMENTAL=0 SCCACHE_DIR=/workspace/.sccache WGPU_BACKEND=vulkan cargo nextest run --release --locked --no-capture -p fe-codegen --test known_color_pass_graph_e2e
env -u MB2_ALLOW_GPU_SKIP TMPDIR=/workspace/tmp CARGO_INCREMENTAL=0 SCCACHE_DIR=/workspace/.sccache WGPU_BACKEND=vulkan cargo nextest run --release --locked --no-capture -p fe-codegen --test rollcall_pass_graph_e2e
env -u MB2_ALLOW_GPU_SKIP TMPDIR=/workspace/tmp CARGO_INCREMENTAL=0 SCCACHE_DIR=/workspace/.sccache WGPU_BACKEND=vulkan cargo nextest run --release --locked --no-capture -p fe-codegen --test precision_fixed_orbit_gpu_oracle
env -u MB2_ALLOW_GPU_SKIP TMPDIR=/workspace/tmp CARGO_INCREMENTAL=0 SCCACHE_DIR=/workspace/.sccache WGPU_BACKEND=vulkan cargo nextest run --release --locked --no-capture -p fe-codegen --test perturbational_mandelbrot_gpu_oracle
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

1. Run the external real-GPU handoff above before beginning the proof GPU port.
2. Finish the authenticated BabyBear sparse AIR: carry the task-derived public
   boundary positions through authenticated `LD01` multi-openings, then feed
   the existing public-bound `LD01`/`LD02`/`BC02` relation through multi-query,
   FRI, and the canonical receipt layers.
3. Lower the BabyBear leaf prover and recursive merges through Fe Conal/CTFE
   WebGPU schedules, then run the complete progressive proof/verify/tamper page
   through Chrome.
4. With multiple nominal child namespaces landed, generalize the render-owned
   DEC path to rich canonical port payloads and nested scopes, then execute it
   in real Chromium.
5. Delete the runtime manifest and finish the legacy disposition.
6. Run the exact G5 command once at the final DONE gate.

The Definition of done is not yet met. In particular, the real-GPU gate,
manifest deletion, Worker/DEC general messaging, complete legacy disposition,
and bounded proof remain open.
