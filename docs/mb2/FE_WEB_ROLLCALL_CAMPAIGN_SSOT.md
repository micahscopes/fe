# Fe web and Rollcall campaign: gate-anchored status

Status: authoritative campaign burn-down

Updated: 2026-08-17

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
  widths, range violations, and output overflow, and commits up to fourteen
  payloads in one typed Poseidon2 call whose first two lanes bind the nominal
  domain and exact bit length. The independent bigint/Plonky3 gate covers 128
  randomized schemas, every bit of its directed mutation case, trailing-zero
  length ambiguity, and the complete 411-bit capacity. The production
  Mandelbrot row, public claim, and Fe-derived auxiliary schema now interpret
  through that same utility as 7, 4, and 14 BabyBear fields. A distinct
  zero-import gate independently reconstructs all three encodings, mutates all
  210 main-row source bits and all 203 auxiliary source bits, checks maximum
  values, and proves range violations fail closed. Commits `034a3ac78`,
  `d3b1f1fbe`, and `6fc7dbea2`; gates `poseidon_baby_bear_oracle.rs` and
  `mandelbrot_baby_bear_encoding_oracle.rs`. The same permutation has also
  lowered to Naga-valid u32-only WGSL with the local Sonatina conditional-loop
  structurizer fixes, but that browser gate is not landed until those commits
  are published and the Fe dependency pin advances. The complete protocol
  retarget remains pending.
- [ ] Make the production claim a chunked recursive high-precision recurrence,
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
- [ ] Interpret the BabyBear proof dependency plan through the Conal/CTFE
  WebGPU scheduler for NTT/LDE, AIR composition, Poseidon/Merkle, and FRI.
  This begins by consolidating two existing, independently gated NTT strands:
  `ntt_schedule.fe` derives the `RBin<Pair, k>` stage tree and
  `BarrierReq<k>` from one type-level depth, while `ntt_par_exec.fe` executes
  the corresponding explicit fork/barrier schedule under revm and checks it
  against the sequential transform. The production field-generic
  `precision::polynomial::radix2_ntt` is not yet an interpretation of that
  derived schedule, and no workgroup/shared-memory WebGPU implementation has
  executed. The work is consolidation plus a new backend interpreter, not a
  fresh Conal NTT design.
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
2. Retarget the protocol to BabyBear with independent exactness gates, then
   derive the multi-limb chunk and recursive-accumulator statement. Do not port
   BN254 Fr to WGSL.
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
