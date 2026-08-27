# Fe web and Rollcall campaign: gate-anchored status

Status: authoritative campaign burn-down

Updated: 2026-08-26

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

## Intent reconciliation, 2026-08-26

This ordering was rechecked against the complete archived session
`e806786e-dff4-43c1-b25f-849ba82a8a02`, its 1,333 prompt-side history
entries, the original `GOAL.md`, `VISION.md`, actor and Conal design records,
the later browser-proof plans, and the current proof and browser evidence. The
review changes sequencing, not the goal or its gates.

The recovered invariant is one ordinary typed Fe denotation, followed by an
exact semantic structure, several analysis and placement interpretations, and
one fixed standards-derived realization. Semantic topology and placement
topology remain distinct. A placement may change dispatches, workgroups,
buffers, retention, and recomputation, but it may not change transcript order,
hash domains, recursive interval order, receipt layout, or the authored
mathematics. JavaScript, Rust, Chrome, Vulkan, and revm may realize standards,
compile artifacts, execute artifacts, or serve as independent oracles. They
may not own application policy or a second implementation of the proof.

The operating method remains `write -> derive -> prove -> place -> run ->
measure`. Each increment crosses that whole line at deliberately small depth.
This replaces both subsystem-at-a-time completion and browser-only showcase
forks. Byte identity and successful compilation are evidence about artifacts,
not semantic correctness; independent value and mutation gates remain the G2
authority.

Two older sequencing assumptions are now retired. Browser revm was originally
placed first because it was the only unknown infrastructure risk; S0 is now a
real Chrome-tested generic `fe-revm-browser` engine. BabyBear retargeting and a
first real WebGPU proof placement are also complete. The current unknown is
compact, scalable interpretation of the same BabyBear proof dependency graph
across scalar and WebGPU placements.

The attempted policy-sized scalar path supplied useful negative evidence. The
114-query typed policy, receipt carrier, codec, prover/verifier boundaries, and
mutation surface source-check cleanly, but lowering the selected exact prover
entry produced no artifact or diagnostic after more than two hours and was
stopped. This is a compile-practicality failure of fully static expansion, not
a cryptographic rejection. Preserve the typed policy and receipt model, but
make query iteration and opening verification value-level and loop-compact.
Do not respond by baking generated queries, copying a second prover, or
weakening the 114-query policy.

Real Chrome WebGPU is the early acceptance rail. The existing
`chrome-devtools-mcp` configuration targets the external browser at
`http://10.0.0.1:9222`; use that path whenever the endpoint is live. Native
`wgpu`/Vulkan and llvmpipe remain fast exactness and portability gates, but do
not replace Chrome hardware evidence. No second GPU broker is needed unless
the existing fixed browser harness proves insufficient.

## Campaign gates

- G1, anti-bake: a new consumer works without compiler/runtime demo names.
- G2, semantic exactness: an independent model checks values and behavior, not
  just bytes, hashes, or successful compilation.
- G3, quality: performance and generated-kernel claims use measured receipts.
- G4, no scaffolding: application policy is Fe-authored; JavaScript and Rust
  remain fixed standards adapters, toolchain code, or independent oracles.
- G5, boundary: the exact repository CI command is green:
  `cargo nextest run --release --workspace --all-features --no-fail-fast --locked --exclude fe-language-server --exclude fe-bench`.

The completed gallery and architecture audits refine the remaining proof and
inspection work into six convergence gates. These are subdivisions of the
existing Definition of done, not a second campaign checklist:

- [~] **G-NTT:** one exact factor tree drives scalar and portable stage-grid
  WebGPU NTT/LDE interpretations. DIT, DIF, Bush where legal, and at least one
  irregular factorization retain independent value gates. Real browser WebGPU
  parity is required. A disposable API probe using the existing release
  compiler identified as `fe 26.2.0 (4447fdd27)` established that
  `RBin<Pair, 3>` normalization itself is cheap relative to loading `std`, while
  a recursive scalar interpreter remains practical. Timings were 33.22 seconds
  for a plain `std::conal` import, 26.84 seconds for normalization, 42.05
  seconds when the interpreter derived depth in the same traversal, and 55.32
  seconds when it also solved a separate recursive
  `BalancedBinarySchedule` obligation. The debug baseline had not completed
  after 378.15 seconds and was stopped. Therefore the public design should use
  one product interpretation for execution plus analysis, and release compile
  budgets must gate it. These timings are compiler-cost evidence, not semantic
  correctness evidence. Future type-function, FCO, provider, and graph-
  interpreter probes report both the user-facing release path and a bounded
  debug/developer path when appropriate. Ordinary demo edits do not pay the
  current multi-minute debug baseline merely to duplicate a release gate.
  Heavy semantic proof oracles use a different policy: the
  `mandelbrot_recursive_fixed_oracle` target is selected only by the
  `expensive-release-oracles` feature and rejects debug compilation. G5's
  release/all-features command includes it, while ordinary debug iteration
  does not compile the target at all. The reusable scalar and portable WebGPU
  baseline for this gate is now implemented in the focused
  `parallel_structure`, `parallel_ntt`, and `parallel_ntt_webgpu` ingots at
  commits `3a6a7f47f`, `051248e18`, and `9e356546a`. The factor policies reuse
  the canonical `std::conal::{Par, Pair, Comp}` constructors, with `Unit` as a
  factor-language name for `Par`; `Dit<4>` is definitionally the existing
  `RBin<Pair, 4>`, rather than a numerically similar parallel type. Recursive
  Fe type functions derive `Dit`, `Dif`, and composition-balanced `Bush` trees;
  explicit `Comp` nesting admits irregular factorizations. One `FactorTree`
  interpretation derives points, factors, butterflies, pair exchanges,
  dependency depth, association depth, composition nodes, fanout, and live
  values. A scalar interpreter and a portable `StageGrid` interpreter consume
  the same constructors. The WebGPU interpretation gives each butterfly lane
  its own progress cursor, so repeated actor dispatches advance exact widths
  `2, 4, 8, 16` without a racy host or shared cursor. The zero-import
  `parallel_structure_oracle` pins DIT, DIF, Bush, irregular, and Conal `RBin`
  normal forms. The zero-import scalar oracle checks all four 16-point
  policies, base-field NTT/INTT and coset LDE, and quartic-extension NTT/LDE
  against direct polynomial evaluation. A separate Rust `wgpu` oracle compiles
  compiler-derived browser-profile WGSL, executes forward and inverse N=16
  transforms on llvmpipe, and checks direct-DFT values, canonical round trips,
  stage receipts, progress, validity, and trap buffers across three vector
  families. The emitted prepare, forward, and inverse shaders are 4,302,
  38,868, and 41,987 bytes and require no workgroup shared memory or barriers.
  Structural and stage receipts are analysis evidence only; the direct DFT is
  the independent correctness oracle. Commit `c8288f5f6` migrated the
  production `mandelbrot_proof_gpu` toy checkpoint from a whole-transform
  scalar LDE call to the same typed stage-grid vocabulary. Four contiguous
  trace columns now execute prepare, two inverse stages, coset lift, four
  forward stages, and finish as Fe-authored WebGPU passes over private progress
  cursors. A focused llvmpipe gate checks the exact `1, 2, 1, 4, 1` repetition
  schedule, all 40 progress words, four coset predicates, trap freedom, and all
  64 LDE values against the independent direct DFT. The complete graph retains
  its independent Plonky3 Poseidon gate and clean/tampered mutation behavior.
  Larger production domains, fused shared-memory or subgroup placements, and
  real Chrome hardware parity for this staged path remain open, so G-NTT stays
  partial.
- [~] **G-LAYOUT:** compiler/FCO-derived typed regions replace application
  proof-tape offsets, and an independent decoder checks every region and width.
  Commit `143e0b799` establishes the reusable `region_layout` ingot and the
  provider-privacy compiler floor it needs. An ordinary Fe record of
  `Region<T>` fields now derives declaration-order offsets and total canonical
  width from `CanonicalWords`; its physical coordinates remain private and its
  checked relative accessors fail closed. Release gates prove the derived
  layout against an independent Wasm decoder, reject direct field and private-
  constructor forging plus cross-payload confusion, validate emitted
  browser-profile WGSL, and execute typed stores on llvmpipe without allowing
  an out-of-region write to reach the next region. The production Mandelbrot
  proof tape still uses application constants, so migration and a full
  receipt-wide independent decoder remain before this gate can close.
- [x] **G-RECEIPT:** the complete nonrecursive BabyBear receipt accepts cleanly
  and rejects claim, domain, transcript, query, path, and encoding mutations.
  The first four-query process-isolated exact gate compiled a 23,636,877-byte
  zero-import Fe prover, executed that exact persisted Wasm in a fresh process,
  and emitted a 47,552-byte canonical receipt. It then compiled a fresh
  10,129,225-byte zero-import Fe verifier and executed that artifact in a fourth
  process over only the copied receipt bytes. The clean receipt accepted. A
  test-only Fe structural interpreter then mutates the typed decoded carrier,
  without host-owned offsets: the base root and transcript chain, an
  authenticated AIR value, its Merkle sibling, a composition opening, and the
  recursively located terminal FRI evaluation all reject. A raw canonical
  validity-word mutation also rejects. The complete gate passes 1/1 in
  1,891.64 seconds; prover execution takes 258.37 seconds after its compiler
  arena exits, and verifier execution takes 10.74 seconds after its compiler
  arena exits. Claim, domain, sampled-query, and malformed-encoding coverage
  remains independently exercised by the focused layer gates.

  Fe commit `9c0d8306e` makes policy-sized Merkle opening storage an ordinary
  typed browser-arena placement of the same ordered dependency graph. The
  separate CTFE-derived 100-bit policy now crosses the assembled boundary with
  114 authenticated queries. A fresh 15,397,939-byte zero-import
  Fe prover compiled in 2,109.22 seconds, executed in a separate process in
  539.32 seconds, and emitted a canonical 948,808-byte receipt. Its Wasm and
  receipt SHA-256 digests are respectively
  `90dc23f002dac6b80ea595dc1247c90c737ba853e1b89baa216504239d71da06`
  and
  `c789a067f63b4ab73d8a4c0b36932e4252b6270b0be3e17cc5d5c27980be3ceb`.
  A separately compiled 14,636,637-byte zero-import Fe verifier with SHA-256
  `7c3c0842615854dccda6a69d6a6afc2a0028e1a823283d0f6abb80465e821423`
  accepted only the copied receipt bytes and rejected mutations to the base
  root, authenticated AIR value, Merkle sibling, composition opening, and
  terminal FRI evaluation in 21.05 seconds. The prover compiler peaked at
  13,288,420 KiB RSS and the prover runtime at 1,740,228 KiB. This closes the
  scalar assembled-receipt gate. It does not constitute recursive
  cryptography, WebGPU proof generation, or evidence that this one-iteration
  verifier is cheaper than direct orbit replay.
- [~] **G-RECURSE:** the field-neutral multi-limb carrier already merges only
  ordered adjacent iteration intervals with identical statements and shared
  boundary commitments. `VerifiedRecursiveInterval<L>` now makes the
  proof-backed layer nominally unforgeable: its carrier is private, and only
  the production sparse receipt verifier or an ordered merge of two verified
  authorities can mint a valid value. A negative Fe gate rejects direct
  construction. A process-isolated exact gate against the committed dependency
  snapshot generated a 46,656-byte canonical production receipt, admitted its
  exact `0 -> 1` interval, and rejected a mutated validity word. This is an
  in-memory trusted relation, not recursive cryptography. A second exact gate
  uses one leaf-indexed Fe prover to emit real `0 -> 1` and `1 -> 2` receipts of
  46,656 and 46,808 bytes. A fresh Fe verifier admits both, mints their private
  authorities, and merges exactly two leaves over `0 -> 2`; duplicate-left,
  swapped-order, and mutated-right inputs all fail closed. The fixed-size
  recursive proof/provider and encoding, logarithmic aggregation evidence, and
  bounded disk claim remain.
- [ ] **G-BROWSER:** the Fe resident region picker proves through WebGPU and
  verifies through both Fe-Wasm and revm-Wasm with cancellation, backpressure,
  mutation, timing, and device-loss evidence. A click selects one private
  parameter and therefore a zero-radius public disk; optional dragging expands
  the public disk around that witness. The user selects one of two existential
  predicates: the hidden point's critical orbit survives through public bound
  `N`, or it escapes by `N`. The receipt binds the disk, bound, and predicate,
  while the point and orbit may remain private. Escape is a definitive
  non-membership certificate; survival is only a finite bounded-prefix claim
  and must not be relabeled as full membership or convergence. A later
  attracting-fixed-point mode may instead prove a rigorous invariant-
  neighborhood and contraction certificate. The fixed browser adapter reports
  raw pointer facts and realizes Fe-requested capture; the Fe actor owns the
  down/drag/up state machine, click-versus-drag threshold, center and radius
  calculation, preview, cancellation, predicate choice, and proof scheduling.
- [ ] **G-INSPECT:** the Fe SourceInspector presents authored, semantic,
  analysis, placement, ABI/layout, artifact, and evidence views from one
  content-addressed `SourceAtlas`, with no gallery `docs.json` or runtime render
  manifest.

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
- [~] Compiler-derived rich values now cross typed structured-child mailboxes.
  Attach opaque ports through Fe-owned spawn/Worker placement and carry those
  canonical values through the general `MessagePort` source next.
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
  Variants containing bytes, strings, or lists remained fail-closed at this
  checkpoint. The 16-test canonical-interface suite,
  ten-test resident-actor suite, four-test DEC suite, 53-test fixed browser and
  codec suite, and focused scoped-task publication verifier pass. The canonical
  interface guide now documents nominal role selection, typed mailbox edges,
  private generated transport names, fixed resident exports, the remaining
  spelling-based render compatibility paths, and the possible future omission
  of semantically unnecessary behavior names.
  Descriptor-bearing structured-child values are now landed as the next
  canonical bridge. `BrowserString`, `BrowserBytes`, and bounded
  `BrowserList<T, N>` derive two Wasm lanes from nominal semantic metadata,
  requests are copied out of the parent Wasm memory into owned transferable
  values, and response sessions allocate through the parent's checked
  canonical stack. Generated post-return release runs only after the exact Fe
  continuation consumes the response. The Wasm compiler admits segment-local
  arena rewind only when semantic descriptor recursion, exact suspension
  liveness, pending-token SSA lineage, a closed pure-callee graph, and the
  nominal `AskBegin` edge jointly prove that no borrowed address escapes.
  Private helpers which return a rich `TaskOutcome` remain ineligible, while
  the public consuming task rewinds entry and continuation segments to their
  exact incoming cursors. The executed Fe parent/child gate carries UTF-8,
  bytes, and a bounded u32 list, returns receipt `533`, and finishes with the
  canonical allocator at baseline and zero completion tokens. The release
  evidence is 13/13 resident actors, 2/2 Worker mailbox cases, the generated
  rich WebIDL continuation, 9/9 canonical-interface cases, and 37/37 fixed
  completion-broker cases. No request field schema, response-width table,
  actor selector, or runtime manifest was added.
  Rich values may now remain live through a second suspension without retaining
  canonical Wasm storage. Semantic `#[host_type]` identity makes the compiler
  emit a borrowed descriptor lane with derived stride, alignment, and maximum
  length. The fixed task adapter copies the returned bytes synchronously into
  private owned storage, while Sonatina commit `48b3ba7e` supplies an opt-in
  opaque checkpoint/rewind pair for this generated protocol. Each start or
  resume checkpoints after live input allocations, captures a returned rich
  frame, rewinds the segment-local suffix, and then releases any re-lowered
  input through checked post-return. No pointer, Wasm allocation, field schema,
  or JSON survives the await. Fixed mechanics reject missing cleanup authority,
  invalid lengths, null, misaligned, and out-of-bounds descriptors. The compiled
  release gate runs two different UTF-8 values concurrently through a later
  timer suspension, then cancels a third task during that later suspension;
  values remain exact, broker tokens return to zero, and the allocator returns
  to its byte-exact baseline. The 9/9 task-machine and 37/37 completion-broker
  suites are green. General owned rich task results remain open.
  Recursive nominal scopes now compose through arbitrary acyclic package
  depth. The compiler follows each child actor's own `ScopedTask` roots,
  recursively derives its nominal children, materializes the nested package
  tree, and rejects a repeated nominal actor in one supervision ancestry path.
  Each nested Worker receives a closed canonical actor Wasm plus a separate Fe
  continuation Wasm. This preserves the actor caller's resettable request arena
  without invalidating suspended host values held by the task module's checked
  canonical stack. No task manifest, actor selector, or authored route was
  added. The fixed Worker host derives scope and mailbox imports from the task
  Wasm, starts only compiler-published self-less task machines, binds their
  lifetime to the Worker, and surfaces unexpected task failure to ordinary
  Worker supervision. A real Bun package gate executes Parent -> Middle ->
  Leaf supervision and a typed nested request, observes stable epoch zero, then
  cancels with no leaked completion tokens. The complete 12-test
  `resident_actor` release suite passes, including the sibling regression and
  nominal-cycle rejection. Wasm preparation also now reifies aggregate
  constants in synthesized task entry and continuation bodies, matching the
  ordinary authored-body path instead of leaking target-specific `ConstRef`
  handles into portable lowering.
  Remaining: attach opaque ports, carry rich canonical values through that
  `MessagePort` placement, and run the render-owned DEC task path in real
  Chromium.
  Recursive resumable SCCs must remain explicitly refused until linked affine
  frames are sound.

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
- [ ] Evolve SourceInspector into a rich Fe-owned source explorer without
  discarding the existing fe-web component experience. Preserve syntax
  highlighting, semantic links, fuzzy search, symbol outlines, documentation
  and signatures, definitions/references/implementors, navigation history,
  deep links, keyboard/focus behavior, and authored-source/WGSL/Wasm/generated
  provenance views. The resident Fe actor must own query interpretation,
  ranking, routing, selection, cross-link, outline, and explanation policy.
  The fixed host may realize DOM ranges, scrolling, history, and raw standards
  events, but must not acquire source-navigation policy. Feed the component a
  typed, content-addressed compiler `SourceAtlas` or equivalent binary value,
  never a caller-authored JSON manifest. First harden deterministic multi-file
  semantic indexing and add real browser gates, then use this application to
  drive ergonomic owned resident text, scoped browser handles, and rich-code
  patch operations in Fe.
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
  baseline to beat. The 2026-08-24 compile-parallelism audit fixes the order of
  work: add check-compatible phase and Salsa counters, remove repeated HIR,
  voucher, dependency-digest, and duplicate gallery compile work, then pilot
  bounded read-only concurrency. Use Salsa forks only outside tracked queries;
  keep DOM planning/publication serial; begin with two owned-database web or
  codegen workers and at most four analysis-only workers; retain a deterministic
  `jobs = 1` reference. Gates require identical sorted diagnostics, artifacts,
  proof words, roots, transcripts, challenges, codewords, and mutation matrices,
  plus measured release median/p95 and peak RSS. The full evidence and five
  landable slices are in
  `/workspace/scratch/mb2-fe-compile-parallelism-audit-2026-08-24.md`.

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
  The authenticated sparse LDE carrier is now implemented and independently
  gated. One role-branded `MerkleMultiOpening` representation in the narrow
  `merkle_receipt` ingot supplies canonical counted values plus the established
  `MerkleMultiPath`; arithmetic-only `merkle_core` consumers do not inherit the
  generated receipt codec. `LD01` and `LD02` writers use the exact shared leaf
  constructors used by their root commitments, normalize unsorted duplicate
  requests through `merkle_core`, require the reconstructed root to equal the
  transcript-bound root, and retain only requested evaluations. Verifiers
  reject nonzero unused capacity and reconstruct the dependency-bound `LD02`
  leaves from the exact retained `LD01` root. A typed row adapter recovers one
  `SparseOpenedAirRow` only when both authenticated paths contain the same
  index. The zero-import Fe/Wasm gate matches independent Plonky3 `LD01` and
  `LD02` roots and selected LDE values, then rejects ten value, sibling, index,
  unused-capacity, dependency-root, and missing-row mutations. It passes 1/1
  in 838.33 seconds. A separate small carrier-codec gate proves canonical
  roundtrip, semantic authentication, truncation and trailing-data rejection,
  and authenticated value-mutation rejection in 2.80 seconds.
  A fresh uncached rerun also exposed that the older all-in-one toy receipt
  fixture now emits a single Wasm function larger than wasmparser's 7,654,321
  byte limit. Removing the new carrier from that fixture's dependency graph
  leaves the same failure and byte offset, so this is a pre-existing
  whole-receipt generated-decoder cost, not an `LD01`/`LD02` semantic
  regression. The validator limit was not weakened. Production receipt
  encoding must use staged canonical decoders rather than one monolithic
  generated function.
  `SL03` and `SI01` remain exact semantic checkpoints rather than production
  proof roots. `LD01`, `LD02`, and the shared `BC02` composition root cover the
  production codewords, including the fixed-position public relation.
  Commits `e3108816e` and `1e39c20d0` derive the production four-query carrier
  and every capacity from `FriSchedule<1, 13>` rather than maintaining a second
  receipt-shape table. Commit `8b7d2a41c` adds an independently executed
  request-set predicate that rejects missing, extra, out-of-domain, and
  noncanonical unused leaves. Commit `fce3a5d7e` applies that predicate to
  every schedule-derived FRI multipath. The authoritative zero-import
  Fe/Wasm plus independent Plonky3/BigInt integration gate passes 1/1 after
  that hardening, rejecting its established receipt, transcript, opening,
  path, root, terminal, and coset-shift mutations. This run took 2,606.02
  seconds and peaked near 7.2 GiB RSS. The semantic result is green, while the
  whole-ingot compilation cost requires a decomposed gate and compiler-cost
  follow-up rather than normalization as an acceptable edit loop. Commit
  `29916442a` assembles the production sparse receipt entirely in Fe: it binds
  the interval statement,
  `LD01`, `LD02`, and `BC02` roots; folds the complete FRI schedule; samples
  queries only after the terminal transcript; opens the exact AIR,
  composition, and FRI request sets; recomputes the positive and negative
  public-bound `BC02` values from authenticated sparse rows; and verifies all
  four schedule-derived fold chains.
  The exact compiler gate for the real `L = 4`, `TRACE = 4096`, `LDE = 8192`
  specialization passes. This is not yet an independently executed complete
  receipt gate, a succinct recursive leaf proof, or browser proof evidence.
  Its current four-query plan freezes and tests protocol structure only. It
  does not claim a target security level. Before recursive proving, an audited
  Fe policy must derive the query count and related capacities from explicit
  soundness parameters, with an independent calculation gate; four must not
  survive merely as a convenient checkpoint constant.
  Commit `9e3214661` adds the staged production canonical boundary. A generic
  `decode_canonical_words` dual complements the existing fixed-width encoder,
  while the receipt splits header, sparse AIR openings, and FRI into explicit
  Fe-owned codec stages without changing the FCO-derived field order. Browser
  and native hosts receive only `BrowserBytes`; they own no receipt field table
  or decode sequence. The zero-import Wasm gate roundtrips the canonical empty
  production carrier and rejects an invalid boolean, truncation, and trailing
  data, resetting every failure to the Fe-derived empty value. It passes 1/1
  in 124.48 seconds. This proves the bounded staged codec and malformed-input
  floor, not acceptance of a semantically valid production proof.
  The next gate must construct one canonical production receipt, accept it,
  reject targeted transcript, opening, and fold mutations against independent
  models, and feed it through staged canonical decoding. Only that executed
  AIR/FRI receipt may replace semantic replay in the recursive parent carrier.
  The production boundary is now split into separate prover and verifier
  artifacts. The prover writes canonical receipt bytes without invoking the
  verifier internally; the exact Rust gate copies only those bytes into a
  fresh zero-import Fe/Wasm verifier instance, then requires acceptance and a
  targeted mutation rejection. The prover ingot Fe-checks, and the split gate
  builds with `--no-run`, but its first exact execution exposed a compile-cost
  blocker before Wasm emission. The measured prover-only preflight visited
  19,589 semantic specializations, assembled 10,857 runtime functions and 103
  constant regions, and held about 9.7 GiB RSS. It was stopped after roughly
  52 minutes while preparing inline value bodies, without a semantic or
  lowering error. This is materially smaller than the earlier combined
  prover/verifier attempt near 14.8 GiB, so the split is retained, but it is
  not acceptance evidence. Opt-in phase timing identified repeated expanded
  normalization in `prove_production_sparse_interval`, the receipt writer and
  encoder layers, `clear_production_receipt`, and the specialized FRI fold and
  query helpers. The next production-proof slice must express the receipt
  codec and FRI/prover dependency work as compact Fe interpreters over
  CTFE-derived plans, preserve every independent bigint and mutation oracle,
  and show a material reduction in specialization count, package size, peak
  memory, and compile latency before recursion is layered on top.
  Commits `ac3d009ea` and `af2439257` bound whole-package analysis to the
  selected entry graph and cache every base inline body. A fresh exact split
  prover run then completed semantic preflight for 19,525 specializations,
  assembled 10,793 functions and 103 constant regions, and completed inline
  preparation within one 30-second polling interval. It reached portable Wasm
  lowering after 2,642.14 seconds instead of stalling in inline preparation.
  The remaining cost is concentrated in the expanded production schedule:
  `fold_fri_first` took 332.35 seconds,
  `open_fri_multi_query_layers__gd7ee` took 322.59 seconds,
  `write_production_sparse_baby_bear_receipt` took 162.75 seconds, and the
  other FRI/query specializations contributed additional tens of seconds.
  This is direct evidence for a flat CTFE-derived FRI/query/receipt plan with
  scalar, Wasm, and WebGPU interpreters, while retaining the recursive branded
  carrier as the specification oracle.
  That run passed the earlier provider-reborrow seam and then failed closed
  when an arena-owned pointer stored in the Fe prover workspace was loaded by
  a later actor stage. Commits `fa5f29053` and `359ba8438` add field-sensitive
  typed arena provenance across a closed runtime package. A field is trusted
  only when every store to that exact nominal-layout field has an arena-owned
  root and source; opaque, dynamically indexed, public-forged, and mixed-origin
  paths remain rejected. Focused positive, reborrow, staged-actor, and forged
  address gates pass 6/6, and the Wasm-lowering unit suite passes 12/12.
  Commit `fe3d93eaf` adds a bounded exact production-stage checkpoint rather
  than a reduced surrogate. It allocates the real 4,096-row base evaluation
  and 8,192-row codeword objects, initializes them in Fe, executes the shared
  production LDE at coset shift 7, lowers 66 Fe functions to a 305,035-byte
  zero-import Wasm module, and executes successfully. The complete split
  receipt gate now also executes. Commit `002fc25c1` represents structurally
  oversized local aggregates by one compiler-owned arena address while
  retaining their Fe `AggregateValue` semantics and by-value deep copies. The
  focused 8,192-leaf composition opening dropped from 65,549 emitted Wasm
  locals to 816 and still passed its semantic authentication check. Across the
  complete prover the highest measured local count is now 8,205. Local
  Sonatina companions `1fb9968e` and `512f76aa` remove the unrelated 16 MiB
  generated-memory ceiling and add opt-in emitted-body pressure diagnostics.
  The exact split gate then passed 1/1 in 1,204.33 seconds: the zero-import
  prover retained all 19,525 semantic specializations and 10,793 runtime
  functions, emitted 13,905,487 Wasm bytes, executed, and returned a 29,672-byte
  canonical receipt. A separately compiled zero-import verifier retained
  14,720 specializations and 7,859 runtime functions, decoded those copied
  bytes, accepted the receipt, and rejected a mutated canonical header. The
  prover arena ended near 23 MiB, establishing that the prior allocator trap
  was the hard module cap rather than runaway proof allocation.
  This closes the executed production sparse receipt and canonical transport
  boundary for the current one-transition `L = 4` checkpoint. It is not yet a
  succinct recursive proof or a security claim. A subsequent process-isolated
  exact gate executes a 20,413,169-byte zero-import Fe prover and a fresh
  8,703,961-byte zero-import Fe verifier. The verifier accepts the clean
  canonical receipt, then rejects Fe-side typed mutations to the base root and
  transcript chain, an authenticated AIR value, its Merkle sibling, a
  composition opening, and the recursively located terminal FRI evaluation;
  it also rejects a raw canonical validity-word mutation. The host owns only
  receipt bytes and mutation selectors, never field offsets or a receipt
  schema. The complete gate passes 1/1 in 5,401.40 seconds, with the verifier
  child finishing in 708.27 seconds. Focused independent bigint gates remain
  the semantic truth, and claim, domain, sampled-query, and malformed-encoding
  mutations must still execute at this assembled boundary. Commits
  `a3b42c545`, `135daa0ff`, and `4a0e3240d` now derive the
  first explicit production query policy in Fe. The conservative Q16
  direct-domain policy selects 103 queries for a conjectured 100-bit leaf and
  114 queries when reserving a union-bound budget for at most 1,024 composed
  proofs. It fails closed for 2,048 proofs under the present commitment phase.
  The query transcript uses one nominal `FQ02` domain plus a canonical indexed
  field element, independently checked against Plonky3 at indices 0, 1, 99,
  100, 114, 65,537, and `p - 1`. This removes the former decimal-tag and
  99-query ceiling. A compact `FriQueryRangePlan<1, 114>` carries the selected
  plan in O(1) type structure; attempting to normalize the old 114-deep nested
  plan consumed one CPU for five minutes without completing, so large query
  plans no longer rely on that representation.
  Commit `9b47c2b40` also interprets the exact authored sparse composition fold
  as four zerofier-family constraint counts. Its zero-import Fe/Wasm gate
  passes 1/1 in 392.53 seconds, requires every family to be nonempty, requires
  their sum to equal the derived total, and confirms the `L = 4` AIR remains
  within the policy's conservative 8,192-constraint cap. This is structural
  count evidence from the production evaluator, not a replacement for the
  existing independent semantic and mutation oracles.
  Commit `f7aff8f74` now interprets that same evaluator through an
  `AirPolynomialDegree` semiring. Reflection populates every base and
  interaction trace column as a degree-one variable, transcript challenges
  remain degree-zero constants, and the ordinary evaluator sequence derives
  family-specific expression degrees without a copied field list or
  constraint body. The interpreter derives maximum expression degree 19 and,
  after the four family zerofiers, a production composition-degree bound of
  73,709. An independent Rust quotient calculation from the reported family
  degrees agrees exactly. That bound does not fit the claimed 4,096-row
  trace-degree domain. `proof_security` now requires
  a nonzero composition bound strictly below the trace domain and therefore
  fails closed instead of treating the present four-query receipt as a
  low-degree proof. The shape gate passes 1/1 in 82.39 seconds. The focused
  security gate passes 2/2 in 23.73 seconds, including primary-parameter
  validation, recomputation of every derived policy lane, mutations, and an
  exact high-word `u64` canonical-codec round trip. That round trip exposed a
  general Wasm lowering defect where address-taken scalar Slots were read as
  their private i32 pointer carriers. Commit `30303a2dc` loads their typed
  pointees in every value lane; its direct return, arithmetic, call, copy,
  branch, and f32 regression passes 1/1, and the focused lowering unit slice
  passes 12/12.
  This is an honest protocol blocker, not a reason to enlarge the domain or
  weaken the policy. Commit `aca9e388d` adds a reusable Fe-authored quadratic
  arithmetic plan with witness and constraint interpreters. Commit
  `6dcd20a70` uses it for the production product copy-address DAG and lowers the
  shared copy-bus inverse identity to an equivalent quadratic relation on the
  already-constrained boolean selector. The same 17-node product plan now
  drives witness generation and constraint evaluation. Its zero-import Wasm
  oracle rejects every committed node mutation under three independent fold
  challenges. A second independent Rust oracle reconstructs every product row
  and clean product, round, linear, and boundary receipt, rejects inverse
  mutations, and confirms that coordinated mutations survive only until the
  terminal equality check.

  Commit `b4c6f550d` derives component degrees from the exact production
  evaluators. Commit `9cf134e54` adds three semantic coordinate roles and
  removes repeated high-degree coordinate interpolation from the product,
  linear, and boundary copy buses. Commit `25d20d75a` then expresses the nested
  signed-linear finish choice as one seven-multiplication Fe plan interpreted
  into witnesses and quadratic residuals. An independent BabyBear
  reconstruction matches every committed node and rejects all seven node
  mutations under three fold challenges. The production base-LDE checkpoint
  executes the widened 51-field row through zero-import Wasm.

  Commit `eb9224f8f` interprets the complete public-boundary copy expression
  through one shared fourteen-node quadratic plan. Direct port inspection,
  witness generation, constraint evaluation, and degree analysis now consume
  that same Fe-authored expression. The independent BabyBear oracle
  reconstructs every node and requires each single-node mutation to fail
  under the independent challenge set. The exact shape gate confirms that the
  boundary component fell from degree 7 to degree 2. At that checkpoint the
  canonical base row became 65 fields wide; its assembled receipt remained to
  be rerun against the widened schema.

  Commit `421e93e2a` interprets the five rounding ports through one shared
  thirty-eight-node quadratic plan. The independent BabyBear oracle matches
  every node on guard, output-digit, current-sign, round-consumer, and
  finish-consumer rows, then rejects all thirty-eight single-node mutations.
  The production 4,096-row base placement and 8,192-row LDE execute through
  zero-import Wasm with the widened 103-field base row. That exact production
  gate passes in 768.09 seconds.

  Commit `adc203410` interprets all eight signed-linear copy ports through one
  shared fifty-two-node quadratic plan. Witness generation, row/link/terminal
  evaluation, direct port inspection, and degree analysis consume that same
  Fe-authored expression. It reuses the already committed effective-right and
  different-sign arithmetic nodes rather than deriving a second sign-choice
  graph. An independent BabyBear reconstruction matches all fifty-two nodes
  across every semantic signed-linear row class and rejects every single-node
  mutation under five independent fold challenges. That focused gate passes
  in 327.92 seconds. The exact shape gate confirms that the linear all-row
  component fell from degree 7 to degree 2 and that its terminal contribution
  fell from degree 6 to degree 2. The widened 155-field base row then executes
  the full production 4,096-row placement and 8,192-row LDE through
  zero-import 395,467-byte Wasm in 1,019.47 seconds.

  Commit `7001de5b7` linearizes the terminal balance shared by all four copy
  buses. Pair-row constraints already accumulate every preceding row, while
  the independent control terminal and ordinary all-row constraints require
  the final row to be canonical padding with zero port coefficients. The
  terminal therefore checks the stored prefix accumulator directly instead of
  recomputing a quadratic zero delta. A focused zero-import Fe/Wasm gate proves
  all four padding-row deltas are zero and separately mutates the product,
  round, linear, and boundary accumulators; every mutation fails exactly one
  linear terminal equation. It passes in 620.55 seconds. The exact shape gate
  passes in 693.76 seconds and confirms that the last-row family fell from
  degree 2 to degree 1. A broad scalar receipt run independently reproduced the
  product and round receipts before exposing stale pre-plan constraint-count
  expectations; those counts now include every committed plan residual, while
  the complete broad rerun remains part of final receipt revalidation.

  Commit `3d9429b7b` extends the arithmetic plan from its seven-node signed
  finish graph into one shared twenty-three-node row-arithmetic DAG. It covers
  bitness, radix reconstruction, carry reconstruction, convolution products,
  product-sign selection, signed-linear magnitude selection, and the final
  sign choice. The plan shares the radix `state_before * value` product with
  signed-linear selection, expresses difference selection as one affine
  adjustment, and reuses the accumulator bit-square in the finish balance.
  An independent BabyBear reconstruction matches all twenty-three nodes and
  rejects every single-node mutation; that focused gate passes in 116.70
  seconds. The exact structural interpreter passes in 323.25 seconds and
  proves the arithmetic all-row component fell from degree 4 to degree 2. The
  widened 171-field base row then executes full production 4,096-row placement
  and 8,192-row LDE through zero-import 395,467-byte Wasm in 690.37 seconds.

  Commit `b974ed27e` expresses outgoing arithmetic adjacency as a second shared
  eighteen-node DAG. It materializes phase-link selectors, carry reset and
  retained state, product and rounding handoffs, and all three signed-linear
  ripple links. An independent BabyBear oracle scans the actual production
  schedule to prove every node has a nonzero semantic case, matches all nodes,
  and rejects each single-node mutation under five challenges. That gate
  passes in 99.11 seconds. A new pair-component structural report executes the
  exact six production adjacency evaluators independently rather than hiding
  them behind the family maximum. It proves arithmetic and every copy-bus
  adjacency are degree 2, while control adjacency is degree 5. The exact shape
  gate passes in 439.40 seconds. The widened 189-field base row then executes
  the production 4,096-row placement and 8,192-row LDE through zero-import
  395,467-byte Wasm in 658.72 seconds.

  The local-control increment now expresses the three coordinate-domain
  polynomials through one shared three-node quadratic DAG. Witness generation,
  constraint evaluation, degree analysis, and the committed base-row schema
  consume that same Fe plan. The independent BabyBear oracle reconstructs all
  three nodes on every one of the 4,096 production control rows and rejects
  every directed node mutation under three challenges. That focused gate
  passes in 184.92 seconds. The exact shape gate passes in 630.07 seconds and
  proves the final control all-row component fell from degree 3 to degree 2.

  The control-adjacency increment now expresses the remaining deterministic
  transition relation through six shared Fe plan families: 23 phase links, six
  boundary and padding links, 12 range-role links, eight carry links, nine
  product links, and ten signed-linear links. These 68 named nodes are the one
  source consumed by witness generation, constraint evaluation, degree
  interpretation, and the future GPU scheduler. An independent BabyBear model
  matches every node on all 4,095 production links, rejects each directed node
  mutation under three fold challenges, and retains the complete canonical and
  directed schedule-mutation audit. That strengthened zero-import Fe/Wasm gate
  evaluates 1,486,555 constraints and passes in 1,493.11 seconds. The exact
  structural interpreter passes in 2,094.65 seconds and proves that control
  adjacency fell from degree 5 to degree 2.

  The widened schema also received a decomposed transport audit instead of an
  impractical monolithic receipt compile. An independent oracle derives the 17
  product-address nodes that were missing from its stale 84-field interaction
  model, then matches all 5,507 carrier words in the exact Fe-produced base and
  interaction LDE codewords. The corrected nominal widths are 192 base fields
  and 152 interaction fields. That exact codeword gate passes in 1,961.53
  seconds. The production multipath gate generates each opening once, copies
  it in Fe, and applies verifier-side mutations to those copies. This preserves
  the clean and mutation semantics without regenerating the complete prover
  eleven times. Clean authentication passes while mutations of opened values,
  siblings, indices, unused capacity, the dependency-bound root, and requested
  row selection all fail closed. That unoptimized semantic baseline passes in
  2,940.80 seconds. Broad all-export compilation is no longer treated as
  useful evidence: one attempt reached 12.65 GiB after 99 minutes, and the
  older repeated-prover multipath form reached 9.8 GiB after 115 minutes.
  Focused entry lowering and independently composed gates are required for
  this slice. A separate performance fixture must opt into Sonatina's
  optimization pipeline explicitly; Cargo release mode alone does not change
  the default byte-equivalent Fe Wasm lowering.

  The exact production shape is now 691 constraints with family degrees
  `[2, 2, 1, 1]`, all-row component degrees `[2, 2, 2, 2, 2, 2]`, and pair-row
  component degrees `[2, 2, 2, 2, 2, 2]`. The maximum expression degree is 2
  and the composition-degree bound is 4,095, so the authored AIR now fits the
  4,096-row target domain. This closes the degree-reduction gate that began at
  degree 19 and bound 73,709. A fixed preprocessed control root remains
  deliberately rejected: it would add a verification-key and fixed-column
  authentication obligation, while a prover-chosen fixed root would be
  unsound. Direct shared plans keep the authored transition relation, witness,
  constraints, degree analysis, and future GPU schedule structurally
  identical.

  Exact codeword, commitment, opening, and assembled receipt revalidation is
  now the active gate. The focused toy `TRACE = 4`, `LDE = 16` production
  composition gate checks clean output plus base, interaction, and statement
  mutations against independent Plonky3-derived roots, transcript challenges,
  all sixteen composition values, and the final root. It passes 1/1 in 117.48
  seconds. During revalidation it exposed and corrected a stale Rust-oracle
  mutation at column 38; the Fe production fixture and the independent oracle
  now both direct their mutation at base-LDE column 35. The separate typed
  LD01/LD02 multipath gate passes 1/1 in 54.96 seconds, including clean row
  reconstruction and twelve clean, directed, or unknown mutation outcomes.
  The former broad test redundantly regenerated a 4,096-leaf legacy trace for
  each opened row and was stopped after exceeding 6.5 GiB RSS. Its distinct
  LDE and opening semantics remain in focused gates; the production mutation
  gate now consumes the one receipt that already carries all sixteen
  numerators.

  The four-query production regression now binds every interpreted security
  parameter into the transcript before composition. `SparseDirectFriTranscriptProfile`
  contains the derived direct-FRI policy, the exact 691-constraint AIR shape,
  its degree families and composition bound, plus the query count actually
  executed by the receipt. Its reflection-derived 44 canonical words are
  injectively packed into 47 BabyBear fields and committed under `SP01`; the
  canonical statement-and-root AIR transcript is extended under `SP02`.
  Prover and verifier consume a private-field
  `SparseCompositionTranscriptDigest`, so an extended challenge digest cannot
  masquerade as the canonical base AIR transcript. The composition writer
  rederives both codeword roots and requires exact retained-transcript equality
  before evaluating internal or fixed-position public constraints. Independent
  Plonky3 packing/Poseidon tests, policy arithmetic, exact AIR-shape checks,
  and six malformed-profile mutations pass 6/6. The first complete run exposed
  a real stale-challenge bug because composition had been generated before the
  profile extension; the nominal two-level transcript repair closes it. The
  fresh exact production gate emits and accepts the 47,552-byte receipt and
  retains the typed mutation matrix.

  To keep this evidence viable on the 19 GiB machine, the exact gate now uses
  four process legs: compile prover, execute the persisted validated Wasm,
  compile verifier, execute the persisted validated verifier. Compiler arenas
  therefore never overlap Wasmtime JIT or Fe proof state. The first combined
  attempt crossed below the mandated 3 GiB available-memory floor after
  emitting the same prover module and was stopped. The isolated rerun kept the
  artifact and proof semantics identical and passed 1/1 in 1,891.64 seconds.

  The staged canonical boundary no longer traverses the complete receipt once
  to count words before traversing it again to encode them. A rejected
  `CanonicalWordStream::MAX_WORDS` experiment preserved the nominal schema but
  moved the same recursive cost into associated-const analysis; it was not
  retained. `BrowserWordWriter::growing` instead starts with a small
  performance-only region, doubles through Fe-authored allocation and copy
  operations, fails closed at the checked browser-memory ceiling, and exposes
  only the exact emitted prefix. Rust and JavaScript know neither the receipt
  schema nor its capacity. The ordinary reflection-derived stream remains the
  sole field-order authority.

  The release production-codec gate completes diagnostics in 23.71 seconds,
  emits a 915,084-byte zero-import Wasm module in 195.92 seconds, and passes
  clean roundtrip plus invalid-boolean, truncation, and trailing-data rejection
  in 221.22 seconds overall. A separate canonical-words oracle forces 257
  writes through three growth boundaries, checks every copied word, rejects an
  out-of-policy request, and rejects an oversized allocation; it passes in
  58.19 seconds. The same investigation hardened CTFE failures to retain a
  compact body-diagnostic class and the full instantiated callee chain rather
  than an opaque salsa identifier. The 114-query encoder now uses the same
  growing Fe writer, but its full receipt execution remains the next security
  gate.

  The assembled regression receipt still carries four queries. The compact
  114-query range must subsequently drive real sampling, openings,
  authentication paths, and FRI folds before a separate security-sized
  receipt is claimed. The direct-domain policy remains explicitly conjectured
  rather than a DEEP-ALI soundness claim. The Sonatina companions also need a
  published revision and normal Fe dependency pin before this gate is
  reproducible without the local path patch.
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
  The first browser placement checkpoint is now landed in
  `mandelbrot_proof_gpu`. Commits `78b73c0b9`, `7915e22c5`, and `6173d0b3d`
  derive typed compute invocation and repeated dispatch from Fe actor passes.
  Commit `1b894cf17` interprets the four-column LDE and Poseidon2 commitment
  schedule as six Fe-authored GPU passes, including 396 fixed repeated
  commitment steps over 32 lanes. The proof actor derives the canonical 141
  Poseidon2 parameters once on the GPU from an 80-bit Grain state represented
  by three `u32` words, then stores their Montgomery forms in the typed proof
  tape. There is no application constant table or host-side parameter shim.
  The runtime parameter stream and permutation match every independent
  Plonky3 value. Exact LDE and repeated-dispatch execution gates pass on
  llvmpipe, the actor compilation gate passes, and the largest commitment
  shader contracted from 213 KiB to 150 KiB. The complete graph reached
  pipeline compilation on llvmpipe but did not finish within eight minutes,
  so that attempt was stopped and is not execution evidence.
  Sonatina commit `c6659dd1` supplies the structured shared-control-flow fix
  required by the generated proof shaders. With that local backend revision,
  the release compiler and canonical gallery cold build are green: 13 render
  bundles, three component projections, and 92 assets. The browser card is a
  placement and commitment checkpoint, not yet a recursive succinct proof.
  Hardware Chromium then exposed a second general backend defect: every
  invocation materialized the full 8,192-word private-heap capacity even when
  static allocation analysis required only a small prefix. The 32-lane
  commitment kernel therefore requested roughly 1 MiB of private heap per
  workgroup and crashed Chromium's GPU process during pipeline compilation.
  Sonatina commit `03bff3d6` retains the same fail-closed capacity ceiling but
  emits only the statically required heap. Fe commit `85339db39` rejects a
  capacity-sized heap in the proof compile gate. The commitment kernel now
  uses 135 words per lane; the other proof passes use 42, 54, 179, and 178
  words. On AMD Radeon 780M through RADV, Chromium compiled the 149,767-byte
  commitment pipeline in 15.94 seconds with zero WGSL diagnostics and no
  device loss. The cold canonical gallery then loaded all 13 render surfaces
  in sequence. Clean mode rendered all four validity bands as exact RGBA
  `[87, 117, 226, 255]`; mutation mode retained trace, LDE, and nonzero-root
  validity while changing only the expected commitment verdict to exact RGBA
  `[255, 176, 222, 255]`. This is real hardware execution evidence for the
  placement checkpoint only. It does not satisfy authenticated FRI,
  recursion, interactive point selection, Wasm receipt verification, or the
  Mandelbrot verifier executed through the existing revm-in-Wasm rail.
  The first reusable scheduling baseline now consolidates that arithmetic plan
  with the existing Conal vocabulary. `parallel_structure::Dit<k>` is
  definitionally `std::conal::RBin<Pair, k>`, and scalar plus portable WebGPU
  interpreters consume the same `Par`, `Pair`, and `Comp` constructors.
  Compiler-derived N=16 forward and inverse transforms execute on llvmpipe and
  match an independent direct DFT. The production toy checkpoint now consumes
  the portable batched stage-grid interpreter for all four 4-to-16 coset LDEs,
  and its full commitment graph remains exact against the direct-DFT and
  Plonky3 oracles in clean and tampered modes. No larger-domain or fused
  workgroup/shared-memory proof transform has executed. The remaining work is
  production-sized placement and device-tuned interpreters over the shared
  plan, not a fresh NTT implementation.
- [ ] Build and run the complete recursive proof experience in the canonical
  gallery after the BabyBear prover exists. The Fe-authored component lets a
  user click a private high-precision Mandelbrot parameter, optionally drag to
  expand a public disk around it, and prove either survival through a public
  iteration bound or escape by that bound. It schedules leaf chunks
  progressively on WebGPU,
  recursively merges adjacent certified intervals, presents the typed receipt
  plus proof-size and timing evidence, and verifies it in-browser through Fe.
  The circle relation and orbit witness remain inside the same proof rather
  than being checked by host code. A separate attracting-fixed-point mode may
  later prove an invariant disk, contraction bound, and entry of the critical
  orbit, which is meaningful in a way that bare existence of a complex fixed
  point is not. The Chrome gate must exercise region selection, successful
  generation and verification, cancellation and backpressure, deliberate
  receipt mutation rejection, and console and device-loss recovery. Shader
  compilation alone cannot satisfy this gate.
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

1. Add the first post-checkpoint WebGPU proof slice at toy scale. Expose one
   ordinary Fe factor-2 FRI pair denotation from the existing BabyBear proof
   code, make the scalar fold and a portable one-invocation-per-pair placement
   consume it, and extend `mandelbrot_proof_gpu` without hand-authored WGSL.
   Gate the values against the scalar path and an independent field oracle,
   gate a mutation, then execute the exact card through real Chrome. This is
   the next `write -> derive -> prove -> place -> run -> measure` slice.
2. Extend that placement into the complete toy `16 -> 8 -> 4 -> 2 -> 1` FRI
   chain with ordered layer commitments and transcript-derived challenges.
   Keep large buffers device-resident and cross the host boundary only for
   required transcript observations and final typed receipt extraction. Record
   dispatch, readback, live-memory, shader-size, pipeline-compile, and
   device-loss evidence.
3. Compact the policy-sized scalar boundary. Retain the 114-query Fe-derived
   `FriQueryRangePlan` and typed receipt, but replace recursively expanded query
   execution with checked value-level loops over canonical request buffers.
   Re-run the four-process exact prover/verifier/mutation gate, and pin the
   required Sonatina revision only after it finishes.
   This slice is now partial. Wasm trait preflight traverses only dynamic-return
   dependencies, while all policy-sized sample, AIR-request, composition,
   Merkle, and FRI-opening paths borrow their Fe-owned carriers. A second
   schedule interpretation writes the complete `16 -> 8 -> 4 -> 2 -> 1` FRI
   chain and its sparse query layers into caller-owned typed storage. Its
   focused release gate passes 1/1 in 51.49 seconds against the established
   value interpreter, whose values already have independent Rust/Plonky3
   coverage, including invalid-shift rejection. The staged canonical codec
   gate remains green 1/1 in 233.38 seconds. A bounded detailed graph trace
   reached 3,079 runtime instances. Its dominant owners were canonical codec
   and provider operations: `write` 425 times, `encode` 249, `word_count` 164,
   `from_u32` 162, and `encode_stream` 160. Poseidon functions appeared about
   35 times each and FRI-layer functions 12 to 13 times each. Therefore the
   primary multiplication is reflected receipt encoding, not the proof
   recurrence itself.

   The first full exact retry reached a concrete Wasm lowering diagnostic after
   about 34 minutes: an ordinary Fe record containing typed `BrowserPtr`
   handles could not be materialized into compiler-owned aggregate storage.
   In aggregate layout, a `BrowserPtr<T>` field is a memory `RawAddr` branded
   with `T`'s concrete target layout. The backend now preserves that typed word,
   plus the already-admitted whole memory-provider word, as an exact `i32` lane
   in compiler-owned aggregate storage. Untyped raw addresses, object and const
   references, and non-memory transports retain their fail-closed rules. A
   semantic fixture allocates two typed Fe objects, returns their handles in an
   ordinary record, stores and reloads that record through typed memory, then
   uses both preserved handles and checks the resulting values. The complete
   focused typed-allocation and provenance suite passes 14/14 in release mode,
   including the forged-provider rejection.

   A subsequent one-hour exact 114-query retry remained CPU-active in Fe-to-
   Wasm lowering, observed about 4.8 GiB peak RSS with at least 12 GiB system
   memory available, and emitted neither a Wasm artifact nor a new diagnostic
   before the explicit timeout. This is not receipt, security, or proof-runtime
   evidence.

   The first exact retry after landing the Fe-owned growing canonical writer
   reached a definitive backend boundary. In release mode the runtime graph
   completed with 16,811 Fe specializations and 92 constant regions. Portable
   lowering completed all 16,811 functions, but Wasm validation rejected the
   module with `too many locals: locals exceed maximum` at byte offset
   14,182,530. The run took about 32 minutes to reach validation, peaked near
   13.9 GiB RSS, and retained at least 3.4 GiB available system memory. The
   trace exposed aggregate results with 166,444 and 117,196 flattened lanes,
   per-layer invalid multipaths with up to 47,883 lanes, and thirteen distinct
   `write_multi_path_and_root_from_leaves` lowerings, one taking 41.41 seconds.
   This is a representation failure, not a cryptographic or receipt mismatch.
   Preserve the typed receipt and schedule as semantic authority, but move FRI
   layers, Merkle multipaths, and canonical transport behind Fe-owned typed
   memory handles and value-level loops. Gate each compact interpreter against
   the existing typed implementation and its mutation oracles before retrying
   the exact 114-query boundary.

   A measured canonical stage-grid experiment was then rejected. The provider
   derived declaration-order routing from the same Fe schema, and both the
   cumulative-bound and linear typed-route forms passed the independent
   canonical oracle. The linear form passed in release mode after 257.65
   seconds. It nevertheless made exact compilation less tractable: a fair warm
   six-minute trace remained inside root-body lowering for graph instance 1,
   instead of making measurable progress through the former 3,079 instances.
   The experiment was reverted rather than landing semantically correct code
   with pathological compiler behavior. Do not retry that heterogeneous
   routing shape. Preserve the existing canonical Fe codec and measure exact
   runtime-body interning or streamed prepared-MIR retention next. Re-run the
   full four-process 114-query boundary only after a focused gate demonstrates
   lower graph size, wall time, or peak memory without changing codec values.

   The subsequent value-level and arena-owned receipt repair crossed the real
   scalar protocol boundary. The exact Fe prover compiled to valid zero-import
   Wasm, executed in 285.99 seconds, and emitted one canonical 948,808-byte
   receipt. A separately compiled 15,269,897-byte Fe verifier accepted that
   receipt and rejected the complete typed mutation matrix, validity-word
   mutation, truncation, and trailing bytes in 17.73 seconds. Merkle browser
   storage matches both the pure Fe interpreter and an independent Rust tree
   oracle. The caller-owned FRI writer matches its independently checked Fe
   value interpreter. These are receipt and protocol results, but they are not
   yet a completed fresh four-process rerun.

   The first fresh rerun exposed a compiler scaling regression rather than a
   proof mismatch. Its prover reached valid Wasm at 16,392,448 bytes after
   3,869.88 seconds, but late lowering grew to 13.7 GiB RSS. The run was
   interrupted at the campaign's 3 GiB system-availability floor immediately
   after validation and before the child persisted the module. Inspection
   showed that every address-carried Fe value copy had become its own Sonatina
   control-flow loop. This kept Wasm bodies valid and compact but multiplied
   the lowering graph.

   Sonatina commit `a6cb7bc6` therefore adds one generic, overlap-safe
   `Memcopy` IR instruction with precise read/write effects, parser and
   verifier coverage, and direct Wasm `memory.copy` lowering. Its overlap test
   passes. Fe's actual Wasm interpreter emits that instruction for aggregate
   value copies, while the shared shader/SPIR-V interpreter retains the
   checked portable loop. The 8,192-word address-carried value gate proves
   execution and the presence of bulk memory, and the complete typed arena and
   provenance suite passes 17/17 in 20.41 seconds.

   The exact 114-query verifier then compiled and validated at 14,636,637
   bytes in 675.65 seconds, versus 15,269,897 bytes in 1,123.70 seconds before
   bulk memory. That is about a 40 percent wall-time reduction. The module
   contains 783 `memory.copy` operations and accepts the retained real receipt
   while rejecting the full mutation matrix in 41.00 seconds. This proves the
   optimization at the production verifier boundary.

   The final compiler-memory slice consumes prepared Fe MIR bodies as they are
   lowered, retains only their call interfaces, and asks the GNU allocator to
   release completed body spans at bounded intervals. Local Sonatina commit
   `172a3489` adds an owned Wasm backend entry that consumes each Sonatina
   function body after deriving its WAFFLE body, while the borrowed and owned
   paths share validation and final emission. Borrowed-versus-owned bytes,
   export names, validation, instantiation, and mutable execution match in a
   release gate. The overlap-safe `memory.copy` release gate also passes.
   Fe's complete typed allocation and provenance suite passes 17/17 in 14.80
   seconds, and the generated resumable actor continuation gate passes 1/1 in
   9.98 seconds.

   With those changes, the fresh 114-query verifier compiled in 394.80 seconds
   at 6,566,328 KiB sampled peak RSS and retained its exact 14,636,637-byte
   artifact and SHA-256
   `7c3c0842615854dccda6a69d6a6afc2a0028e1a823283d0f6abb80465e821423`.
   The fresh prover compiled and persisted valid Wasm in 2,109.22 seconds,
   rather than the earlier interrupted 3,869.88-second attempt. It emitted
   15,397,939 bytes with SHA-256
   `90dc23f002dac6b80ea595dc1247c90c737ba853e1b89baa216504239d71da06`
   at 13,288,420 KiB sampled peak RSS, leaving 3,443,664 KiB available at the
   lowest sample. Executing that exact artifact produced the canonical
   948,808-byte receipt in 539.32 seconds. The separately compiled verifier
   accepted it and rejected the complete typed mutation matrix in 21.05
   seconds. This closes the scalar four-process 114-query boundary. The two
   local Sonatina commits still require publication and an exact Fe dependency
   pin before the coordinated compiler increment is reproducible from a clean
   checkout.

   The Sonatina refresh was also audited against the actual
   `fe-lang/sonatina` `main` at
   `8e6c99f67cf3f20b9672cab61d8655c2ff33a6a7`, not the stale `micah/main`
   tracking branch. Relative to merge base
   `039a9f530856a7c0097ffc9dad904eed3bf33fe3`, the MB2 worktree has 157 local
   commits and upstream has 40 commits, principally EVM memory placement,
   stackification, pointer escape analysis, and shared optimizer changes. A
   final-tree merge simulation found five textual conflict regions across
   `cfg_edit`, GVN, SCCP simplification, and expression simplification. Refresh
   Sonatina in an isolated worktree after the codec compaction checkpoint, run
   focused Wasm, SPIR-V, and EVM gates there, and move the Fe dependency pin
   only when the exact scalar receipt and browser proof gates remain green. Do
   not blindly rebase the live 157-commit proof substrate.
4. Carry the portable schedule to production-sized NTT/LDE, AIR composition,
   Poseidon/Merkle, FRI folding, and opening extraction. Close typed proof
   regions as each buffer enters the graph, preserve direct-DFT, Plonky3, and
   independent bigint gates, and run each widened stage in Chrome before adding
   another. Add Merkle retention/recomputation and a peak-memory policy before
   mobile-sized execution.
5. G-RECEIPT is closed at the scalar 114-query boundary. Continue through
   G-RECURSE and G-BROWSER. The current recursive carrier and
   verified-adjacent-interval authority are semantic scaffolding, not a
   recursive cryptographic proof. First bind the security-sized verifier to
   the private leaf authority, then derive fixed-size leaf and merge proof
   circuits. Schedule independent leaves and sibling merges as a typed balanced
   reduction, and run the progressive point/disk picker, cancellation,
   generation, mutation rejection, Fe-Wasm verification, and revm-Wasm
   verification in Chrome.
6. Finish the four standalone hardware tests in the external runbook. The
   `mandelbrot_proof_gpu` clean and tampered Chromium modes already pass on AMD
   Radeon 780M through RADV, but the named Rollcall, fixed precision,
   perturbation, and known-color binaries still require no-skip hardware
   receipts.
7. With multiple nominal child namespaces landed, generalize the render-owned
   DEC path to rich canonical port payloads and nested scopes, then execute it
   in real Chromium.
8. Close G-INSPECT, delete the runtime manifest, and finish the legacy
   disposition.
9. Run the exact G5 command once at the final DONE gate.

## Bonus compiler leverage, subordinate to the proof path

These are bounded side-goals, not new campaign gates. Take them when a focused
increment directly reduces a measured proof or gallery compiler cost. Do not
delay the exact scalar receipt, production WebGPU placement, recursion, or the
browser proof flow merely to complete this list.

1. Compact FCO-derived canonical codecs. Derive one typed receipt schema or
   stage grid, then interpret it with small value-level loops instead of
   monomorphizing a distinct `write`, `encode`, `encode_stream`, and
   `word_count` call chain for every reflected field. Preserve the authored Fe
   schema and every independent decode and mutation oracle.
2. Intern exact runtime bodies after substitutions and runtime
   representations are resolved. Sharing requires identical signatures,
   constants, effects, layouts, and callee bindings. Source bytes or generated
   bytes are not a correctness criterion.
3. Stream prepared MIR through dependency-ordered lowering and release bodies
   after their escape, ABI, and call-graph obligations are discharged. Record
   peak RSS as well as wall time.
4. Persist normalized semantic bodies, prepared runtime bodies, and final
   artifacts across compiler processes under compiler-version and semantic
   digests. A cache hit must be observationally identical to a clean build and
   retain the same independent gates.
5. Keep repeated dimensions such as FRI query count as checked data in one
   derived schedule where semantics permit it, rather than multiplying nominal
   programs. Transcript order, domain separation, receipt layout, and typed
   bounds remain fixed.
6. Parallelize independent body lowering only after compaction and under an
   explicit RAM budget. Faster duplication is not the objective; smaller exact
   compilation is.

7. Integrate first-class pointers as a staged semantic transplant from Sean's
   pointer work, not as a mechanical branch rebase and not as a new proof gate.
   The order is language syntax and types, borrow and provenance rules,
   pointer-bearing products and storage-escape rejection, complete pointee
   identity in runtime MIR, Wasm's exact `i32` carrier, then migration of
   proof-critical and browser carriers. Preserve MB2 runtime-control effects,
   FCO normalization, arena rewind and suspension checks, and independent
   receipt gates. The existing typed `BrowserPtr` aggregate fix is a
   compatibility oracle. Completion means the equivalent first-class pointer
   cases pass and the forgeable `BrowserPtr` and `MemPtr` wrappers can be
   deleted, not merely wrapped again.

The Definition of done is not yet met. In particular, the real-GPU gate,
manifest deletion, Worker/DEC general messaging, complete legacy disposition,
and bounded proof remain open.
